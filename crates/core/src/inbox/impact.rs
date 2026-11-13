//! What approving a suggestion would change (Inbox impact preview; subagent
//! 1 of Block E fills this in). Computed against today's data each time
//! (decision record "Inbox impact preview", point 6), through
//! `change::dry_run` and the preview functions. M3.14 reuses it.

use std::fmt;

use super::SuggestionId;
use super::approve::ApproveError;
use crate::db::Database;

/// What approving a suggestion would change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Impact {
    _todo: (),
}

impl fmt::Display for Impact {
    /// One line for the user, e.g. "recategorises 143 past transactions; 12
    /// have a category you set".
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("Block E subagent 1")
    }
}

/// What approving suggestion `id` would change now. Reads only.
pub fn impact(_db: &Database, _id: SuggestionId) -> Result<Impact, ApproveError> {
    todo!("Block E subagent 1")
}
