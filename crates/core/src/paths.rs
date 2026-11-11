//! Where Coffer keeps its data.
//!
//! The data directory is the platform data dir (`~/.local/share/coffer` on
//! Linux, `~/Library/Application Support/Coffer` on macOS), unless
//! `COFFER_DATA_DIR` names another one. The database, and later its backups,
//! live there.

use std::ffi::OsString;
use std::path::PathBuf;

use directories::ProjectDirs;

/// Environment variable that overrides the data directory, e.g. to keep a
/// development database away from the real one.
pub const DATA_DIR_ENV: &str = "COFFER_DATA_DIR";

/// File name of the database inside the data directory.
pub const DATABASE_FILE: &str = "coffer.db";

/// The platform has no home directory to put a data directory in.
#[derive(Debug, thiserror::Error)]
#[error("couldn't find a data directory for Coffer; set {DATA_DIR_ENV} to choose one")]
pub struct NoDataDir;

/// The data directory: `COFFER_DATA_DIR` when set and not empty, otherwise
/// the platform data dir. Doesn't create it ([`crate::db::Database::open`]
/// does).
pub fn data_dir() -> Result<PathBuf, NoDataDir> {
    resolve_data_dir(std::env::var_os(DATA_DIR_ENV))
}

/// The database file inside [`data_dir`].
pub fn database_path() -> Result<PathBuf, NoDataDir> {
    Ok(data_dir()?.join(DATABASE_FILE))
}

fn resolve_data_dir(env_override: Option<OsString>) -> Result<PathBuf, NoDataDir> {
    match env_override {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => ProjectDirs::from("", "", "Coffer")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .ok_or(NoDataDir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_wins_when_set() {
        let dir = resolve_data_dir(Some("/some/where".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/some/where"));
    }

    #[test]
    fn empty_override_is_ignored() {
        assert_eq!(
            resolve_data_dir(Some(OsString::new())).ok(),
            resolve_data_dir(None).ok()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_default_is_xdg_data_home() {
        let dir = resolve_data_dir(None).unwrap();
        assert!(dir.ends_with("coffer"), "{}", dir.display());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_default_is_application_support() {
        let dir = resolve_data_dir(None).unwrap();
        assert!(
            dir.ends_with("Library/Application Support/Coffer"),
            "{}",
            dir.display()
        );
    }
}
