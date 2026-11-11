// Compiles the repository's `presets/` into the binary, so an installed
// Coffer has them without the source tree. Writes `bundled_presets.rs` in
// OUT_DIR: a `&[(path under presets/, contents)]` of every `.toml` file at
// any depth, sorted by path. Where each file sits is checked when the
// presets load (`Presets::from_files`), not here, so a misplaced file fails
// the tests with a clear message instead of being skipped.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let root = manifest.join("../../presets");
    // Cargo watches a directory recursively, so adding, editing or removing
    // a preset rebuilds coffer-import.
    println!("cargo::rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    if root.is_dir() {
        collect(&root, &root, &mut files);
    }
    files.sort();

    let mut out = String::from("&[\n");
    for (relative, absolute) in files {
        writeln!(out, "    ({relative:?}, include_str!({absolute:?})),").expect("in memory");
    }
    out.push_str("]\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("set by Cargo"));
    fs::write(out_dir.join("bundled_presets.rs"), out).expect("OUT_DIR is writable");
}

/// Every `.toml` file under `dir`, as (`/`-separated path under `root`,
/// absolute path).
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("presets/ is readable") {
        let path = entry.expect("presets/ is readable").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            let relative = path
                .strip_prefix(root)
                .expect("under presets/")
                .components()
                .map(|part| part.as_os_str().to_str().expect("UTF-8 preset path"))
                .collect::<Vec<_>>()
                .join("/");
            let absolute = path
                .canonicalize()
                .expect("preset path resolves")
                .to_str()
                .expect("UTF-8 preset path")
                .to_owned();
            files.push((relative, absolute));
        }
    }
}
