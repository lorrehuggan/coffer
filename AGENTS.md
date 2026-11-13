# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, etc.) working in this repository.

Coffer is a local-first personal finance dashboard: a Rust + GPUI desktop app that
imports bank exports (CSV, OFX/QFX) into a local SQLite database, shows them as tables and
charts, and exposes the data to AI clients through an MCP server.

- **Design and rationale:** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — read before changing
  architecture, schema, import behaviour, or the MCP surface.
- **What to build next:** [`docs/ROADMAP.md`](docs/ROADMAP.md) — milestones and status.
- **Visual design system:** [`docs/DESIGN.md`](docs/DESIGN.md) (sourced from the Paper
  file) — read before any UI work. Use its tokens and component specs; never invent
  colours, sizes, or radii, and never ship stock `gpui-component` styling. Missing values
  get designed in Paper first.

## Current state

M0 and the design track (D1–D6) are done and signed off; every M1 item is done,
M1.9 checked against a real Monzo export. The Cargo
workspace below exists (M1.1). `crates/core` has the database setup (`db`, `paths`: data
dir, WAL, permissions, migrations; M1.3), the initial schema
(`crates/core/migrations/V1__initial_schema.sql`, M1.4) and the
money types (`money`: `Currency`, `Money`, exact decimal parsing; M1.5) and the
normalized import output (`statement`: `Statement`, `StatementLine`, flagged rows; M1.6)
and categories (`category`: `CategoryKey`, M1.8; `CategoryKind`, `DEFAULT_CATEGORIES`
and `CategoryTree` via `Database::categories`, seeded by `V2__default_categories.sql`,
M1.14) and rules (`rule`: validated `RuleDefinition`, `Pattern`, `RuleAction`; the
store and the `RuleSet` engine; M1.15, every action kind since M1.24) and transfer-pair detection
(`transfer`, M1.16) and sub-accounts (`sub_account`: pots found by key,
auto-created, rows mirrored and paired as confirmed transfers; M1.17, migration
`V4__sub_account_key.sql`) and the import commit (`import`: preview → backup → commit →
undo with dedupe and reconciliation, then preset categories, the pot mirror, merchant
names, rules and transfer pairing in the same DB transaction; `balance`: balance checks; `account`:
minimal accounts and entered balances; `id`: row IDs; M1.11–M1.13, migration
`V3__import_flags.sql`) and local backups (`backup`: `VACUUM INTO` snapshots in
`<data dir>/backups`, rotation, checked restore; taken before import commits and
migrations; M1.28) and splits (`split`: lines summing exactly to the parent, the
`ledger_line` view; M1.20, `V5__transaction_split.sql`) and tags and user notes (`tag`,
`note`; private tags; M1.21, `V6__tags.sql`) and manual rows and exclusions (`manual`,
`exclusion`; M1.22, no migration) and merchant names (`merchant`: deterministic
clean-up, aliases, `merchant_display_source`; run in the import commit before rules;
M1.23, `V7__merchant_alias.sql`) and change batches (`change`: recorded, undoable
changes to user-facing fields, `dry_run` previews; `V8__change_batch.sql`) and every rule
action at import plus applying a rule to history (`rule::preview_apply`/`commit_apply`;
M1.24) and refunds, reimbursements and shared costs (`link`; `V9__transaction_link.sql`,
which redefines `ledger_line`; M1.25) and the possible-duplicates queue (`duplicate`;
`V10__duplicate_candidate.sql`; M1.26) and balance checks and lock dates
(`reconciliation`; M1.27) and export (`export`; M1.29). The import commit also detects
possible duplicates and suggests links. Undo and manual deletes clear split lines, tag
links, change items, links and duplicate candidates. M1.20–M1.27 have no CLI commands
yet. `crates/import/tests/m1_exit.rs` walks the M1 exit criteria end to end.
`crates/import` has the `Importer` trait, `SourceFile` and detection (M1.6) and the CSV
importer (`csv`: `CsvImporter` over a `CsvMapping`; M1.7) and the preset loader
(`preset`: `Preset`, `Presets::bundled`; M1.8) and the first preset, `presets/uk/monzo.toml`
(M1.9, with `[sub_account_transfer]` for pots since M1.17) and the OFX/QFX importer
(`ofx`: `OfxImporter`, own SGML/XML reader; M1.10); synthetic fixtures are in
`fixtures/csv/` (test-only mappings in `fixtures/csv/mappings/`), `fixtures/presets/` and
`fixtures/ofx/`, with end-to-end importer-path tests in `crates/import/tests/` and
`crates/app/tests/` (M1.19); `crates/mcp` is a stub. The GUI is in `crates/app` (package `coffer-app`, lib
`coffer_app`, binary `coffer`); the GUI doesn't open the database yet. `coffer` opens the
Transactions screen on fixture data (`crates/app/src/transactions.rs`), and
`coffer --showcase` opens the component showcase. The CLI (`coffer_app::cli`, M1.18) has
`import`, `accounts` (`list`, `add`), `undo-import`, `backup` (`--list`) and
`restore <file> [--yes]` (M1.28) and `export --format csv|json` (M1.29); the other
commands don't exist yet.
`cargo run --example snapshot -- <dir>` renders both headlessly to PNG for comparison with
Paper. Shared files stay at the root: `assets/` (fonts, brand) and `design/` (tokens).
Check the tree before assuming a module exists.

## Environment

- Dev shell: devenv (Nix) loaded via direnv (`.envrc`). Run commands inside it so the
  Rust toolchain and the GPUI runtime libraries (`LD_LIBRARY_PATH`) are available.
  Outside direnv, use `devenv shell -- <command>`.
- Rust stable, edition 2024. Toolchain components: clippy, rustfmt, rust-analyzer.
- Add system libraries in `devenv.nix`, not with ad-hoc installs.
- Set `COFFER_DATA_DIR` to a scratch directory when running Coffer against test data,
  so it never touches the real database in the platform data dir.
- Paper (design work): the Paper MCP server is the local Paper Desktop app, configured
  as `paper` and loaded lazily. Run `mcp_connect` with `paper` before any Paper task. Its
  tools (`mcp_paper_*`) only appear on the next turn, so connect, then end the turn or
  ask the user before using them. Most tools require `fileId` even where the description
  says it's optional; Coffer's is in `docs/DESIGN.md` → "Source of truth". If the tools
  fail, ask the user to check that Paper Desktop is running, rather than working around it.

## Commands

```sh
cargo build
cargo test                                        # all tests (whole workspace)
cargo test -p <package> <name>                    # one package / test, e.g. -p coffer-core
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
devenv test                                       # environment check (cargo check)
lefthook run pre-push --force                      # everything the pre-push hook runs
```

Before reporting work as done: `cargo fmt --all`, clippy with `-D warnings`, and
`cargo test` pass. The GUI can't be exercised headlessly; say so when a change is only
build-verified.

Git hooks are managed by lefthook (`lefthook.yml`, installed on devenv shell entry).
Only `pre-push` is used: data guard (no statements/databases outside `fixtures/`),
`--locked` lockfile check, fmt, clippy `-D warnings`, tests, and docs with
`RUSTDOCFLAGS=-D warnings`. There is no CI yet, so this hook is the gate; don't bypass
it with `--no-verify`.

## Architecture

```
crates/core    domain types, SQLite schema + migrations, rules, transfer matching,
               dedupe, reporting queries, schedules, backups and export
crates/import  Importer trait; csv (generic + presets), ofx
crates/mcp     rmcp stdio server over core
crates/app     GPUI dashboard
presets/       <region>/<bank>.toml — CSV mappings (data, not code)
fixtures/      redacted sample files for tests
```

Dependency direction: `app` and `mcp` depend on `core` and `import`; `import` depends on
`core`. `core` must not depend on GPUI, MCP, or any network/AI crate. Keep business
logic out of `app` so it stays testable without a window.

One binary, `coffer`: no subcommand opens the GUI; `mcp` runs the MCP server
(`coffer mcp --client <name>` names the client, which selects its access level);
`import`, `accounts`, `undo-import`, `inbox`, `backup`, `restore`, `export` are headless
CLI commands.

## Invariants

Changing any of these needs an explicit decision recorded in `docs/ARCHITECTURE.md`.

- **Money is integer minor units + ISO 4217 currency.** Never `f32`/`f64` for amounts.
  Exchange-rate conversion happens only at reporting time.
- **Import is deterministic.** No model or network calls in the import path. Imports
  are previewed, deduplicated (source ID first, fingerprint fallback), reconciled when
  balances exist, committed in one DB transaction, and recorded as an undoable batch.
- **Never silently drop or alter data.** Failed reconciliation or unparseable rows are
  flagged to the user, not skipped. Imported source fields (date, amount, raw
  description, external ID) are never edited: corrections are undoing a batch, excluding
  a row with a recorded reason (it stays visible under a filter), or editing user-facing
  fields (category, display merchant, tags, notes, splits). Changes to many existing
  transactions are previewed and committed as an undoable `change_batch`.
- **MCP writes go to the inbox only.** MCP tools may write to `suggestion` and
  `report` and nothing else. Transactions, rules, categories, and accounts change
  only through user action in the app or CLI. The server's own `mcp_access_log` audit
  record is not a tool write (decision record in `docs/ARCHITECTURE.md`, 2026-11-09).
  Approval happens only in Coffer's own UI or CLI, never through MCP elicitation,
  sampling, or MCP Apps.
- **No PII in MCP output.** No account numbers, sort codes, IBANs, names, or addresses.
  Merchant addresses and user notes are excluded by default; share labels, private tags,
  and attachments are never exposed. Per-client access levels (`aggregate`,
  `pseudonymous`, `standard`) are enforced by not registering tools above the level.
  Every MCP tool has a test asserting this.
- **Treat transaction text as untrusted input.** Descriptions and notes can contain
  prompt-injection text; never let them drive control flow beyond matching. MCP returns
  raw text only in the sanitised, labelled `description_untrusted` field. AI-written
  reports and rationales are untrusted when read back.
- **The only network call is the opt-in ECB rate fetch.** No telemetry, no other
  outbound requests. Saved reports render without HTML, images, or live links, so they
  can't trigger a request.
- **Schema changes are new migrations.** Never edit a migration that has shipped.

## Personal financial data

- Never commit real statements or real-derived data. Real exports live outside the repo
  or in the gitignored `samples/` directory.
- Fixtures in `fixtures/` must be synthetic or fully redacted: fake merchants, IDs,
  amounts, notes, and addresses; keep only the structure (headers, formats, quirks).
- Don't read real statements wholesale into an AI context. Inspect structure instead:
  headers, row counts, value shapes (digits → `9`, letters → `a`), and distinct values
  of low-cardinality columns. Ask the user before reading actual merchant names,
  amounts, or notes.
- Never log transaction contents at `info` level or above.
- Backups, exports and attachments are personal data too: they live in the platform data
  dir or wherever the user saves them, never in the repo.

## Bank presets

- One file per bank/account type: `presets/<region>/<bank>.toml` (regions: `uk`, `us`,
  `eu` for now). The field reference is `presets/README.md`, the decisions are in
  `docs/ARCHITECTURE.md` → "Presets"; `presets/uk/monzo.toml` (M1.9) is the
  reference example. Every `.toml` there is compiled in (`crates/import/build.rs`).
- Every preset needs a matching redacted fixture at `fixtures/presets/<region>/<bank>.*`
  (a test checks it's there and imports) and a test that checks its rows.
- Prefer a signed amount column over debit/credit pairs when the export has both.
- New file formats (QIF, CAMT.053, MT940, …) are new `Importer` implementations, not
  preset features.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` only in the binary and its CLI layer
  (`coffer_app::cli`, in the lib so it's testable).
- Keep dependencies few; justify new ones. Check license compatibility with
  `MIT OR Apache-2.0`.
- Add or update tests with every behaviour change. Importer quirks found in real files
  (e.g. Monzo refunds appearing as positive "Money Out") become fixture rows plus tests.
- When a roadmap item is finished, tick it in `docs/ROADMAP.md`. When a design decision
  changes, update `docs/ARCHITECTURE.md` in the same change.
- Don't commit, push, or tag unless the user asks.
