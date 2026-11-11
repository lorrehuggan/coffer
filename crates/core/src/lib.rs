//! Coffer's core: domain types, the SQLite schema and migrations, rules,
//! transfer matching, deduplication, reporting queries, schedules, backups
//! and export.
//!
//! Synchronous and UI-free, so everything here is testable without a window.
//! It must not depend on GPUI, MCP, or any network or AI crate.

pub mod account;
pub mod balance;
pub mod category;
pub mod db;
pub mod id;
pub mod import;
pub mod money;
pub mod paths;
pub mod rule;
pub mod statement;
pub mod sub_account;
pub mod transfer;
