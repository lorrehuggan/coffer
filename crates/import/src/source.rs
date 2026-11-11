//! The file being imported.

use std::fmt;
use std::path::Path;

/// A bank file read into memory: its name and its bytes, undecoded.
///
/// Decoding is the importer's job, since the encoding comes from the format
/// or the preset. Only the file name is kept, never its directory, which can
/// contain the user's name (`import_batch.source_file`).
#[derive(Clone)]
pub struct SourceFile {
    name: String,
    bytes: Vec<u8>,
}

impl SourceFile {
    /// Reads the file at `path`.
    pub fn read(path: &Path) -> std::io::Result<SourceFile> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(SourceFile { name, bytes })
    }

    /// A file already in memory (a drop into the window, a test). Anything
    /// before the last `/` or `\` in `name` is dropped.
    pub fn from_bytes(name: &str, bytes: impl Into<Vec<u8>>) -> SourceFile {
        let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
        SourceFile {
            name: name.to_owned(),
            bytes: bytes.into(),
        }
    }

    /// The file name without its directory, e.g. `statement.csv`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The extension in lowercase, without the dot: `qfx` for
    /// `Statement.QFX`. `None` when there isn't one.
    pub fn extension(&self) -> Option<String> {
        let (stem, extension) = self.name.rsplit_once('.')?;
        (!stem.is_empty() && !extension.is_empty()).then(|| extension.to_ascii_lowercase())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Shows the name and size only: the contents are transaction data.
impl fmt::Debug for SourceFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceFile")
            .field("name", &self.name)
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_keeps_only_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Statement.CSV");
        std::fs::write(&path, b"a,b\n").unwrap();

        let file = SourceFile::read(&path).unwrap();

        assert_eq!(file.name(), "Statement.CSV");
        assert_eq!(file.extension().as_deref(), Some("csv"));
        assert_eq!(file.bytes(), b"a,b\n");
    }

    #[test]
    fn from_bytes_drops_directories() {
        assert_eq!(
            SourceFile::from_bytes("/home/someone/x.ofx", []).name(),
            "x.ofx"
        );
        assert_eq!(
            SourceFile::from_bytes(r"C:\Users\someone\x.qfx", []).name(),
            "x.qfx"
        );
        assert_eq!(SourceFile::from_bytes("x.csv", []).name(), "x.csv");
    }

    #[test]
    fn extension() {
        let extension = |name| SourceFile::from_bytes(name, []).extension();
        assert_eq!(extension("a.QFX").as_deref(), Some("qfx"));
        assert_eq!(extension("a.tar.csv").as_deref(), Some("csv"));
        assert_eq!(extension("noext"), None);
        assert_eq!(extension(".hidden"), None);
        assert_eq!(extension("trailing."), None);
    }

    #[test]
    fn debug_leaves_out_the_contents() {
        let file = SourceFile::from_bytes("x.csv", "SECRET MERCHANT,-12.40");
        let debug = format!("{file:?}");
        assert!(!debug.contains("SECRET"), "{debug}");
        assert!(debug.contains("x.csv") && debug.contains("22"), "{debug}");
    }
}
