//! Reporting queries for the read tools (ROADMAP M2.2, M2.14): monthly
//! summaries, spending by category, budgets, net worth and the transaction
//! searches the MCP server and later the app run.
//!
//! Every query takes a [`crate::db::Reader`] and returns money as
//! [`crate::money::Money`] (integer minor units), never floats.
//!
//! # The shared pieces
//!
//! - [`Period`] (a month, `YYYY-MM`) and [`DateRange`] (inclusive, `from <=
//!   to`), both validated.
//! - [`Ledger`]: a query over the `ledger_line` view, the one place reports
//!   read transactions from. It keeps only rows that count in reports
//!   (`in_reports`: not excluded, not in a confirmed transfer, and for a
//!   refund's part, its purchase isn't either), so split purchases count per
//!   line and confirmed refunds net against their purchase's category. Each
//!   [`LedgerLine`] carries its parent's transaction ID. Amounts are the
//!   user's share by default ([`Basis::Personal`], `personal_minor`:
//!   "Decision record: refunds, reimbursements and shared costs", point 3);
//!   [`Basis::Gross`] is the whole line amount.
//! - [`Totals`]: sums per currency. Amounts in different currencies are
//!   never added together; conversion waits for M4.6.
//! - [`Limit`] and [`Capped`]: at most [`MAX_ROWS`] rows per call, with a
//!   `truncated` flag.
//! - [`balance`]: an account's balance at the end of a day, from its stated
//!   balances and its transactions (gross, like the bank's).
//!
//! # The queries
//!
//! | Module | Queries |
//! |---|---|
//! | [`summary`] | `monthly_summary` |
//! | [`spending`] | `spending_by_category` |
//! | [`net_worth`] | `net_worth_history` |
//! | [`budget_status`] | `budget_status` |
//! | [`rows`] | `search_transactions`, `uncategorized_transactions`, `merchant_history` |
//! | [`catalog`] | `list_accounts`, `list_rules`, `list_reports` |
//!
//! Nothing here logs transaction text.

pub mod balance;
pub mod budget_status;
pub mod catalog;
pub mod ledger;
pub mod net_worth;
pub mod period;
pub mod rows;
pub mod spending;
pub mod summary;
pub mod totals;

#[cfg(test)]
pub(crate) mod test_ledger;

pub use ledger::{Basis, Ledger, LedgerLine, LineCategory};
pub use period::{DateRange, Period};
pub use totals::{Capped, DEFAULT_ROWS, Limit, MAX_ROWS, Totals};

use crate::category::CategoryKey;
use crate::id::AccountId;
use crate::money::MoneyError;

/// Why a reporting query failed. Messages never contain transaction text
/// or tag names: they may reach an MCP client or a log.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a month is written YYYY-MM, e.g. 2026-11")]
    InvalidPeriod,

    #[error("a date is written YYYY-MM-DD, e.g. 2026-11-13")]
    InvalidDate,

    #[error("the start {from} is after the end {to}")]
    EmptyRange {
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    },

    #[error("there is no account {0}")]
    UnknownAccount(AccountId),

    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    /// No such tag, or a private one: which rows carry a private tag never
    /// leaves the app, so the two answers are the same.
    #[error("there is no tag with that name")]
    UnknownTag,

    #[error("the smallest amount is larger than the largest")]
    InvalidAmountRange,

    /// A total overflowed `i64`.
    #[error(transparent)]
    Money(#[from] MoneyError),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A stored rule no longer validates. The message names the rule and
    /// the problem, never its pattern or action text.
    #[error(transparent)]
    Rule(#[from] crate::rule::StoreError),
}

impl Error {
    /// Whether the caller's input was wrong (a malformed month, an unknown
    /// category), rather than the database. An MCP tool turns these into
    /// "invalid arguments" with this message; the rest stay generic.
    pub fn is_invalid_input(&self) -> bool {
        match self {
            Error::InvalidPeriod
            | Error::InvalidDate
            | Error::EmptyRange { .. }
            | Error::UnknownAccount(_)
            | Error::UnknownCategory(_)
            | Error::UnknownTag
            | Error::InvalidAmountRange => true,
            Error::Money(_) | Error::Database(_) => false,
            Error::Rule(_) => false,
        }
    }
}
