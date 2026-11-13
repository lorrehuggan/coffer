//! Access policy for every call: the client's level, the gate, and what the
//! access log records (ROADMAP M2.11–M2.13). The server's wrapper
//! ([`crate::server`]) calls these and nothing else decides access.
//!
//! This file decides access: the enabled / requested / ignored policy and
//! the kill switch, read on every call. The `redact` submodule decides what
//! the log keeps of a call: [`redact_args`], [`purpose`] and
//! [`result_hash`].

mod redact;

pub use redact::{purpose, redact_args, result_hash};

use coffer_core::access_log::Outcome;
use coffer_core::db::Reader;
use coffer_core::mcp_client::{self, AccessLevel, ClientAccess, ClientName};
use coffer_core::setting;

use crate::registry::ToolSpec;
use crate::store::{Store, StoreError};

/// What a client may do now: the kill switch and the client's own row,
/// read together on every `tools/list` and every call, so a change in the
/// app or the CLI applies at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access {
    /// The kill switch ([`setting::mcp_enabled`]): `false` refuses every
    /// client, whatever its row says.
    pub switched_on: bool,
    /// The client's row ([`mcp_client::access`]).
    pub client: ClientAccess,
}

impl Access {
    /// The level whose tools the client sees and may call now: `None` when
    /// the kill switch is off or the client isn't enabled.
    pub fn level(self) -> Option<AccessLevel> {
        if self.switched_on {
            self.client.level()
        } else {
            None
        }
    }

    /// The level the access log records: the client's enabled level, also
    /// when the kill switch refuses the call.
    pub fn logged_level(self) -> Option<AccessLevel> {
        self.client.level()
    }
}

/// Why the client's access couldn't be read or its request recorded.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Setting(#[from] setting::Error),

    #[error(transparent)]
    Client(#[from] mcp_client::Error),

    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The client's access now, without writing anything. A name with no row
/// is [`ClientAccess::Unknown`].
pub fn current(reader: &Reader, client: &ClientName) -> Result<Access, Error> {
    // The kill switch first: if it can't be read, nothing else matters.
    let switched_on = setting::mcp_enabled(reader)?;
    let client = mcp_client::access(reader, client)?;
    Ok(Access {
        switched_on,
        client,
    })
}

/// The client's access for a request from it: [`current`], and a name with
/// no row is recorded as `requested` through the [`Store`] (decision record
/// 2026-11-10, point 2), whether or not the kill switch is on, so the user
/// sees who asked. Recording is idempotent; the server never enables a
/// client or changes a level.
pub fn for_request(reader: &Reader, store: &Store) -> Result<Access, Error> {
    let mut access = current(reader, store.client())?;
    if access.client == ClientAccess::Unknown {
        if store.record_requested()? {
            tracing::debug!("recorded a new client as requested");
        }
        access.client = ClientAccess::Requested;
    }
    Ok(access)
}

/// The gate's answer for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Run the tool for a client at this level.
    Allow(AccessLevel),
    /// Refused, with the outcome to log (never [`Outcome::Ok`] or
    /// [`Outcome::Error`]).
    Refuse(Outcome),
}

/// Whether a client with `access` may call `tool` now. The kill switch
/// comes first (`refused_off`), then the client's row
/// (`refused_not_enabled` for a requested, ignored or revoked client), then
/// the tool's level (`refused_level`: a tool above the client's level
/// isn't registered for it).
pub fn gate(access: Access, tool: &ToolSpec) -> Gate {
    if !access.switched_on {
        return Gate::Refuse(Outcome::RefusedOff);
    }
    match access.client {
        ClientAccess::Enabled(level) if tool.level <= level => Gate::Allow(level),
        ClientAccess::Enabled(_) => Gate::Refuse(Outcome::RefusedLevel),
        ClientAccess::Unknown
        | ClientAccess::Requested
        | ClientAccess::Ignored
        | ClientAccess::Revoked => Gate::Refuse(Outcome::RefusedNotEnabled),
    }
}

/// What a refused call tells the client.
pub fn refusal_message(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::RefusedOff => "AI access to Coffer is switched off.",
        Outcome::RefusedNotEnabled => {
            "This client isn't enabled in Coffer. The user can enable it in Coffer's settings."
        }
        Outcome::RefusedLevel => "This tool isn't available at this client's access level.",
        Outcome::Ok | Outcome::Error => "Refused.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Writes;

    const STANDARD_TOOL: ToolSpec = ToolSpec {
        name: "standard_tool",
        description: "A standard tool.",
        level: AccessLevel::Standard,
        writes: Writes::Nothing,
    };

    fn access(switched_on: bool, client: ClientAccess) -> Access {
        Access {
            switched_on,
            client,
        }
    }

    #[test]
    fn the_kill_switch_comes_first() {
        let off = access(false, ClientAccess::Enabled(AccessLevel::Standard));
        assert_eq!(gate(off, &STANDARD_TOOL), Gate::Refuse(Outcome::RefusedOff));
        assert_eq!(off.level(), None);
        assert_eq!(off.logged_level(), Some(AccessLevel::Standard));
        let unknown = access(false, ClientAccess::Requested);
        assert_eq!(
            gate(unknown, &STANDARD_TOOL),
            Gate::Refuse(Outcome::RefusedOff)
        );
    }

    #[test]
    fn a_client_that_isnt_enabled_is_refused() {
        for client in [
            ClientAccess::Unknown,
            ClientAccess::Requested,
            ClientAccess::Ignored,
            ClientAccess::Revoked,
        ] {
            let access = access(true, client);
            assert_eq!(
                gate(access, &STANDARD_TOOL),
                Gate::Refuse(Outcome::RefusedNotEnabled),
                "{client:?}"
            );
            assert_eq!(access.level(), None);
        }
    }

    #[test]
    fn a_tool_above_the_level_is_refused() {
        let aggregate = access(true, ClientAccess::Enabled(AccessLevel::Aggregate));
        assert_eq!(
            gate(aggregate, &STANDARD_TOOL),
            Gate::Refuse(Outcome::RefusedLevel)
        );
        let standard = access(true, ClientAccess::Enabled(AccessLevel::Standard));
        assert_eq!(
            gate(standard, &STANDARD_TOOL),
            Gate::Allow(AccessLevel::Standard)
        );
    }
}
