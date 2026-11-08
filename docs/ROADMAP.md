# Roadmap

Coffer is a local-first personal finance dashboard: import bank statements,
see where your money goes, and talk to your data through any MCP-capable AI client
(Claude Code, Claude Desktop, Codex). Design decisions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md); this file tracks what gets built, in what order.

Status legend: ✅ done · 🚧 in progress · ⬜ not started

Each milestone ends with something usable, and has explicit exit criteria.

---

## M0 — Design ✅

- ✅ Design interview; decisions recorded in `docs/ARCHITECTURE.md`
- ✅ Dev environment (devenv/Nix, Rust stable, GPUI runtime libs)
- ✅ GPUI hello-world template builds (`cargo build`)

---

## Design track — runs alongside M1–M2 ⬜

Prove the visual system in Paper and in GPUI before the GUI milestones depend on it.
Specs live in [`DESIGN.md`](DESIGN.md). D1 and D2 are independent and can run in
parallel. D2 needs M1's first item (the Cargo workspace) so the theme lands in
`crates/app`. D5 needs D2 and D3.

**D1 — Tokens in Paper**

- ⬜ Add the proposed colour, font-weight and opacity tokens from `DESIGN.md`; change the
  weight tokens to 400/500/600 and add 700
- ⬜ Bind the existing artboard nodes to tokens
- ⬜ Re-export `design/tokens.json` / `design/tokens.css`; drop the "Proposed" marks for
  everything now in Paper

**D2 — GPUI theme + token showcase (fidelity spike, part 1)**

- ⬜ Theme module in `crates/app` holding every `DESIGN.md` token as typed values (no
  strings, no literals outside the module)
- ⬜ Test that checks the Paper-sourced values against `design/tokens.json`, so drift fails
  the build
- ⬜ Bundle fonts: Inter 400/500/600/700 and Geist Mono 400/600 (static instances, SIL OFL,
  licence files included)
- ⬜ Map `gpui-component`'s theme onto the tokens; list the components that need a custom
  build instead (likely: toggle, segmented control, agent card)
- ⬜ Showcase window (`coffer --showcase`): colour swatches, type scale, buttons in every
  state, list row (default/hover/selected/focus), transaction rows with mono amounts,
  agent card
- ⬜ Record findings in `DESIGN.md` → "Implementing in GPUI": alpha colours, 1px hairlines,
  shadows, an outside focus ring, tabular mono figures, animation easing

**D3 — Transactions screen in Paper**

- ⬜ 1440×900, realistic data: long merchant names, pot transfers, a foreign-currency
  payment, a refund, uncategorised and AI-suggested rows, a multi-selection

**D4 — Overview screen in Paper**

- ⬜ Display figures, income-vs-spending chart, category bar list, agent card
- ⬜ Adjust tokens (palette, status colours, neutral amounts) if the screens show problems

**D5 — Transactions in GPUI (fidelity spike, part 2)**

- ⬜ Build the D3 screen in GPUI with fixture data; compare it against the Paper screenshot
  at 1x and 2x
- ⬜ Decision: GPUI can deliver the design → continue; otherwise reconsider the UI toolkit
  before M3

**D6 — Remaining screens**

- ⬜ Import, Inbox, Trends, Budgets, Net worth, Reviews, accounts panel
- ⬜ Coffer logo / wordmark

**Exit criteria:** tokens live in Paper and in the GPUI theme with a drift test;
Transactions and Overview are designed; the GPUI build of Transactions matches the Paper
screen closely enough that the designer signs off.

---

## M1 — Core + import (headless) ⬜

Import real statements into a local database from the command line.

- ⬜ Convert to a Cargo workspace: `crates/core`, `crates/import`, `crates/mcp`, `crates/app`
- ✅ Add `LICENSE-MIT`, `LICENSE-APACHE`; set `license = "MIT OR Apache-2.0"`
- ⬜ SQLite setup: platform data dir, WAL mode, 0600 permissions, versioned migrations
- ⬜ Schema: accounts, import batches, transactions, balance snapshots, categories,
  rules, transfer groups, budgets, FX rates, suggestions, reports
- ⬜ Money type: integer minor units + ISO 4217 currency (no floats anywhere)
- ⬜ `Importer` trait (detect → parse → normalize)
- ⬜ CSV importer: encoding detection, delimiter, header/skip rows, date formats
  (`DD/MM` vs `MM/DD`), separate time column, decimal style (`1,234.56` vs `1.234,56`),
  signed amount or debit/credit columns, merchant vs raw description, source ID,
  source category/type, original (local) amount + currency, optional balance column
- ⬜ TOML preset format + loader (`presets/<region>/<bank>.toml`), including
  source-category → category mapping
- ⬜ First preset: `presets/uk/monzo.toml` with a redacted fixture (built from a real export)
- ⬜ OFX/QFX importer (SGML 1.x and XML 2.x), `FITID` IDs, `LEDGERBAL` balances
- ⬜ Deduplication: source ID when present (OFX `FITID`, CSV ID column);
  fingerprint with occurrence index otherwise
- ⬜ Balance reconciliation where the file has balances; failed imports flagged, not
  committed silently. Manual balance anchor for accounts whose exports have none
- ⬜ Import batches with undo
- ⬜ Default two-level category set with stable keys
- ⬜ Rules engine (contains / regex / amount range / account), priority ordering
- ⬜ Transfer-pair detection (opposite amounts, own accounts, N-day window)
- ⬜ Sub-accounts (Monzo Pots, Starling Spaces, Revolut Vaults): preset-driven detection,
  auto-created pot accounts, mirrored pot-side entries auto-paired as transfers
- ⬜ CLI: `coffer import <file> [--account] [--preset] [--dry-run]`,
  `coffer accounts`, `coffer undo-import <batch>`
- ⬜ Redacted fixtures + tests for every importer path

**Exit criteria:** your own CSV/OFX exports import without duplicates on
re-import, reconcile where balances exist, and get categorized by rules.

---

## M2 — MCP server ⬜

Analyse real data from Claude Code or Codex before any GUI exists.

- ⬜ `coffer mcp` stdio server (`rmcp`), read-only DB connection for queries
- ⬜ Read tools: `list_accounts`, `monthly_summary`, `spending_by_category`,
  `search_transactions`, `merchant_history`, `recurring_payments`, `budget_status`,
  `net_worth_history`, `uncategorized_transactions`, `list_categories`,
  `list_rules`, `list_reports`
- ⬜ Inbox-only writes: `propose_categorizations`, `propose_rule`,
  `propose_transfer_pair`, `save_report`
- ⬜ `monthly_review` prompt
- ⬜ PII guard: no account numbers or personal details in any tool output (tested)
- ⬜ Recurring-payment / subscription detection in core
- ⬜ CLI inbox until the GUI exists: `coffer inbox [approve|reject]`
- ⬜ Setup docs for Claude Code, Claude Desktop, and Codex
- ⬜ MCP tool tests against fixture databases

**Exit criteria:** ask "where did my money go last month?" in Claude Code and get
an answer backed by visible tool calls; approve AI-suggested categories from the inbox.

---

## M3 — GUI foundation ⬜

- ⬜ App shell: sidebar navigation, window state, built on the D2 theme. Every screen in
  M3–M4 follows its Paper design from the design track; no stock component styling
- ⬜ Accounts management (create, edit, opening balance; pots shown under their parent)
- ⬜ Transactions view: virtualized `DataTable`, filter/sort/search, inline category edit
- ⬜ Import flow: pick file → detect/choose preset → column-mapping screen with live
  preview → dedupe/reconcile summary → commit
- ⬜ Save/export a mapping as a preset
- ⬜ Inbox: bulk approve/reject AI suggestions
- ⬜ Rules and categories editors
- ⬜ Transfer-pair confirmation

**Exit criteria:** day-to-day use (import, categorize, review inbox) without the CLI.

---

## M4 — Analytics ⬜

- ⬜ Monthly overview: income vs spending vs net (bar/line), category breakdown, MoM change
- ⬜ Trends: category and merchant over time, top merchants
- ⬜ Recurring payments / subscriptions view
- ⬜ Budgets: per-category monthly targets with progress
- ⬜ Net worth: balances over time across accounts
- ⬜ Multi-currency reporting in home currency; opt-in ECB rate fetch + manual rates
- ⬜ Reviews: browse saved MCP reports

**Exit criteria:** the dashboard answers "how am I doing?" at a glance.

---

## M5 — Open-source release (v0.1) ⬜

- ⬜ README (screenshots, install, MCP setup), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
- ⬜ "Add your bank" guide: preset TOML + redacted fixture, no Rust required
- ⬜ Presets for common UK, US, and euro-area banks
- ⬜ Issue templates (bank preset request, bug, importer request)
- ⬜ CI: fmt, clippy, tests; build on Linux, macOS, Windows (best-effort)
- ⬜ Release artifacts: Linux tarball + Nix flake (+ AppImage), macOS `.dmg`
- ⬜ Data location, backup, and privacy documentation

**Exit criteria:** a stranger in the UK, US, or euro area can install it, import
their bank's export, and connect an AI client by following the README.

---

## Later

Good candidates for contributors are marked 🙋.

- 🙋 More bank presets
- 🙋 QIF importer
- 🙋 CAMT.053 (ISO 20022) importer
- 🙋 MT940 importer
- PDF import: local text extraction + per-layout templates; model-assisted
  extraction strictly opt-in
- Encrypted database (SQLCipher)
- Streamable-HTTP MCP inside the running GUI (e.g. "jump to transaction")
- Sankey view of income → categories
- 🙋 UI translations
- Windows as an officially supported platform
