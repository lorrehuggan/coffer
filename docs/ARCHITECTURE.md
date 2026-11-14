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
| Categorization | Local rules engine first; AI-proposed categories/rules land in the Inbox for approval. Approving a categorisation sets those transactions' categories (an undoable `bulk_edit` batch, source `ai_approved`); only approving a `rule` suggestion creates a rule. The app also files `rule` suggestions itself (source `coffer`) after repeated identical manual recategorisations: deterministic counting, no ML. Learned classifiers are not used |
| Transfers | Auto-detect opposite-amount pairs across own accounts within 3 days (`TransferWindow::DEFAULT`); user confirms; excluded from income/spend |
| Pots / sub-accounts | Monzo Pots, Starling Spaces, Revolut Vaults etc. are sub-accounts of their parent. A preset's `[sub_account_transfer]` identifies pot transfers; the import commit creates the pot when its key (the bank's ID for it, or its name where the export has no ID, as Monzo's doesn't) is new, mirrors the row onto it (`origin = mirrored`) and pairs the two as a confirmed transfer. Pots count toward net worth (opening balance entered once per pot). See "Decision record: sub-accounts" |
| AI integration | `coffer mcp` stdio MCP server (`rmcp`). Read-only query tools + inbox-only writes. No account numbers or PII in tool output |
| Splits | A transaction can be split into lines that sum exactly to its amount, in its currency (enforced in core, in the same DB transaction as the edit). Reports, budgets and MCP read the `ledger_line` view: one row per split line, or the transaction itself when it isn't split |
| Tags and notes | Many tags per transaction, alongside categories. Tag names are user text: MCP sees them only at `standard` access, and never for tags marked private. The imported note stays read-only; the user's note is a separate field. Neither note is exposed over MCP by default |
| Manual entries and corrections | Manual rows have `origin = manual` and no import batch. Imported source fields (date, amount, raw description, external ID) are never edited: a bad import is undone, or a row is excluded with a recorded reason and stays visible under an "excluded" filter. User-facing fields (category, display merchant, tags, notes, splits) are editable. Manual rows are the user's own data: every field is editable and they can be deleted. Exclusion reasons and their effect on balances: see the decision record "Transactions additions" |
| Merchant names | `merchant_display` is derived: deterministic clean-up of the raw text, then the merchant alias table, then rule actions. It can be recomputed at any time; the raw text is untouched. A name set by a rule or the user (`merchant_display_source`) is never recomputed. See "Decision record: splits, tags and notes, manual rows, merchant names" |
| Refunds, reimbursements, shared costs | `transaction_link` ties a refund to its purchase (it nets against the purchase's category instead of counting as income), a reimbursement to the expense it repays, and a settlement to a share label. `my_share_minor` (default: the full amount) gives personal and gross views; the difference builds an owed-to-me balance per local share label. Share labels never leave the app. Still single-user: no one else's accounts are modelled |
| Duplicates | Exact duplicates are prevented at import (source ID, then fingerprint). Near-duplicates (same account and amount within ±N days, different text) become `duplicate_candidate` rows for review. Merging excludes one row, never deletes it, and can be undone |
| Reconciliation | Every `balance_snapshot` is an assertion checked against the computed balance; a failure reports the window since the last passing assertion. `account.reconciled_through` locks history: imports, manual edits, entered balances, merges, rule applies or undos touching locked dates are flagged and need an explicit unlock. In a change batch any exclusion change counts, whatever its reason (user, 2026-11-12). See "Decision record: M1 wave 4" |
| Bulk changes | Changes to many existing transactions (a rule applied to history, duplicate merges, bulk edits) are previewed, then committed as an undoable `change_batch` that records old and new values, like import batches |
| Schedules and forecast | A detected recurring series becomes a schedule only when the user confirms it. The forecast is the last known balance plus schedule occurrences (plus the remaining flex budget), per account per day, in integer arithmetic, using cached FX rates only |
| Budgets | Per-category monthly amounts with optional rollover. Each budget is in a group: fixed, non-monthly (accrues 1/12 of a yearly amount, seeded from yearly schedules) or flex |
| Asset and liability accounts | Account kinds `asset` and `liability` hold manually valued items (house, pension, mortgage, loan). Valuations are `balance_snapshot` rows with source `valuation`; net worth uses the last value on or before each date. Investment holdings and prices are deferred (ROADMAP L.14) |
| Backups and export | Rotating `VACUUM INTO` snapshots (0600) in `<data dir>/backups` (0700) before every import commit, migration and restore; by default the last 10 automatic backups plus the newest of each earlier month are kept, and manual ones are never deleted. Restore runs `PRAGMA integrity_check` and a schema-version check on a copy before swapping files (see "Decision record: local backups"). Export is CSV/JSON with a documented, versioned schema (split lines carry their parent ID); user notes and private tags only with `--include-notes` / `--include-private-tags` (user, 2026-11-12), share labels never. Beancount/hledger journal export follows in M5.8. Local files only, no network |
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
   balance, and is flagged like any other change there. *Amended 2026-11-12 by the
   user:* an exclusion a rule applies counts as a change under a lock whatever its
   reason (it changes the locked window's reported totals), so an Apply or its Undo that
   would change an exclusion there needs an unlock; so does any exclusion change in a
   change batch (merges included). See "Decision record: M1 wave 4", point 3.
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
   - A restore doesn't roll the access log back (added 2026-11-13, user): the log
     records what AI clients did, which restoring data doesn't undo. Rows newer than the
     backup's newest are carried over from the database it replaced, after the restored
     file is migrated, and the 12-month retention applied (decision record "M2: MCP
     server", F.9).

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
   bank's ID for the pot, only on pots, unique per parent, and
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
8. **Amended 2026-11-13 (M1.9, user decision): Monzo pots are keyed by name.** Point 1
   assumed Monzo's export gives a pot ID (`pot_…` in `Description`). Checked against a
   real export, it doesn't: `Description` is empty on every pot transfer, and only
   `Name` says which pot it is. Rather than flag every pot transfer, the Monzo preset
   uses `key = Name`. The cost: renaming a pot in the Monzo app makes a new pot in
   Coffer at the next import (its earlier rows stay on the old one), and two pots with
   the same name are one. The core is unchanged: it still matches `sub_account_key`
   exactly and never renames a pot; the key is just a name for this bank. A preset whose
   export has a stable ID keeps using it. If Monzo adds pot IDs, the preset moves to
   them in a new decision, since existing keys would no longer match.
9. **Pot transfers can carry their own category (2026-11-13, user decision).**
   `[sub_account_transfer] category` gives every matching row that category ahead of
   `category_map` (stored as `preset_map`, like the map's). Monzo files pot transfers
   under "Transfers", which also holds payments to and from people, so the map can't
   tell them apart; the Monzo preset sets `transfer.savings`.

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

### Decision record: M1 wave 4 — change batches, rule actions, links, duplicates, balance checks and export (2026-11-13)

M1.24–M1.27 and M1.29. Migrations `V8__change_batch.sql`, `V9__transaction_link.sql` and
`V10__duplicate_candidate.sql`; M1.24, M1.27 and M1.29 needed none (V11 stays free).
Points 2 and 3 were decided by the user on 2026-11-12.

**Shared**

1. **Change batches** (`coffer_core::change`, V8). A `Recorder` sets a field, reads the
   value back (a timestamp filled in, tags sorted, a derived name recomputed) and keeps
   old and new; `finish` writes the batch and one item per transaction and field, or
   nothing when nothing changed. It runs in the caller's DB transaction, so a preview
   is the same code in a rolled-back transaction (`change::dry_run`). `undo` restores
   items newest first, and only where the field still holds the batch's value (rules
   decision point 6); a category counts as still holding when its category and source
   are unchanged (a deleted rule's cleared `rule_id` doesn't count, rules point 7), a
   derived name when it is still derived. A value that can't be written any more (its
   category deleted, the row split since) is counted as changed since. A split is set
   only on a row with no category or share of its own, so a recorder clears those first
   and undo puts them back after the lines go.
2. **Export leaves notes and private tags out by default.** `--include-notes` adds the
   user's note, split lines' notes, the exclusion note and the bank's imported note (in
   Monzo's export that is text the user wrote); `--include-private-tags` adds a separate
   `private_tags` column and allows filtering by a private tag. Share labels are never
   exported.
3. **A rule's exclusion counts as a change under a lock, whatever its reason.** Any
   exclusion change made through a change batch on a row dated on or before its
   account's `reconciled_through` is refused, and so is undoing one there: rule applies,
   merges and their undos all need an unlock first. This amends rules decision point 8
   (which flagged only `duplicate`). Single-row exclusions (`exclusion::exclude`) still
   meet the lock only into or out of `duplicate`.

**A — rule actions (M1.24, `coffer_core::rule`)**

4. **Rules win field by field, tags included.** Category and split are one field (the
   first rule to set either takes it); merchant, tags, note, exclusion and share are one
   rule each, so a second matching `add_tag` rule adds nothing.
5. **Shares** are `amount × bp / 10000`, rounded half away from zero, which keeps the
   amount's sign and stays within it. 10000 bp stores `NULL` ("all of it"); the share
   label is left as it is.
6. **Split lines** round down in absolute value, then the leftover minor units go one
   each to the largest remainders, ties to the earlier line: the lines sum exactly and
   keep the parent's sign. A row isn't split when a line would get 0, when it is in a
   suggested or confirmed transfer, or when it is money in with a suggested or confirmed
   link (B); it keeps its category and the import goes on (`CommitSummary::
   splits_skipped`). With a split, the rule set's share goes on each line; without one,
   on the row (never on linked money in).
7. **Import uses the same planner as Apply**, writing through a `change::Recorder` that is
   never finished, so the import records no change batch. Nothing the user set is changed
   at import.
8. **Apply** (`preview_apply` → `commit_apply`) plans one rule against every transaction
   it matches, evaluating the whole current rule set so it changes only the fields it
   wins. Split rows are skipped and listed. Values the user set (a `user`/`ai_approved`
   category, a user-set name, any existing note, exclusion or share, and a category or
   share a split would clear) change only on rows passed as included; including a row
   with nothing user-set is refused. The commit re-plans and refuses a stale preview,
   and refuses while any change it would make meets a lock (point 3). One `rule_apply`
   batch per apply; tags it created stay after undo, without their links.
9. Notes and shares have no "set by" column, so a note or share a rule set at import
   counts as user-set on a later Apply.

**B — refunds, reimbursements and shared costs (M1.25, `coffer_core::link`, V9)**

10. **One link per pair, in any status.** A rejected row stays, so the pair is never
    suggested again; unlinking is `reject` and its undo `confirm`. Only confirmed links
    count, in the caps and in `ledger_line`.
11. **Netting is extra `ledger_line` rows.** A confirmed refund or reimbursement adds a
    row on the inflow (its date and account) in the purchase's category, share
    (pro rata, truncated toward zero), label and `reimbursable`; the inflow's own row
    keeps the unlinked rest (0 when fully linked). Against a split purchase the link is
    divided over the lines by largest remainder. A settlement adds a row with no
    category and a share of 0. A transaction's rows still sum to its amount, so balances
    read the same rows. `in_reports` drops excluded rows and confirmed transfers (a
    link part also when its purchase is); `personal_minor` is the user's own spending or
    income: `my_amount_minor`, or 0 when not in reports or reimbursable. A linked purchase
    is at most 3,037,000,499 minor units so the pro-rata products stay integers.
12. **Reimbursable means the whole purchase is claimed**: it counts as 0 of the user's
    spending, outstanding or repaid; set back to "just me", it counts in full and repaid
    amounts net against it like refunds. "Shared" and "claim back" exclude each other
    and apply to money out only; a split purchase is shared per line.
13. **Owed to me** per label and currency is Σ(`my_amount_minor − amount_minor`) over the
    label's rows in reports and not reimbursable; a settlement subtracts because its
    share is 0. Labels are trimmed, at most 64 characters, compared exactly.
14. **Suggestions** (`suggest_for_batch` at import, `suggest_all`): money in that isn't a
    duplicate, a transfer, split, shared or already suggested. A refund: a purchase on
    the same account with the same `merchant_display` (ASCII case ignored) within 90
    days up to the refund, with enough left; an exact remaining amount wins, then the
    newest. Otherwise a reimbursement: the 12 newest outstanding claims on or before the
    payment, every subset tried, summing to the payment exactly with the newest within
    60 days; fewest claims, then newest, win.
15. A linked row's amount and currency can't change (`manual::update` refuses), and
    linked money in can't be split or shared.

**C — possible duplicates (M1.26, `coffer_core::duplicate`, V10)**

16. **A pair** is two rows on one account, same currency and non-zero amount, at most
    3 days apart (`DuplicateWindow::DEFAULT`, both ends included, at most 31), neither
    excluded, not from the same import batch, at least one imported or mirrored. Two
    imported rows must differ in `description_normalized` (identical text is the exact
    dedupe's job or a genuine repeat); a manual row pairs whatever its text. A pair
    already recorded in any status isn't raised again.
17. **Deterministic**: smallest gap first, then pairs whose display names agree, then
    IDs; each row in at most one new pair per run. Transfers and split rows take part.
18. **A merge** is one `duplicate_merge` batch (one candidate or many) that excludes the
    duplicate as `duplicate` in place and marks the candidate `merged` with `kept`. The
    default keeps a row in an active transfer, then one with a source ID, then imported
    over manual, then the earlier import (review queues point 3); the user can swap.
    User-set values on the duplicate (category, split, share, name, note, tags,
    `reimbursable`) move to the kept row where it has none, as items in the same batch.
    A stale preview, a non-pending candidate, rows that no longer match and conflicting
    requests are refused; so is a locked duplicate (point 3).
19. **Undoing a merge** puts every candidate in it back to `pending`, even one whose
    exclusion changed since (it can then be dismissed). Deleting a transaction deletes
    its candidates. Undoing the import of a kept row leaves its merged duplicate excluded;
    `UndoPreview::merged_into` says so, and undoing the merge includes it again.

**D — balance checks, lock dates and export (M1.27, M1.29)**

20. **Every balance is checked** (`coffer_core::reconciliation`): the opening balance
    first on its day, then snapshots by ID, then by date. The first is the starting
    point; each later one is checked against the last passing one plus the transactions
    between (only `duplicate` exclusions leave the sum; a split row counts once). A
    valuation sets the balance and restarts the count. A failure names the window
    `(since, date]`, the stated and computed amounts and the difference, and is never
    the base for the next check.
21. **Locking**: `lock` only on the date of a passing check with no failed check on or
    before it; moving the lock later is allowed, earlier is refused; `unlock` clears it
    entirely. `account::add_balance` now refuses a date on or before the lock. Still to
    guard when they arrive: editing or deleting an entered balance, the opening balance
    or date, closing an account, deleting one.
22. **Export** (`coffer_core::export`, `coffer export`) is one row per `ledger_line` row:
    split lines carry the parent's `transaction_id` with their `split_id`/`line_no`, and
    link parts their `link_id`. Nothing is filtered by status. Filters AND together, a
    filter's values OR; dates inclusive; a group's category key includes its
    subcategories; tags match ignoring case. Money is an exact decimal string beside its
    `*_minor` integer; JSON numbers are integers only; JSON is `{schema: "coffer.export",
    schema_version: 1, includes, rows}`, CSV has a header row and quotes only when
    needed. Opt-in columns come last, so the default header doesn't change. A file is
    created 0600, an existing one is refused without `--force` (which replaces it
    atomically), and the database itself is refused as the output. The column table is
    in the `export` module's docs.

Open: CSV cells beginning with `=`, `+`, `-` or `@` are left as they are (a spreadsheet
may read them as formulas); `external_id` is always exported; share labels may want an
opt-in flag later; share labels compare case-sensitively.

### Decision record: M2: MCP server (2026-11-13)

M2 was built in four parallel blocks (B access, log and text safety; C read tools and
reporting; D recurring payments and schedules; E the inbox) on Wave 0's spine, then
integrated. Marked **(user)**: decided by the user on 2026-11-13, either directly or by
reviewing each implementation default one by one after the real-data check (all kept
as built). Everything else follows an earlier decision record or invariant.

**A. The user's decisions.**

1. **Names at `standard` are accepted (user).** `standard` is full access by design: a
   payee who is a person can appear under their name as `merchant`, and in
   `description_untrusted`. Below `standard` a name never appears (tested with a row
   whose bank text is only a name, `crates/mcp/tests/person_names.rs`).
2. **No account names at `aggregate` (user).** `net_worth_history` gives each account a
   `label`, its kind and its number among accounts of that kind by ID (`current 1`,
   `pot 2`), and `name: null`; names appear from `pseudonymous` up.
3. **No text search below `standard` (user).** `search_transactions` refuses `query` (and
   the tag filter, the other user-text match) below `standard` with a clear error; date,
   account, category and amount filters stay.
4. **Descriptions are hidden below `standard` (user).** `Call::untrusted` returns
   `[hidden: descriptions name merchants, so they aren't shared at this access level]`
   below `standard`, since bank text names the merchant a token hides.
5. **Block D's schedule schema (user):** `start_date` instead of `next_date`, the new
   `recurring_dismissal` table, and the correction to the lock claim (D7 below).
6. **Wave 1's defaults are accepted (user):** CLI names `coffer ai-access`, `coffer
   budgets`, `coffer schedules`, `coffer inbox`; pseudonym tokens are HMAC-SHA256; the
   tool levels as built (the table in "MCP surface"); `Amount.home` stays `null` through
   M2.

**B. Access control, the access log and the text envelope (M2.11–M2.13).**

1. The kill switch is on unless set off; MCP stays off until a client is enabled
   (user).
2. The pseudonym key is 32 bytes from SQLite's `randomblob` (no RNG crate), created by
   `coffer ai-access enable`, never by the server; without one a `pseudonymous` call is
   refused with a message saying to enable the client again (user).
3. Tokens are `M-` plus 12 hex digits (48 bits: about 1.8 × 10⁻⁷ chance of any collision
   at 10,000 merchants), of the trimmed, lower-cased display name (user).
4. Client lifecycle: enabling an enabled client keeps its level; `revoke` sets the level
   back to `aggregate`; `ignore` also revokes; a name can be enabled before the server has
   seen it (user).
5. An unknown name is recorded as `requested` on any request, even with the kill switch
   off, so the user sees who asked (user).
6. A 5-second poll sends `tools/list_changed` to an idle client; it only reads (user).
7. Redaction: numbers, booleans, dates and schema enum values are stored as themselves;
   any other string, digits-only included, as `{"length":N,"type":"string"}`; undeclared
   keys only counted, `"_undeclared": N` (user).
8. The result hash is SHA-256 of compact JSON with keys sorted at every level (user).
9. The envelope withholds the whole field on any pattern match; URLs with a scheme, `www.`
   hosts and markdown/HTML links are withheld and bare domains made inert
   (`NETFLIX[.]COM`), which settles "made inert" against "withheld"; fullwidth letters
   are folded, full NFKC and homoglyphs are out of scope; `Call::merchant` withholds a
   display name that reads like instructions at `standard` too (user).
10. `coffer ai-access log` shows the redacted arguments (cut at 120 characters) and the
    cleaned purpose (cut at 60), never the result hash (user).
11. Retention: failing to delete old log rows is a warning, and the server starts anyway
    (user).

**C. Read tools and reporting (M2.2, M2.14 reads).**

1. Money counts by its category's kind: money in under a spending category (refunds
   included) lowers spending and isn't income; transfer categories count as neither;
   uncategorised money counts by the sign of the user's share; a claimed-back purchase
   counts as nothing (user).
2. Spending, budgets and the row tools' `my_amount` use the user's share; balances and
   net worth are gross (user).
3. Net worth: liabilities always lower the total, even entered positive; a closed account
   drops out of months ending after its closing day; an account with no stated balance is
   summed from zero and flagged `unanchored` (user).
4. Budgets: a month's budget overrides the standing one; a budget counts spending only in
   its own currency and lists the rest beside it; rollover follows the group (reported,
   applied from M4.9); budgets only on spending categories; `coffer budgets set` defaults
   to the one currency all accounts share and otherwise needs `--currency` (user).
5. Row limits: 50 by default, at most 200, with `truncated` (user).
6. Account-details check (`reporting::rows::has_account_identifier`): a merchant holding a
   sort code, 8+ digit run or IBAN becomes `null`, a description holding one
   `[withheld: account details]`; dates, amounts and short card references like `*1234`
   aren't caught; `withheld_count` still counts only text withheld as instructions
   (user).
7. `list_rules` shows public tags plus a count of private ones, `set_note` as the action
   only, the share action as its percentage only (user).
8. `list_reports` is newest first, every report `ai_authored: true`, text stripped of
   invisible characters (decision record 2026-11-10, point 4).
9. A refund of a split purchase comes back as one row per line category, as the ledger
   nets it.

**D. Recurring payments and schedules (M2.6, M2.10).**

1. **`start_date`, not `next_date` (user).** A schedule stores its first tracked
   occurrence; occurrence *k* is `Cadence::nth(start, k)`, counted from the start (a
   schedule on the 31st falls on 28/29 February and 31 March again). Stored matches and
   skips name occurrence dates, so the dates must be fixed.
2. **`recurring_dismissal(series_key, dismissed_at)` (user)** records "Not recurring".
3. `schedule_match` records who matched (`auto`/`user`) and when; a transaction fulfils
   at most one occurrence of any schedule; triggers keep a match on the schedule's account
   and an occurrence never both matched and skipped.
4. Series keys are an unkeyed FNV-1a hash of account, merchant, direction, currency,
   cadence and position, stable as payments arrive; kept out of MCP output (a guessed
   merchant could be checked against one). The CLI shows them (user).
5. Matching runs at import (that import's rows, active schedules) and when a schedule is
   created or confirmed (the account's earlier rows after its lock date). The window for
   occurrence *k* is `[nth(k) − tolerance_days, nth(k+1) − tolerance_days)`; it compares
   the whole amount, the normalised merchant and the sign; excluded rows and transfers
   never match automatically; the lower schedule ID wins a tie; `update` and `resume`
   match nothing, so an unmatched payment stays unmatched (user).
6. "Imports cover the date" means the account's latest imported or mirrored transaction,
   excluded ones included; manual rows don't count. `today` is passed in, read from
   SQLite's clock (user).
7. **Lock dates apply to the occurrence's date (user: the correction).** Skip, unskip,
   match and unmatch are refused on or before the lock, and deleting a schedule with a
   match or skip there. A payment arriving late but within its window can fulfil an
   occurrence dated on or before the lock, so an import's matches *can* touch a locked
   period: the earlier assumption that they never do was wrong.
8. Ended is final; a paused schedule can still be skipped, matched and edited; an ended
   one refuses skips, matches and edits, but its matches and skips can be removed and it
   can be deleted (user).
9. A new schedule's tolerance: days per cadence (weekly 1, fortnightly and four-weekly 2,
   monthly 3, quarterly 5, yearly 7), at most under half the shortest gap; amount: the
   series' spread or a tenth of its typical amount, whichever is larger (user).
10. Detection groups by account, normalised merchant, direction and currency; chains
    each cadence within the day tolerance, skipping at most one payment (never two in a
    row, at most one per four); fixed amounts within 2% (each price held twice, at most
    `max(1, n/4)` changes, typical = latest); otherwise variable within a factor of two of
    the median (one extra payment needed, typical = median of the last 6); refuses a
    series with similar amounts around it or one chance would give 1/32 of the time;
    reports only the current run of a restarted payment. Full rules in `recurring.rs`
    (user).
11. Known limits: two series from one merchant at the same amount aren't reported;
    detection runs on every `recurring_payments` call and isn't benchmarked on large
    ledgers; on synthetic noise about 1 ledger in 300 gives a false series.

**E. The inbox (M2.3, M2.7, M2.14 writes).**

1. At most 200 pending suggestions per source; a duplicate doesn't count (user).
2. Caps: 1–100 categorisation items, rationale 1,000 characters, display name 100 (no
   control characters), rejection reason 500 and not blank, report title 200, period 32,
   body 100,000, title and period not blank (user).
3. One pending suggestion per `(kind, canonical payload)` from any source; filing an
   identical one returns the existing ID and the first proposer stays the source. The
   canonical form sorts items, orders actions and writes out defaults (user).
4. An agent's rule may only use `set_category`, `split_template`, `set_merchant` and
   `add_tag`; `exclude`, `set_note` and `my_share` are refused (user).
5. Each approval is one `BEGIN IMMEDIATE` transaction that re-checks the payload, applies
   it and marks it approved. A categorisation is one `bulk_edit` batch with source
   `ai_approved` and overwrites user-set categories (the user approved those rows
   explicitly, and the preview counts them); setting a split line's category rewrites
   that transaction's lines. A transfer pair confirms detection's own group or creates a
   confirmed one, refused if either side is linked. A link is confirmed at the payload's
   amount or what both sides can still take (user).
6. No approval meets a lock today: `reconciled_through` guards only exclusions, which no
   suggestion makes; `ApproveError::Locked` is a safeguard.
7. Settling is narrow: a pending categorisation is settled by an approved rule only when
   every item names a transaction the rule's batch gave that category; split-line items
   and partial overlaps stay pending (user).
8. Coffer's own rule suggestions: after 3 or more unsplit rows with one `merchant_display`
   get the same category from the user (counted from the current state, so undoing an
   edit lowers the count), Coffer files `merchant is <cleaned merchant>` →
   `set_category`, only when every counted row's merchant folds to one text, with a fixed
   rationale and no merchant text. It files nothing when an identical suggestion is
   pending or was ever decided (a rejection is permanent), a stored rule has exactly this
   definition, or the source is at its cap (user).
9. "Approved" means "was applied": undoing its batch or deleting its rule leaves it
   `approved`.

**F. Integration.**

1. **Restores and running servers.** The server opens the database per call and keeps no
   connection between calls, so an idle server never blocks `coffer restore` (which
   refuses a database another connection holds). From the checked backup until the
   restored database is open, `restore` holds the operating system's exclusive file lock
   (`File::try_lock`, std, no dependency) on `<database>-restoring`
   (`backup::restore_lock_path`), and waits up to 5 seconds for a running call to close its
   connection. The lock, not the file, means "restoring": the file stays, empty between
   restores, and is never deleted, so no process can lock a file that has left the
   directory while another locks a new one under the same name. The server checks with a
   momentary shared lock before and after opening and refuses while the lock is held, with
   "Coffer is restoring a backup"; those refusals are logged afterwards, through the
   pending file (F.8). A second restore can't take the lock and is refused (after
   retrying for a second, past the server's momentary checks). A check made before the
   restore holds the lock sees no restore, which is sound because `restore` changes
   nothing until it holds the lock and then waits for earlier connections; that's why
   there is no grace period and no create-then-rename (which `std`'s replacing rename
   can't do safely on Windows). **A crashed or killed restore** loses its lock with its
   process, so it blocks nothing: the database is complete at every step (untouched before
   the atomic rename, the synced backup after it, a migration all or nothing), and
   `open_existing` refuses one left at an older schema with its usual "run a `coffer`
   command" message. The restore writes a marker (start time, backup) into the file while
   it holds the lock and empties it at the end, so `coffer restore` finds a stopped one,
   warns, and goes ahead, backing the current database up first as always. A lock file
   the server can't open or lock is refused with a message naming the file. File locks are
   `flock`/`LockFileEx`; on a network file system they may be missing (restore refuses,
   MCP refuses with that message) or local to one machine. No network and no
   process-to-process messages (user).
2. **`monthly_review`** is listed only to enabled clients; its text is built from the
   tools at the client's level, so it never names one the client can't call; getting it
   isn't logged, since it returns no data (user).
3. **Saved reports are capped at 500 per client**; the 501st is refused with a clear
   error, counted and inserted in one transaction (user). The error tells the client
   that the user can make room with `coffer inbox delete-report`, and names nothing of
   the user's. `coffer inbox reports` notes any client at the cap with the command that
   clears it (user).
4. **`coffer inbox approve 1 2 3` is all or nothing** (`inbox::approve_all`, one
   transaction). A repeated ID is refused before anything is tried; a suggestion that an
   earlier approval in the same list settled is skipped, not an error, and the CLI says
   which rule settled it (user). `reject` still decides each ID on its own.
5. **`coffer inbox reports` and `coffer inbox report <id>`** show saved reports: the list
   has no bodies; a report is printed as plain text, each body line prefixed `  | ` so it
   can't pass for Coffer's own output, labelled as written by the client and unverified.
   The CLI strips exactly what `envelope::strip_invisible` strips (user).
   **`coffer inbox delete-report <id>... [--yes]`** and
   **`coffer inbox delete-report --client <name> [--before <YYYY-MM-DD>] [--yes]`**
   delete saved reports (`inbox::delete_reports`): the user's only way to free a
   client's allowance, so the cap is never a dead end. Without `--yes` they list what
   would go (ID, period, client, date, title, stripped and labelled as above) and change
   nothing, as `coffer restore` and `coffer schedules delete` do; nothing is asked on
   stdin. IDs are all or nothing in one transaction: an unknown or repeated ID deletes
   none. `--before` is the start of that day in UTC, as `created_at` is stored; a client
   with no matching reports deletes nothing and isn't an error. No backup is taken first
   (deleting reports changes no money data); a report stays in backups taken before it
   was deleted until they rotate out. Nothing else refers to a report by ID: no foreign
   key, suggestion payload, `applied_json` or change batch names one, and the access log
   keeps only a hash of `save_report`'s result; `report.id` is `AUTOINCREMENT`, so a
   deleted ID is never reused. No MCP tool or server code path deletes a report: the
   tool × level matrix checks that no call changes or removes an existing suggestion or
   report, and that the server's source never calls core's deletion (user).
6. **`coffer categorize <transaction-id>... <category-key>`** sets categories as the user
   through `inbox::own::recategorize`, so Coffer's own rule suggestions fire from the CLI
   until the GUI's inline edit (M3). It refuses a repeated ID, an unknown category or
   a split row, and prints the change batch and any rule suggestion it filed (user).
7. The tool × level matrix (`crates/mcp/tests/matrix.rs`) lists every tool with its level
   and arguments; a new tool fails it until added.
8. **Calls refused while the database can't be opened are logged too** (exit criterion:
   every call is in the log). The log is in the database, so a call refused while a
   restore holds the lock (`refused_restoring`), while the lock can't be checked or while
   `open_existing` fails, e.g. with migrations pending (`refused_unavailable`), is
   recorded in a pending file next to the database, `<database>-access-pending`
   (`access_log::pending`), and moved into `mcp_access_log` with its own time the next
   time the database is opened to write: any server's next call or listing (whichever
   gets there, after its restore-lock check), `coffer ai-access`, and `restore` once the
   restored database is open. Neither existing outcome fitted: `error` means a tool failed, and the
   `refused_*` ones are the gate's (user).
   - A line is the log row's entry and nothing more: client, tool, the call's time,
     outcome, the cleaned purpose (cut to 500) and the redacted arguments, as JSON. No
     result hash (nothing was read). The file is `0600`; each append is synced. Several
     servers append and flush under std's exclusive file lock (`File::lock`) on the file,
     so no new dependency (user).
   - No duplicates: each line gets a UUIDv7 (the `uuid` crate already in core) stored as
     `pending_id`, unique (V14); the flush inserts every line in one transaction with
     `ON CONFLICT (pending_id) DO NOTHING`, commits, then empties the file. A crash
     between the two re-inserts lines that are all ignored, so neither the step order nor
     a marker in the database is needed. Keeping them only in memory was rejected: a
     client that exits before the database is back would lose them (user).
   - Bounded: at most 1,000 entries and 1 MiB. Calls past either are counted per client,
     tool and outcome in `<database>-access-pending-dropped` (replaced whole by a rename),
     and each count becomes one summary row (`summarised_calls`, timed at the first such
     call, no arguments or purpose). The flush renames the counts aside before inserting,
     so counts made after a crash land in a new file with new IDs and aren't lost
     (user).
   - A line cut short by a crash is ended before the next append and skipped on flush,
     like a line the log's schema refuses; the flush counts them and warns. If the pending
     file can't be written either, the call is still refused and the failure goes to the
     server's own log (user).
   - The log is listed newest first by time, then ID, so moved-in rows appear where they
     happened (user).
9. **A restore keeps the access log** (decision record 2026-11-10, point 6). Restoring a
   backup used to replace `mcp_access_log` with the backup's copy, so the rows written
   between the backup and the restore were left only in the pre-restore backup. Now
   `restore`, once the restored database is open and migrated and while it still holds
   the restore lock, copies from the pre-restore backup (a snapshot of the replaced
   database, only read) every log row newer than the restored log's newest, keeping its
   time, then deletes rows past the 12-month retention, in one transaction
   (`access_log::carry_forward`); then it flushes the pending file (F.8). The replaced
   database may be at an older schema: before V11 it has no log, before V14 its rows have
   no `pending_id` or `summarised_calls`. Rows at or before the backup's newest time are
   the backup's own. A `pending_id` already present isn't copied twice. A failed carry
   doesn't fail the restore, which is done by then: `coffer restore` says the rows are
   in the pre-restore backup (user).

### Decision record: the app shell and window state (2026-11-14)

M3.1. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` or an earlier record.

1. **Navigation is the title bar's view tabs, not a sidebar.** ROADMAP M3.1 said "sidebar
   navigation", written before the design track. Paper's shell (Rules `B1J-0`, Settings ·
   AI clients `AQ2-0`, and `DESIGN.md` → "Finance screens") navigates with eight view tabs
   in the title bar (Overview, Transactions, Trends, Budgets, Net worth, Inbox, Reviews,
   Rules) and a Settings icon button before the window controls. The left panel belongs to
   the view: the five views of the money share the accounts panel, and Inbox, Reviews,
   Rules and Settings get their own lists with their screens. Paper wins, so the shell
   (`crates/app/src/shell.rs`) follows it. Keyboard: ⌘1–⌘8 open the views in tab order and
   ⌘, opens Settings (Ctrl on Linux and Windows). Paper draws no view shortcuts; these are
   keyboard paths, not visual values. The tabs are `gpui-base` `Tab`s (tab role, selected
   state, position in the list), not Tab stops. Clicking the Settings button while
   Settings is open goes back to the view it was opened from (Transactions if the window
   opened on Settings); ⌘, only opens it, as the platform shortcut does elsewhere (user).
2. **Window state lives in the `setting` table (user).** Key `app.window`, a JSON
   `WindowState` (`coffer_core::setting`): mode (windowed, maximised, full screen), the
   restore bounds in whole logical pixels, and the open view's key. Core checks the range
   (sides 1–32768, origins within ±32768, a view key of at most 32 `[a-z_]`) and refuses an
   out-of-range write; a stored value that doesn't parse reads as an error and the app uses
   its defaults. It isn't exported and no MCP tool reads it. Restoring a backup rolls it
   back with everything else, which is harmless. The rejected option was a separate
   `window.json` in the data dir, which would have kept the GUI off the database until M3.2.
3. **So the GUI opens the database from M3.1 on.** `coffer` (no subcommand) opens it with
   `Database::open` at startup, creating and migrating it as the CLI does (with the
   migration backup), and opens it again for each save. Like the MCP server, it holds no
   connection between reads and writes, and checks the restore lock first: during a
   restore it reads nothing and skips the save. The window state is a convenience: any
   failure (no data dir, a database Coffer can't open, a bad value) leaves the window at its
   defaults with a warning on stderr (`COFFER_LOG`, as `coffer mcp`), and never stops the
   app.
4. **When it's saved.** After the window has stayed put for 500ms following a move, resize
   or view change (one write per drag), on a background thread; and at once, on the main
   thread, when the window closes or the app quits.
5. **How it's restored.** A saved window whose title bar wouldn't show at least 40×40px
   on any display (a display unplugged since) opens centred at 1440×900 instead; one smaller
   than the 1280×800 minimum grows to it; with no displays reported, the saved bounds are
   used. An unknown view key (from a newer build) opens the default view, Transactions,
   the only screen built so far. Maximised and full-screen windows reopen so, with their
   restore bounds.
6. **Views not built yet show the shell and an empty state (user).** Every tab works.
   Until its roadmap item lands, a view shows its shell (the accounts panel on the money
   views, nothing on the others), with the Empty state component (`13L-0`) centred on the
   canvas, as `DESIGN.md` → "Review queues" centres an empty canvas: "{View} isn't built
   yet", a line saying Transactions works on sample data, and the ⌘2 shortcut.

### Decision record: accounts management (2026-11-14)

M3.2. Marked **(user)**: decided by the user on 2026-11-14 when asked before the build.
Everything else follows `DESIGN.md` → "Accounts panel" or an earlier record.

1. **A preset per account (user).** The inspector's Preset select ("used by the next
   import") is stored on the account: `account.preset_id` (V15), the preset's stable ID
   (`uk-monzo`), the same text `import_batch.preset_id` records. Only current, savings and
   credit card accounts have one (a `CHECK`): cash, asset and liability accounts are kept
   by the user, and a pot's rows arrive with its parent's imports. Core doesn't check the
   ID against the presets (they live in `coffer-import`, and user presets come in M3.5);
   the app and `coffer accounts add --preset` offer only presets they have, and an
   import whose preset has gone falls back to detection. The import flow (M3.4) is what
   reads it; `coffer import` doesn't yet.
2. **Editing is core's (`account::update`).** Name, bank, kind, currency and preset change
   together through `AccountEdit`; the name is trimmed and can't be blank, a blank bank
   or preset is none, and a kind that isn't imported keeps no preset. What the account
   holds (`account::usage`) decides the rest, checked before the schema's triggers so
   the error says why: the currency is fixed once the account has transactions,
   balances, pots or a rule with amount bounds, and always for a pot (it follows its
   parent); a pot stays a pot and nothing becomes one (imports make pots); an account
   with transactions can't become an asset or liability; one with balances can't move
   in or out of the valuation kinds; one with pots stays current or savings. Account
   edits aren't change batches: they touch no transaction.
3. **The opening balance is the first entered balance.** The New account dialog's
   optional Balance is a `manual` `balance_snapshot` written in the same DB transaction
   as the account (`account::create_with_balance`), so a refused balance adds no
   account. It is the account's starting point, as "Decision record: import commit,
   dedupe and undo", point 6 already has it; `account.opening_balance_minor` stays for
   the checks that read it, and nothing writes it. "Add balance…" records more
   (`account::add_balance`, one per day, after the lock date).
4. **The panel's numbers** (`reporting::account_tree`): open accounts in creation order,
   each pot under its parent; the latest balance at the end of each account's own last
   active day; an account other than a pot with no stated balance shows "No balance"
   and is left out of net worth (its sum from zero is right only if it started empty),
   while a pot counts from its transfers; net worth per currency, never converted, a
   liability always subtracting (as `net_worth_history`); `danger` only for a current,
   savings, cash or pot account below zero; flagged when a balance check failed.
5. **"Close account…" waits for M3.17 (user).** It isn't in the M3.2 line, closing a bank
   account still needs its lock-date guard (M1 wave 4, point 21), and M3.17 brings the
   close dialog for valuation accounts. The inspector has no footer until then.
6. **The app holds no connection (as M3.1).** `accounts::store::AccountStore` opens the
   database for each read or write, creating and migrating it as the CLI does, and checks
   the restore lock first. Reads and writes are a person's accounts, so they run on the
   main thread; the panel and inspector reread everything after a change. A database
   that can't be read leaves the panel empty with the reason in `danger`.
7. **Where the account inspector shows (user).** On Transactions, while an account is
   selected and no transaction is: clicking an account clears the transaction
   selection. The table isn't filtered by it until it reads the database (M3.3); on the
   views without an inspector, selection only highlights the row.

### Decision record: Inbox (2026-11-14)

M3.6 and M3.14. Marked **(user)**: decided by the user on 2026-11-14 when asked before
the build. Everything else follows `DESIGN.md` → "Inbox", "Inbox impact preview" and the
"Inbox impact preview" record above. No migration.

1. **The preview lists, core computes (`inbox::preview`).** `impact` counts; `preview`
   lists the rows approving would change (a rule's or a name's matches, a
   categorisation's items), what each has now and after, the rows left alone because the
   user set them, the spending each category gains or loses, the rule that decides a
   rule's matches today, the categorisations a rule settles, a wide rule summed by
   merchant (the first 3, the rest, the ones you set), and a transfer's or refund's
   effect on its month. It plans with the approval's own code in a rolled-back
   transaction, so the list can't drift from what approving does. Up to 10 matches are
   listed, else the newest 6. `describe` resolves what any suggestion names (decided ones
   too); `covered_by` finds the pending rule that would settle a categorisation.
2. **Approving a rule from the Inbox applies it to history.** As the D6.6 preview says
   ("Changes 5 past transactions"): `ApproveOptions { apply_to_history: true }`, never
   over a category the user set (point 1 above). The CLI keeps `--apply-to-history`
   opt-in.
3. **Approve all and Reject all are all or nothing**, per group (one kind from one
   source): `approve_all` and a new `reject_all` in one DB transaction. Approve all leaves
   out wide rules and any suggestion that can't be approved now. A suggestion a rule
   earlier in the list settled is skipped, as `approve_all` already does.
4. **No undo yet (user).** Undoing an approval would have to remove the rule, alias,
   link or transfer it made as well as revert its batch, and V13's trigger keeps a
   decided suggestion decided, so it needs a migration and its own item. After an
   approval the D6.3 bar says what changed, without Undo; the D6.6 copy that offers an
   undo is left out.
5. **A categorisation with several items is one row (user)**: "N transactions", opening
   in place like a rule with each transaction listed. Approving it approves every item,
   as core always has.
6. **Category colours are ranked when read (user).** `reporting::palette`: the six
   spending categories with the most spent in the 12 months to today get `chart-1` …
   `chart-6`, the rest the "Other" marker. Not stored yet, so they can change as spending
   does; storing them on the category ("Data visualisation") is still to come, and M3.3
   can use the same function.
7. **Untrusted text is plain text.** Rationales, patterns, names, client names and bank
   text go through `envelope::shown` (the CLI's stripping, moved to core): control,
   invisible and bidi characters removed, whitespace collapsed, cut to a length. GPUI
   draws text without markup or links, and nothing in it is acted on.
8. **The app holds no connection (as M3.2).** `inbox::store::InboxStore` opens the
   database per call and checks the restore lock. The pending list is read with every
   preview when the Inbox opens and after each decision, on the main thread (decision
   record "Inbox impact preview", point 6). The tab's badge counts pending suggestions
   from AI clients; Coffer's own aren't counted.
9. **Left for later (user).** D6.4's CHECKS section (M3.12), "Show in Transactions"
   (needs M3.3's filters) and "Edit in Rules…" (M3.7, M3.13) aren't drawn. Client display
   names come with the AI access settings (M3.15); until then the Inbox shows the
   `--client` name.

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

Single binary: `coffer` (GUI: the app shell around the views, see "Decision record: the
app shell and window state"), `coffer mcp` (stdio MCP server),
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

Later: `ureq` for the ECB fetch, reading the ECB Data Portal's CSV
(`data-api.ecb.europa.eu`, `format=csvdata`) with the `csv` crate already in import, so no
XML parser; `mt940`; an own CAMT.053 mapping on `roxmltree` (the `camt053` crate is barely
used); own QIF parser. Avoid GPL crates (`ofxy`, `tree_magic_mini`'s GPL data feature).
Research and candidates per roadmap item: [`research/crates.md`](research/crates.md)
(2026-11-13).

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
   balances and check assertions. All of these run today (schedule matching since
   M2.10)
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
    text (empty for a pot transfer or a Monzo-to-Monzo payment). `[sub_account_transfer]`
    recognises pot transfers by `Type` = "Pot transfer" (M1.17) and keys them by `Name`
    ("Decision record: sub-accounts", point 8).
  - `Address` is ignored, so merchant addresses never reach the database from Monzo.
  - Monzo's "Transfers" category isn't mapped: Monzo files pot transfers and payments to
    and from people under it, and the latter are spending or income. Pot transfers get
    `transfer.savings` from `[sub_account_transfer] category` instead. "Savings" maps to
    `transfer.savings`; Family and Finances map to the `family` and
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
  transfer"`, `key = Name`, `name = Name`, `category = "transfer.savings"` (points 8 and 9
  of that record). Optional `category`: a default category key every matching row gets,
  ahead of `category_map`.
  - **Checked against a real Monzo export (2026-11-13, M1.9),** by structure only (header,
    counts, value shapes, distinct types and categories): the header matches; the file
    is UTF-8 without a BOM, LF line endings, quoting only where needed; £0.00 card checks
    have `Money Out` "0.00"; pot transfers and Monzo-to-Monzo payments have an empty
    `Description`; pot transfers are in "Transfers"; IDs are all `tx_…` and unique.
    The fixture follows it. Notes over several lines and refunds in `Money Out` didn't
    occur in that export and stay as fixture rows from Monzo's known behaviour.

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
  which override the preset's category and apply every other action (M1.24), suggests
  transfer pairs (`transfer::detect_for_batch`), then records possible duplicates
  (`duplicate::detect_for_batch`) and suggests refund and reimbursement links
  (`link::suggest_for_batch`); a failure in any step rolls the whole import back.
  `CommitSummary` counts the file's rows categorised by the preset and by rules, the
  mirrored rows and created pots, the pairs suggested, rule splits skipped, possible
  duplicates and link suggestions. Undo clears the batch's split lines, tag links, change
  items, links (either side) and duplicate candidates before deleting its rows;
  `UndoPreview::merged_into` counts rows elsewhere merged into the batch's rows, which
  stay excluded. The M1.28 backup is taken
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
  and including `report`, except `transaction_split` and `ledger_line` (V5, M1.20),
  `tag`/`transaction_tag` (V6, M1.21), `merchant_alias` (V7, M1.23),
  `change_batch`/`change_item` (V8, M1.24/M1.26), `transaction_link` (V9, M1.25; it also
  redefines `ledger_line`), `duplicate_candidate` (V10, M1.26), `mcp_client`,
  `mcp_access_log` and `setting` (V11, M2.1), the schedule tables and
  `recurring_dismissal` (V12, M2.6/M2.10), the inbox's decision columns (V13, M2.3) and
  `mcp_access_log` rebuilt with two outcomes, `pending_id` and `summarised_calls` (V14),
  and `account.preset_id` (V15, M3.2).
- refinery embeds migrations in file-name order (`V10__` before `V1__`);
  `db::migrations::embedded` sorts them by version.

- `account(id, parent_id?, name, institution, kind [current|savings|credit_card|pot|cash|
  asset|liability|...], currency, opening_balance_minor?, opening_date?,
  reconciled_through?, closed_on?, sub_account_key?, preset_id?)` — pots are child
  accounts of their parent; a pot's parent is a `current` or `savings` account in the same
  currency; `sub_account_key` (V4, pots only, unique per parent, read-only once set) is
  the bank's ID for a pot the import created; `preset_id` (V15, current, savings and
  credit card only) is the preset the next import uses ("Decision record: accounts
  management");
  `asset` and `liability` accounts hold `valuation` snapshots only, no transactions
- `import_batch(id, account_id, source_file, format [csv|ofx|qif|camt053|mt940],
  preset_id?, imported_at, status [committed|undone])` — `source_file` is the file name
  without its directory; QFX is `ofx`
- `import_flag(id, batch_id, location_kind [line|entry], location, kind [unreadable|
  source_id_conflict|locked|running_balance], detail)` — a row the import left out or
  doubts (M1.11–M1.13, V3): its place in the file and core's reason, never the row's
  text. Read-only; undo deletes it with its batch
- `change_batch(id, kind [rule_apply|duplicate_merge|bulk_edit], created_at,
  status [applied|undone], undone_at?)` and `change_item(id, batch_id, transaction_id,
  field [category|merchant|user_note|tags|exclusion|split|share|reimbursable],
  old_value, new_value)` — undo for changes to existing transactions (V8). Values are
  JSON, one shape per field (`coffer_core::change::Value`); one item per batch,
  transaction and field; items and a batch's kind and date are read-only, and an undone
  batch stays undone. Deleting a transaction deletes its items
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
  share_label?, amount_minor, status [suggested|confirmed|rejected], created_at)` —
  refunds and reimbursements target a purchase; settlements target a share label (V9).
  One link per pair in any status; the inflow is money in, the target money out, in one
  currency; confirmed links never take more than either side has; a linked inflow isn't
  split and has no share; a linked row's amount and currency are fixed. `ledger_line`
  gains `link_id`, `link_kind`, `target_id`, `target_split_id`, `in_reports` and
  `personal_minor` (see "Decision record: M1 wave 4", section B)
- `duplicate_candidate(id, transaction_a, transaction_b, reason [overlapping_import|
  manual_entry], status [pending|merged|dismissed], kept? [a|b], change_batch_id?,
  created_at)` — a pair once, `transaction_a < transaction_b`, on one account; `kept` and
  the `duplicate_merge` batch are set exactly when merged (V10)
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
- `schedule(id, account_id, merchant_display, category_id?, cadence [weekly|fortnightly|
  four_weekly|monthly|quarterly|yearly], start_date, amount_minor, currency,
  tolerance_minor, tolerance_days, status [active|paused|ended], series_key?,
  created_at)`, `schedule_skip(schedule_id, occurrence_date, skipped_at)` and
  `schedule_match(schedule_id, occurrence_date, transaction_id, matched_by [auto|user],
  matched_at)` (V12) — `start_date` is the first tracked occurrence; occurrence *k* is
  `Cadence::nth(start_date, k)`, always counted from the start, and the next date is
  derived, never stored. Account, cadence, start, currency and series key are read-only;
  an ended schedule stays ended. A transaction fulfils at most one occurrence (UNIQUE),
  on the schedule's own account; an occurrence is never both matched and skipped; matches
  and skips are only inserted or deleted. Missed and late are derived, not stored
  ("Decision record: M2: MCP server", section D)
- `recurring_dismissal(series_key, dismissed_at)` (V12) — a detected series the user
  marked "Not recurring"; it isn't suggested again
- `budget(id, category_id, month?, amount_minor, currency, rollover,
  group_kind [fixed|non_monthly|flex], opening_set_aside_minor?, created_at)` — rollover
  is derived from `ledger_line`, never stored; `opening_set_aside_minor` is what a
  non-monthly budget had already saved when it was created. `month` empty is the standing
  budget and `YYYY-MM` overrides one month. Fixed and flex amounts are monthly; a
  non-monthly amount is yearly
- `fx_rate(date, base, quote, rate_e9, source [ecb|manual])` — 1 `base` = `rate_e9` /
  10⁹ `quote`: fixed point, so no float enters a conversion
- `suggestion(id, kind [categorization|rule|transfer_pair|merchant_alias|refund_link],
  payload_json, rationale, source_client, created_at, status [pending|approved|rejected],
  decided_at?, decision_reason?, change_batch_id?, applied_json?)` — `source_client =
  coffer` for suggestions the app files itself. `payload_json` is the validated canonical
  form; one pending suggestion per `(kind, payload_json)` (partial unique index
  `suggestion_pending_once`). V13 adds the decision columns and triggers: the proposal
  fields are read-only, a suggestion is filed pending, a decided one stays decided and has
  `decided_at`, only a rejection has a reason (≤ 500 characters), and only an approval has
  a change batch or `applied_json` (a record of what it created, with no foreign keys)
- `report(id, period, title, body_markdown, source_client, created_at)`
- `mcp_client(name, enabled, access_level [aggregate|pseudonymous|standard],
  status [requested|known|ignored], created_at, last_seen_at?)` (V11) — keyed by the name
  `coffer mcp --client` was given (1–64 of `A–Z a–z 0–9 . _ -`, never `coffer`); an
  enabled client is `known`. An unknown name creates a `requested` row that isn't
  enabled. Revoking sets `enabled = 0` and the level back to `aggregate`
- `mcp_access_log(id, at, client, tool, access_level?, outcome [ok|refused_off|
  refused_not_enabled|refused_level|refused_restoring|refused_unavailable|error],
  purpose?, args_redacted_json, row_count, result_hash?, withheld_count, pending_id?,
  summarised_calls?)` (V11, rebuilt by V14) — read-only; never stores returned data.
  `client` is the name (no foreign key, so a log row outlives its client); a refused or
  failed call has no rows, hash or withheld count; `withheld_count` is how many
  descriptions the call withheld as instructions. `refused_restoring` and
  `refused_unavailable` are calls refused while the database couldn't be opened, moved in
  from the pending file with their own time and `pending_id` (unique); `summarised_calls`
  is set only on a row standing for calls past the pending file's cap (decision record
  "M2: MCP server", F.8). Rows older than 12 months are deleted when a server starts
- `setting(key, value)` (V11) — `mcp.enabled` (the kill switch; on unless set off) and
  `mcp.pseudonym_key` (32 random bytes, created by `coffer ai-access enable`, never
  exposed)

## MCP surface

`coffer mcp --client <name>` (`crates/mcp`, rmcp over stdio). Setup for Claude Code,
Claude Desktop and Codex: [`mcp-setup.md`](mcp-setup.md). Decisions: "Decision record:
M2: MCP server (2026-11-13)".

**Tools by level** (a client sees and can call the tools at its level and below; a call
above it is refused `refused_level`):

| Level | Tools |
|---|---|
| `aggregate` | `list_categories`, `list_reports`, `monthly_summary(from, to)`, `spending_by_category(period)`, `budget_status(month)`, `net_worth_history(from, to)`, `save_report(period, title, body_markdown)` |
| `pseudonymous` | also `list_accounts`, `search_transactions(filters, limit)`, `uncategorized_transactions(limit)`, `recurring_payments()` |
| `standard` | also `list_rules`, `merchant_history(merchant)`, `propose_categorizations`, `propose_rule`, `propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link` |

Reads go through `ledger_line`, so split purchases count per category, and a confirmed
refund nets against its purchase's category. Amounts are `{minor, currency, home}` with
integer minor units; `home` stays `null` until home-currency conversion (M4.6), and totals
are listed per currency, never added across currencies. Every tool takes an optional
`purpose` string, which is logged and never interpreted. Added with M4.8:
`cash_flow_forecast`.

**What each level withholds**:

- **Merchants.** At `pseudonymous` every merchant is a token, `M-` plus 12 hex digits of
  HMAC-SHA256 of the trimmed, lower-cased display name under the per-database
  `mcp.pseudonym_key`: stable across calls, different between databases. Without a key a
  `pseudonymous` call is refused.
- **Descriptions.** Bank text appears only at `standard`, only when asked for
  (`include_description`), and only as `description_untrusted`. Below `standard` the
  field holds a fixed `[hidden: …]` text: bank text names the merchant and would undo the
  tokens.
- **Text search.** `search_transactions`' free-text `query` and its tag filter need
  `standard`: below it, counting the rows a text matches would reveal what lies behind a
  token. Dates, accounts, categories and amounts filter at `pseudonymous`.
- **Account names.** At `aggregate`, `net_worth_history` gives each account a `label`
  (its kind and number, e.g. `pot 2`) and no name; names appear from `pseudonymous` up,
  since a name can name a person.
- **Names in bank text.** A merchant or description that is a person's name reaches a
  client only at `standard`, which is full access by design (user, 2026-11-13).
- **Never, at any level:** account numbers, sort codes and IBANs (a merchant holding one
  is `null`, a description holding one is `[withheld: account details]`), notes (the
  bank's and the user's), share labels, private tags, merchant addresses and
  attachments. Public tags only at `standard`.

**Inbox writes** (never modify transactions/rules directly): `propose_categorizations`,
`propose_rule` (actions `set_category`, `split_template`, `set_merchant`, `add_tag` only),
`propose_transfer_pair`, `propose_merchant_alias`, `propose_refund_link`, `save_report`.
Payloads are validated against the database when filed and again when approved; an
identical pending suggestion returns the existing ID (`already_pending`). Caps: 200
pending suggestions and 500 saved reports per client; only the user deletes reports
(`coffer inbox delete-report`), never a tool. The source is always the calling client,
never an argument.

**Prompts**: `monthly_review(month)` — patterns, anomalies, subscription creep, budget
variance and suggestions, ending with `save_report`. Its steps name only tools registered
at the client's level; `month` must parse as `YYYY-MM` before it enters the text.

**Access control** (M2.11). On every call the server reads the kill switch
(`mcp.enabled`) and the client's row: switched off → `refused_off`; unknown (recorded as
`requested`), requested, ignored or revoked → `refused_not_enabled`; a tool above the
level → `refused_level`. A lower level applies to the next call, and the server sends
`notifications/tools/list_changed` (also from a 5-second poll, so an idle client hears).
Only the CLI or app enables a client or changes a level.

**Access log** (M2.12): tools write only `suggestion` and `report`. Separately, the
server appends exactly one `mcp_access_log` row per tool call as its own audit record,
whatever the outcome; if it can't be written, the call fails and no data goes out. The
AI's inputs to it are the untrusted `purpose` (stored without invisible characters) and
the arguments, redacted by the tool's schema: numbers, booleans, dates and schema enum
values are kept, any other string becomes `{"length":N,"type":"string"}`, undeclared keys
are only counted (`"_undeclared": N`). `result_hash` is SHA-256 of the result's canonical
JSON (compact, keys sorted). `coffer ai-access log` shows the log. A call refused because
the database can't be opened (a restore holds its lock: `refused_restoring`; migrations
pending or the lock unreadable: `refused_unavailable`) can't be written there: it waits
in `<database>-access-pending` and goes into the log, at its own time, the next time the
database is opened to write (decision record "M2: MCP server", F.8). The one exception,
returning no data: getting a prompt.

**Restores.** The server opens the database for each call and keeps no connection
between calls. `coffer restore` holds the file lock on `<database>-restoring` while it
swaps the files; the server checks it before and after opening and refuses the call while
it's held, so the next call reads the restored file; the refused calls go into the
restored database's log, and so do the rows the replaced database logged after the
backup was taken (F.9). The lock goes with the restore's process, so a crashed restore
blocks nothing (decision record "M2: MCP server", F.1).

Transaction descriptions are untrusted input (prompt-injection surface); inbox-only
writes are the containment. In addition:

- **Text envelope** (`coffer_core::envelope`, M2.13). `description_untrusted` is
  sanitised in four steps: control, zero-width, bidi, tag and BOM characters are
  stripped and fullwidth letters folded to ASCII; the whole field is withheld, as
  `[withheld: N characters that read like instructions]`, when it matches a documented
  pattern list (role markers such as `system:`, "ignore previous/above…", a URL with a
  scheme, a `www.` host, a markdown or HTML link, a long base64 run); bare domains are
  made inert (`NETFLIX[.]COM`); the length is capped. The count goes to
  `mcp_access_log.withheld_count`, and the app's "Withheld from AI" filter runs the same
  check (`envelope::reads_like_instructions`). A display merchant that reads like
  instructions is withheld at `standard` too.
- **Impact preview.** The inbox shows what approving each suggestion would change (e.g. how
  many past transactions a rule would recategorise), so an overly broad rule or a fake
  transfer pair planted by injected text is visible before approval.
- **Reports.** Saved reports render as a markdown subset: no HTML, no images, links shown as
  plain text, so a report can't trigger a network request. `list_reports` marks them
  `ai_authored`, since stored AI text is untrusted when read back, and strips control,
  zero-width and bidi characters from the period, title and body (decision record,
  2026-11-10); the stored text is unchanged. `coffer inbox` shows them the same way.
- **Client setup.** [`mcp-setup.md`](mcp-setup.md) recommends a client profile with
  Coffer as the only data source and no web or fetch tools.

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
- Crate research for M3–M5 and Later: [`research/crates.md`](research/crates.md).
