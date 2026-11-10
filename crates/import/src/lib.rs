//! Coffer's statement importers: the `Importer` trait (detect → parse →
//! normalize), CSV (generic and `presets/<region>/<bank>.toml`) and OFX/QFX.
//!
//! Import is deterministic: no model or network calls. Populated from M1.6
//! onward (`docs/ROADMAP.md`).
