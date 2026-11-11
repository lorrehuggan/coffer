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
| Storage | SQLite, WAL mode, `synchronous = FULL`, file mode 0600 (data dir 0700), `coffer.db` in the platform data dir (`~/.local/share/coffer`; `~/Library/Application Support/Coffer` on macOS), overridable with `COFFER_DATA_DIR`. Not encrypted in v1 (SQLCipher possible later) |
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

### Decision record: budgets and the forecast (2026-11-10)

The open questions from the D6.10 designs (`DESIGN.md` → "Budgets: groups, rollover,
pace and upcoming"), decided by the user on the designer's recommendation:

1. **Flex spending leaves accounts in proportion to history.** The forecast spreads each
   Flex budget's remainder evenly over the rest of the month. It splits that remainder
   across accounts by where the category was spent over the last 3 months. An account
   with no Flex spending in that window (savings, pots, Joint in the fixture) gets none.
   Later months spend their full Flex budget the same way. Every step is integer minor
   units, with remainders spread so each day's amounts sum exactly. Fixed and
   non-monthly budgets add nothing to the forecast beyond their schedules, which avoids
   counting them twice.
2. **A payment is missed only once imports cover its date.** A schedule occurrence is
   *missed* when its date plus `tolerance_days` has passed, nothing matched it, and the
   account's latest imported transaction is on or after that date. Until then it is only
   *not imported yet*, and nothing is flagged. A missed occurrence stays in the forecast,
   counted from tomorrow, until the user matches it to a transaction or skips it.
   Missed is derived, not stored. A skip is stored (`schedule_skip(schedule_id,
   occurrence_date, skipped_at)`), and so is a user's match (`schedule_match(schedule_id,
   occurrence_date, transaction_id)`). A series the user marks "Not recurring" is not suggested again.
3. **Rollover carries both ways, derived from history.** With rollover on, a month's
   carry-in is the previous month's LEFT, positive or negative, so it accumulates.
   Nothing about it is stored: it's recomputed from `ledger_line` and the budgets, so
   editing or recategorising a past transaction flows into later months. Turning
   rollover off starts the budget fresh from the next month; turning it on starts
   carrying from the current month. Fixed budgets don't offer rollover, and non-monthly
   budgets always roll over.
4. **Pace is linear by day of the month in v1.** On day *d* of an *n*-day month, pace is
   *d*/*n* of the month's amount (budget plus rollover). Pace weighted by past months'
   daily pattern can come later; it's a reporting choice, not a schema one.
5. **A non-monthly amount is seeded once, then belongs to the user.** A new non-monthly
   budget's yearly amount starts as the total of the category's active yearly
   schedules. After that it's the user's number and doesn't follow schedule changes.
   A new yearly schedule in that category asks before adding its amount (the schedule
   dialog's "Set aside" toggle). The set-aside amount accrues from the month the budget
   is created. It starts at £0 unless the user enters what's already saved, which is
   stored as an opening set-aside. The 1/12 accrual stays a default (decision record of
   2026-11-09, point 4).
6. **Without scheduled income, the horizon is 30 days.** "Lowest before payday" uses the
   account's next scheduled income as the horizon. An account with none shows "Lowest in
   the next 30 days", and the Payday figure is left out. `cash_flow_forecast` (M4.8)
   reports the same horizon and says which one it used.

### Decision record: safe report rendering and withheld text (2026-11-10)

The open questions from the D6.11 designs (`DESIGN.md` → "Safe report rendering"),
decided by the user on the designer's recommendation:

1. **Emphasis renders as weight 500.** `*emphasis*` in a saved review is 500 in
   `text-strong`. Coffer doesn't bundle an italic face for reviews alone.
2. **The withheld count lives on the access log.** `mcp_access_log` gains
   `withheld_count` (a new migration): how many descriptions that call withheld. The
   Reviews inspector sums it over a review's calls. The app's "Withheld from AI" filter
   re-runs the same deterministic check over raw descriptions, so the log never records
   which transactions were withheld, in keeping with "never stores returned data".
3. **The marker is a string with a count.** A withheld `description_untrusted` reads
   `[withheld: N characters that read like instructions]`. The field stays a string, and
   a client can't mistake the marker for bank text.
4. **Reports read back are stripped and marked, not withheld.** `list_reports` returns
   `body_markdown` with control, zero-width and bidi characters removed and
   `ai_authored: true`. Nothing inside a report is withheld: the marker covers the whole
   body. The stored body is never changed; the app strips the same characters when it
   renders a review and counts them.

### Decision record: review queues (2026-11-10)

The open questions from the D6.4 designs (`DESIGN.md` → "Review queues"), decided by the
user on the designer's recommendation:

1. **The oldest balance is the starting point, not a check.** An account's first
   `balance_snapshot` is where Coffer starts counting, and every later one is checked
   against it plus the transactions between them. A `valuation` snapshot sets the
   balance and is never a check.
2. **A period is locked only through a passing check.** `reconciled_through` can be set
   only to the date of a passing check, and only when every check up to that date
   passes. Unlocking clears it entirely; there's no partial unlock.
3. **A merge keeps the best-sourced row.** By default it keeps the row with a source ID,
   then an imported row over a manual one, then the one imported first. The user can
   swap the rows before merging.
4. **The near-duplicate window is 3 days.** It's a default, not an invariant (see
   "Defaults, not invariants").
5. **A dismissed pair stays dismissed.** A `duplicate_candidate` with `status =
   dismissed` is never raised again for those two transactions. A later import that
   brings a third match creates a new pair.

### Decision record: refunds, reimbursements and shared costs (2026-11-10)

The open questions from the D6.5 designs (`DESIGN.md` → "Refunds, reimbursements and
shared costs"), decided by the user on the designer's recommendation:

1. **A refund counts on its own date.** It's negative spending in the purchase's category
   (`ledger_line`), in the month the money came back. The purchase's month doesn't
   change, so a closed month stays as it was when a late refund arrives.
2. **Link suggestions are deterministic and live in the inspector.** Coffer suggests:
   - a *refund* when money comes in on the same account under the same
     `merchant_display` as a purchase in the previous 90 days that has at least that
     much left to refund;
   - a *reimbursement* when money comes in that equals one outstanding claim, or a set
     of them, to the penny, within 60 days of the newest.

   Suggestions are `transaction_link` rows with `status = suggested`, shown in the
   inspector and on Owed to you, not in the Inbox. A rejected pair is never suggested
   again. The AI's `refund_link` suggestions stay in the Inbox (D6.6).
3. **Spending is your share by default.** Spending, budgets, Overview, Trends and MCP
   reads use `my_share_minor`. A purchase being claimed back counts as £0 while it's
   outstanding, and switching it to "Just me" counts it in full again. Balances, net
   worth and the Transactions table are always gross. A gross toggle on reports can come
   later.
4. **An overpayment is capped.** A link's `amount_minor` is at most what the purchase can
   still take or what the person owes. The rest of the incoming money stays unlinked and
   counts by its own category.
5. **A link needs one currency in v1.** Both sides must be in the same currency;
   cross-currency refunds wait for L.28.

### Decision record: Inbox impact preview (2026-11-10)

The open questions from the D6.6 designs (`DESIGN.md` → "Inbox impact preview"), decided
by the user on the designer's recommendation:

1. **Approving an agent's rule never changes a category the user set.** From the Inbox
   it applies only to uncategorised rows and rows with `category_source = rule`. Including
   the user's own rows is a choice made only in D6.7's apply-to-past preview ("Change
   these too"). The Inbox has no such toggle.
2. **A rule is wide when it spans more than 5 merchants or moves income.** That means its
   matches span more than 5 distinct `merchant_display` values, or any match would move
   income into a spending category. This is a default, not an invariant.
3. **Approve all always skips wide rules.** A wide rule needs its own approval with the
   preview open.
4. **A rule's approval settles the categorisations it covers.** Pending `categorization`
   suggestions for rows the rule changes are marked approved under the rule's
   `change_batch`, so the Inbox doesn't ask twice.
5. **Merchant names don't change which rules match.** Rules match the cleaned bank text
   before names apply (D6.7), so a name suggestion is cosmetic, and its preview says so.
6. **The impact is computed when shown.** It runs against today's data each time a
   suggestion is opened, so a later import or edit can't make a stale preview look
   smaller than the real change.

### Decision record: rules (2026-11-10)

The open questions from the D6.7 designs (`DESIGN.md` → "Rules"), decided by the user on
the designer's recommendation:

1. **Rules win field by field.** In priority order, the first matching rule sets a field,
   and a later rule can still set a different field (a tag rule below a category rule
   both apply). A rule is "never reached" when every transaction it matches already has
   all its fields set by rules above.
2. **A new rule goes first.** A rule from an edit or from the Inbox is usually narrower
   than the rules it corrects. The Position select moves it.
3. **A rule never overwrites a category the user set unless asked.** Never on import.
   On Apply, only rows with `category_source = user` that the user includes with "Change
   these too". The same applies to agent rules (Inbox impact preview, point 1).
4. **Rule edits don't re-run on history.** Rules run on import. History changes only
   through Apply, which records one `change_batch` (`rule_apply`) per apply.
5. **Merchant names resolve Is, then Starts with, then Pattern.** An `exact` match wins,
   then the longest `prefix`, then `regex` in list order. Names apply at once with no
   batch, because the display name is derived.
6. **Undo skips rows changed since.** Undoing a batch restores a field only where it
   still holds the batch's `new_value`. Rows changed since keep the later value, and the
   undo dialog counts them.
7. **Deleting a rule leaves its values.** Its transactions keep what it set, and their
   `rule_id` is cleared. The inspector's "Set by" reads "A deleted rule", and a later
   rule or Apply may change them.
8. **Locked periods guard balances only.** `reconciled_through` doesn't cover categories,
   tags, notes or names, which don't change a balance, so Apply and Undo inside a locked
   period aren't flagged. An Exclude action with reason `duplicate` does change a
   balance, and is flagged like any other change there.

### Decision record: Settings, AI access and backups (2026-11-10)

The open questions from the D6.8 designs (`DESIGN.md` → "Settings"), decided by the user
on the designer's recommendation:

1. **The kill switch is read on every call.** It's a `setting` that the server reads on
   every call, not only at start, so turning it off refuses the next call from a client
   that is already connected. Refused calls are logged: `mcp_access_log` gains `outcome
   [ok|refused_off|refused_not_enabled|refused_level]` (a new migration).
2. **Unknown clients are recorded as requested.** The first `coffer mcp --client X` with
   an unknown name creates an `mcp_client` row that isn't enabled and is marked
   `requested` (a new migration), and the call is refused. "Ignore" hides the row without
   enabling it. Only the app or CLI enables a client, never the server, and enabling one
   starts it at `aggregate`.
3. **A lower level applies at once.** The server checks the client's level on every call
   and refuses tools above it. It also sends `notifications/tools/list_changed`, so the
   client drops those tools from its list.
4. **Argument redaction keeps the structure.** `args_redacted_json` keeps keys, dates,
   enum values, numbers and IDs. Free-text values are stored only as their type and
   length.
5. **The access log is kept for 12 months.** That's a default, not an invariant; the rows
   are small, one per call.
6. **Backup metadata lives in the file name.**
   - A backup is also taken before every restore and on "Back up now". The reason and
     time live in the file name, so the list needs no table that a restore would roll
     back.
   - A backup with an older schema is restored and then migrated; a newer one is refused.
   - While a restore swaps the files, running MCP servers refuse calls. They reopen the
     database afterwards.

### Decision record: accounts you keep yourself (2026-11-10)

The open questions from the D6.9 designs (`DESIGN.md` → "Accounts you keep yourself"),
decided by the user on the designer's recommendation:

1. **A valuation can carry a private note.** `balance_snapshot` gains an optional `note`
   (a new migration). It's user text, treated like `user_note`, and isn't sent over MCP
   by default.
2. **Valuation accounts hold valuations only.** `asset` and `liability` accounts hold
   `valuation` snapshots and no transactions. A payment toward one stays on the account
   that paid it, as spending or a transfer the user categorises, and net worth catches
   up at the next valuation. Linking payments to a loan balance can come later.
3. **Closing sets `closed_on`.** `account.closed_on` (a date; a new migration) applies to
   every kind of account. Closing a valuation account also writes a £0 `valuation`
   snapshot on that date, which reopening removes. The closing date must be later than
   the account's last valuation, so the £0 never replaces a value the user entered and
   reopening loses nothing; the close dialog offers the day after the last valuation as
   the earliest date. An account with transactions keeps D6.1's close-only rule.
4. **Only an account without transactions can be deleted.** A valuation account or a
   new empty one can be deleted together with its snapshots. Deleting can't be undone,
   so the confirmation says how many values go with it.
5. **A cash count is a checked balance.** A count is a `balance_snapshot` with source
   `manual`, checked like any other balance (the first is the starting point; Review
   queues, point 1). "Add and save" writes the snapshot and the difference transaction
   in one DB transaction. Coffer never posts a difference on its own.

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

Packages are named `coffer-core`, `coffer-import`, `coffer-mcp` and `coffer-app`. The
binary is built from `coffer-app`, which depends on the other three; shared files stay at the
repository root (`assets/` for fonts and brand marks, `design/` for the exported tokens).

Single binary: `coffer` (GUI), `coffer mcp` (stdio MCP server),
`coffer import <file>` (headless import, used from M1), `coffer backup` / `coffer restore`,
`coffer export`.

The GUI and MCP processes share the SQLite file; WAL allows concurrent readers.

## Dependencies

Versions live in `[workspace.dependencies]` in the root `Cargo.toml`; crates opt in with
`<crate>.workspace = true`. Chosen 2026-11 (crates.io research); revisit when pins lift.

| Area | Crates | Notes |
|---|---|---|
| Storage | `rusqlite` (`bundled`), `refinery` | `rusqlite` pinned to 0.40 until `refinery` supports 0.41. `refinery` checksums applied migrations, enforcing "never edit a shipped migration". No connection pool: one connection per process, WAL + `busy_timeout` + `BEGIN IMMEDIATE`; the app and the CLI open with `Database::open`, which creates the database if needed and runs migrations (all pending ones in one transaction); `coffer mcp` opens with `Database::open_existing`, which never creates or migrates and refuses a missing or older database, telling the user to open Coffer or run a CLI command. Both check an existing file first (a SQLite database, not a newer schema, applied migrations matching the embedded ones) and leave a refused file untouched: no permission change, WAL switch or write. SQLCipher later = `bundled-sqlcipher-vendored-openssl` |
| Import | `csv`, `serde`, `encoding_rs`, `ofx-kit`, `chrono`, `toml` | Encoding comes from the preset; decode errors are flagged, never guessed. Strip the BOM before `csv`. `ofx-kit` is young and single-maintainer: pinned exactly and kept behind the `Importer` trait (fallback: `ofx-rs`). Dates must round-trip through their format string |
| Money / FX | none | Own `Money { minor: i64, currency }` (`coffer_core::money`, M1.5) with a hand-written, test-frozen exponent table. FX in `i128` fixed point with one explicit rounding. Decimal types from format crates are converted to minor units at the import boundary, never stored. See "Money" below |
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

### The `Importer` trait

M1.6, decided 2026-11-10. Steps 1–3 above are one importer each (CSV, OFX, later QIF,
CAMT.053, MT940); steps 4–7 are core's and don't depend on the format.

- **The normalized form lives in core.** `coffer_core::statement::Statement` is what every
  importer produces and what dedupe, reconciliation and the commit consume, so it sits in
  `core` (which can't depend on `import`). It has the format, the preset ID if any, one
  currency, the file's rows in order and any statement-level balances (`LEDGERBAL`). A
  statement isn't tied to an account: the user picks it at import, and the commit checks
  the currencies agree.
- **Rows are never dropped.** Each row is a `StatementLine` or a `RowProblem`, at its
  `SourceLocation` (line in a text file, entry in a structured one), kept in file order
  for the fingerprint's occurrence index and running balances. A line whose amount or
  running balance isn't in the statement's currency is flagged as it's added; a
  statement-level balance in another currency fails the file. Only a file that can't be
  read as a whole is an `ImportError`. Neither kind of error contains the file's text.
- **`description_normalized` is computed, not carried**
  (`statement::normalize_description`: whitespace collapsed, trimmed, Unicode uppercase),
  so it can't disagree with the raw text. Changing it changes the fingerprint.
- **Three traits in `coffer-import`.** `Detect` (`name`, `format`, `detect`) says what an
  importer is and how well a file fits: `No < Extension < Format < Source` (`Source` is a
  preset's own header row, `Format` is merely valid CSV or OFX). `Importer: Detect` adds
  `parse` (into the format's own `Parsed` type) and `normalize` (into a `Statement`).
  `AnyImporter: Detect`, implemented for every `Importer`, hides `Parsed` so importers
  can be listed as `&dyn AnyImporter` and adds `import` (parse, then normalize). Each
  method has one home, so calls never need disambiguating.
- **Choosing is the caller's.** `coffer_import::detect` ranks the importers that
  recognise a file, best first, ties in the given order. No candidate or a tie at the top
  means asking the user (or `--preset`), never guessing.
- `SourceFile` holds the undecoded bytes and the file name only, never the directory; its
  `Debug` leaves the contents out. Decoding is the importer's, from the format or preset.

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

## Money

`coffer_core::money` (M1.5). Decided 2026-11-10:

- **Currencies come from a fixed table.** `Currency` can only be built from a code in a
  hand-written ISO 4217 table, which gives its minor units (`GBP` 2, `JPY` 0, `KWD` 3,
  `CLF` 4); the tests freeze it, because changing a value changes what every stored
  amount in that currency means. The table holds the active list, apart from codes with
  no minor unit (precious metals, `XDR`, `XSU`, `XUA`, test codes), plus codes withdrawn
  since 2014 (`HRK`, `LTL`, `SLL`, `ZWL`, …), since older statements and card payments
  abroad still name them. ISO's minor units win where everyday use differs (`MGA`,
  `MRU`). An unknown code, including one read back from the database, is an error.
- **Arithmetic is checked.** Adding, subtracting or comparing two currencies is an
  error, as is overflowing `i64`; nothing panics or wraps. There are no operator
  overloads for the same reason.
- **Import reads amounts exactly or not at all.** `Money::parse_decimal` takes the
  preset's decimal mark (`.` or `,`); thousands may be grouped by the other mark, a
  space (including no-break spaces) or an apostrophe, in groups of three; the sign is a
  leading `-`, `−` or `+`, or parentheses. Extra decimals are accepted only when they
  are zeros (`12.340` GBP): anything else would mean rounding, so it's refused and the
  row is flagged. Currency symbols, codes and trailing minus signs are the importer's
  to deal with (a preset feature if a bank needs one). Parse errors don't contain the
  text, since errors get logged.
- **Display formatting is the app's.** Core's only text form is the plain, locale-free
  `-1234.56` (`to_decimal_string`, for exports and the CLI), which parses back
  unchanged. Symbols, locale separators, `−` and `+` follow `DESIGN.md` → "Money" in
  the app.

## Data model (initial)

The schema is SQL in `crates/core/migrations/`. Conventions (M1.4; the tests in
`crates/core/src/db/schema_tests.rs` check them):

- Every table is `STRICT`, so a money column (`*_minor`, INTEGER) refuses a fractional
  value. Currency codes are three capital letters; the ISO 4217 list is checked in Rust.
- Dates are `YYYY-MM-DD`, times `HH:MM:SS`, months `YYYY-MM`, timestamps UTC
  `YYYY-MM-DDTHH:MM:SS.SSSZ`, each checked to round-trip through SQLite's date functions.
  Enums are TEXT keys listed in a `CHECK`; booleans are 0/1.
- IDs are `INTEGER PRIMARY KEY AUTOINCREMENT`, so an ID is never reused after a delete
  (an undone import) and an old suggestion or log entry can't come to name another row.
- Foreign keys have no `ON DELETE` action and are all indexed: deleting is explicit in
  core, never a side effect. Deleting a rule clears `transaction.rule_id` in core.
- A trigger refuses edits to an imported or mirrored row's source fields (account, batch,
  date, time, amount, currency, merchant, descriptions, external ID, fingerprint, source
  category and type, original amount, note); a transaction's `id`, `origin` and
  `created_at` never change.
- Partial unique indexes enforce dedupe, source ID first: `(account_id, external_id)`,
  and `(account_id, fingerprint)` among rows without an external ID (a row with a source
  ID is never matched by fingerprint, since the occurrence index depends on the file's
  order). Manual rows carry neither key. An insert that collides with a stored row is
  refused by a trigger before SQLite resolves the conflict, so `INSERT OR REPLACE` can't
  swap a row past the read-only trigger, and core never uses `REPLACE` or upserts on
  `"transaction"` (a test scans for them); `INSERT OR IGNORE` on a duplicate is an error
  too, because core dedupes first.
- Triggers also keep categories to two levels with keys that match their place (a
  top-level key is one part, a subcategory's is `<parent key>.<part>`) and make a
  category's `key` and `parent_id` read-only (keys are stable; only the label changes),
  keep a transaction in its account's currency, keep `asset`/`liability` accounts to
  `valuation` snapshots only (and valuations to those accounts), put a pot under a
  `current` or `savings` account in the same currency and keep it there once it holds
  transactions or balances, fix an account's currency once it has transactions, balances,
  pots or a rule with an amount bound (and keep such a rule from moving to an account in
  another currency), keep an imported row and a file's balance on the
  batch's account (a mirrored row or balance on a pot of it), and make an import batch
  read-only except for `status`. Undo deletes a batch's transactions and balances, then
  sets `undone` in the same DB transaction: a batch can't become `undone` while rows
  reference it, never returns to `committed`, and an undone batch takes no rows.
- An account has at most one entered balance (`batch_id` empty: `statement`, `manual`,
  `valuation`) per date, since each asserts the same end-of-day balance; a correction
  edits it. Balances from files may repeat a date across overlapping exports, and a
  mismatch is a failed check, not a refused insert.
- `transaction` is an SQL keyword, so the table name is always quoted (`"transaction"`).
- Tables arrive with the roadmap item that needs them. M1.4 created the ones below up to
  and including `report`, except `change_batch`/`change_item`, `transaction_split`,
  `ledger_line`, `tag`, `merchant_alias`, `transaction_link`, `duplicate_candidate` and
  the schedule tables; `mcp_client`, `mcp_access_log` and `setting` come with M2.

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|cash|
  asset|liability|...], currency, opening_balance_minor?, opening_date?,
  reconciled_through?, closed_on?)` — pots are child accounts of their parent; a pot's
  parent is a `current` or `savings` account in the same currency;
  `asset` and `liability` accounts hold `valuation` snapshots only, no transactions
- `import_batch(id, account_id, source_file, format [csv|ofx|qif|camt053|mt940],
  preset_id?, imported_at, status [committed|undone])` — `source_file` is the file name
  without its directory; QFX is `ofx`
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
  `manual` rows have no batch and no `external_id` or `fingerprint`; `note` is the imported note (read-only), `user_note` the
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
- `balance_snapshot(id, account_id, batch_id?, date, balance_minor,
  source [ofx|csv|statement|manual|valuation], note?, created_at)` — each one is an
  assertion; an account's first is its starting point and every later one is a check;
  `note` is user text, not sent over MCP by default. `ofx` and `csv` snapshots belong to
  their import batch and are removed when it is undone; the others have no batch
- `category(id, parent_id?, key, label)` — two levels, stable keys
  (`food.groceries` under `food`; key and parent read-only), translatable labels
- `rule(id, priority, match_field [merchant|description|source_category|source_type],
  match_kind [is|starts_with|contains|regex], pattern, amount_min?, amount_max?,
  account_id?, created_at)` and `rule_action(id, rule_id, kind [set_category|
  set_merchant|add_tag|set_note|exclude|split_template|my_share], value_json)` — one
  action per kind per rule; an `exclude` action's value is a reason key; `priority` isn't
  unique (ties go by ID)
- `transfer_group(id, status [suggested|confirmed|rejected], created_at)`
- `schedule(id, account_id, merchant_display, category_id?, cadence, next_date,
  amount_minor, currency, tolerance_minor, tolerance_days, status [active|paused|ended])`,
  `schedule_skip(schedule_id, occurrence_date, skipped_at)` and
  `schedule_match(schedule_id, occurrence_date, transaction_id)` — a missed occurrence is
  derived, not stored
- `budget(id, category_id, month?, amount_minor, currency, rollover,
  group_kind [fixed|non_monthly|flex], opening_set_aside_minor?, created_at)` — rollover
  is derived from `ledger_line`, never stored; `opening_set_aside_minor` is what a
  non-monthly budget had already saved when it was created. `month` empty is the standing
  budget and `YYYY-MM` overrides one month. Fixed and flex amounts are monthly; a
  non-monthly amount is yearly
- `fx_rate(date, base, quote, rate_e9, source [ecb|manual])` — 1 `base` = `rate_e9` /
  10⁹ `quote`: fixed point, so no float enters a conversion
- `suggestion(id, kind [categorization|rule|transfer_pair|merchant_alias|refund_link],
  payload_json, rationale, source_client, created_at, status [pending|approved|rejected])`
  — `source_client = coffer` for suggestions the app files itself
- `report(id, period, title, body_markdown, source_client, created_at)`
- `mcp_client(id, name, access_level [aggregate|pseudonymous|standard], enabled,
  status [requested|known|ignored], created_at, last_seen_at?)` — an unknown name creates
  a `requested` row that isn't enabled
- `mcp_access_log(id, client_id, at, tool, args_redacted_json, purpose?, row_count,
  withheld_count, result_hash, access_level,
  outcome [ok|refused_off|refused_not_enabled|refused_level])` — never stores returned data;
  `withheld_count` is how many descriptions the call withheld
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
  base64 blobs, URLs) is withheld and replaced by the marker
  `[withheld: N characters that read like instructions]`, and the app counts what was
  withheld (`mcp_access_log.withheld_count`).
- **Impact preview.** The inbox shows what approving each suggestion would change (e.g. how
  many past transactions a rule would recategorise), so an overly broad rule or a fake
  transfer pair planted by injected text is visible before approval.
- **Reports.** Saved reports render as a markdown subset: no HTML, no images, links shown as
  plain text, so a report can't trigger a network request. `list_reports` marks them
  `ai_authored`, since stored AI text is untrusted when read back, and strips control,
  zero-width and bidi characters from the body (decision record, 2026-11-10).
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
- **Rules**: rules in priority order with their actions, apply-to-history preview, merchant
  names, and the history of changes to existing transactions (undo).
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
