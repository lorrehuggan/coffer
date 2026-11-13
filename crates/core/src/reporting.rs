//! Reporting queries for the read tools (ROADMAP M2.2, M2.14): monthly
//! summaries, spending by category, budgets, net worth and the transaction
//! searches the MCP server and later the app run.
//!
//! Block C: queries take a [`crate::db::Reader`], read `ledger_line` (splits,
//! refund links; excluded rows and confirmed transfers left out) and return
//! money as [`crate::money::Money`], never floats.
