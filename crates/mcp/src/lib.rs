//! Coffer's MCP server (`coffer mcp`): rmcp over stdio, on top of the core
//! API.
//!
//! Tools may write only to the inbox (`suggestion`, `report`), never expose
//! PII, and are registered per client access level (`docs/ARCHITECTURE.md`).
//! Populated in M2 (`docs/ROADMAP.md`).
