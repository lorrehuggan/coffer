//! App-wide settings in the `setting` table (V11): the MCP kill switch and
//! the per-database key for merchant pseudonyms, which is never exposed
//! (ROADMAP M2.11; `docs/ARCHITECTURE.md` → "Decision record: Settings, AI
//! access and backups (2026-11-10)").
//!
//! Block B: typed keys and their values, read through a [`crate::db::Reader`]
//! on every MCP call (the kill switch) and written by the app or the CLI.
