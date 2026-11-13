//! Totals: `monthly_summary`, `spending_by_category`, `budget_status`,
//! `net_worth_history` (ROADMAP M2.2). No transaction rows: these are
//! `aggregate`-level tools.
//!
//! Block C.

use coffer_core::reporting;

use crate::registry::{Registry, ToolError};

/// Adds this group's tools.
pub fn register(_registry: &mut Registry) {}

/// A reporting error as the client sees it: the caller's mistake (a
/// malformed month, an unknown category) as invalid arguments with core's
/// message, which never holds data from the database; anything else as
/// the generic core error. Shared by the `catalog`, `aggregate` and `rows`
/// tools.
#[allow(dead_code)] // Used by the tools Block C adds.
pub(crate) fn reporting_error(error: reporting::Error) -> ToolError {
    if error.is_invalid_input() {
        ToolError::InvalidArguments(error.to_string())
    } else {
        ToolError::core(error)
    }
}
