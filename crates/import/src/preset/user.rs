//! Presets the user saves from the mapping screen (M3.5).
//!
//! They live in the data directory, never the repository: `<data
//! dir>/presets/<region>/<bank>.toml`, laid out like the repository's
//! `presets/` so a saved file can be contributed as it is. The directories
//! are 0700 and the files 0600 on Unix, like the database. They load next
//! to the bundled presets ([`Presets::with_user_presets`]); a saved preset
//! can't take a bundled preset's ID, and one that no longer loads is
//! reported, not used. Decision record: `docs/ARCHITECTURE.md` → "Decision
//! record: the import flow".

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::{LoadError, Preset, Presets};

/// A saved preset that can't be used.
#[derive(Debug, thiserror::Error)]
pub enum UserPresetError {
    #[error("couldn't read the saved preset {path}: {error}")]
    Read {
        path: String,
        #[source]
        error: std::io::Error,
    },
    #[error("the saved {0}")]
    Load(#[from] LoadError),
}

/// Why a preset couldn't be saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("there is already a preset with the ID {0:?}; choose another name")]
    Taken(String),
    #[error("a preset is already saved at {}", .0.display())]
    Exists(PathBuf),
    #[error("couldn't save the preset: {0}")]
    Io(#[from] std::io::Error),
}

/// The bundled presets and the ones saved in `dir`, with the saved ones
/// that couldn't be used and why. Fails only if the bundled presets don't
/// load (a broken build).
pub fn load_user_presets(dir: &Path) -> Result<(Presets, Vec<UserPresetError>), LoadError> {
    let bundled = Presets::bundled()?;
    let (files, mut errors) = read_dir(dir);
    let (presets, load_errors) = bundled.with_user_presets(
        files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str())),
    );
    errors.extend(load_errors.into_iter().map(UserPresetError::Load));
    Ok((presets, errors))
}

/// Saves `preset` in `dir` as `<region>/<bank>.toml` (its ID less the
/// region), refusing an ID `known` already has and an existing file.
/// Returns the file's path.
pub fn save_user_preset(
    dir: &Path,
    preset: &Preset,
    known: &Presets,
) -> Result<PathBuf, SaveError> {
    if known.get(preset.id()).is_some() {
        return Err(SaveError::Taken(preset.id().to_owned()));
    }
    let stem = preset
        .id()
        .strip_prefix(preset.region())
        .and_then(|rest| rest.strip_prefix('-'))
        .expect("a preset's ID starts with its region");
    let region_dir = dir.join(preset.region());
    create_private_dir(dir)?;
    create_private_dir(&region_dir)?;
    let path = region_dir.join(format!("{stem}.toml"));

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(SaveError::Exists(path));
        }
        Err(error) => return Err(error.into()),
    };
    file.write_all(preset.to_toml().as_bytes())?;
    file.sync_all()?;
    tracing::info!(preset = preset.id(), "saved a preset");
    Ok(path)
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Every `<region>/<file>.toml` in `dir` as `(path under dir, contents)`,
/// sorted by path, and the files that couldn't be read. A missing `dir` is
/// no presets. Other files are left alone.
fn read_dir(dir: &Path) -> (Vec<(String, String)>, Vec<UserPresetError>) {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let Ok(regions) = fs::read_dir(dir) else {
        return (files, errors);
    };
    for region in regions.flatten() {
        let region_path = region.path();
        if !region_path.is_dir() {
            continue;
        }
        let Some(region_name) = region.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&region_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "toml") || !path.is_file() {
                continue;
            }
            let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let relative = format!("{region_name}/{file_name}");
            match fs::read_to_string(&path) {
                Ok(text) => files.push((relative, text)),
                Err(error) => errors.push(UserPresetError::Read {
                    path: relative,
                    error,
                }),
            }
        }
    }
    files.sort();
    (files, errors)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::preset::PresetParts;

    fn mine(id: &str, name: &str) -> Preset {
        let bundled = Presets::bundled().unwrap();
        let mut mapping = bundled.get("uk-monzo").unwrap().mapping().clone();
        mapping.sub_account_transfer = None;
        Preset::new(PresetParts {
            id: id.to_owned(),
            name: name.to_owned(),
            region: "uk".to_owned(),
            mapping,
            ignore: Vec::new(),
            category_map: BTreeMap::new(),
            sub_account_category: None,
        })
        .unwrap()
    }

    #[test]
    fn a_saved_preset_loads_next_to_the_bundled_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (bundled, errors) = load_user_presets(dir.path()).unwrap();
        assert!(errors.is_empty());
        assert!(bundled.iter().all(|preset| !preset.is_saved()));

        let preset = mine("uk-my-bank", "My bank");
        let path = save_user_preset(dir.path(), &preset, &bundled).unwrap();
        assert_eq!(path, dir.path().join("uk").join("my-bank.toml"));

        let (presets, errors) = load_user_presets(dir.path()).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        let saved = presets.get("uk-my-bank").unwrap();
        assert!(saved.is_saved());
        assert_eq!(saved.name(), "My bank");
        assert_eq!(saved.mapping(), preset.mapping());
        assert_eq!(presets.len(), bundled.len() + 1);
    }

    #[cfg(unix)]
    #[test]
    fn saved_presets_are_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("presets");
        let path = save_user_preset(
            &root,
            &mine("uk-my-bank", "My bank"),
            &Presets::bundled().unwrap(),
        )
        .unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join("uk")), 0o700);
    }

    #[test]
    fn an_id_already_taken_or_a_file_already_there_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = Presets::bundled().unwrap();
        assert!(matches!(
            save_user_preset(dir.path(), &mine("uk-monzo", "Monzo"), &bundled),
            Err(SaveError::Taken(id)) if id == "uk-monzo"
        ));
        save_user_preset(dir.path(), &mine("uk-mine", "Mine"), &bundled).unwrap();
        // The file is there even though `known` doesn't list it.
        assert!(matches!(
            save_user_preset(dir.path(), &mine("uk-mine", "Mine again"), &bundled),
            Err(SaveError::Exists(_))
        ));
    }

    #[test]
    fn a_broken_or_clashing_saved_preset_is_reported_and_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let uk = dir.path().join("uk");
        fs::create_dir_all(&uk).unwrap();
        fs::write(uk.join("broken.toml"), "id = ").unwrap();
        fs::write(uk.join("monzo.toml"), mine("uk-monzo", "Clash").to_toml()).unwrap();
        fs::write(uk.join("good.toml"), mine("uk-good", "Good").to_toml()).unwrap();
        fs::write(uk.join("notes.txt"), "not a preset").unwrap();

        let (presets, errors) = load_user_presets(dir.path()).unwrap();
        assert!(presets.get("uk-good").is_some_and(Preset::is_saved));
        assert!(!presets.get("uk-monzo").unwrap().is_saved());
        assert_eq!(errors.len(), 2, "{errors:?}");
    }
}
