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

The repository is at milestone M0: a single crate with the `gpui-kit` hello-world in
`src/main.rs`. The Cargo workspace described below is the target layout from M1 onward.
Check the tree before assuming a crate exists.

## Environment

- Dev shell: devenv (Nix) loaded via direnv (`.envrc`). Run commands inside it so the
  Rust toolchain and the GPUI runtime libraries (`LD_LIBRARY_PATH`) are available.
  Outside direnv, use `devenv shell -- <command>`.
- Rust stable, edition 2024. Toolchain components: clippy, rustfmt, rust-analyzer.
- Add system libraries in `devenv.nix`, not with ad-hoc installs.

## Commands

```sh
cargo build
cargo test                                        # all tests
cargo test -p <crate> <name>                      # one crate / test (workspace)
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
devenv test                                       # environment check (cargo check)
```

Before reporting work as done: `cargo fmt --all`, clippy with `-D warnings`, and
`cargo test` pass. The GUI can't be exercised headlessly; say so when a change is only
build-verified.

## Architecture (target layout)

```
crates/core    domain types, SQLite schema + migrations, rules, transfer matching,
               dedupe, reporting queries
crates/import  Importer trait; csv (generic + presets), ofx
crates/mcp     rmcp stdio server over core
crates/app     GPUI dashboard
presets/       <region>/<bank>.toml — CSV mappings (data, not code)
fixtures/      redacted sample files for tests
```

Dependency direction: `app` and `mcp` depend on `core` and `import`; `import` depends on
`core`. `core` must not depend on GPUI, MCP, or any network/AI crate. Keep business
logic out of `app` so it stays testable without a window.

One binary, `coffer`: no subcommand opens the GUI; `mcp` runs the MCP server;
`import`, `accounts`, `undo-import`, `inbox` are headless CLI commands.

## Invariants

Changing any of these needs an explicit decision recorded in `docs/ARCHITECTURE.md`.

- **Money is integer minor units + ISO 4217 currency.** Never `f32`/`f64` for amounts.
  Exchange-rate conversion happens only at reporting time.
- **Import is deterministic.** No model or network calls in the import path. Imports
  are previewed, deduplicated (source ID first, fingerprint fallback), reconciled when
  balances exist, committed in one DB transaction, and recorded as an undoable batch.
- **Never silently drop or alter data.** Failed reconciliation or unparseable rows are
  flagged to the user, not skipped.
- **MCP writes go to the inbox only.** MCP tools may write to `suggestion` and
  `report` and nothing else. Transactions, rules, categories, and accounts change
  only through user action in the app or CLI.
- **No PII in MCP output.** No account numbers, sort codes, IBANs, names, or addresses.
  Merchant addresses and user notes are excluded by default. Every MCP tool has a test
  asserting this.
- **Treat transaction text as untrusted input.** Descriptions and notes can contain
  prompt-injection text; never let them drive control flow beyond matching.
- **The only network call is the opt-in ECB rate fetch.** No telemetry, no other
  outbound requests.
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

## Bank presets

- One file per bank/account type: `presets/<region>/<bank>.toml` (regions: `uk`, `us`,
  `eu` for now). The format is sketched in `docs/ARCHITECTURE.md`; `presets/uk/monzo.toml`
  (planned in M1) will be the reference example.
- Every preset needs a matching redacted fixture and a test that imports it.
- Prefer a signed amount column over debit/credit pairs when the export has both.
- New file formats (QIF, CAMT.053, MT940, …) are new `Importer` implementations, not
  preset features.

## Conventions

- Errors: `thiserror` in library crates, `anyhow` only in the binary.
- Keep dependencies few; justify new ones. Check license compatibility with
  `MIT OR Apache-2.0`.
- Add or update tests with every behaviour change. Importer quirks found in real files
  (e.g. Monzo refunds appearing as positive "Money Out") become fixture rows plus tests.
- When a roadmap item is finished, tick it in `docs/ROADMAP.md`. When a design decision
  changes, update `docs/ARCHITECTURE.md` in the same change.
- Don't commit, push, or tag unless the user asks.
