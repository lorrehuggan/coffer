# Coffer — Architecture

A local-first personal finance dashboard. Import bank statements, see where money goes,
and talk to your data through any MCP-capable AI client (Claude Code, Claude Desktop, Codex).

Built for one user first; released as open source (UK, US, euro area at launch).

## Principles

- **Local-first.** All data lives in a local SQLite file. The app never needs an account or an API key.
- **Deterministic import.** Parsing statements does not involve a model. Every import is
  verifiable (balance reconciliation where balances are available).
- **AI is external and supervised.** AI clients connect over MCP. They can read through
  narrow query tools and write only to a suggestion inbox the user approves.
- **Contributable without Rust.** Bank presets are data files.

## Decisions

| Area | Decision |
|---|---|
| Stack | Rust 2024, GPUI via `gpui-kit` / `gpui-component` (charts, `DataTable`, sidebar/dock built in) |
| Platforms | Linux + macOS official; Windows compiles in CI, community-supported |
| License | `MIT OR Apache-2.0` |
| Storage | SQLite, WAL mode, file mode 0600, `~/.local/share/coffer` (platform data dir). Not encrypted in v1 (SQLCipher possible later) |
| Money | Integer minor units + ISO 4217 currency per account/transaction. Never floats |
| Currency | Multi-currency; one home currency for reporting. ECB reference rates fetched opt-in and cached (the app's only network call); manual rates as fallback |
| Formats (v1) | CSV (generic mapping + presets), OFX/QFX. QIF, CAMT.053, MT940 later via `Importer` trait |
| PDF | Deferred. Plan: local text extraction + per-layout templates; any model-based extraction opt-in only |
| Categorization | Local rules engine first; AI-proposed categories/rules land in the Inbox for approval; approvals become rules |
| Transfers | Auto-detect opposite-amount pairs across own accounts within N days; user confirms; excluded from income/spend |
| Pots / sub-accounts | Monzo Pots, Starling Spaces, Revolut Vaults etc. are sub-accounts of their parent. A preset rule identifies pot transfers; the importer creates the mirrored pot-side entry, auto-pairs it as a confirmed transfer, and pots count toward net worth (opening balance entered once per pot) |
| AI integration | `coffer mcp` stdio MCP server (`rmcp`). Read-only query tools + inbox-only writes. No account numbers or PII in tool output |

## Architecture

```
crates/
  core/    domain types, schema + migrations, rules engine, transfer matching,
           reporting queries, dedupe. No UI, no AI.
  import/  `Importer` trait; csv (generic + presets), ofx
  mcp/     rmcp server over core
  app/     GPUI dashboard
presets/   <region>/<bank>.toml  — CSV column mappings (data, not code)
fixtures/  redacted sample files; every importer and preset has one
```

Single binary: `coffer` (GUI), `coffer mcp` (stdio MCP server),
`coffer import <file>` (headless import, used from M1).

The GUI and MCP processes share the SQLite file; WAL allows concurrent readers.

## Import pipeline

1. **Detect** format (extension + sniffing) and encoding (UTF-8, Windows-1252, Latin-1).
2. **Map** (CSV): match a preset or use the mapping screen — date column + format,
   optional separate time column, amount as signed column or debit/credit pair
   (prefer a signed column when both exist — e.g. Monzo puts some refunds as
   positive values in "Money Out"), merchant/display name, raw description,
   optional source ID, source category, transaction type, original (local)
   amount + currency, optional balance column, decimal separator
   (`1,234.56` vs `1.234,56`), date order (UK/EU `DD/MM` vs US `MM/DD`).
   A mapping can be saved/exported as a preset.
3. **Normalize** to `{account, date, time?, amount_minor, currency, merchant?,
   description_raw, description_normalized (whitespace-collapsed, uppercased),
   external_id?, source_category?, source_type?, original_amount_minor?,
   original_currency?, balance_after?}`.
4. **Dedupe**: by source ID when the file has one (OFX `FITID`, CSV ID column such as
   Monzo's `Transaction ID`); otherwise by fingerprint
   `(account, date, amount, normalized description, occurrence index)` so identical
   same-day transactions both survive.
5. **Reconcile**: when opening/closing or running balances exist, verify
   `opening + Σ = closing`; failures are flagged, never silently committed. Many CSV
   exports (e.g. Monzo) have no balances; then nothing is reconciled and net worth
   anchors on a user-entered balance for that account.
6. **Preview → commit**: show new / already-imported / flagged counts; commit in one
   DB transaction; record an `import_batch` so an import can be undone.
7. **Post-process**: apply rules (which can match merchant, raw description, source
   category, source type, amount, account), map source categories via the preset,
   run transfer-pair detection, record balances.

### Preset format (sketch, based on a real Monzo export)

```toml
# presets/uk/monzo.toml
id = "uk-monzo-current"
name = "Monzo (current account)"
region = "uk"
currency = "GBP"
encoding = "utf-8"
delimiter = ","
skip_rows = 0
external_id = { column = "Transaction ID" }
date = { column = "Date", format = "%d/%m/%Y" }
time = { column = "Time", format = "%H:%M:%S" }    # optional
amount = { column = "Amount" }                     # or { debit = "Paid out", credit = "Paid in" }
merchant = { column = "Name" }
description = { columns = ["Description"] }
source_category = { column = "Category" }
source_type = { column = "Type" }
original_amount = { column = "Local amount", currency_column = "Local currency" }
note = { column = "Notes and #tags" }
# balance = { column = "Balance" }                 # Monzo exports have none
decimal = "."
ignore = ["Emoji", "Address", "Receipt", "Category split", "Money Out", "Money In"]

[sub_account_transfer]  # rows matching this move money to/from a pot
match = { column = "Type", equals = "Pot transfer" }
sub_account_name = { column = "Name" }          # pot is created on first sight
kind = "pot"

[category_map]          # source category -> our category key
"Groceries" = "food.groceries"
"Eating out" = "food.eating_out"
"Transport" = "transport"
"Transfers" = "transfer"
# ...
```

## Data model (initial)

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|...],
  currency, opening_balance_minor?, opening_date?)` — pots are child accounts of their parent
- `import_batch(id, account_id, source_file, format, preset_id?, imported_at, status)`
- `transaction(id, account_id, batch_id, date, time?, amount_minor, currency, merchant?,
  description_raw, description_normalized, external_id?, fingerprint, source_category?,
  source_type?, original_amount_minor?, original_currency?, category_id?,
  category_source [rule|preset_map|user|ai_approved], transfer_group_id?, note?,
  origin [imported|mirrored])` — `mirrored` rows are the generated pot-side half of a
  sub-account transfer; undoing their import batch removes them too
- `balance_snapshot(account_id, date, balance_minor, source [ofx|csv|manual])`
- `category(id, parent_id?, key, label)` — two levels, stable keys, translatable labels
- `rule(id, priority, match_kind, pattern, amount_min?, amount_max?, account_id?, category_id)`
- `transfer_group(id, status [suggested|confirmed|rejected])`
- `budget(id, category_id, month?, amount_minor, currency)`
- `fx_rate(date, base, quote, rate, source)`
- `suggestion(id, kind [categorization|rule|transfer_pair], payload_json, rationale,
  source_client, created_at, status [pending|approved|rejected])`
- `report(id, period, title, body_markdown, source_client, created_at)`

## MCP surface

**Read tools** (all results in home currency + original currency; no account numbers/PII;
merchant addresses and user notes are not exposed by default):
`list_accounts`, `monthly_summary(from, to)`, `spending_by_category(period)`,
`search_transactions(query, filters, limit)`, `merchant_history(merchant)`,
`recurring_payments()`, `budget_status(month)`, `net_worth_history(from, to)`,
`uncategorized_transactions(limit)`, `list_categories`, `list_rules`, `list_reports`.

**Inbox writes** (never modify transactions/rules directly):
`propose_categorizations`, `propose_rule`, `propose_transfer_pair`, `save_report`.

**Prompts**: `monthly_review(month)` — patterns, anomalies, subscription creep,
budget variance, suggestions; ends by calling `save_report`.

Transaction descriptions are untrusted input (prompt-injection surface); inbox-only
writes are the containment.

## Dashboard (v1)

- **Transactions**: filterable/sortable table, inline category edit, import flow.
- **Inbox**: approve/reject AI suggestions in bulk.
- **Monthly overview**: income vs spending vs net per month; category breakdown with MoM delta.
- **Trends**: category/merchant over time, top merchants, recurring payments.
- **Budgets & net worth**: per-category monthly targets; balances/net worth over time
  (from OFX `LEDGERBAL`, CSV balance column, or manual opening balance).
- **Reviews**: saved MCP reports.

## Milestones

1. **Core + import (headless)** — workspace split, schema/migrations, CSV (generic + preset)
   and OFX importers, dedupe, reconciliation, rules, transfer detection;
   `coffer import` CLI; fixtures + tests.
2. **MCP server** — read tools, inbox writes, `monthly_review` prompt; usable from
   Claude Code/Codex against real data.
3. **GUI foundation** — app shell, transactions table, import + mapping screen, inbox.
4. **Analytics** — monthly overview, trends, recurring, budgets, net worth, FX.
5. **Open-source release** — README, CONTRIBUTING (preset how-to), presets for common
   UK/US/EU banks, CI (Linux/macOS/Windows build), release artifacts (tarball, Nix, .dmg).

## Deferred / open

- PDF import; QIF, CAMT.053, MT940 importers.
- Encrypted DB.
- Streamable-HTTP MCP inside the GUI (live "jump to transaction").
- i18n of UI strings beyond category labels.
