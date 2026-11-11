// `refinery::embed_migrations!` reads `migrations/` while the crate compiles,
// but a proc macro can't tell Cargo about the directory. Without this, adding
// a migration file wouldn't rebuild coffer-core and the new migration would be
// silently missing from the binary.
fn main() {
    println!("cargo::rerun-if-changed=migrations");
}
