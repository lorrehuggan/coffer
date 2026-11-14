//! Coffer's core: domain types, the SQLite schema and migrations, rules,
//! transfer matching, deduplication, reporting queries, schedules, backups
//! and export.
//!
//! Synchronous and UI-free, so everything here is testable without a window.
//! It must not depend on GPUI, MCP, or any network or AI crate.

pub mod access_log;
pub mod account;
pub mod backup;
pub mod balance;
pub mod budget;
pub mod category;
pub mod change;
pub mod checks;
pub mod clock;
pub mod db;
pub mod duplicate;
pub mod envelope;
pub mod exclusion;
pub mod export;
pub mod id;
pub mod import;
pub mod inbox;
pub mod link;
pub mod manual;
pub mod mcp_client;
pub mod merchant;
pub mod money;
pub mod note;
pub mod paths;
pub mod reconciliation;
pub mod recurring;
pub mod reporting;
pub mod rule;
pub mod schedule;
pub mod setting;
pub mod split;
pub mod statement;
pub mod sub_account;
pub mod tag;
pub mod transfer;
