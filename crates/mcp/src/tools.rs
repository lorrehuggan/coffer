//! The tools, one module per group. Each group's `register()` adds its
//! tools to the registry; adding a tool is a change inside one group file
//! (`README.md` in this crate). A group is a kind of data, not a level:
//! each tool declares its own level.
//!
//! | Module | Tools | Block |
//! |---|---|---|
//! | [`catalog`] | reference data: `list_categories`; `list_accounts`, `list_rules`, `list_reports` | C |
//! | [`aggregate`] | totals: `monthly_summary`, `spending_by_category`, `budget_status`, `net_worth_history` | C |
//! | [`rows`] | transaction rows: `search_transactions`, `merchant_history`, `uncategorized_transactions` | C |
//! | [`recurring`] | `recurring_payments`, schedules | D |
//! | [`inbox`] | `propose_*`, `save_report` | E |

pub mod aggregate;
pub mod catalog;
pub mod inbox;
pub mod recurring;
pub mod rows;

use crate::registry::Registry;

/// Every tool, at every level.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    catalog::register(&mut registry);
    aggregate::register(&mut registry);
    rows::register(&mut registry);
    recurring::register(&mut registry);
    inbox::register(&mut registry);
    registry
}
