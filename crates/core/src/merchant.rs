//! Merchant names (ROADMAP M1.23): the name a transaction shows
//! (`transaction.merchant_display`), derived from the bank's text.
//!
//! # Deriving a name
//!
//! 1. **Source text.** The bank's merchant (`transaction.merchant`) when the
//!    export has a non-blank one, else the raw description
//!    (`description_raw`); see [`source_text`]. A row with neither has no
//!    name (`NULL`).
//! 2. **Clean-up.** [`clean`] removes card numbers, payment-method words,
//!    processor prefixes and trailing dates and references, and keeps the
//!    bank's case.
//! 3. **Merchant names (aliases).** The stored [`MerchantAlias`]es, loaded
//!    once into an [`AliasSet`], resolve the cleaned text (decision record
//!    "rules", point 5): an `exact` match wins, then the longest matching
//!    `prefix`, then the first matching `regex` by ID (oldest first). Every
//!    kind ignores case; plain patterns are compared with whitespace
//!    collapsed. No match keeps the cleaned text.
//!
//! The name is derived, so it can be recomputed at any time and the bank's
//! text is never changed. `transaction.merchant_display_source` says who set
//! it: `NULL` is derived and recomputed; `rule` (a rule's `set_merchant`,
//! M1.24) and `user` are never overwritten by a recompute.
//!
//! # When names are computed
//!
//! - On import, [`apply_to_batch`] names the batch's rows inside the
//!   import's own database transaction.
//! - A new, edited or deleted alias applies to existing transactions at
//!   once, with no change batch (the name is derived): [`add`], [`edit`] and
//!   [`delete`] recompute every derived row ([`recompute_all`]) in the same
//!   database transaction and return how many rows changed.
//! - [`Database::open`](crate::db::Database::open) names every existing row
//!   when it applies the migration that introduced names.
//! - [`set_user_name`] and [`clear_user_name`] set a name on one
//!   transaction, or hand it back to the derived name.
//!
//! # Rules
//!
//! Names never change which rules match (decision record "Inbox impact
//! preview", point 5): a rule on [`MatchField::Merchant`] matches
//! `clean(transaction.merchant)`, before any alias applies.
//!
//! Patterns, names and transaction text are user, agent or bank text: errors
//! never repeat them, and they are never logged.
//!
//! [`MatchField::Merchant`]: crate::rule::MatchField::Merchant

mod clean;
mod store;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension};

pub use clean::clean;
pub(crate) use store::add_in;
pub use store::{
    Error, InvalidAlias, add, clear_user_name, delete, edit, get, list, set_user_name,
};

use crate::id::{BatchId, MerchantAliasId, TransactionId};
use crate::rule::{
    MatchKind, Pattern, PatternError, UnknownKey, collapse_whitespace, fold, key_enum,
};

/// The schema version whose migration added merchant names
/// (`V7__merchant_alias.sql`). Opening a database from before it names
/// every existing row.
pub(crate) const SCHEMA_VERSION: crate::db::SchemaVersion = 7;

key_enum! {
    /// How an alias's pattern is tested against the cleaned bank text, as
    /// `merchant_alias.match_kind` stores it. Every kind ignores case.
    AliasKind, "alias match kind" {
        /// The whole cleaned text equals the pattern ("Is").
        Exact => "exact",
        /// The cleaned text starts with the pattern ("Starts with").
        Prefix => "prefix",
        /// The pattern is a regular expression ("Pattern"), validated like a
        /// rule's.
        Regex => "regex",
    }
}

impl AliasKind {
    /// The rule match kind whose validation an alias pattern shares.
    const fn match_kind(self) -> MatchKind {
        match self {
            AliasKind::Exact => MatchKind::Is,
            AliasKind::Prefix => MatchKind::StartsWith,
            AliasKind::Regex => MatchKind::Regex,
        }
    }
}

/// Why an alias definition was refused.
///
/// Never repeats the pattern or the name: they are user (or agent) text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AliasError {
    /// The pattern is empty or only whitespace.
    #[error("the pattern is blank")]
    BlankPattern,

    /// The pattern doesn't validate: too long, or a `regex` that doesn't
    /// compile within the limits rules use.
    #[error(transparent)]
    Pattern(#[from] PatternError),

    /// The name is empty or only whitespace.
    #[error("the name is blank")]
    BlankName,
}

/// A merchant name a transaction shows: not blank, and without whitespace
/// at either end (it is trimmed).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

impl DisplayName {
    /// Trims `name` and checks it isn't blank.
    pub fn new(name: &str) -> Result<DisplayName, AliasError> {
        let name = name.trim();
        if name.is_empty() {
            Err(AliasError::BlankName)
        } else {
            Ok(DisplayName(name.to_owned()))
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // User text: length only.
        f.debug_struct("DisplayName")
            .field("len", &self.0.len())
            .finish()
    }
}

/// What an alias matches and the name it gives: a `merchant_alias` row
/// minus its ID and creation time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasDefinition {
    kind: AliasKind,
    pattern: Pattern,
    name: DisplayName,
}

impl AliasDefinition {
    /// Checks `pattern` for `kind` and builds the definition.
    ///
    /// A pattern is not blank and at most
    /// [`MAX_PATTERN_LEN`](crate::rule::MAX_PATTERN_LEN) bytes; a `regex`
    /// pattern compiles case-insensitively within the limits rules use
    /// ([`Pattern`]). The pattern is stored as given.
    pub fn new(kind: AliasKind, pattern: &str, name: DisplayName) -> Result<Self, AliasError> {
        if pattern.trim().is_empty() {
            return Err(AliasError::BlankPattern);
        }
        let pattern = Pattern::new(kind.match_kind(), pattern)?;
        Ok(AliasDefinition {
            kind,
            pattern,
            name,
        })
    }

    /// How the pattern is tested.
    pub const fn kind(&self) -> AliasKind {
        self.kind
    }

    /// The pattern, as stored.
    pub fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    /// The name matching transactions show.
    pub const fn name(&self) -> &DisplayName {
        &self.name
    }

    /// What two definitions are compared by to find a duplicate: the kind
    /// and, for plain patterns, the pattern as matching sees it (folded,
    /// whitespace collapsed).
    fn match_key(&self) -> (AliasKind, String) {
        let text = match self.kind {
            AliasKind::Exact => exact_key(self.pattern()),
            AliasKind::Prefix => prefix_key(self.pattern()),
            AliasKind::Regex => self.pattern().to_owned(),
        };
        (self.kind, text)
    }
}

/// A stored alias: its ID and its definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerchantAlias {
    id: MerchantAliasId,
    definition: AliasDefinition,
}

impl MerchantAlias {
    /// An alias with this ID, e.g. to build an [`AliasSet`] without a
    /// database.
    pub const fn new(id: MerchantAliasId, definition: AliasDefinition) -> MerchantAlias {
        MerchantAlias { id, definition }
    }

    /// The alias's row ID.
    pub const fn id(&self) -> MerchantAliasId {
        self.id
    }

    /// What it matches and the name it gives.
    pub const fn definition(&self) -> &AliasDefinition {
        &self.definition
    }
}

/// A resolved merchant name, and the alias that gave it (`None`: the
/// cleaned text itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved<'a> {
    /// The name to show.
    pub name: &'a str,
    /// The alias that matched, if any.
    pub alias: Option<MerchantAliasId>,
}

/// The stored aliases, ready to resolve many rows: loaded and validated
/// once, then reused (one import, one recompute).
///
/// The default is no aliases: every name is the cleaned text.
#[derive(Clone, Debug, Default)]
pub struct AliasSet {
    /// Exact patterns by [`exact_key`]; the lowest ID wins a tie.
    exact: HashMap<String, (MerchantAliasId, String)>,
    /// Prefixes by [`prefix_key`], longest first, then by ID.
    prefixes: Vec<(String, MerchantAliasId, String)>,
    /// Regexes in ID order.
    regexes: Vec<(Regex, MerchantAliasId, String)>,
}

impl AliasSet {
    /// An alias set from aliases in any order.
    pub fn new(aliases: impl IntoIterator<Item = MerchantAlias>) -> AliasSet {
        let mut aliases: Vec<MerchantAlias> = aliases.into_iter().collect();
        aliases.sort_by_key(MerchantAlias::id);
        let mut set = AliasSet::default();
        for alias in aliases {
            let definition = &alias.definition;
            let name = definition.name.as_str().to_owned();
            match definition.kind {
                AliasKind::Exact => {
                    set.exact
                        .entry(exact_key(definition.pattern()))
                        .or_insert((alias.id, name));
                }
                AliasKind::Prefix => {
                    set.prefixes
                        .push((prefix_key(definition.pattern()), alias.id, name));
                }
                AliasKind::Regex => {
                    let regex = definition
                        .pattern
                        .regex()
                        .expect("a regex pattern keeps its compiled regex")
                        .clone();
                    set.regexes.push((regex, alias.id, name));
                }
            }
        }
        set.prefixes
            .sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.1.cmp(&b.1)));
        set
    }

    /// Loads and validates every stored alias. Takes a connection, so it
    /// can run inside another module's open transaction.
    pub fn load(conn: &Connection) -> Result<AliasSet, Error> {
        Ok(AliasSet::new(store::load_all(conn)?))
    }

    /// Whether there are no aliases.
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.prefixes.is_empty() && self.regexes.is_empty()
    }

    /// The name for already-[cleaned](clean) text: an exact match, else
    /// the longest prefix, else the first regex by ID, else the text
    /// itself.
    pub fn resolve<'a>(&'a self, cleaned: &'a str) -> Resolved<'a> {
        if self.is_empty() {
            return Resolved {
                name: cleaned,
                alias: None,
            };
        }
        let key = exact_key(cleaned);
        let found = self
            .exact
            .get(&key)
            .map(|(id, name)| (*id, name))
            .or_else(|| {
                // The text is trimmed, so a prefix's trailing space (a word
                // boundary) matches only where another word follows.
                self.prefixes
                    .iter()
                    .find(|(prefix, ..)| key.starts_with(prefix.as_str()))
                    .map(|(_, id, name)| (*id, name))
            })
            .or_else(|| {
                self.regexes
                    .iter()
                    .find(|(regex, ..)| regex.is_match(cleaned))
                    .map(|(_, id, name)| (*id, name))
            });
        match found {
            Some((id, name)) => Resolved {
                name,
                alias: Some(id),
            },
            None => Resolved {
                name: cleaned,
                alias: None,
            },
        }
    }

    /// The derived name of a transaction with this bank text: its
    /// [source text](source_text), [cleaned](clean), then resolved. `None`
    /// when the row has no text at all.
    ///
    /// Pure and deterministic, for the import commit and manual rows.
    pub fn display_name(&self, merchant: Option<&str>, description_raw: &str) -> Option<String> {
        let cleaned = clean(source_text(merchant, description_raw)?);
        Some(self.resolve(&cleaned).name.to_owned())
    }

    /// Sets the derived name of every row of an import batch whose name
    /// isn't set by a rule or the user (`merchant_display_source IS NULL`),
    /// mirrored rows included. Returns how many rows changed.
    ///
    /// Opens no transaction of its own: the import commit runs it inside
    /// its own.
    pub fn apply_to_batch(&self, conn: &Connection, batch: BatchId) -> rusqlite::Result<usize> {
        self.recompute(
            conn,
            "WHERE batch_id = ?1 AND merchant_display_source IS NULL",
            [batch],
        )
    }

    /// Recomputes the derived name of every transaction whose name isn't
    /// set by a rule or the user. Returns how many rows changed.
    ///
    /// Opens no transaction of its own.
    pub fn recompute_all(&self, conn: &Connection) -> rusqlite::Result<usize> {
        self.recompute(conn, "WHERE merchant_display_source IS NULL", [])
    }

    /// Recomputes the derived name of one transaction, unless a rule or the
    /// user set it. Returns whether it changed; a missing row is
    /// [`Error::TransactionNotFound`].
    ///
    /// For manual rows, after they are added or their text changes. Opens
    /// no transaction of its own.
    pub fn apply_to_transaction(
        &self,
        conn: &Connection,
        transaction: TransactionId,
    ) -> Result<bool, Error> {
        let exists = conn
            .query_row(
                "SELECT 1 FROM \"transaction\" WHERE id = ?1",
                [transaction],
                |_| Ok(()),
            )
            .optional()?;
        if exists.is_none() {
            return Err(Error::TransactionNotFound(transaction));
        }
        let changed = self.recompute(
            conn,
            "WHERE id = ?1 AND merchant_display_source IS NULL",
            [transaction],
        )?;
        Ok(changed > 0)
    }

    /// Recomputes the rows `filter` selects (a `WHERE` clause over
    /// `"transaction"`), writing only names that change.
    fn recompute(
        &self,
        conn: &Connection,
        filter: &str,
        params: impl rusqlite::Params,
    ) -> rusqlite::Result<usize> {
        let mut select = conn.prepare(&format!(
            "SELECT id, merchant, description_raw, merchant_display FROM \"transaction\" \
             {filter} ORDER BY id"
        ))?;
        let rows = select
            .query_map(params, |row| {
                Ok((
                    row.get::<_, TransactionId>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut update = conn.prepare(
            "UPDATE \"transaction\" SET merchant_display = ?1 \
             WHERE id = ?2 AND merchant_display_source IS NULL",
        )?;
        let mut changed = 0;
        for (id, merchant, description, current) in rows {
            let name = self.display_name(merchant.as_deref(), &description);
            if name != current {
                changed += update.execute(rusqlite::params![name, id])?;
            }
        }
        Ok(changed)
    }
}

/// The bank text a name is derived from: the merchant when it isn't blank,
/// else the raw description when it isn't blank, else `None`.
pub fn source_text<'a>(merchant: Option<&'a str>, description_raw: &'a str) -> Option<&'a str> {
    merchant
        .filter(|text| !text.trim().is_empty())
        .or_else(|| Some(description_raw).filter(|text| !text.trim().is_empty()))
}

/// Sets the derived name of every row of an import batch; see
/// [`AliasSet::apply_to_batch`]. Loads the aliases from `conn` first, and
/// opens no transaction of its own.
pub fn apply_to_batch(conn: &Connection, batch: BatchId) -> Result<usize, Error> {
    Ok(AliasSet::load(conn)?.apply_to_batch(conn, batch)?)
}

/// Recomputes every derived name; see [`AliasSet::recompute_all`]. Loads
/// the aliases from `conn` first, and opens no transaction of its own.
pub fn recompute_all(conn: &Connection) -> Result<usize, Error> {
    Ok(AliasSet::load(conn)?.recompute_all(conn)?)
}

/// Recomputes one transaction's derived name; see
/// [`AliasSet::apply_to_transaction`]. Loads the aliases from `conn` first.
pub fn apply_to_transaction(conn: &Connection, transaction: TransactionId) -> Result<bool, Error> {
    AliasSet::load(conn)?.apply_to_transaction(conn, transaction)
}

/// An exact pattern, or cleaned text, as exact matching compares it:
/// whitespace collapsed and trimmed, then folded.
fn exact_key(text: &str) -> String {
    fold(&collapse_whitespace(text))
}

/// A prefix as matching compares it: leading whitespace dropped, runs of
/// whitespace collapsed to one space (a trailing one is kept: `ACME `
/// matches `ACME STORE` but not `ACMEX`), then folded.
fn prefix_key(text: &str) -> String {
    let mut squeezed = String::with_capacity(text.len());
    let mut space = false;
    for c in text.trim_start().chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                squeezed.push(' ');
                space = false;
            }
            squeezed.push(c);
        }
    }
    if space {
        squeezed.push(' ');
    }
    fold(&squeezed)
}
