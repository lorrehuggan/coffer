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
| Categorization | Local rules engine first; AI-proposed categories/rules land in the Inbox for approval; approvals become rules. The app also files `rule` suggestions itself (source `coffer`) after repeated identical manual recategorisations: deterministic counting, no ML. Learned classifiers are not used |
| Transfers | Auto-detect opposite-amount pairs across own accounts within N days; user confirms; excluded from income/spend |
| Pots / sub-accounts | Monzo Pots, Starling Spaces, Revolut Vaults etc. are sub-accounts of their parent. A preset rule identifies pot transfers; the importer creates the mirrored pot-side entry, auto-pairs it as a confirmed transfer, and pots count toward net worth (opening balance entered once per pot) |
| AI integration | `coffer mcp` stdio MCP server (`rmcp`). Read-only query tools + inbox-only writes. No account numbers or PII in tool output |
| Splits | A transaction can be split into lines that sum exactly to its amount, in its currency (enforced in core, in the same DB transaction as the edit). Reports, budgets and MCP read the `ledger_line` view: one row per split line, or the transaction itself when it isn't split |
| Tags and notes | Many tags per transaction, alongside categories. Tag names are user text: MCP sees them only at `standard` access, and never for tags marked private. The imported note stays read-only; the user's note is a separate field. Neither note is exposed over MCP by default |
| Manual entries and corrections | Manual rows have `origin = manual` and no import batch. Imported source fields (date, amount, raw description, external ID) are never edited: a bad import is undone, or a row is excluded with a recorded reason and stays visible under an "excluded" filter. User-facing fields (category, display merchant, tags, notes, splits) are editable. Manual rows are the user's own data: every field is editable and they can be deleted. Exclusion reasons and their effect on balances: see the decision record "Transactions additions" |
| Merchant names | `merchant_display` is derived: deterministic clean-up of the raw text, then the merchant alias table, then rule actions. It can be recomputed at any time; the raw text is untouched |
| Refunds, reimbursements, shared costs | `transaction_link` ties a refund to its purchase (it nets against the purchase's category instead of counting as income), a reimbursement to the expense it repays, and a settlement to a share label. `my_share_minor` (default: the full amount) gives personal and gross views; the difference builds an owed-to-me balance per local share label. Share labels never leave the app. Still single-user: no one else's accounts are modelled |
| Duplicates | Exact duplicates are prevented at import (source ID, then fingerprint). Near-duplicates (same account and amount within ±N days, different text) become `duplicate_candidate` rows for review. Merging excludes one row, never deletes it, and can be undone |
| Reconciliation | Every `balance_snapshot` is an assertion checked against the computed balance; a failure reports the window since the last passing assertion. `account.reconciled_through` locks history: imports, manual edits, merges or undos touching locked dates are flagged and need an explicit unlock |
| Bulk changes | Changes to many existing transactions (a rule applied to history, duplicate merges, bulk edits) are previewed, then committed as an undoable `change_batch` that records old and new values, like import batches |
| Schedules and forecast | A detected recurring series becomes a schedule only when the user confirms it. The forecast is the last known balance plus schedule occurrences (plus the remaining flex budget), per account per day, in integer arithmetic, using cached FX rates only |
| Budgets | Per-category monthly amounts with optional rollover. Each budget is in a group: fixed, non-monthly (accrues 1/12 of a yearly amount, seeded from yearly schedules) or flex |
| Asset and liability accounts | Account kinds `asset` and `liability` hold manually valued items (house, pension, mortgage, loan). Valuations are `balance_snapshot` rows with source `valuation`; net worth uses the last value on or before each date. Investment holdings and prices are deferred (ROADMAP L.14) |
| Backups and export | Rotating `VACUUM INTO` snapshots (0600) in the data dir before every import commit and migration; by default the last 10 plus one per month are kept. Restore runs `PRAGMA integrity_check` and a schema-version check before swapping files. Export is CSV/JSON with a documented schema (split lines carry their parent ID); Beancount/hledger journal export follows in M5.8. Local files only, no network |
| MCP access | MCP is off until the user enables a client. Each client has an access level (`aggregate`, `pseudonymous` or `standard`); tools above it are never registered. The client is named by `coffer mcp --client <name>` in its configuration. This is a guardrail against over-sharing, not authentication: any local process can read the database file (at-rest encryption is L.6). Every tool call is recorded in `mcp_access_log`. Approval happens only in Coffer's own window or CLI; MCP elicitation, sampling and MCP Apps are not used for it |

### Decision record: feature research (2026-11-09)

The rows above from Splits to MCP access come from
[`research/feature-ideas.md`](research/feature-ideas.md). Four points were checked
against the invariants and decided as follows:

1. **The MCP access log is not an MCP write.** The invariant "MCP tools may write to
   `suggestion` and `report` and nothing else" still holds for every tool. The server
   appends to `mcp_access_log` as its own audit record, in the same way it already writes
   logs to stderr. The AI can't choose what gets written; its only input is the optional
   `purpose` string, which is stored as untrusted text and never interpreted. A log of
   what the AI read makes the inbox model easier to verify, so the benefit outweighs a
   strict reading of the invariant.
2. **Imported source fields are immutable.** Date, amount, raw description and external ID
   are never edited. Corrections go through undoing a batch, excluding a row with a
   recorded reason (still visible under a filter), or user-facing fields such as category,
   display merchant, splits and notes. This is stricter than most apps, but it keeps
   "never silently alter data" and keeps dedupe and reconciliation stable across
   re-imports.
3. **MCP access levels are a guardrail, not a security boundary.** They stop a client
   from being shown more than the user chose to share. They don't stop a local process
   that reads the SQLite file directly; at-rest protection is L.6 (SQLCipher).
4. **Defaults, not invariants.** Keeping 10 backups plus one per month, and non-monthly
   budgets accruing 1/12 of the yearly amount, are starting values. Change them freely
   without a new decision.

### Decision record: Transactions additions (2026-11-10)

The open questions from the D6.3 designs (`DESIGN.md` → "Transactions additions"),
answered on the designer's recommendation at the user's request:

1. **Exclusion has a reason key, a note and a date.** The reason is one of `duplicate`,
   `bank_error` (the bank's mistake, such as a double charge), `disputed` (a charge
   being contested, including fraud) and `other`. `excluded_note` holds optional user
   text, which is required for `other`; it's treated like `user_note` and isn't exposed
   over MCP by default. `excluded_at` gives the inspector its date. A fixed key keeps
   reports and filters exact, and an aggregate client can count exclusions without
   seeing any text. Exclusion is a field on `transaction`, not a one-item
   `change_batch`: the filter and the inspector read it on every row, and "Include
   again" is the undo. Multi-row exclusions (duplicate merges, a rule applied to
   history) still go through `change_batch`.
   - **Balances depend on the reason.** Every excluded row leaves spending, income,
     budgets and reports. Only `duplicate` also leaves the account's computed balance,
     because a duplicate never happened at the bank. The other reasons record money that
     really moved: it stays in the balance until the correcting refund arrives, which is
     linked to the charge (`transaction_link`). Without this rule, excluding a double
     charge would break reconciliation. Excluding or including a `duplicate` row in a
     locked window is flagged like any other change there.
2. **Every transaction has `created_at`**, not only manual ones. It's a UTC timestamp set
   on insert. It costs one column and also serves "recently added" and support
   questions. Edits need no `updated_at`, because `change_item` and the import batches
   already record them.
3. **The rule prompt needs at least one earlier match.** "Create a rule from this
   edit?" shows after a category edit only if the proposed pattern matches at least one
   earlier transaction, so the merchant has been seen twice. A rule from a single
   sighting is a guess, and prompting on every one-off merchant would teach the user to
   ignore the prompt.
   - **When it doesn't show:** for transfers, split rows, or rows a rule already
     matched (D6.7 handles changing an existing rule).
   - **"Not now"** is remembered for that pattern, so the inline prompt doesn't return.
     The passive route stays: after repeated identical recategorisations, the app files
     a `rule` suggestion to the Inbox (source `coffer`; see "Categorization").

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
`coffer import <file>` (headless import, used from M1), `coffer backup` / `coffer restore`,
`coffer export`.

The GUI and MCP processes share the SQLite file; WAL allows concurrent readers.

## Dependencies

Versions live in `[workspace.dependencies]` in the root `Cargo.toml`; crates opt in with
`<crate>.workspace = true`. Chosen 2026-11 (crates.io research); revisit when pins lift.

| Area | Crates | Notes |
|---|---|---|
| Storage | `rusqlite` (`bundled`), `refinery` | `rusqlite` pinned to 0.40 until `refinery` supports 0.41. `refinery` checksums applied migrations, enforcing "never edit a shipped migration". No connection pool: one connection per process, WAL + `busy_timeout` + `BEGIN IMMEDIATE`; only the GUI runs migrations. SQLCipher later = `bundled-sqlcipher-vendored-openssl` |
| Import | `csv`, `serde`, `encoding_rs`, `ofx-kit`, `chrono`, `toml` | Encoding comes from the preset; decode errors are flagged, never guessed. Strip the BOM before `csv`. `ofx-kit` is young and single-maintainer: pinned exactly and kept behind the `Importer` trait (fallback: `ofx-rs`). Dates must round-trip through their format string |
| Money / FX | none | Own `Money { minor: i64, currency }` with a hand-written, test-frozen exponent table. FX in `i128` fixed point with one explicit rounding. Decimal types from format crates are converted to minor units at the import boundary, never stored |
| Rules | `regex`, `aho-corasick` | `regex` is linear-time, so user rules can't hang the app |
| Plumbing | `thiserror` (libs), `anyhow` (binary), `clap`, `tracing`, `tracing-subscriber`, `directories`, `uuid` (v7) | Logs go to stderr |
| MCP | `rmcp` (`transport-io`), `tokio` (current-thread), `schemars` | Only the `mcp` crate / `coffer mcp` path uses tokio; `core` stays synchronous |

Later: `ureq` + `roxmltree` for the ECB fetch, `mt940`, `camt053` (no default features),
own QIF parser. Avoid GPL crates (`ofxy`, `tree_magic_mini`'s GPL data feature).

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
6. **Preview → commit**: show new / already-imported / flagged counts, and flag rows dated
   on or before the account's `reconciled_through` lock; take a backup; commit in one
   DB transaction; record an `import_batch` so an import can be undone.
7. **Post-process** (deterministic, no model): clean merchant text and apply merchant
   aliases into `merchant_display`; apply rules (which can match merchant, raw description,
   source category, source type, amount, account) and their actions; map source categories
   via the preset; run transfer-pair detection; match schedule occurrences; queue
   near-duplicate candidates; record balances and check assertions.

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

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|cash|
  asset|liability|...], currency, opening_balance_minor?, opening_date?,
  reconciled_through?)` — pots are child accounts of their parent
- `import_batch(id, account_id, source_file, format, preset_id?, imported_at, status)`
- `change_batch(id, kind [rule_apply|duplicate_merge|bulk_edit], created_at,
  status [applied|undone])` and `change_item(batch_id, transaction_id, field, old_value,
  new_value)` — undo for changes to existing transactions
- `transaction(id, account_id, batch_id?, date, time?, amount_minor, currency, merchant?,
  merchant_display?, description_raw, description_normalized, external_id?, fingerprint,
  source_category?, source_type?, original_amount_minor?, original_currency?, category_id?,
  category_source [rule|preset_map|user|ai_approved], rule_id?, transfer_group_id?, note?,
  user_note?, my_share_minor?, share_label?, reimbursable,
  excluded_reason? [duplicate|bank_error|disputed|other], excluded_note?, excluded_at?,
  origin [imported|mirrored|manual], created_at)` — `mirrored` rows are the generated
  pot-side half of a sub-account transfer, and undoing their import batch removes them too;
  `manual` rows have no batch; `note` is the imported note (read-only), `user_note` the
  user's; `category_id` is empty when the transaction is split. The three `excluded_*`
  fields are set and cleared together (`excluded_note` is required for `other`);
  `created_at` is when the row entered the database (the batch commit for imported rows)
- `transaction_split(id, transaction_id, line_no, amount_minor, category_id?,
  my_share_minor?, share_label?, user_note?)` — lines sum exactly to the parent amount
- `ledger_line` (view) — the split lines, or the transaction itself when unsplit. Reports skip
  excluded rows and confirmed transfers; a linked refund takes its purchase's category
- `tag(id, name, private)`, `transaction_tag(transaction_id, tag_id)`
- `merchant_alias(id, match_kind [exact|prefix|regex], pattern, display_name)`
- `transaction_link(id, kind [refund|reimbursement|settlement], inflow_id, target_id?,
  share_label?, amount_minor, status [suggested|confirmed|rejected])` — refunds and
  reimbursements target a purchase; settlements target a share label
- `duplicate_candidate(id, transaction_a, transaction_b, reason,
  status [pending|merged|dismissed], change_batch_id?)`
- `balance_snapshot(account_id, date, balance_minor,
  source [ofx|csv|statement|manual|valuation])` — each one is an assertion
- `category(id, parent_id?, key, label)` — two levels, stable keys, translatable labels
- `rule(id, priority, match_kind, pattern, amount_min?, amount_max?, account_id?)` and
  `rule_action(rule_id, kind [set_category|set_merchant|add_tag|set_note|exclude|
  split_template|my_share], value_json)` — an `exclude` action's value is a reason key
- `transfer_group(id, status [suggested|confirmed|rejected])`
- `schedule(id, account_id, merchant_display, category_id?, cadence, next_date,
  amount_minor, currency, tolerance_minor, tolerance_days, status [active|paused|ended])`
- `budget(id, category_id, month?, amount_minor, currency, rollover,
  group_kind [fixed|non_monthly|flex])`
- `fx_rate(date, base, quote, rate, source)`
- `suggestion(id, kind [categorization|rule|transfer_pair|merchant_alias|refund_link],
  payload_json, rationale, source_client, created_at, status [pending|approved|rejected])`
  — `source_client = coffer` for suggestions the app files itself
- `report(id, period, title, body_markdown, source_client, created_at)`
- `mcp_client(id, name, access_level [aggregate|pseudonymous|standard], enabled,
  created_at, last_seen_at?)`
- `mcp_access_log(id, client_id, at, tool, args_redacted_json, purpose?, row_count,
  result_hash, access_level)` — never stores returned data
- `setting(key, value)` — includes the per-database key for merchant pseudonyms, which is
  never exposed

## MCP surface

**Read tools** (all results in home currency + original currency; no account numbers/PII;
merchant addresses and user notes are not exposed by default):
`list_accounts`, `monthly_summary(from, to)`, `spending_by_category(period)`,
`search_transactions(query, filters, limit)`, `merchant_history(merchant)`,
`recurring_payments()`, `budget_status(month)`, `net_worth_history(from, to)`,
`uncategorized_transactions(limit)`, `list_categories`, `list_rules`, `list_reports`.
Reads go through `ledger_line`, so split purchases count per category. They include tags
(not private ones, and only at `standard` access) and confirmed refund links. Notes and
share labels are never exposed. Every tool takes an optional `purpose` string, which is
logged and never interpreted. Added with M4.8: `cash_flow_forecast`.

**Inbox writes** (never modify transactions/rules directly):
`propose_categorizations`, `propose_rule`, `propose_transfer_pair`,
`propose_merchant_alias`, `propose_refund_link`, `save_report`.

**Prompts**: `monthly_review(month)` — patterns, anomalies, subscription creep,
budget variance, suggestions; ends by calling `save_report`.

**Access levels** (per client, set in the app):

- `aggregate`: totals only (`monthly_summary`, `spending_by_category`, `budget_status`,
  `net_worth_history`, `list_categories`, `list_reports`); no transaction rows, no writes
  except `save_report`.
- `pseudonymous`: transaction rows with each merchant replaced by a stable keyed-hash token
  (e.g. `M-7f3a`); the mapping never leaves the app.
- `standard`: the full surface above.

**Access log**: tools write only `suggestion` and `report`. Separately, the server appends
one `mcp_access_log` row per call as its own audit record; the AI's only input to it is the
untrusted `purpose` string. The app shows the log.

Transaction descriptions are untrusted input (prompt-injection surface); inbox-only
writes are the containment. In addition:

- **Text envelope.** Tools return `merchant_display` by default. Raw text appears only as
  `description_untrusted`: length-capped, with control, zero-width and bidi characters
  stripped and URLs made inert. Text matching injection patterns ("ignore previous…",
  base64 blobs, URLs) is withheld and replaced by a marker, and the app counts what was
  withheld.
- **Impact preview.** The inbox shows what approving each suggestion would change (e.g. how
  many past transactions a rule would recategorise), so an overly broad rule or a fake
  transfer pair planted by injected text is visible before approval.
- **Reports.** Saved reports render as a markdown subset: no HTML, no images, links shown as
  plain text, so a report can't trigger a network request. `list_reports` marks them
  `ai_authored`, since stored AI text is untrusted when read back.
- **Client setup.** Setup docs recommend a client profile with Coffer as the only data
  source and no web or fetch tools.

## Dashboard (v1)

- **Transactions**: filterable/sortable table, inline category edit, import flow; split
  editor, tags and notes in the inspector; manual transactions; exclude with a reason.
- **Inbox**: approve/reject AI suggestions in bulk, with an impact preview per suggestion.
- **Review queues**: possible duplicates; reconciliation (assertion results, lock/unlock);
  refunds, outstanding claims and owed-to-me balances.
- **Monthly overview**: income vs spending vs net per month; category breakdown with MoM delta.
- **Trends**: category/merchant over time, top merchants, recurring payments.
- **Upcoming**: confirmed schedules, missed payments, daily balance forecast per account.
- **Budgets & net worth**: per-category monthly targets with rollover, groups and a pace line;
  balances/net worth over time (from OFX `LEDGERBAL`, CSV balance column, manual opening
  balance, or asset/liability valuations).
- **Reviews**: saved MCP reports, safely rendered.
- **Settings**: AI access (clients, access levels, access log, kill switch), backups and
  export.

## Milestones

1. **Core + import (headless)** — workspace split, schema/migrations, CSV (generic + preset)
   and OFX importers, dedupe, reconciliation, rules, transfer detection;
   `coffer import` CLI; fixtures + tests. Also splits, tags and notes, manual
   transactions, merchant clean-up, rule actions, refund/reimbursement links,
   near-duplicate queue, balance assertions and lock, backups, CSV/JSON export.
2. **MCP server** — read tools, inbox writes, `monthly_review` prompt; usable from
   Claude Code/Codex against real data. Also schedules, access levels, the access log and
   the untrusted-text envelope.
3. **GUI foundation** — app shell, transactions table, import + mapping screen, inbox. Also
   the split editor, manual entries, links, duplicates and reconciliation, rule previews,
   inbox impact preview, AI access settings, backups, asset/liability accounts.
4. **Analytics** — monthly overview, trends, recurring, budgets, net worth, FX. Also
   upcoming payments and forecast, budget rollover/groups/pace, safe report rendering.
5. **Open-source release** — README, CONTRIBUTING (preset how-to), presets for common
   UK/US/EU banks, CI (Linux/macOS/Windows build), release artifacts (tarball, Nix, .dmg),
   Beancount/hledger export.

## Deferred / open

- PDF import; QIF, CAMT.053, MT940 importers.
- Encrypted DB.
- Streamable-HTTP MCP inside the GUI (live "jump to transaction").
- i18n of UI strings beyond category labels.
- Budget period start (payday-aligned months, ROADMAP L.15): every report depends on it,
  so decide before M4.1 ships.
- Investment holdings and prices, goals, attachments (ROADMAP L.11–L.14). Any online price
  fetch would be a second network call and needs its own decision.
- Feature research and the list of rejected ideas: [`research/feature-ideas.md`](research/feature-ideas.md).
