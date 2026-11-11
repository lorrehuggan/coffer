//! Coffer's statement importers: the [`Importer`] trait (detect → parse →
//! normalize), CSV (generic and `presets/<region>/<bank>.toml`) and OFX/QFX.
//!
//! An importer turns a [`SourceFile`] into a
//! [`Statement`](coffer_core::statement::Statement), the normalized form that
//! core deduplicates, reconciles and commits. Import is deterministic: no
//! model or network calls. [`csv::CsvImporter`] reads delimited text through
//! a [`csv::CsvMapping`]; [`preset::Presets`] loads the bank presets that
//! produce those mappings. [`ofx::OfxImporter`] reads OFX 1.x (SGML) and
//! 2.x (XML) files, QFX included.

pub mod csv;
mod importer;
pub mod ofx;
pub mod preset;
mod source;

pub use importer::{AnyImporter, Candidate, Detect, Detection, ImportError, Importer, detect};
pub use source::SourceFile;
