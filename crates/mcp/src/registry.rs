//! The tool registry: what each tool is, the level it needs, whether it
//! writes, and its handler.
//!
//! A tool is a [`ToolSpec`] and a plain function
//! `fn(&Call<'_>, A) -> Result<Reply<T>, ToolError>`, added to the
//! [`Registry`] by its group's `register()` in [`crate::tools`]:
//!
//! - `A` is the tool's own arguments (`Deserialize + JsonSchema`, or
//!   [`NoArgs`]). The registry adds the optional `purpose` argument every
//!   tool takes, so a tool never declares or reads it: the server logs it
//!   and nothing interprets it.
//! - `T` is the result (`Serialize + JsonSchema`), a JSON object built from
//!   the types in [`crate::output`]. It is returned as structured content
//!   with an output schema.
//! - [`Call`] is everything a handler may touch: the read-only connection,
//!   the client's level, constructors for merchant labels, untrusted text
//!   and amounts, and, for a tool that declares [`Writes::Inbox`], the
//!   inbox.
//!
//! A client sees and can call only the tools at or below its level
//! ([`Registry::tools_for`]); the server refuses the rest.

use std::cell::Cell;
use std::sync::Arc;

use coffer_core::db::Reader;
use coffer_core::envelope;
use coffer_core::money::Money;
use coffer_core::setting::PseudonymKey;
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use coffer_core::mcp_client::AccessLevel;

use crate::output::{Amount, MerchantLabel, UntrustedText};
use crate::store::Inbox;

/// Whether a tool writes, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writes {
    /// Reads only.
    Nothing,
    /// Files suggestions or saves reports ([`Call::inbox`]). Never anything
    /// else: `AGENTS.md` → Invariants.
    Inbox,
}

/// What a tool is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolSpec {
    /// `snake_case`, at most 64 characters, unique.
    pub name: &'static str,
    /// What the tool returns, for the AI client. Plain words; never PII.
    pub description: &'static str,
    /// The lowest access level that sees the tool.
    pub level: AccessLevel,
    pub writes: Writes,
}

/// A handler's result: the body, and how many rows it carries (the
/// transactions, categories or other records it describes) for the access
/// log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply<T> {
    pub body: T,
    pub rows: u64,
}

impl<T> Reply<T> {
    pub fn new(body: T, rows: u64) -> Self {
        Reply { body, rows }
    }
}

/// Why a tool call failed. The message is shown to the AI client, so it
/// never holds data from the database.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The arguments don't fit the tool.
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),

    /// A core query or write failed. The cause stays in the server's debug
    /// log.
    #[error("Coffer couldn't complete the request")]
    Core(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A tool used something it didn't declare, or the server is broken.
    #[error("internal error: {0}")]
    Internal(&'static str),
}

impl ToolError {
    /// Wraps a core error: `.map_err(ToolError::core)?`.
    pub fn core(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        ToolError::Core(Box::new(error))
    }
}

/// Arguments for a tool that takes none (besides `purpose`).
#[derive(Debug, Clone, Copy, Default, serde::Deserialize, JsonSchema)]
pub struct NoArgs {}

/// The arguments a client sends: the tool's own, plus `purpose`.
#[derive(serde::Deserialize, JsonSchema)]
struct Args<A> {
    /// Why you are calling this tool, in a few words. Coffer records it in
    /// its access log for the user to read; it doesn't change the result.
    #[serde(default)]
    #[allow(dead_code)] // Read from the raw arguments, for the log.
    purpose: Option<String>,
    #[serde(flatten)]
    args: A,
}

/// The merchant label [`Call::merchant`] gives below `standard` when the
/// database has no pseudonym key. Not a token, so it can't collide with one.
pub const HIDDEN_MERCHANT: &str = "M-hidden";

/// What [`Call::untrusted`] gives below `standard`, in place of bank text.
/// Coffer's words, like the withheld marker.
pub const HIDDEN_DESCRIPTION: &str =
    "[hidden: descriptions name merchants, so they aren't shared at this access level]";

/// What a handler may use during one call.
pub struct Call<'a> {
    reader: &'a Reader,
    level: AccessLevel,
    inbox: Option<Inbox<'a>>,
    /// The per-database key for merchant tokens. The server refuses a
    /// `pseudonymous` call without one, so it is there whenever
    /// [`Call::merchant`] needs it.
    pseudonym_key: Option<PseudonymKey>,
    withheld: Cell<u64>,
}

impl<'a> Call<'a> {
    pub(crate) fn new(
        reader: &'a Reader,
        level: AccessLevel,
        inbox: Option<Inbox<'a>>,
        pseudonym_key: Option<PseudonymKey>,
    ) -> Self {
        Call {
            reader,
            level,
            inbox,
            pseudonym_key,
            withheld: Cell::new(0),
        }
    }

    /// The read-only connection. Queries are core functions that take it.
    pub fn reader(&self) -> &'a Reader {
        self.reader
    }

    /// The calling client's access level. The registry already keeps tools
    /// above it away; a tool reads it to leave out fields, such as tags
    /// below `standard`.
    pub fn level(&self) -> AccessLevel {
        self.level
    }

    /// The inbox, for a tool registered with [`Writes::Inbox`]. Any other
    /// tool gets an error.
    pub fn inbox(&self) -> Result<&Inbox<'a>, ToolError> {
        self.inbox
            .as_ref()
            .ok_or(ToolError::Internal("this tool isn't registered as writing"))
    }

    /// How a merchant is shown to this client.
    ///
    /// - `standard`: the display name, without invisible characters
    ///   ([`envelope::strip_invisible`]). A display name comes from bank
    ///   text too (a row without a merchant takes its description), so one
    ///   that reads like instructions ([`envelope::reads_like_instructions`])
    ///   is withheld like a description: the marker, counted in the access
    ///   log's `withheld_count`.
    /// - `pseudonymous`: a stable token, `M-` and 12 hex digits
    ///   ([`envelope::pseudonym`]): the same name gives the same token in
    ///   this database, a different one in another.
    /// - `aggregate` returns no rows, so no tool should call this; if one
    ///   does, it gets the token too. Without a key (never the case at
    ///   `pseudonymous`: the server refuses the call) it gets
    ///   [`HIDDEN_MERCHANT`].
    pub fn merchant(&self, display: &str) -> MerchantLabel {
        if self.level == AccessLevel::Standard {
            let visible = envelope::strip_invisible(display);
            if envelope::reads_like_instructions(&visible).is_some() {
                return MerchantLabel::new(self.withhold(&visible));
            }
            return MerchantLabel::new(visible);
        }
        MerchantLabel::new(match &self.pseudonym_key {
            Some(key) => envelope::pseudonym(key, display),
            None => HIDDEN_MERCHANT.to_owned(),
        })
    }

    /// The withheld marker for `text`, counted for the access log.
    fn withhold(&self, text: &str) -> String {
        self.withheld.set(self.withheld.get() + 1);
        let sanitized = envelope::sanitize(text);
        debug_assert!(sanitized.is_withheld());
        sanitized.text
    }

    /// Bank text as `description_untrusted`: sanitised and capped, or
    /// withheld ([`envelope::sanitize`]). Counts what was withheld for the
    /// access log.
    ///
    /// Below `standard` it is always [`HIDDEN_DESCRIPTION`]: bank text names
    /// the merchant (`BEAN THERE TESTVILLE GBR`), so sending it would undo
    /// the pseudonyms. That isn't counted as withheld.
    pub fn untrusted(&self, raw: &str) -> UntrustedText {
        if self.level < AccessLevel::Standard {
            return UntrustedText::new(HIDDEN_DESCRIPTION.to_owned());
        }
        let sanitized = envelope::sanitize(raw);
        if sanitized.is_withheld() {
            self.withheld.set(self.withheld.get() + 1);
        }
        UntrustedText::new(sanitized.text)
    }

    /// An amount for the client: integer minor units in the original
    /// currency, plus home currency once that is decided.
    pub fn amount(&self, money: Money) -> Amount {
        Amount::original(money)
    }

    /// How many descriptions this call withheld so far.
    pub(crate) fn withheld(&self) -> u64 {
        self.withheld.get()
    }
}

/// A handler's result, type-erased for the server.
pub(crate) struct Handled {
    pub value: serde_json::Value,
    pub rows: u64,
}

type Handler = Arc<dyn Fn(&Call<'_>, JsonObject) -> Result<Handled, ToolError> + Send + Sync>;

/// A registered tool.
#[derive(Clone)]
pub struct Registered {
    spec: ToolSpec,
    tool: Tool,
    handler: Handler,
}

impl Registered {
    pub fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    /// The tool as `tools/list` shows it, with its schemas.
    pub fn tool(&self) -> &Tool {
        &self.tool
    }

    pub(crate) fn run(&self, call: &Call<'_>, args: JsonObject) -> Result<Handled, ToolError> {
        (self.handler)(call, args)
    }
}

/// Every tool the server knows, at every level.
#[derive(Clone, Default)]
pub struct Registry {
    tools: Vec<Registered>,
}

impl Registry {
    /// An empty registry. [`crate::tools::registry`] has every tool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tool.
    ///
    /// # Panics
    ///
    /// On a programming error, which the registry tests catch: a name that
    /// isn't `snake_case` or is taken, an empty description, or a result
    /// type whose schema isn't an object.
    pub fn add<A, T>(
        &mut self,
        spec: ToolSpec,
        handler: fn(&Call<'_>, A) -> Result<Reply<T>, ToolError>,
    ) where
        A: DeserializeOwned + JsonSchema + 'static,
        T: Serialize + JsonSchema + 'static,
    {
        assert!(
            !spec.name.is_empty()
                && spec.name.len() <= 64
                && spec
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
            "tool name {:?} must be snake_case, at most 64 characters",
            spec.name
        );
        assert!(
            self.get(spec.name).is_none(),
            "tool {} is registered twice",
            spec.name
        );
        assert!(
            !spec.description.trim().is_empty(),
            "tool {} needs a description",
            spec.name
        );

        let annotations = ToolAnnotations::new()
            .read_only(spec.writes == Writes::Nothing)
            .destructive(false)
            .open_world(false);
        let tool = Tool::new(spec.name, spec.description, JsonObject::new())
            .with_input_schema::<Args<A>>()
            .with_output_schema::<T>()
            .with_annotations(annotations);
        assert_eq!(
            tool.output_schema
                .as_ref()
                .and_then(|schema| schema.get("type"))
                .and_then(|kind| kind.as_str()),
            Some("object"),
            "tool {}'s result must be a JSON object",
            spec.name
        );

        let handler: Handler = Arc::new(move |call, args| {
            let Args { args, .. } = serde_json::from_value::<Args<A>>(args.into())
                .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
            let reply = handler(call, args)?;
            let value = serde_json::to_value(reply.body)
                .map_err(|_| ToolError::Internal("the result didn't serialise"))?;
            if !value.is_object() {
                return Err(ToolError::Internal("the result isn't a JSON object"));
            }
            Ok(Handled {
                value,
                rows: reply.rows,
            })
        });
        self.tools.push(Registered {
            spec,
            tool,
            handler,
        });
    }

    /// The tool called `name`, at any level.
    pub fn get(&self, name: &str) -> Option<&Registered> {
        self.tools.iter().find(|tool| tool.spec.name == name)
    }

    /// Every registered tool, at every level.
    pub fn all(&self) -> impl Iterator<Item = &Registered> {
        self.tools.iter()
    }

    /// The tools a client at `level` sees, in registration order.
    pub fn tools_for(&self, level: AccessLevel) -> Vec<Tool> {
        self.tools
            .iter()
            .filter(|tool| tool.spec.level <= level)
            .map(|tool| tool.tool.clone())
            .collect()
    }
}
