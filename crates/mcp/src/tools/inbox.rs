//! Inbox writes: `propose_categorizations`, `propose_rule`,
//! `propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link`,
//! `save_report` (ROADMAP M2.3, M2.14). Registered with
//! [`Writes::Inbox`](crate::registry::Writes::Inbox); they write through
//! [`Call::inbox`](crate::registry::Call::inbox) only. `save_report` is
//! `aggregate`, the proposals `standard`.
//!
//! Block E.

use crate::registry::Registry;

/// Adds this group's tools.
pub fn register(_registry: &mut Registry) {}
