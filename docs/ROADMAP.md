# Roadmap

Coffer is a local-first personal finance dashboard: import bank statements,
see where your money goes, and talk to your data through any MCP-capable AI client
(Claude Code, Claude Desktop, Codex). Design decisions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md); this file tracks what gets built, in what order.

Status legend: ✅ done · 🚧 in progress · ⬜ not started

Tasks are numbered `<milestone>.<n>` (e.g. `M1.3`, `D2.4`; `L` for Later) so they can be
referenced. Numbers are stable: append new tasks rather than renumbering.

Each milestone ends with something usable, and has explicit exit criteria.

---

## M0 — Design ✅

- ✅ M0.1 Design interview; decisions recorded in `docs/ARCHITECTURE.md`
- ✅ M0.2 Dev environment (devenv/Nix, Rust stable, GPUI runtime libs)
- ✅ M0.3 GPUI hello-world template builds (`cargo build`)

---

## Design track — runs alongside M1–M2 🚧

Prove the visual system in Paper and in GPUI before the GUI milestones depend on it.
Specs live in [`DESIGN.md`](DESIGN.md). Coffer's Paper file ("Coffer", created in D1.1) is
separate from "Festive comet", which is a reference file only; D1–D6 are designed in
Coffer's file. D1 and D2 are independent and can run in parallel. D2 needs M1.1 (the
Cargo workspace) so the theme lands in `crates/app`. D3 and D4 need D1; D5 needs D2 and D3.

**D1 — Coffer Paper file: tokens and components**

- ✅ D1.1 Create the Coffer file in Paper (via the Paper MCP); record its URL in `DESIGN.md` →
  "Source of truth"
- ✅ D1.2 Create every token in `DESIGN.md`: the reference set plus the "Proposed" colour,
  font-weight and opacity tokens, with weights at 400/500/600/700. Also names the
  type-scale, spacing, radius and state values the theme already used (96 tokens in all)
- ✅ D1.3 Foundations artboard: colour swatches, type scale, spacing and radii, built from tokens
  only (no literal values). Exception: 1px swatch ring and outline (Paper has no shadow or
  border-width tokens)
- ✅ D1.4 Components artboard: the components `DESIGN.md` uses as specified (buttons in every
  state, list row, transaction row with mono amounts, field, select, toggle, segmented
  control, badge, chip, agent card, …), every node bound to tokens. Added 24 size tokens
  (control heights, geometry, column widths) to make that possible, plus `danger-hover`
  and `danger-pressed` for the solid danger button. Exceptions: border, ring and shadow
  geometry, icon paths, data-driven bar lengths
- ✅ D1.5 Export `design/tokens.json` / `design/tokens.css` from the new file; drop the "Proposed"
  marks; update the D2 drift test, which pins the reference file's 450/550/650 weights.
  122 tokens (hash `572f3e11`); the drift test maps every one, following `var()` aliases.
  "Proposed" now marks only designs not yet drawn in Paper (finance screens, charts)

**D2 — GPUI theme + token showcase (fidelity spike, part 1)**

- ✅ D2.1 Theme module in `crates/app` holding every `DESIGN.md` token as typed values (no
  strings, no literals outside the module)
- ✅ D2.2 Test that checks the Paper-sourced values against `design/tokens.json`, so drift fails
  the build. Both live in `src/theme/tokens.rs` until the M1 workspace split moves them
  to `crates/app`
- ✅ D2.3 Bundle fonts: Inter 400/500/600/700 and Geist Mono 400/600 (static instances, SIL OFL,
  licence files included). Files in `assets/fonts/`, loaded and set on the theme by
  `src/theme/fonts.rs`
- ⬜ D2.4 Map `gpui-component`'s theme onto the tokens; list the components that need a custom
  build instead (likely: toggle, segmented control, agent card)
- ⬜ D2.5 Showcase window (`coffer --showcase`): colour swatches, type scale, buttons in every
  state, list row (default/hover/selected/focus), transaction rows with mono amounts,
  agent card
- ⬜ D2.6 Record findings in `DESIGN.md` → "Implementing in GPUI": alpha colours, 1px hairlines,
  shadows, an outside focus ring, tabular mono figures, animation easing

**D3 — Transactions screen in Paper**

- ⬜ D3.1 1440×900, realistic data: long merchant names, pot transfers, a foreign-currency
  payment, a refund, uncategorised and AI-suggested rows, a multi-selection

**D4 — Overview screen in Paper**

- ⬜ D4.1 Display figures, income-vs-spending chart, category bar list, agent card
- ⬜ D4.2 Adjust tokens (palette, status colours, neutral amounts) if the screens show problems

**D5 — Transactions in GPUI (fidelity spike, part 2)**

- ⬜ D5.1 Build the D3 screen in GPUI with fixture data; compare it against the Paper screenshot
  at 1x and 2x
- ⬜ D5.2 Decision: GPUI can deliver the design → continue; otherwise reconsider the UI toolkit
  before M3

**D6 — Remaining screens**

- ⬜ D6.1 Import, Inbox, Trends, Budgets, Net worth, Reviews, accounts panel
- ⬜ D6.2 Coffer logo / wordmark

**Exit criteria:** tokens live in Paper and in the GPUI theme with a drift test;
Transactions and Overview are designed; the GPUI build of Transactions matches the Paper
screen closely enough that the designer signs off.

---

## M1 — Core + import (headless) ⬜

Import real statements into a local database from the command line.

- ⬜ M1.1 Convert to a Cargo workspace: `crates/core`, `crates/import`, `crates/mcp`, `crates/app`
- ✅ M1.2 Add `LICENSE-MIT`, `LICENSE-APACHE`; set `license = "MIT OR Apache-2.0"`
- ⬜ M1.3 SQLite setup: platform data dir, WAL mode, 0600 permissions, versioned migrations
- ⬜ M1.4 Schema: accounts, import batches, transactions, balance snapshots, categories,
  rules, transfer groups, budgets, FX rates, suggestions, reports
- ⬜ M1.5 Money type: integer minor units + ISO 4217 currency (no floats anywhere)
- ⬜ M1.6 `Importer` trait (detect → parse → normalize)
- ⬜ M1.7 CSV importer: encoding detection, delimiter, header/skip rows, date formats
  (`DD/MM` vs `MM/DD`), separate time column, decimal style (`1,234.56` vs `1.234,56`),
  signed amount or debit/credit columns, merchant vs raw description, source ID,
  source category/type, original (local) amount + currency, optional balance column
- ⬜ M1.8 TOML preset format + loader (`presets/<region>/<bank>.toml`), including
  source-category → category mapping
- ⬜ M1.9 First preset: `presets/uk/monzo.toml` with a redacted fixture (built from a real export)
- ⬜ M1.10 OFX/QFX importer (SGML 1.x and XML 2.x), `FITID` IDs, `LEDGERBAL` balances
- ⬜ M1.11 Deduplication: source ID when present (OFX `FITID`, CSV ID column);
  fingerprint with occurrence index otherwise
- ⬜ M1.12 Balance reconciliation where the file has balances; failed imports flagged, not
  committed silently. Manual balance anchor for accounts whose exports have none
- ⬜ M1.13 Import batches with undo
- ⬜ M1.14 Default two-level category set with stable keys
- ⬜ M1.15 Rules engine (contains / regex / amount range / account), priority ordering
- ⬜ M1.16 Transfer-pair detection (opposite amounts, own accounts, N-day window)
- ⬜ M1.17 Sub-accounts (Monzo Pots, Starling Spaces, Revolut Vaults): preset-driven detection,
  auto-created pot accounts, mirrored pot-side entries auto-paired as transfers
- ⬜ M1.18 CLI: `coffer import <file> [--account] [--preset] [--dry-run]`,
  `coffer accounts`, `coffer undo-import <batch>`
- ⬜ M1.19 Redacted fixtures + tests for every importer path

**Exit criteria:** your own CSV/OFX exports import without duplicates on
re-import, reconcile where balances exist, and get categorized by rules.

---

## M2 — MCP server ⬜

Analyse real data from Claude Code or Codex before any GUI exists.

- ⬜ M2.1 `coffer mcp` stdio server (`rmcp`), read-only DB connection for queries
- ⬜ M2.2 Read tools: `list_accounts`, `monthly_summary`, `spending_by_category`,
  `search_transactions`, `merchant_history`, `recurring_payments`, `budget_status`,
  `net_worth_history`, `uncategorized_transactions`, `list_categories`,
  `list_rules`, `list_reports`
- ⬜ M2.3 Inbox-only writes: `propose_categorizations`, `propose_rule`,
  `propose_transfer_pair`, `save_report`
- ⬜ M2.4 `monthly_review` prompt
- ⬜ M2.5 PII guard: no account numbers or personal details in any tool output (tested)
- ⬜ M2.6 Recurring-payment / subscription detection in core
- ⬜ M2.7 CLI inbox until the GUI exists: `coffer inbox [approve|reject]`
- ⬜ M2.8 Setup docs for Claude Code, Claude Desktop, and Codex
- ⬜ M2.9 MCP tool tests against fixture databases

**Exit criteria:** ask "where did my money go last month?" in Claude Code and get
an answer backed by visible tool calls; approve AI-suggested categories from the inbox.

---

## M3 — GUI foundation ⬜

- ⬜ M3.1 App shell: sidebar navigation, window state, built on the D2 theme. Every screen in
  M3–M4 follows its Paper design from the design track; no stock component styling
- ⬜ M3.2 Accounts management (create, edit, opening balance; pots shown under their parent)
- ⬜ M3.3 Transactions view: virtualized `DataTable`, filter/sort/search, inline category edit
- ⬜ M3.4 Import flow: pick file → detect/choose preset → column-mapping screen with live
  preview → dedupe/reconcile summary → commit
- ⬜ M3.5 Save/export a mapping as a preset
- ⬜ M3.6 Inbox: bulk approve/reject AI suggestions
- ⬜ M3.7 Rules and categories editors
- ⬜ M3.8 Transfer-pair confirmation

**Exit criteria:** day-to-day use (import, categorize, review inbox) without the CLI.

---

## M4 — Analytics ⬜

- ⬜ M4.1 Monthly overview: income vs spending vs net (bar/line), category breakdown, MoM change
- ⬜ M4.2 Trends: category and merchant over time, top merchants
- ⬜ M4.3 Recurring payments / subscriptions view
- ⬜ M4.4 Budgets: per-category monthly targets with progress
- ⬜ M4.5 Net worth: balances over time across accounts
- ⬜ M4.6 Multi-currency reporting in home currency; opt-in ECB rate fetch + manual rates
- ⬜ M4.7 Reviews: browse saved MCP reports

**Exit criteria:** the dashboard answers "how am I doing?" at a glance.

---

## M5 — Open-source release (v0.1) ⬜

- ⬜ M5.1 README (screenshots, install, MCP setup), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
- ⬜ M5.2 "Add your bank" guide: preset TOML + redacted fixture, no Rust required
- ⬜ M5.3 Presets for common UK, US, and euro-area banks
- ⬜ M5.4 Issue templates (bank preset request, bug, importer request)
- ⬜ M5.5 CI: fmt, clippy, tests; build on Linux, macOS, Windows (best-effort)
- ⬜ M5.6 Release artifacts: Linux tarball + Nix flake (+ AppImage), macOS `.dmg`
- ⬜ M5.7 Data location, backup, and privacy documentation

**Exit criteria:** a stranger in the UK, US, or euro area can install it, import
their bank's export, and connect an AI client by following the README.

---

## Later

Good candidates for contributors are marked 🙋.

- 🙋 L.1 More bank presets
- 🙋 L.2 QIF importer
- 🙋 L.3 CAMT.053 (ISO 20022) importer
- 🙋 L.4 MT940 importer
- L.5 PDF import: local text extraction + per-layout templates; model-assisted
  extraction strictly opt-in
- L.6 Encrypted database (SQLCipher)
- L.7 Streamable-HTTP MCP inside the running GUI (e.g. "jump to transaction")
- L.8 Sankey view of income → categories
- 🙋 L.9 UI translations
- L.10 Windows as an officially supported platform
