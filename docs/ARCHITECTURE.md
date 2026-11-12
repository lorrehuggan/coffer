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
| Transfers | Auto-detect opposite-amount pairs across own accounts within 3 days (`TransferWindow::DEFAULT`); user confirms; excluded from income/spend |
| Pots / sub-accounts | Monzo Pots, Starling Spaces, Revolut Vaults etc. are sub-accounts of their parent. A preset's `[sub_account_transfer]` identifies pot transfers; the import commit creates the pot when its bank ID is new, mirrors the row onto it (`origin = mirrored`) and pairs the two as a confirmed transfer. Pots count toward net worth (opening balance entered once per pot). See "Decision record: sub-accounts" |
| AI integration | `coffer mcp` stdio MCP server (`rmcp`). Read-only query tools + inbox-only writes. No account numbers or PII in tool output |
| Splits | A transaction can be split into lines that sum exactly to its amount, in its currency (enforced in core, in the same DB transaction as the edit). Reports, budgets and MCP read the `ledger_line` view: one row per split line, or the transaction itself when it isn't split |
| Tags and notes | Many tags per transaction, alongside categories. Tag names are user text: MCP sees them only at `standard` access, and never for tags marked private. The imported note stays read-only; the user's note is a separate field. Neither note is exposed over MCP by default |
| Manual entries and corrections | Manual rows have `origin = manual` and no import batch. Imported source fields (date, amount, raw description, external ID) are never edited: a bad import is undone, or a row is excluded with a recorded reason and stays visible under an "excluded" filter. User-facing fields (category, display merchant, tags, notes, splits) are editable. Manual rows are the user's own data: every field is editable and they can be deleted. Exclusion reasons and their effect on balances: see the decision record "Transactions additions" |
| Merchant names | `merchant_display` is derived: deterministic clean-up of the raw text, then the merchant alias table, then rule actions. It can be recomputed at any time; the raw text is untouched. A name set by a rule or the user (`merchant_display_source`) is never recomputed. See "Decision record: splits, tags and notes, manual rows, merchant names" |
| Refunds, reimbursements, shared costs | `transaction_link` ties a refund to its purchase (it nets against the purchase's category instead of counting as income), a reimbursement to the expense it repays, and a settlement to a share label. `my_share_minor` (default: the full amount) gives personal and gross views; the difference builds an owed-to-me balance per local share label. Share labels never leave the app. Still single-user: no one else's accounts are modelled |
| Duplicates | Exact duplicates are prevented at import (source ID, then fingerprint). Near-duplicates (same account and amount within ±N days, different text) become `duplicate_candidate` rows for review. Merging excludes one row, never deletes it, and can be undone |
| Reconciliation | Every `balance_snapshot` is an assertion checked against the computed balance; a failure reports the window since the last passing assertion. `account.reconciled_through` locks history: imports, manual edits, merges or undos touching locked dates are flagged and need an explicit unlock |
| Bulk changes | Changes to many existing transactions (a rule applied to history, duplicate merges, bulk edits) are previewed, then committed as an undoable `change_batch` that records old and new values, like import batches |
| Schedules and forecast | A detected recurring series becomes a schedule only when the user confirms it. The forecast is the last known balance plus schedule occurrences (plus the remaining flex budget), per account per day, in integer arithmetic, using cached FX rates only |
| Budgets | Per-category monthly amounts with optional rollover. Each budget is in a group: fixed, non-monthly (accrues 1/12 of a yearly amount, seeded from yearly schedules) or flex |
| Asset and liability accounts | Account kinds `asset` and `liability` hold manually valued items (house, pension, mortgage, loan). Valuations are `balance_snapshot` rows with source `valuation`; net worth uses the last value on or before each date. Investment holdings and prices are deferred (ROADMAP L.14) |
| Backups and export | Rotating `VACUUM INTO` snapshots (0600) in `<data dir>/backups` (0700) before every import commit, migration and restore; by default the last 10 automatic backups plus the newest of each earlier month are kept, and manual ones are never deleted. Restore runs `PRAGMA integrity_check` and a schema-version check on a copy before swapping files (see "Decision record: local backups"). Export is CSV/JSON with a documented schema (split lines carry their parent ID); Beancount/hledger journal export follows in M5.8. Local files only, no network |
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
9. **Rule definitions are validated in core before they're stored** (`coffer_core::rule`).
   A `regex` pattern must compile case-insensitively within a 1 MiB compiled-size limit
   and a nesting depth of 32. Every pattern is at most 1,000 bytes. Errors never repeat
   the pattern. Each action's `value_json` has one strict shape, which refuses unknown or
   repeated fields, wrong types and floats: `set_category` is a category key string;
   `set_merchant` and `set_note` are non-blank strings; `add_tag` is a non-empty array of
   distinct tag names; `exclude` is a reason key, never `other` (a rule carries no note);
   `split_template` is an array of two or more `{"category", "basis_points"}` lines in
   distinct categories, each above 0 and summing to exactly 10000; `my_share` is
   `{"basis_points": 0..=10000}`. Shares are integer basis points, and the engine turns
   them into minor units. Amount bounds are inclusive and compare against the signed
   `amount_minor`. A test keeps the Rust enum keys equal to the schema's CHECK lists.

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

### Decision record: categories (2026-11-11)

M1.14, the default category set. The set itself is under "Categories" below.

1. **The defaults are seeded by a migration** (`V2__default_categories.sql`), not by Rust
   at every open. A migration runs once, in the same transaction as the schema, and is
   checksummed like any other, so a default the user deleted never comes back and
   `coffer mcp`, which never writes the schema, needs nothing extra.
   `coffer_core::category::DEFAULT_CATEGORIES` is the same list in Rust (a test keeps
   them equal) for code that has no database, such as the bundled presets' tests.
2. **A category has a kind: `spending`, `income` or `transfer`** (`category.kind`).
   Reports need to know which side a category is on, and so does the "moves income into
   a spending category" check on wide rules (Inbox impact preview, point 2). A
   subcategory has its group's kind, and a kind never changes, since changing it would
   move a category's whole history between income and spending. Changing it means a new
   category and moving the rows (an undoable change batch). A new category without a
   kind is `spending`.
3. **Keys are forever; labels belong to the user.** Renaming changes the label only (the
   V1 trigger already makes `key` and `parent_id` read-only), so presets, rules, budgets
   and MCP clients keep working. Default labels are English (en-GB) and keys name the
   concept, not a country's word for it: `home.property_tax` is labelled "Council tax",
   and an en-US label would say "Property tax". Translating default labels (until the
   user renames one) can key off `DEFAULT_CATEGORIES`.
4. **User categories share the key space.** A category the user adds (M3.7) gets its key
   from its label when it's created (lowercase ASCII letters and digits, other runs as
   `_`, a numeric suffix if taken, a fallback when nothing is left), and the key doesn't
   follow later renames. It can be a new group or go under any group, taking the group's
   kind. A later Coffer that adds defaults inserts each key only where it's free, so a
   user's category with the same key is kept as it is.
5. **Defaults are ordinary rows.** The user can rename them, and can delete one that
   nothing uses. Deleting is refused while a transaction, split line, budget or
   schedule points at it (foreign keys) or a rule action or preset names its key (core
   checks rules; an unmatched preset key just leaves rows uncategorised). Hiding a used
   category instead of deleting it is for M3.7 to design.
6. **No category for "uncategorised", refunds or reimbursements.** Uncategorised and
   split rows have no `category_id`. A linked refund takes its purchase's category and
   a reimbursement nets against its expense (`transaction_link`); money in under a
   spending category lowers that category's spending and isn't income.
7. **Bundled presets map only to default keys**, checked by a test in
   `coffer_import::preset`. Loading a preset doesn't check keys against the defaults,
   since a preset the user saves (M3) may name their own categories; the import maps a
   key that isn't in the database to nothing and leaves the row for rules or the user.

### Decision record: import commit, dedupe and undo (2026-11-11)

Decided while building M1.11–M1.13 (`coffer_core::import`, `coffer_core::balance`,
`coffer_core::account`); the details are under "Import pipeline" → "Dedupe,
reconciliation and the commit".

1. **Preview, then commit what was previewed.** `import::preview` reads the database and
   writes nothing; `import::commit` plans again inside its own `BEGIN IMMEDIATE`
   transaction and refuses (`Stale`) if the plan or the account differs from the
   preview. What the user confirmed is exactly what is stored, even if another process
   imported in between.
2. **Flagged rows are recorded, not stored as transactions.** A row that can't be read
   has no date or amount to store, so "imported as flagged" (`DESIGN.md` → Import) means
   the batch keeps an `import_flag` row (its place in the file and core's reason, never
   its text; a new migration). The same goes for a source ID already taken by a
   different transaction, a row in a locked period, and a running balance that doesn't
   follow from the rows before it (that row itself is imported). Committing with flags
   or a failed reconciliation needs an explicit confirmation from the caller. The Review
   screen's "Import N transactions" counts new rows only; flagged rows are listed
   beside it.
3. **A source ID with a different date or amount is a conflict, not a duplicate.** It is
   flagged and not imported. The same ID, date and amount with different text is the
   same transaction (banks re-word descriptions). A repeated ID within one file is
   "repeated in file" when date and amount agree, otherwise flagged.
4. **Re-importing a file stores nothing.** With no new transaction and no new balance,
   `commit` returns `NothingNew` and records no batch. A balance from a file is new
   unless an earlier import recorded the same balance on the same day; undoing that
   earlier batch takes the balance with it, and importing the file again restores it.
5. **From running balances, only the opening is recorded.** A file with a balance column
   is checked row by row (in whichever order fits: oldest or newest first), and records
   one `balance_snapshot`: the balance at the end of the day before its first row. The
   last day of an export taken during the day is incomplete, so an end-of-day balance
   for it would fail once that day's later rows arrive. The next file's opening checks
   this one's rows. Balances the file states (OFX `LEDGERBAL`) are recorded as given;
   the importer is responsible for their date being an end of day.
6. **The manual anchor is an entered balance.** `account::add_balance` records a
   `manual` (or `statement`) `balance_snapshot`: an end-of-day balance, one per account
   and day. It is checked like every other balance (Review queues, point 1). The
   account's `opening_balance_minor` / `opening_date`, when set, count as one more
   end-of-day balance on that date, so nothing entered there is ignored; core itself
   records entered balances as snapshots.
7. **Undo removes the batch's rows with everything on them.** `import::undo` deletes the
   batch's transactions (mirrored pot rows included), balances and flags and marks it
   `undone`, in one DB transaction. The user's edits on those rows (category, notes,
   share, exclusion, splits, tags, a user-set name; later links) go with them:
   `undo_preview` counts the edited rows so the confirmation can say so, and importing
   the file again brings the rows back without the edits. Undo deletes split lines and
   tag links first (M1.20, M1.21); tables that will reference a transaction must be
   cleared by undo when they arrive, and until then a reference makes the undo fail as
   a whole.
8. **Undo dissolves transfer pairs.** A `transfer_group` with one of the batch's rows is
   deleted and its other rows become ordinary transactions again, to be paired again
   when the file is re-imported (M1.16). A pair with one side gone isn't a transfer.
9. **Undo names later imports that relied on the batch.** A later overlapping import
   skipped the rows this batch already had, so undoing it removes them from the
   account. `undo_preview` lists later committed batches into the same account;
   importing those files again brings the rows back.
10. **Locks hold.** A new row dated on or before `reconciled_through` is flagged and stays
    out; undo refuses a batch with a transaction or balance on or before its account's
    lock date. Both need the user to unlock first (M1.27).

### Decision record: rules engine, transfer detection and the CLI (2026-11-11)

Decided while building M1.15, M1.16 and M1.18 (`coffer_core::rule`,
`coffer_core::transfer`, `coffer_app::cli`) and wiring them into `import::commit`. The
transfer window and the order of preset map and rules were decided by the user.

1. **Rules override the preset's category map.** On import, a new row first gets the
   category its preset maps its source category to (`category_source = preset_map`;
   the CSV importer puts it on `StatementLine::category`). The rules then run on the
   batch, and a matching `set_category` rule replaces it (`category_source = rule`,
   `rule_id`). Rows no rule matches keep the preset's category. Rules never change a
   `user` or `ai_approved` category (rules decision record, point 3). A preset key with
   no category (the user deleted it) leaves the row uncategorised.
2. **Transfer window: 3 days, both ends included** (`TransferWindow::DEFAULT`; at most
   31). A pair is two rows on different accounts (pots and their parent included; never
   `asset` or `liability`), in one currency, with exactly opposite non-zero amounts, at
   most the window apart, neither excluded, neither already in a transfer group of any
   status (a `rejected` group keeps its rows, so a rejected pair isn't suggested again),
   and neither dated after its account's `closed_on`. Each pair becomes a `suggested`
   `transfer_group`. Detection never changes categories.
3. **Closest pairs first.** Candidate pairs are taken in order of the gap in days, then
   the gap in time when both rows have one (a pair with both times first), then the
   lower and higher transaction IDs; taking one removes its rows from the rest. The
   result depends only on row values and IDs, so it is deterministic, and two identical
   transfers each way on one day pair one-to-one. This doesn't maximise the number of
   pairs: out on the 10th and the 12th with one deposit on the 12th pairs the 12th.
4. **Rules, preset categories and transfer detection run inside the import's DB
   transaction,** in that order, on the batch's rows only (mirrored rows included). An
   error in any of them rolls the import back: a rule whose category no longer exists
   fails the import when it matches a row (not when it matches nothing), and the user
   fixes the rule. The rule set is loaded once per commit.
5. **Matching.** `is`, `starts_with` and `contains` compare text folded per character
   (lowercase, uppercase, lowercase again: `Straße`, `STRASSE` and `strasse` are equal,
   final `ς` is `σ`) without Unicode tables. `regex` uses the regex crate's
   case-insensitive mode, so a regex `straße` doesn't match `STRASSE`. The description
   is matched with whitespace collapsed but its case kept, so a regex sees the letters
   the bank wrote; patterns are compared as written. A missing field (no merchant)
   never matches.
6. **Rule store.** A new rule goes first; `create_at` and `move_to` place one at a
   position, and every write renumbers priorities 0, 1, 2, … in order. A rule naming a
   category or split category that doesn't exist is refused when saved. Errors about a
   stored rule that no longer validates name its ID and the action kind, never its
   text. Deleting a rule clears `rule_id` and keeps the category (rules decision
   record, point 7).
7. **`coffer import` acts only on a confident, unique detection.** OFX by its format; a
   preset only by its own header row (`Detection::Source`). A CSV that merely parses,
   or a tie, is an error that lists the presets for `--preset`. A multi-statement OFX
   file needs `--statement N` (from 1); without it the command lists the statements by
   kind, the account number's last four digits, currency and row count. `--account`
   takes an ID or an exact name; without it, the import goes to the only open account in
   the file's currency that holds transactions, else the command lists them. (A pot
   counted until M1.17; it no longer does: "Decision record: sub-accounts", point 7.)
8. **The CLI confirms with `--yes`, and `--dry-run` writes nothing.** An import with
   flagged rows or a balance mismatch is refused (exit 1) without `--yes`; `undo-import`
   without `--yes` previews and exits 0. `accounts` (list) and `undo-import` need an
   existing database; `import` and `accounts add` create it. Opening applies pending
   migrations, as the app does. `accounts add` checks the pot rules (a pot needs a
   current or savings parent in its currency; only a pot has a parent) before the
   schema does, for readable messages.
9. **`anyhow` in `coffer_app::cli`.** The CLI is the binary's layer but lives in the
   `coffer-app` lib so tests can run commands against a scratch database and read their
   output; it uses `anyhow` like the binary. Library crates (`core`, `import`, `mcp`)
   still use `thiserror` only. The CLI sets up no logging and never prints row text it
   wasn't asked for (a test checks fixture descriptions don't appear).

### Decision record: sub-accounts (2026-11-11)

Decided while building M1.17 (`coffer_core::sub_account`, `V4__sub_account_key.sql`) and
wiring it into `import::commit`. The preset format is under "Presets".

1. **Pots are found by key, never by name.** `account.sub_account_key` (V4) holds the
   bank's ID for the pot (Monzo's `pot_…`), only on pots, unique per parent, and
   read-only once set. A key the parent has no pot for creates one: kind `pot`, the
   parent's currency and institution, named from the latest-dated row that names it (the
   later row on a tie), else after the key. Existing pots keep their names, so a pot
   renamed at the bank stays one account. The same key under another parent is another
   pot. A pot the user created by hand has no key and isn't matched; the import creates a
   keyed pot beside it (linking the two is for the accounts screen, M3).
2. **The mirror runs inside the import's DB transaction,** after the batch's rows are
   stored and before rules and transfer detection, which both see the mirrored rows.
   Each mirrored row is the parent row seen from the pot: same batch, date and time,
   opposite amount and original amount, the parent's other source fields, note and
   category, `origin = 'mirrored'`. The preview plans it (`ImportPreview::sub_accounts`:
   pots to create, existing and closed pots, rows to mirror), and the plan is part of the
   stale check.
3. **The mirrored row keeps the parent's `external_id` and `fingerprint`,** so the pot's
   dedupe indexes hold at most one mirror per parent row; mirroring a row twice fails.
4. **The pair is `confirmed` from the start** (`transfer::record_confirmed_pair`): the
   bank says it's a transfer. Detection skips both rows, since they are grouped.
5. **All or nothing.** Pot transfers into an account that isn't current or savings, or a
   pot locked on or before one of its rows, fail the import, in the preview already;
   half a transfer is never imported. A closed pot still takes its rows, and the preview
   says so. A matching row without a pot ID is flagged (`RowProblem::SubAccountKeyMissing`).
6. **Counts and undo.** `CommitSummary::transactions` and the category counts are the
   file's rows; `mirrored` and `pots_created` are counted apart. `BatchInfo::transactions`
   includes mirrored rows. Undo deletes the mirrored rows with the batch and dissolves
   their groups; `UndoPreview::transfers` counts only pairs with a row outside the batch,
   since both sides of a pot pair go. Created pots stay (the user may have entered an
   opening balance), and importing the file again finds them by key. Re-importing the
   same file mirrors nothing, because its rows are already imported. Files imported
   before M1.17 keep their pot rows without a pot side; undo and import them again to
   mirror them.
7. **Pots aren't a default import target.** `coffer import` without `--account` no
   longer counts pots: their rows come with the parent's export, and once an import has
   created pots the parent must stay the only candidate. A pot still takes a file named
   with `--account`.

### Decision record: local backups (2026-11-11)

Decided while building M1.28 (`coffer_core::backup`, `coffer backup`, `coffer restore`).
Retention and location were decided by the user.

1. **Where and how.** Backups are `VACUUM INTO` snapshots in `backups/` next to the
   database (`<data dir>/backups`, `0700`; files `0600`), derived from the database
   path, so `COFFER_DATA_DIR` moves them too. Each is written to a hidden temporary name,
   fsynced, then hard-linked to its final name (a rename after checking the name is free
   where hard links aren't supported), so a partial file never looks like a backup and no
   backup is overwritten.
2. **The name is the metadata.** `coffer-<UTC yyyymmddThhmmss.mmmZ>-<reason>.db`, reason
   `import`, `migration`, `restore` or `manual`. Only names that parse exactly are
   backups; nothing else in the directory is listed, rotated or deleted. The schema
   version is read from the file, not the name.
3. **Rotation.** Automatic backups (import, migration, restore) keep the 10 most recent
   plus the newest of each earlier calendar month (UTC). Manual backups (`coffer
   backup`, "Back up now") are never deleted automatically. A failed rotation is logged
   and doesn't fail the backup.
4. **When.** `import::commit` takes one before its DB transaction (`VACUUM INTO` can't run
   inside one), only when it will write: not for nothing new or a missing confirmation,
   and never for a preview or `--dry-run`. `Database::open` takes one before applying
   pending migrations to an existing database (not to a new one). A restore takes one of
   the current database first. If the backup fails, the import or migration doesn't
   happen.
5. **Restore checks a copy first.** The backup is copied next to the database and the
   copy is checked: `PRAGMA integrity_check`, a migration history matching this build's,
   and a schema no newer than it. The user's backup file is only read. An older schema is
   restored and then migrated by `Database::open` (which backs the restored file up as a
   `migration` backup first). `coffer restore` without `--yes` shows the check and
   changes nothing.
6. **Restore never skips its safety backup.** If the current database can't be backed up
   (it's damaged), the restore is refused, and the user moves the file aside first. Its
   `-wal`/`-shm` are checkpointed and removed before the rename, never after. A database
   still open elsewhere (its `-wal` survives our close) is refused. A refused restore
   leaves the current database untouched. No other Coffer process may have the database
   open during a restore; MCP servers refusing calls during the swap come with M2.

### Decision record: importer paths and locked balances (2026-11-11)

From M1.19, which added end-to-end tests for every importer path (CSV, OFX/QFX,
presets, dedupe, reconciliation, commit, undo, CLI; `crates/import/tests/*_paths.rs`,
`crates/app/tests/cli_paths.rs`) and found one bug.

1. **A balance from the file in a locked period is left out.** A running-balance opening
   or an OFX `LEDGERBAL` dated on or before the account's `reconciled_through` is
   `FileBalanceStatus::Locked`: shown in the preview, not recorded and not checked, like
   a row in the locked period. Recording it added a check inside the lock and made the
   batch impossible to undo without unlocking; a file wholly inside the lock now imports
   nothing.
2. **Test presets aren't bundled.** Mappings that read the generic CSV fixtures live in
   `fixtures/csv/mappings/<region>/*.toml` and are loaded by the tests only.

### Decision record: splits, tags and notes, manual rows, merchant names (2026-11-12)

M1.20–M1.23 (M1 wave 3). Migrations `V5__transaction_split.sql`, `V6__tags.sql` and
`V7__merchant_alias.sql`; M1.22 needed none. Points 1 and 2 were decided by the user.

1. **A new, edited or deleted merchant alias applies at once, to derived names only.**
   `transaction.merchant_display_source` (V7) is NULL when the name is derived (the
   bank text cleaned, then the aliases), `rule` when a rule action set it (M1.24) and
   `user` when the user did (`merchant::set_user_name`). Alias changes recompute every
   derived row in the same DB transaction, with no batch (rules decision point 5), and
   never touch a `rule` or `user` name. A set name can't be blank.
2. **Manual rows are allowed on every account that holds transactions**: current,
   savings, credit card, pot and cash; never on `asset`/`liability` (accounts decision
   point 2). A manual row has no dedupe keys, so one entered ahead of the bank's export
   is matched later by near-duplicate review, not by the import.
3. **Splits** (`coffer_core::split`). Two or more lines, numbered from 1, each non-zero
   with the parent's sign, summing exactly to the parent's amount in its currency; lines
   may be uncategorised. Setting a split replaces all lines (their IDs change) and clears
   the parent's category, `rule_id`, share and share label: each line carries its own.
   `reimbursable`, notes and the exclusion stay on the parent. Removing a split leaves the
   row uncategorised. The schema guards what one row can: while a transaction has lines
   it takes no category or share and its amount and currency don't change (a manual
   row's edit is refused in words first, `manual::Error::Split`). A row in a suggested
   or confirmed transfer can't be split, a group holding a split row can't become one,
   and transfer detection skips split rows. A row in a `rejected` group, or an excluded
   row, can be split. Applying rules to history (M1.24) must skip split rows.
4. **`ledger_line` doesn't filter.** It has every transaction (or its lines), with
   `excluded_reason`, `transfer_status` and `my_amount_minor` (the share, or the whole
   amount) for each report to filter and sum by; the balance, the excluded filter and
   transfer views read the same rows.
5. **Tags** (`coffer_core::tag`). Names are trimmed, keep their case, are at most 64
   characters with no control characters, and are unique ignoring case with the same
   folding as rule matching (`Straße` and `STRASSE` are one tag; SQL `NOCASE` backs it up
   for ASCII). `rule::TagList` uses the same `TagName`, so an `add_tag` value repeating a
   name in another case is refused. Adding a tag twice does nothing; deleting a tag
   removes its links. `transaction_tag` is a `WITHOUT ROWID` link table whose rows are
   only inserted or deleted. Private tags never leave the app: anything that does (MCP,
   exports) reads `tag::public_tags_of` / `without_private`.
6. **User notes** (`coffer_core::note`). Trimmed, line breaks kept, at most 2,000
   characters (checked in Rust); blank clears. Any origin can carry one; the imported
   `note` stays read-only.
7. **Manual rows** (`coffer_core::manual`). Create, read, edit (every field the entry
   holds; other user fields stay) and delete `origin = manual` rows; imported and
   mirrored rows are refused. Locks guard balances only (rules decision point 8): a
   change to the account, date, time, amount, merchant or description, or a create or
   delete, on or before `reconciled_through` (old or new date) is refused; a
   category-only edit there isn't. A row can't be dated after its account's
   `closed_on`. Changing a paired row's account or amount, or deleting it, dissolves its
   transfer group. Deleting clears its split lines and tag links first
   (`manual::release_references`, where later referencing tables join).
8. **Exclusion** (`coffer_core::exclusion`) applies to any row, with the reason, note
   and date of "Transactions additions" point 1. Excluding again changes the reason and
   resets the date. Only a change into or out of `duplicate` is refused inside a lock,
   since only `duplicate` moves the balance.
9. **Merchant clean-up** (`merchant::clean`) is deterministic and keeps the bank's case
   (title-casing gets McDonald's, IKEA and eBay wrong and can't be undone; comparisons
   ignore case). It strips processor prefixes (`SQ *`, `SUMUP *`, `IZ *`, `ZETTLE_`,
   `PAYPAL *`, `CRV*`, `SP `…), payment words (CONTACTLESS, CARD PAYMENT, APPLE PAY…),
   card references and masked numbers, and trailing dates, times, references and long
   digit runs, repeating until nothing changes; it never empties a name. The source text
   is the bank's merchant, else the raw description. `SP ` also strips a real name that
   starts with "SP "; an alias fixes such a case.
10. **Aliases** match the cleaned text ignoring case and runs of whitespace: `exact`, then
    the longest `prefix` (a trailing space marks a word boundary; ties go to the older
    alias), then the first `regex` by ID (oldest first; no order column yet: adding one
    is a plain `ALTER TABLE`). Regexes are validated like rule patterns. Duplicate
    aliases are refused. `merchant_alias.created_at` gives the inspector its date.
11. **Rules match the cleaned merchant.** `MatchField::Merchant` compares
    `clean(merchant)`, as the rules decision said, so names never change which rules
    match. This changed existing behaviour: a merchant rule written against processor
    noise (`starts_with "SQ *"`) no longer matches; one written against the clean name
    now matches the noisy rows. The description field is unchanged.
12. **Names run before rules in the import commit** (after the pot mirror, so mirrored
    rows are named too), so a rule's `set_merchant` (M1.24) has the last word. Opening a
    database from before V7 names every existing row once, right after the migrations,
    in its own DB transaction (refinery's can't run Rust); if that fails,
    `merchant::recompute_all` repairs it.
13. **Undo and deletes clear what references a transaction.** `import::undo` and
    `manual::delete` delete split lines (`split::clear_batch` / `clear`) and tag links
    (`tag::clear_batch` / `clear`) before the rows. `undo_preview`'s `edited` count
    includes rows with a split, tags or a user-set name.

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
`coffer import <file>` (headless import, used from M1), `coffer backup [--list]` /
`coffer restore <file> [--yes]` (M1.28), `coffer export`.

The GUI and MCP processes share the SQLite file; WAL allows concurrent readers.

## Dependencies

Versions live in `[workspace.dependencies]` in the root `Cargo.toml`; crates opt in with
`<crate>.workspace = true`. Chosen 2026-11 (crates.io research); revisit when pins lift.

| Area | Crates | Notes |
|---|---|---|
| Storage | `rusqlite` (`bundled`), `refinery` | `rusqlite` pinned to 0.40 until `refinery` supports 0.41. `refinery` checksums applied migrations, enforcing "never edit a shipped migration". No connection pool: one connection per process, WAL + `busy_timeout` + `BEGIN IMMEDIATE`; the app and the CLI open with `Database::open`, which creates the database if needed and runs migrations (all pending ones in one transaction); `coffer mcp` opens with `Database::open_existing`, which never creates or migrates and refuses a missing or older database, telling the user to open Coffer or run a CLI command. Both check an existing file first (a SQLite database, not a newer schema, applied migrations matching the embedded ones) and leave a refused file untouched: no permission change, WAL switch or write. SQLCipher later = `bundled-sqlcipher-vendored-openssl` |
| Import | `csv`, `serde`, `encoding_rs`, `chrono`, `toml` | Encoding comes from the preset, the OFX header or a BOM; without any, only UTF-8 is accepted, and a file that doesn't decode is refused, never guessed. The BOM is stripped before `csv`. Dates must round-trip through their format string. OFX is read by Coffer's own tolerant SGML/XML reader, not `ofx-kit` (dropped in M1.10, see "The OFX importer") |
| Money / FX | none | Own `Money { minor: i64, currency }` (`coffer_core::money`, M1.5) with a hand-written, test-frozen exponent table. FX in `i128` fixed point with one explicit rounding. Decimal types from format crates are converted to minor units at the import boundary, never stored. See "Money" below |
| Rules | `regex`, `aho-corasick`, `serde_json` | `regex` is linear-time, so user rules can't hang the app. `serde_json` (already in the tree, MIT OR Apache-2.0) decodes `rule_action.value_json` strictly in core |
| Plumbing | `thiserror` (libs), `anyhow` (binary), `clap`, `tracing`, `tracing-subscriber`, `directories`, `uuid` (v7) | Logs go to stderr |
| MCP | `rmcp` (`transport-io`), `tokio` (current-thread), `schemars` | Only the `mcp` crate / `coffer mcp` path uses tokio; `core` stays synchronous |

Later: `ureq` + `roxmltree` for the ECB fetch, `mt940`, `camt053` (no default features),
own QIF parser. Avoid GPL crates (`ofxy`, `tree_magic_mini`'s GPL data feature).

## Import pipeline

1. **Detect** format (extension + sniffing) and encoding (a BOM, else the stated
   encoding: UTF-8, UTF-16, Windows-1252/Latin-1, Latin-9; else UTF-8 only).
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
   on or before the account's `reconciled_through` lock (and leave out the file's
   balances dated then); take a backup (M1.28); commit in one DB transaction; record an
   `import_batch` so an import can be undone.
7. **Post-process** (deterministic, no model, inside the commit's DB transaction): map
   source categories via the preset; mirror sub-account transfers onto their pots
   (M1.17); clean merchant text and apply merchant aliases into `merchant_display`
   (M1.23); apply rules (which can match the cleaned merchant, raw description, source
   category, source type, amount, account) and their actions, a rule's category
   overriding the preset's and its merchant overriding the alias; run transfer-pair
   detection; match schedule occurrences; queue near-duplicate candidates; record
   balances and check assertions. Everything up to transfer detection runs today
   ("Decision record: sub-accounts"; "Decision record: rules engine, transfer detection
   and the CLI"; "Decision record: splits, tags and notes, manual rows, merchant
   names").

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

### The CSV importer

M1.7, decided 2026-11-10. `coffer_import::csv`: `CsvImporter` reads files through a
`CsvMapping`, the Rust form a preset (M1.8) or the mapping screen (M3) produces.
`CsvImporter::new` refuses a mapping that can't work for any file (no description column,
the same debit and credit column, a named column without a header row).

- **Encoding is never guessed.** A byte order mark wins (UTF-8, UTF-16LE/BE) and is
  stripped. Otherwise the mapping's encoding is used; `Auto` means UTF-8 only.
  Single-byte encodings accept any bytes, so picking Windows-1252 for a file that isn't
  would silently change accented letters, and with them the dedupe fingerprint. Decoding
  is strict (no replacement characters), and NULs in the text (UTF-16 without a BOM)
  fail the file. A preset or the user states non-UTF-8 encodings.
- **Delimiter**: stated, or detected from the first 50 records among `,` `;` tab `|`.
  Records whose split can't be trusted (below: an unclosed quote, or several lines when
  the mapping doesn't allow it) are left out, since how a stray quote merges lines
  depends on the delimiter. The first remaining record sets the width (at least 2). The
  candidate under which most other records have the same shape (the same count, or
  extra trailing empty fields on either side) wins, then the wider one; a tie is
  refused, so the mapping has to say.
- **Layout**: `skip_rows` raw lines before the header (account summaries), then an
  optional header. Named columns match header cells exactly after trimming; a missing or
  duplicated name fails the file (`ImportError::MissingColumn` / `DuplicateColumn`; the
  name comes from the mapping, so it can be shown). Files without a header use 0-based
  positions. Line numbers are physical lines, counted from byte offsets with `\r\n`,
  `\n` and a lone `\r` (old Mac exports) each ending one line, both there and in
  `skip_rows` (the `csv` crate's own count only knows `\n`), so flagged rows can be found
  in the file.
- **A skip can't end inside a quoted field** (decided 2026-11-11). `skip_rows` stays
  physical lines, not records: a preamble is often not CSV (free text, stray quotes, other
  column counts), and a record count would let one stray quote swallow the header. But a
  summary cell with a line break in its quotes is two lines, and a count that ends
  between them would read the rest of the cell as the header or first row (a confusing
  missing-column error, or rows merged and flagged at a summary line). So the skipped
  lines are read as CSV and, if they end inside a quoted field, the file fails
  (`ImportError::Malformed`, at the line the field opens on). Only a `"` at the start of a
  field opens one (the `csv` crate's rule), so `Account: 6" pizza fund` is text. Field
  starts depend on the delimiter, and a closing quote at the start of a line opens a field
  under any other, so the check uses the delimiter the file is read with: stated or
  sniffed from the text after the skip. Only when sniffing fails does an open field under
  any candidate count, since the broken skip is then the likelier cause and says more.
- **A row with the wrong column count is flagged** (`RowProblem::ColumnCount`), not
  read: an unquoted delimiter in a description shifts every later field, and reading it
  would import a wrong amount. The file's width is the header's cells up to its last
  non-empty one, or without a header the first trustworthy row's count. Extra trailing
  empty fields on a row are fine, so a delimiter at the end of the rows, of the header,
  or of both reads; a value under an unnamed trailing header cell is flagged as an extra
  column rather than ignored.
- **One record per line unless the mapping says otherwise** (decided 2026-11-11). An
  unclosed quote makes the `csv` crate read the rest of the file as one field, and two
  stray quotes merge the lines between them, sometimes into a record with the right
  column count, so transactions would vanish into a description. So each record knows
  how many physical lines it takes and whether a quoted field is still open at the end
  of the file (`CsvRecord::lines`, `unclosed_quote`, following the `csv` crate's rules:
  a field is quoted only when it starts with `"`, so `6" pizza` stays literal).
  `CsvMapping::multiline_fields` (preset `multiline_fields`, default `false`) says
  whether the export really has line breaks inside quoted fields. Off, a data row over
  several lines is flagged `RowProblem::MultiLineRow { lines }` at the line it starts on;
  on, it's read. Either way a quote still open at the end of the file is flagged
  `RowProblem::UnclosedQuote { lines }`, never read as text. A header row like that
  fails the file (its columns can't be trusted).
- **Dates round-trip.** Formats are `strftime` patterns, checked when built
  (`DateFormat::new`: needs a year, month and day; may include a time; no time zones).
  chrono parses leniently (`%Y` reads `26` as year 26), so each value must format back
  to the same text, ignoring ASCII case, or the row is flagged. `DD/MM` against `MM/DD`
  is the mapping's: `DateFormat::candidates` lists every common format a column's values
  fit, and more than one means asking the user. A separate time column overrides the
  date's time when filled in.
- **Amounts** are read with `Money::parse_decimal` and the mapping's decimal mark, from
  a signed column or a debit/credit pair. A pair is `Unsigned` (positive amounts, money
  out negated; a negative value is flagged as unclear) or `Signed` (each value as
  written). A row filling both columns is flagged unless one of them is zero. An
  optional per-row currency column sets the currency amounts are read in; a row not in
  the statement's currency is then flagged by `Statement::push`.
- **Text**: a single description column is kept as written; several are trimmed and
  joined with a space, empty ones left out. An empty description is allowed. Optional
  text fields are `None` when blank, otherwise kept as written; the external ID is
  trimmed. The original amount is read in its own currency and dropped when it just
  repeats the amount (Monzo fills it on every row).
- **Detection**: `Source` when the header has every column the mapping names,
  `Format` for other readable CSV with at least two columns, `Extension` for an
  unreadable `.csv`/`.tsv`/`.txt`. Two presets that both find their columns tie, and the
  caller asks. A stray quote in a data row doesn't change what the header says, so such
  a file still detects (and its import flags the row); a header-less file needs at least
  one trustworthy row for `Format`.
- `CsvFile` (the `Parsed` type: encoding used, delimiter, header, records with line
  numbers and line counts) is public for the mapping screen. Its `Debug` and
  `CsvRecord`'s show counts
  only.

### Presets

M1.8, decided 2026-11-10. `coffer_import::preset`: a preset is a `CsvMapping` in TOML plus
an ID, a name, a region, ignored columns and a category map. The field reference for
contributors is `presets/README.md`.

```toml
# presets/uk/monzo.toml (M1.9; comments and most of the category map left out)
id = "uk-monzo"
name = "Monzo"
region = "uk"
currency = "GBP"
encoding = "utf-8"                                 # default "auto"
delimiter = ","                                    # default: detected
skip_rows = 0
header = true                                      # false: columns are 1-based positions
multiline_fields = true                            # notes can span lines; default false
external_id = { column = "Transaction ID" }
date = { column = "Date", format = "%d/%m/%Y" }
time = { column = "Time", format = "%H:%M:%S" }    # optional
amount = { column = "Amount" }                     # or { debit = "Paid out", credit = "Paid in", signs = "unsigned" }
decimal = "."
row_currency = { column = "Currency" }             # per-row currency code
original_amount = { column = "Local amount", currency_column = "Local currency" }
merchant = { column = "Name" }
description = { column = "Description" }           # or { columns = ["A", "B"] }
source_category = { column = "Category" }
source_type = { column = "Type" }
note = { column = "Notes and #tags" }
# balance = { column = "Balance" }                 # Monzo exports have none
ignore = ["Emoji", "Address", "Receipt", "Category split", "Money Out", "Money In"]

[category_map]          # source category -> our category key
"Groceries" = "food.groceries"
"Eating out" = "food.eating_out"
"Savings" = "transfer.savings"
```

- **Strict.** Unknown keys are refused (`deny_unknown_fields`), so a misspelt optional
  field fails instead of being silently unmapped. Values that stand alone (columns, date
  and time formats, encoding, delimiter, currency, category keys) are checked while
  deserializing, so the error points at the line; the whole mapping is then checked by
  `CsvImporter::new`. `amount` and `description` take exactly one of their forms.
- **`multiline_fields`** (default `false`, decided 2026-11-11) maps to
  `CsvMapping::multiline_fields`: set it only for an export that really writes line
  breaks inside quoted fields (multi-line notes), since otherwise a row over several
  lines is the sign of a stray quote and is flagged (see "The CSV importer").
- **Positions count from 1** in TOML (`column = 3`), as the error messages do;
  `CsvMapping` stays 0-based.
- **Bundled.** `crates/import/build.rs` compiles every `presets/**/*.toml` into the binary
  (`Presets::bundled`), so an installed Coffer needs no source tree and a preset is
  never half-installed. Placement is checked at load, not in the build script, so a
  misplaced file fails the tests with a message rather than being skipped.
- **IDs are stable and findable.** The ID is recorded on `import_batch.preset_id`, so it
  never changes once shipped. It is `<region>-<file stem>` optionally followed by
  `-more` (`uk/monzo.toml` → `uk-monzo` or `uk-monzo-current`), lowercase letters,
  digits and single hyphens, and unique; the region must match the directory.
  `Presets::from_files` checks all of this for any `(path, text)` set, which is also
  what user presets saved from the mapping screen (M3) can load through.
- **Columns not read are visible.** `ignore` lists header columns the preset reads on
  purpose not at all; `Preset::unread_columns` returns the file's other unread header
  columns, so the preview can say what a bank added instead of dropping it silently.
- **Category map.** `CategoryKey` (`coffer_core::category`) is one or two segments of
  `[a-z0-9_]` joined by `.`, the shape of `category.key`. Source categories match
  exactly, case included, after trimming the row's value; map keys must already be
  trimmed. Whether a key exists is checked against the database's categories when the
  import maps them; a key it doesn't have, like an unmapped category, is left for rules
  or the user. Bundled presets map only to default keys (a test checks it; see
  "Categories").
  A `category_map` without a `source_category` column is refused.
- **Every bundled preset has a fixture** at `fixtures/presets/<region>/<bank>.*` that it
  detects as `Source` and imports; a test enforces it.
- **Monzo is the reference preset** (M1.9, decided 2026-11-11; `presets/uk/monzo.toml`,
  fixture `fixtures/presets/uk/monzo.csv`, tests in
  `crates/import/tests/preset_uk_monzo.rs`). Settled with it:
  - The ID is `uk-monzo`, with no account-type suffix: personal and joint current
    accounts export the same columns, and two presets with the same header would only
    tie at detection. A Monzo export with other columns gets its own file
    (`monzo-<kind>.toml`).
  - `multiline_fields = true`: "Notes and #tags" is free text from the app and can hold
    line breaks inside its quotes, and Monzo's writer quotes correctly, so a row over
    several lines there is a note, not a stray quote. The column count and the
    end-of-file check still catch broken quoting.
  - The signed `Amount` is read and `Money Out` / `Money In` are ignored (refunds can
    sit in "Money Out" as positive values). `Currency` is read as the row currency, so
    a row not in pounds is flagged rather than taken as GBP.
  - `Name` is the merchant (the pot's name for a pot transfer), `Description` the raw
    text (the pot's ID for a pot transfer). `[sub_account_transfer]` recognises pot
    transfers by `Type` = "Pot transfer" (M1.17).
  - `Address` is ignored, so merchant addresses never reach the database from Monzo.
  - Monzo's "Transfers" category isn't mapped: Monzo also files payments to and from
    people under it, which are spending or income. "Savings" (what Monzo gives pot
    transfers) maps to `transfer.savings`; Family and Finances map to the `family` and
    `finance` groups; "General" and "Expenses" are left for rules or the user.
- **`[sub_account_transfer]`** (M1.17, decided 2026-11-11): `kind` (`"pot"` only),
  `when = { column, is }`, `key = { column }`, optional `name = { column }`. A row whose
  `when` column, trimmed, is exactly `is` (case included; `is` must be trimmed and
  non-empty) is a transfer between the account and one of its pots. Its line carries
  `SubAccountRef { kind, key, name }` (trimmed), and the commit mirrors it onto the pot
  ("Decision record: sub-accounts"). A match with a blank key is flagged
  (`RowProblem::SubAccountKeyMissing`) rather than imported without its pot side. Its
  columns count as read (detection, `unread_columns`, `ignore`). Starling Spaces and
  Revolut Vaults become new `kind`s with their own columns. Monzo: `when = Type is "Pot
  transfer"`, `key = Description` (`pot_…`), `name = Name`.

### The OFX importer

M1.10, decided 2026-11-11. `coffer_import::ofx`: `OfxImporter` reads OFX 1.x (SGML) and
2.x (XML) bank (`STMTRS`) and credit card (`CCSTMTRS`) statements. QFX is OFX with
Intuit's extra tags and reads the same way.

- **Own reader, not `ofx-kit`.** `ofx-kit` (planned in M1.1) fails the whole document on
  one bad `STMTTRN` (an unreadable amount, a missing `FITID`), so it can't flag rows; it
  rewrites `,` to `.` in amounts, falls back to Windows-1252 when UTF-8 fails (a guess),
  and puts file text in its errors. It was removed with `rust_decimal`, its only user.
  The reader is a few hundred lines in `ofx/markup.rs`, handles both syntaxes, and
  keeps every value as text so amounts go through `Money::parse_decimal`.
- **Markup.** A start tag followed by text is a value, closed or not (SGML leaves values
  unclosed; XML and some SGML files close them). A start tag followed by another tag is
  an aggregate if it's a known OFX aggregate, an empty value if it's a known value, and
  otherwise an aggregate only if the file closes that name somewhere. Entities
  (`&amp;`, numeric) are decoded; a bare `&` is kept ("M&S"). Comments, CDATA,
  processing instructions and attributes are handled. Tag names are uppercased.
- **Structure is strict.** An aggregate left open, a closing tag for an aggregate that
  isn't open, text where only elements belong, more than one top-level element, a file
  that ends early (a truncated download), a `STMTTRN` outside a statement or inside
  another, and a statement-level value given twice (`CURDEF`, `LEDGERBAL`) fail the
  file with a line number. Guessing would move transactions between statements or lose
  them.
- **Encoding is never guessed.** A BOM wins. Otherwise OFX 1.x `ENCODING:UTF-8` (or
  `UNICODE`) is UTF-8; `ENCODING:USASCII` takes `CHARSET` (`1252` and Latin-1 are
  Windows-1252, `8859-15` is Latin-9, `NONE` is ASCII read as UTF-8, so other bytes
  fail). OFX 2.x uses the XML declaration's `encoding`, else UTF-8, as does a file
  without a header. Decoding is strict, as for CSV; another charset is refused
  (`ImportError::Unsupported`). A file whose header states the wrong charset reads
  wrongly or fails; that is the bank's statement, not a guess.
- **One statement per import.** `Statement` is one account in one currency, so a file
  with several statements (some banks put every account in one download) isn't split
  silently: `OfxImporter::new()` refuses it with `ImportError::MultipleStatements`, and
  `OfxFile::statements()` lists them (kind, currency, transaction count, last four
  characters of the account number) for the user to pick, then
  `OfxImporter::for_statement(index)` imports that one. Investment statements
  (`INVSTMTRS`) are listed but refused (`Unsupported`); holdings are L.14. Other
  message-set responses (closing statements, sign-on) are not transactions and are
  skipped.
- **Statement fields.** `CURDEF` is the statement's currency; missing or unknown fails
  the file. `LEDGERBAL` (`BALAMT` as of `DTASOF`'s date) is the statement's
  `DatedBalance`; one that can't be read fails the file, like a balance in another
  currency. `AVAILBAL` isn't used: it is the ledger balance less holds, or a card's
  available credit, so it would fail reconciliation. `BANKTRANLIST`'s `DTSTART`/`DTEND`
  and the account number are not stored.
- **Dates are taken as written.** `DTPOSTED` is `YYYYMMDD`, optionally `HHMM`, `HHMMSS`
  or `HHMMSS.XXX`, then optionally `[offset:zone]`. The date and time are the bank's
  local ones and the offset is not applied: converting to UTC would move evening
  transactions to another day than the bank's statement and CSV exports show, and break
  dedupe against them. A time is kept whenever the value has one (many banks write a
  placeholder such as `120000`; hiding it is the app's choice, not the importer's).
  Fractions of a second are dropped. `DTUSER` and `DTAVAIL` are not read.
- **Amounts** are `TRNAMT` through `Money::parse_decimal`, never rounded. OFX allows `.`
  or `,` as the decimal mark and no grouping, so a value with a comma and no point uses
  the comma (`1,234` is then too precise for GBP and flagged, not read as 1234). Signs
  are as the file states them: on a credit card a purchase is negative and a balance
  owed is negative.
- **Currencies.** A `CURRENCY` aggregate means `TRNAMT` is in its `CURSYM`, so the amount
  is read in that currency and `Statement::push` flags it when it isn't the statement's
  (converting with `CURRATE` would round). `ORIGCURRENCY` means `TRNAMT` is already in the
  statement's currency; the original amount isn't derived from the rate, for the same
  reason, so `original_amount` stays empty. Both on one transaction is flagged.
- **Text.** `NAME` (or `PAYEE`'s `NAME`) is the merchant. The description is `NAME`,
  `EXTDNAME` and `MEMO` joined with spaces, except that a piece that starts with the text
  so far replaces it (a 32-character `NAME` cut from the `MEMO`) and a piece the text
  already starts with is left out (the same text twice). No text at all is an empty
  description, as in CSV. `PAYEE`'s address, `CHECKNUM`, `REFNUM`, `SIC`, `PAYEEID` and
  `BANKACCTTO`/`CCACCTTO` are not read. `TRNTYPE` is the source type, as written.
- **`FITID`** is the external ID, trimmed; an empty or missing one is `None`, which
  leaves the row to fingerprint dedupe (M1.11). A `FITID` repeated within the file can't
  identify its rows and `(account_id, external_id)` is unique in the database, so every
  occurrence after the first is flagged (`RowProblem::DuplicateExternalId`, naming the
  first); the first is read as usual. Nothing is deduplicated here.
- **Flagged, not skipped:** a missing or unreadable `DTPOSTED` or `TRNAMT`, a value given
  twice, an unknown `CURSYM`, an amount in another currency, a repeated `FITID`, and a
  correction (`CORRECTACTION`, `RowProblem::Unsupported`): applying a `REPLACE` or
  `DELETE` to an earlier transaction would edit imported data, so the user sees it
  instead.
- **Locations.** Transactions are `SourceLocation::Entry(n)`, the n-th `STMTTRN` in the
  whole file in document order (so entries in a second statement don't restart at 1).
  File-level errors give the line.
- **Detection** is `Format` when the file starts like OFX (an `OFXHEADER` block, an XML
  declaration or `<?OFX …?>` followed by OFX, or a bare `<OFX>`), whatever its name;
  `Extension` for another `.ofx` or `.qfx`. OFX has no per-bank presets, so never
  `Source`. A generic CSV mapping doesn't tie with it on OFX files (tested).
- `OfxFile` (the `Parsed` type: version, encoding used, statements) and `OfxStatement`
  show no account number or transaction contents in `Debug`; the parse tree's `Debug`
  shows value lengths only.

### Dedupe, reconciliation and the commit

M1.11–M1.13, decided 2026-11-11 (decision record "import commit, dedupe and undo").
`coffer_core::import`: `preview(db, account, file name, Statement)` →
`ImportPreview` (every row's `RowOutcome`, the file's balances, a `Reconciliation`,
`counts()`, `needs_confirmation()`), then `commit(db, &preview, Confirmations)` →
`Committed::Batch(summary)` or `NothingNew`. `undo_preview(db, batch)` and
`undo(db, batch)` remove a batch; `batches`, `batch` and `batch_flags` read the history.

- **Row outcomes**: `New` (with its fingerprint), `AlreadyImported` (the stored row and
  whether its source ID or fingerprint matched), `RepeatedInFile`, or `Flagged`:
  `Unreadable(RowProblem)`, `SourceIdConflict`, `SourceIdRepeated`, `Locked`. Every row of
  the statement has one, in file order.
- **Source ID first.** A row with an external ID matches only the account's row with that
  ID, and never by fingerprint; source IDs are per account, so the same ID on two
  accounts is two transactions. Excluded rows still count as imported.
- **Fingerprint** (`import::fingerprint`, frozen by a test):
  `v1|<YYYY-MM-DD>|<amount minor>|<currency>|<occurrence>|<normalized description>`.
  The time, merchant and other fields are left out; the account is the unique index's
  other column, not part of the text. The occurrence index counts earlier rows in the
  same file with the same date, amount and normalized description, from 0, separately
  for rows with and without a source ID; flagged rows take none. A stored row without a
  source ID matches when its fingerprint is equal. So identical same-day rows are 0, 1,
  2… and both import; re-importing finds each again; an overlapping export with all of
  a day's rows numbers them the same way. One cut off part-way through a day can number
  that day's identical rows differently. The text is readable rather than hashed: no
  collisions, and nothing the row doesn't already hold. Description normalization is
  `statement::normalize_description`.
- **Running balances** (`Reconciliation::running`): each stated balance must equal the
  previous one plus the amounts since; a flagged row restarts the chain. The file's
  order is the one under which fewer balances fail, ties going by the dates. A mismatch
  is flagged at its row.
- **Balance checks** (`coffer_core::balance::check`, reused by M1.27): the account's
  balances (entered, earlier files, the opening balance, and this file's) are sorted by
  date; the first is the starting point and each later one must equal the last passing
  one plus the transactions after it, up to and including its day. Rows excluded as
  `duplicate` leave the sum. The preview runs it with and without the import and reports
  the checks this file adds, the stored ones whose result it changes, and stored ones
  the file states again. `ReconciliationStatus`: `NoBalances`, `StartingPoint`,
  `Reconciled` or `Mismatch` (the import screen's reconciliation block).
- **Commit** inserts the `import_batch`, its rows (`origin = imported`, `created_at` =
  the batch's `imported_at`), its new balances (`csv` or `ofx`) and its `import_flag`
  rows, in one DB transaction. In the same transaction it stores each new row's preset
  category (`category_source = preset_map`), mirrors sub-account transfers onto their
  pots (`sub_account::mirror`), names every row of the batch
  (`merchant::apply_to_batch`), runs the rules on the batch (`rule::apply_to_batch`),
  which override the preset's category, and suggests transfer pairs
  (`transfer::detect_for_batch`); a failure in any step rolls the whole import back.
  `CommitSummary` counts the file's rows categorised by the preset and by rules, the
  mirrored rows and created pots, and the pairs suggested. The M1.28 backup is taken
  just before the transaction.
- **File balances** are `New`, `AlreadyRecorded`, `Repeated`, or `Locked` (dated on or
  before the lock; left out).

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
  space (including no-break spaces) or an apostrophe, in groups of three and never
  after a leading zero (`0,123` with a `.` mark is refused, not read as 123: it's a
  decimal in the other style); the sign is a leading `-`, `−` or `+`, or parentheses.
  Extra decimals are accepted only when they
  are zeros (`12.340` GBP): anything else would mean rounding, so it's refused and the
  row is flagged. Currency symbols, codes and trailing minus signs are the importer's
  to deal with (a preset feature if a bank needs one). Parse errors don't contain the
  text, since errors get logged.
- **Display formatting is the app's.** Core's only text form is the plain, locale-free
  `-1234.56` (`to_decimal_string`, for exports and the CLI), which parses back
  unchanged. Symbols, locale separators, `−` and `+` follow `DESIGN.md` → "Money" in
  the app.

## Categories

`coffer_core::category` (M1.14; decisions in "Decision record: categories"). Every new
database starts with this two-level set (`V2__default_categories.sql`,
`DEFAULT_CATEGORIES`). Groups are in display order; a transaction can be filed under a
group itself (`transfer`, `other`) as well as under a subcategory.

| Group (kind) | Subcategories |
|---|---|
| `income` Income (income) | `salary` Salary · `self_employment` Self-employment · `benefits` Benefits · `pension` Pension · `interest` Interest · `investments` Investment income · `rental` Rental income · `gifts` Gifts received |
| `home` Home | `rent` Rent · `mortgage` Mortgage · `property_tax` Council tax · `maintenance` Repairs and maintenance · `household` Household |
| `bills` Bills | `energy` Energy · `water` Water · `phone` Phone · `internet` Internet · `tv` TV · `insurance` Insurance · `subscriptions` Subscriptions |
| `food` Food | `groceries` Groceries · `eating_out` Eating out · `takeaway` Takeaways |
| `transport` Transport | `public` Public transport · `taxi` Taxis · `fuel` Fuel · `parking` Parking and tolls · `vehicle` Vehicle costs |
| `shopping` Shopping | `clothing` Clothing · `electronics` Electronics |
| `entertainment` Entertainment | `events` Events and tickets · `hobbies` Hobbies |
| `health` Health | `medical` Medical · `fitness` Fitness · `personal_care` Personal care |
| `travel` Travel | `flights` Flights · `accommodation` Accommodation |
| `family` Family | `childcare` Childcare · `children` Children · `education` Education · `pets` Pets |
| `giving` Giving | `gifts` Gifts · `charity` Charity |
| `finance` Finance | `fees` Fees and charges · `interest` Interest paid · `loans` Loan repayments · `tax` Tax |
| `cash` Cash | — (cash withdrawals; with a cash account, the withdrawal is a transfer instead) |
| `other` Other | — |
| `transfer` Transfers (transfer) | `savings` Savings · `investments` Investments · `credit_card` Credit card payments |

A subcategory's key is `<group>.<key>` (`food.groceries`); groups not marked are
`spending`. `transfer` is for money between the user's own accounts, or to one Coffer
doesn't see: like a confirmed transfer pair, it is neither income nor spending.

- `CategoryKind` is the stored kind; a test keeps its keys equal to the schema's CHECK.
- `Database::categories` returns a `CategoryTree`: groups with their subcategories in
  the order they were created (the defaults first, in the order above), looked up by
  key (`get`, `group_of`). `CategoryTree::load` takes a `&Connection`, so code that
  writes in a transaction can map keys to IDs inside it.
- `CategoryKey` and `CategoryKind` are SQLite `TEXT`; a stored value that doesn't parse
  is an error, not a guess.
- Editing categories (add, rename, delete, hide) is M3.7.

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
  top-level key is one part, a subcategory's is `<parent key>.<part>`), make a
  category's `key`, `parent_id` and `kind` read-only (keys are stable; only the label
  changes) and give a subcategory its group's kind,
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
  and including `report`, except `change_batch`/`change_item`, `transaction_split` and
  `ledger_line` (V5, M1.20), `tag`/`transaction_tag` (V6, M1.21), `merchant_alias` (V7,
  M1.23), `transaction_link`, `duplicate_candidate` and the schedule tables;
  `mcp_client`, `mcp_access_log` and `setting` come with M2.

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|cash|
  asset|liability|...], currency, opening_balance_minor?, opening_date?,
  reconciled_through?, closed_on?, sub_account_key?)` — pots are child accounts of their
  parent; a pot's parent is a `current` or `savings` account in the same currency;
  `sub_account_key` (V4, pots only, unique per parent, read-only once set) is the bank's
  ID for a pot the import created;
  `asset` and `liability` accounts hold `valuation` snapshots only, no transactions
- `import_batch(id, account_id, source_file, format [csv|ofx|qif|camt053|mt940],
  preset_id?, imported_at, status [committed|undone])` — `source_file` is the file name
  without its directory; QFX is `ofx`
- `import_flag(id, batch_id, location_kind [line|entry], location, kind [unreadable|
  source_id_conflict|locked|running_balance], detail)` — a row the import left out or
  doubts (M1.11–M1.13, V3): its place in the file and core's reason, never the row's
  text. Read-only; undo deletes it with its batch
- `change_batch(id, kind [rule_apply|duplicate_merge|bulk_edit], created_at,
  status [applied|undone])` and `change_item(batch_id, transaction_id, field, old_value,
  new_value)` — undo for changes to existing transactions
- `transaction(id, account_id, batch_id?, date, time?, amount_minor, currency, merchant?,
  merchant_display?, merchant_display_source? [rule|user], description_raw, description_normalized, external_id?, fingerprint,
  source_category?, source_type?, original_amount_minor?, original_currency?, category_id?,
  category_source [rule|preset_map|user|ai_approved], rule_id?, transfer_group_id?, note?,
  user_note?, my_share_minor?, share_label?, reimbursable,
  excluded_reason? [duplicate|bank_error|disputed|other], excluded_note?, excluded_at?,
  origin [imported|mirrored|manual], created_at)` — `mirrored` rows are the generated
  pot-side half of a sub-account transfer, and undoing their import batch removes them too;
  `manual` rows have no batch and no `external_id` or `fingerprint`; `note` is the imported note (read-only), `user_note` the
  user's; `category_id` is empty when the transaction is split; `merchant_display_source`
  (V7) is empty while the name is derived and set when a rule or the user named the
  merchant, which recomputes leave alone. The three `excluded_*`
  fields are set and cleared together (`excluded_note` is required for `other`);
  `created_at` is when the row entered the database (the batch commit for imported rows)
- `transaction_split(id, transaction_id, line_no, amount_minor, category_id?,
  my_share_minor?, share_label?, user_note?)` — lines sum exactly to the parent amount
  (checked in core), each non-zero with the parent's sign, in the parent's currency;
  `line_no` from 1, unique per transaction (V5)
- `ledger_line` (view) — the split lines, or the transaction itself when unsplit, with
  `split_id`/`line_no` NULL on a transaction's own row, `transfer_status` and
  `my_amount_minor`. It doesn't filter: reports skip excluded rows and confirmed
  transfers; a linked refund takes its purchase's category
- `tag(id, name, private)`, `transaction_tag(transaction_id, tag_id)` — names unique
  ignoring case, at most 64 characters; the link table is `WITHOUT ROWID` and its rows
  are only inserted or deleted (V6)
- `merchant_alias(id, match_kind [exact|prefix|regex], pattern, display_name,
  created_at)` (V7)
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
- `category(id, parent_id?, key, label, kind [spending|income|transfer])` — two levels,
  stable keys (`food.groceries` under `food`; key and parent read-only), translatable
  labels; a subcategory has its group's kind, and kinds are read-only. The defaults are
  seeded by `V2__default_categories.sql` (see "Categories")
- `rule(id, priority, match_field [merchant|description|source_category|source_type],
  match_kind [is|starts_with|contains|regex], pattern, amount_min?, amount_max?,
  account_id?, created_at)` and `rule_action(id, rule_id, kind [set_category|
  set_merchant|add_tag|set_note|exclude|split_template|my_share], value_json)` — one
  action per kind per rule; an `exclude` action's value is a reason key; `priority` isn't
  unique (ties go by ID). Patterns and `value_json` shapes are checked in core
  (`rule::RuleDefinition`; see "Decision record: rules", point 9)
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
