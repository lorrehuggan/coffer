//! Tools that return transaction rows: `search_transactions`,
//! `merchant_history`, `uncategorized_transactions`
//! (ROADMAP M2.2, M2.14). `pseudonymous` and above. Merchants go out as
//! [`MerchantLabel`](crate::output::MerchantLabel), bank text only as
//! `description_untrusted`.
//!
//! Block C.

use crate::registry::Registry;

/// Adds this group's tools.
pub fn register(_registry: &mut Registry) {}
