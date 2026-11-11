//! Coffer's core: domain types, the SQLite schema and migrations, rules,
//! transfer matching, deduplication, reporting queries, schedules, backups
//! and export.
//!
//! Synchronous and UI-free, so everything here is testable without a window.
//! It must not depend on GPUI, MCP, or any network or AI crate.

pub mod db;
pub mod paths;
