# Coffer

A local-first personal finance dashboard. Coffer imports bank exports (CSV, OFX/QFX)
into a local SQLite database, shows them as tables and charts in a native desktop app,
and lets AI clients (Claude Code, Claude Desktop, Codex) query your data through an
MCP server.

> **Status:** early development (milestone M0). The app is currently a GPUI
> hello-world; see [`docs/ROADMAP.md`](docs/ROADMAP.md) for what comes next.

## Principles

- **Local-first.** Your data stays in a SQLite file on your machine. The only network
  call is an opt-in exchange-rate fetch (ECB). No telemetry.
- **Deterministic import.** Imports are previewed, deduplicated, reconciled against
  balances where possible, committed atomically, and undoable as a batch.
- **Nothing silently dropped.** Unparseable rows and failed reconciliations are
  flagged, not skipped.
- **Safe AI access.** MCP tools are read-only apart from an inbox of suggestions and
  reports, and never return account numbers, names, or addresses.
- **Exact money.** Amounts are integer minor units plus an ISO 4217 currency code.

## Planned usage

```sh
coffer                 # open the dashboard
coffer import <file>   # import a bank export
coffer accounts        # list accounts
coffer undo-import     # undo an import batch
coffer inbox           # review MCP suggestions
coffer mcp             # run the MCP server over stdio
```

## Development

The dev environment is managed with [devenv](https://devenv.sh) (Nix) and loaded via
direnv. It provides the Rust toolchain and the runtime libraries GPUI needs.

```sh
direnv allow           # or prefix commands with: devenv shell --
cargo build
cargo run
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

A [lefthook](https://lefthook.dev) `pre-push` hook (installed automatically when the
devenv shell loads) runs fmt, clippy, tests, and docs, and blocks pushing bank exports
or databases outside `fixtures/`. Run it by hand with `lefthook run pre-push --force`.

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md): design, data model, import pipeline,
  MCP surface
- [`docs/ROADMAP.md`](docs/ROADMAP.md): milestones and status
- [`docs/DESIGN.md`](docs/DESIGN.md): visual design system and tokens
- [`AGENTS.md`](AGENTS.md): guidelines for AI coding agents

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
