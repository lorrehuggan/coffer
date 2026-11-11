# Roadmap

Coffer is a local-first personal finance dashboard: import bank statements,
see where your money goes, and talk to your data through any MCP-capable AI client
(Claude Code, Claude Desktop, Codex). Design decisions live in
[`ARCHITECTURE.md`](ARCHITECTURE.md); this file tracks what gets built, in what order.

Status legend: ✅ done · 🚧 in progress · ⬜ not started

Tasks are numbered `<milestone>.<n>` (e.g. `M1.3`, `D2.4`; `L` for Later) so they can be
referenced. Numbers are stable: append new tasks rather than renumbering.

Each milestone ends with something usable, and has explicit exit criteria.

Tasks M1.20+, M2.10+, M3.9+, M4.8+, M5.8 and L.11+ come from the feature research in
[`research/feature-ideas.md`](research/feature-ideas.md) (2026-11-09). Ideas rejected
there because they conflict with the invariants (bank sync, cloud, multi-user, MCP writes
beyond the inbox, AI in import) are deliberately not on this roadmap.

---

## M0 — Design ✅

- ✅ M0.1 Design interview; decisions recorded in `docs/ARCHITECTURE.md`
- ✅ M0.2 Dev environment (devenv/Nix, Rust stable, GPUI runtime libs)
- ✅ M0.3 GPUI hello-world template builds (`cargo build`)

---

## Design track — runs alongside M1–M2 ✅

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
  the build. Both live in `crates/app/src/theme/tokens.rs`
- ✅ D2.3 Bundle fonts: Inter 400/500/600/700 and Geist Mono 400/600 (static instances, SIL OFL,
  licence files included). Files in `assets/fonts/`, loaded and set on the theme by
  `crates/app/src/theme/fonts.rs`
- ✅ D2.4 Map `gpui-component`'s theme onto the tokens; list the components that need a custom
  build instead (likely: toggle, segmented control, agent card).
  `crates/app/src/theme/component.rs`
  maps every stock colour field to a token (a sentinel test fails on any unmapped or
  non-token field), plus radius, rem base, shadows and motion. The component list (custom:
  toggle, segmented control, view tabs, badge, tooltip, progress bar, agent card and the
  other compositions) is in `DESIGN.md` → "Implementing in GPUI"
- ✅ D2.5 Showcase window (`coffer --showcase`): colour swatches, type scale, buttons in every
  state, list row (default/hover/selected/focus), transaction rows with mono amounts,
  agent card. Built (`crates/app/src/showcase.rs`, components in `crates/app/src/ui/`), plus a
  stock components
  board (input, select, popover, menu, dialog, title bar, underline tabs, stock focus).
  Visually compared against the Paper artboards; no discrepancies
- ✅ D2.6 Record findings in `DESIGN.md` → "Implementing in GPUI": alpha colours, 1px hairlines,
  shadows, an outside focus ring, tabular mono figures, animation easing. Recorded under
  "Rendering findings (D2.6)", from the GPUI renderer source and the font files. Two defects
  fixed with tests: shadow blur was passed to GPUI as CSS blur (GPUI takes σ = blur/2), and the
  outside focus ring's gap was uneven at 1x. The three open questions are closed (decided
  while the user was away; review): disabled buttons with a fill paint the group's composite
  colours over their surface, so they match Paper (3.6:1 for the neutral primary; no new
  tokens, no Paper change); reduced motion is 0ms for every animation, opacity fades
  included ("Motion" changed; no code needed); the agent card's ⇄ is `Glyph::Transfer`

**D3 — Transactions screen in Paper**

- ✅ D3.1 1440×900, realistic data: long merchant names, pot transfers, a foreign-currency
  payment, a refund, uncategorised and AI-suggested rows, a multi-selection. Two artboards
  in the Coffer file: Transactions (`18E-0`, an AI-suggested row selected, its inspector
  with approve/reject) and Transactions · multi-select (`1NP-0`, three rows, bulk-action
  bar, selection summary). Composed from Components clones, all token-bound; one new token,
  `--container-search-field` (re-exported, hash `86e4aeb1`; `size::SEARCH_FIELD`). Fixed a
  2px lane offset on suggested categories (sparkle gap 8 → 6px) in Paper and in
  `ui::transaction_row`, with a layout test. Spec in `DESIGN.md` → "Transactions"

**D4 — Overview screen in Paper**

- ✅ D4.1 Display figures, income-vs-spending chart, category bar list, agent card. Artboard
Overview (`22V-0`, 1440×900): the Transactions shell with the Overview tab active and no
inspector; month switcher (October 2026), Spent/Income/Net figures with deltas, a compact
agent card at the end of the figures row (moved there from the lower right column, which
couldn't hold it and the recurring list), the 12-month chart with October selected and a
July hover tooltip, a ranked bar list of spending by category (six palette colours,
Other, Uncategorised), and recurring payments with a `warning` price rise. All token-bound,
no new tokens. Charts decided: custom, on the `gpui_component::plot` primitives. Spec in
`DESIGN.md` → "Overview", "Data visualisation"
- ✅ D4.2 Adjust tokens if the screens show problems. No token changes: the six chart
colours stay distinct as 3px bars and markers, `warning` and `danger` read in context, and
neutral amounts hold. The screen changed rules instead: a line over bars gets a 1.5px
`canvas` halo (the `text` line on `text-secondary` bars is 1.8:1), bars are a fixed 16px
rather than "group gap 40% of bar width", and totals whose label gives the direction are
unsigned (`DESIGN.md` → "Money", "Chart chrome")

**D5 — Transactions in GPUI (fidelity spike, part 2)**

- ✅ D5.1 Build the D3 screen in GPUI with fixture data; compare it against the Paper screenshot
  at 1x and 2x. `crates/app/src/transactions.rs` (title bar, left panel, toolbar, virtualised
  day-grouped table, inspector for one or several transactions, bulk-action bar, status
  bar), now the window `coffer` opens; `crates/app/examples/snapshot.rs` renders it headlessly.
  Colour exact once Paper's Display P3 export is colour-managed, layout exact at 2x,
  baselines within half a pixel. Letter spacing now applied (`ui::tracked`). Four GPUI
  pitfalls found and fixed. Findings in `DESIGN.md` → "Implementing in GPUI" → "Fidelity
  findings (D5)"
- ✅ D5.2 Decision: GPUI can deliver the design → continue; otherwise reconsider the UI toolkit
  before M3. **Decided (user, 2026-11-09): continue with GPUI.** Colour and layout match
  Paper, and the gaps found were in Coffer's code, not GPUI. Still to check on a real display
  (hover, scrolling, drag and resize, macOS, long lists) as the app grows

**D6 — Remaining screens**

- ✅ D6.1 Import, Inbox, Trends, Budgets, Net worth, Reviews, accounts panel. Six
  1440×900 artboards in the Coffer file, built from the Transactions and Overview shells:
  Inbox (`564-0`), Import · map columns (`5ZQ-0`), Budgets (`6OE-0`), Trends (`781-0`),
  Net worth (`7LH-0`), Reviews (`7YM-0`); plus two states boards: Import · steps (`6J4-0`:
  File and Review, and the reconciled / mismatch / locked-period results) and Accounts
  panel · states (`8D8-0`: row states, the account inspector, the new-account dialog). All
  token-bound. Two new tokens, `--container-dialog-wide` (760px, Import) and
  `--container-reading` (560px, a review's measure), re-exported (hash `6a126241`) with
  `size::DIALOG_WIDE` and `size::READING_MEASURE`. Spec in `DESIGN.md` → "Finance
  screens". Decided while drawing (review):
  - Inbox groups are table sections with lane headers, not one agent card per group; row
    approve/reject show on hover and selection only;
  - a parse failure in the Import preview is imported as flagged, and a balance mismatch
    can be imported only as "Import and flag the mismatch";
  - Budgets is a table with inline bars so the budgets' ends line up;
  - the Net worth area runs to a £0 baseline;
  - an account with transactions can be closed (hidden, history kept), not deleted.
- ✅ D6.2 Coffer logo / wordmark. The strongbox: lid, seam, body and a hasp, filled
  `molten`, on a 16-unit grid so the 16px title-bar size is pixel-aligned. Logo artboard
  (`8MT-0`): construction, title-bar and display lockups, mono variant, app icon (molten
  tile, chrome mark); the three directions explored stay on Logo · concepts (`8JS-0`).
  Every screen's title bar now shows the mark before the wordmark (6px gap), and so does
  the GPUI title bar (`Glyph::Mark`). Exports in `assets/brand/`. Spec in `DESIGN.md` →
  "Logo"
- ✅ D6.3 Transactions additions: split editor (lines, auto-filled remainder, total
  locked), tags, user note beside the read-only imported note, excluded state and
  filter, manual-transaction form, "Create a rule from this edit?" (M3.9, M3.10, M3.13).
  Four 1440×900 artboards duplicated from Transactions:
  - split and tags (`2Q3-0`);
  - excluded, with the Excluded filter chip on (`3FE-0`);
  - rule from an edit (`3WT-0`);
  - manual (cash account) (`4D7-0`).

  Plus a states board (`4X9-0`): unbalanced and balanced split states, the exclude and
  new-transaction dialogs, the undo notification after a bulk change, and the new row
  kinds. All token-bound. One new token, `--container-dialog` (440px), re-exported (hash
  `b442a048`) with `size::DIALOG`. Spec in `DESIGN.md` → "Transactions additions". Its
  three data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: Transactions additions"):
  - exclusion reason key, note and date, where only duplicates leave the balance;
  - `created_at` on every transaction;
  - the rule prompt needs at least one earlier match.
- ✅ D6.4 Review queues: possible duplicates (side-by-side pair, why flagged,
  merge/dismiss) and reconciliation (assertion results per account, failing window,
  lock/unlock) (M3.12). Two 1440×900 artboards from the Inbox shell, Review · possible
  duplicates (`8SE-0`) and Review · balance checks (`98K-0`), plus a states board,
  Review queues · states (`9MQ-0`: lock and unlock dialogs, the merge undo bar, a merge
  into a locked period, the empty queue). All token-bound, no new tokens. Spec in
  `DESIGN.md` → "Review queues". Decided while drawing (review):
  - both queues sit under a new CHECKS section in the Inbox's left panel, neutral (not
    moldavite), and outside the Inbox tab's suggestion count;
  - the selected pair opens in place side by side; shared values are muted so the
    differences stand out;
  - the inspector cross-links the two queues: a pair names the check it fixes, and a
    failing check names the pair that explains it.

  Its five data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: review queues"):
  - the oldest balance is the starting point, not a check;
  - a period locks only through a passing check, with no partial unlock;
  - a merge keeps the row with a source ID, then an imported row, then the oldest;
  - the near-duplicate window is 3 days (a default);
  - a dismissed pair stays dismissed for good.
- ✅ D6.5 Refund, reimbursement and settlement linking; outstanding-claims and owed-to-me
  lists (M3.11). Three 1440×900 artboards: Transactions · refund link (`9WS-0`),
  Transactions · shared cost (`BAT-0`) and Review · owed to you (`CCF-0`, Inbox shell);
  plus a states board, Links and shares · states (`EB0-0`: the link dialog, the
  inspectors after linking, claim back partly repaid, the new row kinds, the empty
  queue). All token-bound, no new tokens. Spec in `DESIGN.md` → "Refunds,
  reimbursements and shared costs". Decided while drawing (review):
  - every transaction gets a Links section, and every outflow a Share section ("Just
    me" / "Shared" / "Claim back"), so links and shares are always in the same place;
  - Coffer's own link suggestions are neutral cards in the inspector and strips on Owed
    to you, not Inbox items; the AI's `refund_link` suggestions stay in the Inbox (D6.6);
  - the table keeps the bank's amounts and says how a row counts in the category cell
    ("Shopping · refund", "· shared", "· to claim");
  - Owed to you is a third CHECKS row in the Inbox, with expense claims and people in
    one list.

  Its five questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  refunds, reimbursements and shared costs"):
  - a refund counts on its own date, in the purchase's category;
  - link suggestions are deterministic (90 days for refunds, 60 days to the penny for
    reimbursements) and live in the inspector, not the Inbox;
  - spending, budgets and MCP reads use your share; balances and net worth stay gross;
  - an overpayment is capped, and the rest stays unlinked;
  - a link needs both sides in one currency in v1.
- ✅ D6.6 Inbox impact preview and the new suggestion kinds (merchant alias, refund link)
  in the agent card; designed with the D6.1 Inbox (M3.14, M2.14). Two 1440×900 artboards
  from the Inbox with the D6.4 CHECKS panel, Inbox · rule impact (`ABK-0`) and Inbox ·
  merchant name impact (`DL8-0`), plus a states board, Inbox · D6.6 states (`EWZ-0`: the
  refund link, transfer pair and categorisation inspectors, Coffer's own suggestion card,
  a wide rule, the undo bar). All token-bound, no new tokens. Spec in `DESIGN.md` →
  "Inbox impact preview". Decided while drawing (review):
  - the selected suggestion opens in place and lists the transactions it would change on
    the group's own lanes, with rows it leaves alone marked "Stays, you set it"; the
    inspector sums it up under "If you approve", like D6.4's "If you merge";
  - groups are ordered by reach (rules, merchant names, transfer pairs, refund links,
    categorisations), and a rule's lane counts the rows it changes, not the ones it
    matches;
  - a wide rule (more than 5 merchants) gets a warning, a by-merchant summary, and is left
    out of Approve all;
  - Coffer's own `rule` suggestions use the same card, neutral, outside the moldavite
    badge.

  Its six questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  Inbox impact preview"):
  - approving an agent's rule never changes a category you set;
  - a rule is wide past 5 merchants or when it moves income (a default);
  - Approve all always skips wide rules;
  - a rule's approval settles the categorisations it covers;
  - merchant names don't change which rules match;
  - the impact is computed when shown.
- ✅ D6.7 Rules editor: rule actions, apply-to-history preview, merchant alias editor
  (M3.13). Four 1440×900 artboards from the Review · possible duplicates shell: Rules
  (`B1J-0`), Rules · apply to past transactions (`EFT-0`), Rules · merchant names
  (`G2O-0`) and Rules · change history (`GZT-0`), plus a states board, Rules · states
  (`HFH-0`: delete and undo dialogs, the undo bar, no rules yet, several actions, the
  add-action menu, a pattern that can't be used, a rule that's never reached). All
  token-bound, no new tokens. Spec in `DESIGN.md` → "Rules". Decided while drawing
  (review):
  - Rules is an eighth view tab after Reviews (decided by the user, 2026-11-10), with
    Rules, Merchant names and Change history in its left panel;
  - the rule editor is the inspector (When, Then, Past transactions), and rules are
    reordered by dragging rows; a rule that always loses to one above is flagged "Never
    reached";
  - applying a rule to history previews it on the canvas in two sections, "Will change"
    and "Category you set", which are left out unless "Change these too" is on;
  - merchant names apply at once, without a preview or undo batch, because the display
    name is derived;
  - a menu pattern (for "+ Add action") is drawn for the first time.

  Its eight questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  rules"):
  - rules win field by field;
  - a new rule goes first;
  - a rule overwrites a category you set only with "Change these too";
  - rule edits don't re-run on history; only Apply does;
  - merchant names resolve Is, then the longest Starts with, then Pattern;
  - undo skips rows changed since, and counts them;
  - deleting a rule leaves its values and clears `rule_id`;
  - locked periods guard balances only.

  The tab isn't yet on the artboards drawn before D6.7, or in the GPUI title bar
- ✅ D6.8 Settings: AI access (clients, access levels, kill switch, access log), backups
  (list, create, restore), export dialog (M3.15, M3.16). Three 1440×900 artboards from
  the Inbox shell: Settings · AI clients (`AQ2-0`), Settings · access log (`DC6-0`) and
  Settings · backups (`F63-0`); plus a states board, Settings · D6.8 states (`GQA-0`: AI
  access off, the enable dialog, restore with checks passed and failed, the export
  dialog, the "Back up now" notification). All token-bound, no new tokens. Spec in
  `DESIGN.md` → "Settings". Decided while drawing (review):
  - Settings isn't an eighth view tab: a settings icon button in the title bar (⌘,)
    opens it, with AI ACCESS and DATA sections in the left panel;
  - a client name that isn't enabled is refused and listed under "Asked to connect",
    and enabling it starts at Aggregate, the least shared level;
  - the kill switch is a toggle in the AI clients toolbar; off is neutral (a banner, the
    clients dimmed, "AI access off" in the status bar), not a warning;
  - the access log shows the client's purpose as quoted, untrusted text, refused calls
    as neutral rows, and free-text arguments as their length only;
  - restoring is a neutral primary action, because a backup of the current state is
    taken first; a backup that fails a check can't be restored.

  Its six questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  Settings, AI access and backups"):
  - the kill switch is read on every call; `mcp_access_log` gains `outcome` (new
    migration);
  - unknown clients are recorded as `requested` and refused;
  - a lower level applies at once, with `tools/list_changed`;
  - argument redaction keeps the structure, and free text as type and length;
  - the access log is kept for 12 months (a default);
  - backup metadata lives in the file name; older schemas migrate, newer are refused.
- ✅ D6.9 Accounts: cash accounts and manual asset/liability accounts with valuations;
  designed with the D6.1 accounts panel (M3.10, M3.17). One 1440×900 artboard,
  Accounts · asset (valuations) (`BSI-0`: House selected, step-line value chart,
  valuations table, account inspector), and a states board, Accounts · D6.9 states
  (`EPF-0`: liability and cash inspectors, New account set to Asset, the Kind menu, the
  cash count, a liability balance, closing an asset). All token-bound, no new tokens.
  Spec in `DESIGN.md` → "Accounts you keep yourself". Decided while drawing (review):
  - an ASSETS AND LIABILITIES section in the left panel; Stocks ISA moves there, cash
    stays under ACCOUNTS;
  - a liability's negative balance isn't `danger` ("Money" now says which negatives
    are);
  - with a valuation account selected, its valuations replace the transaction table,
    and ⌘N adds a valuation;
  - a step line with no area fill, so its y axis needn't start at £0;
  - a short cash count offers to add the difference as a transaction, and never does
    it on its own.

  Its five data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` →
  "Decision record: accounts you keep yourself"):
  - a valuation can carry a private `note` (new migration);
  - valuation accounts hold valuations only;
  - `account.closed_on` for every kind, plus a £0 valuation when closing one;
  - only an account without transactions can be deleted;
  - a cash count is a checked `manual` balance, and Coffer never posts the difference.
- ✅ D6.10 Upcoming payments and balance forecast; budget rollover, groups and pace line,
  designed with the D6.1 Budgets screen (M4.8, M4.9). Two 1440×900 artboards from the
  D6.1 Budgets screen: Budgets · groups, rollover and pace (`CXU-0`) and Budgets ·
  upcoming (`GD5-0`). Plus a states board, Budgets and upcoming · states (`HNB-0`): the
  non-monthly inspector, rollover rows, the schedule dialog, the "Review 2 found"
  popover, a missed payment, an overdrawn forecast, and the empty Upcoming. All
  token-bound, no new tokens. Spec in `DESIGN.md` → "Budgets: groups, rollover, pace and
  upcoming". Decided while drawing (review):
  - Upcoming is the second half of the Budgets tab (Budgets | Upcoming), not a ninth tab;
  - groups are table sections (Fixed, Non-monthly, Flex), and rollover is a signed,
    muted column, so LEFT = Budget + Rollover − Spent;
  - Left to spend counts Flex only, and non-monthly money shows as Set aside;
  - the pace line is one dashed line across the Flex bars; bars aren't coloured by pace;
  - a missed payment stays in the forecast until it's matched or skipped.

  Its six data-model questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision
  record: budgets and the forecast"):
  - Flex spending leaves accounts in proportion to the last 3 months;
  - a payment is missed only once imports cover its date;
  - rollover carries both ways and is derived from history;
  - pace is linear by day;
  - a non-monthly amount is seeded once from yearly schedules, then belongs to the user;
  - without scheduled income, the horizon is 30 days.
- ✅ D6.11 Safe report rendering (markdown subset, inert links, `ai_authored` marker) and
  the withheld-text indicator, designed with the D6.1 Reviews screen (M4.10, M2.13).
  Reviews · safe rendering (`C6D-0`, 1440×900, "Subscription check" with a table, an
  inert link and a blocked image) and a states board, Reviews · D6.11 states (`E43-0`:
  the subset as source and rendering, each case shown as text, the withheld-text
  markers). All token-bound, no new tokens. Spec in `DESIGN.md` → "Safe report
  rendering". Decided while drawing:
  - links render as their text followed by the address in mono `text-muted`, never
    clickable; a link whose text looks like an address shows both;
  - images become a dashed "Image not loaded" block that shows the address; HTML shows
    as written; anything outside the subset keeps its markdown characters;
  - "AI-written" agent badge in the byline, and a "Read back · `ai_authored`" row;
  - the inspector counts what was shown as text, and "What it read" counts withheld
    descriptions (`warning` shield), linking to a new "Withheld from AI" filter.

  Its four questions were decided on 2026-11-10 (`ARCHITECTURE.md` → "Decision record:
  safe report rendering and withheld text"):
  - emphasis renders as weight 500, with no italic face;
  - `withheld_count` on `mcp_access_log` (new migration), no transaction IDs logged;
  - the marker reads `[withheld: N characters that read like instructions]`;
  - `list_reports` strips hidden characters and sets `ai_authored`, without withholding.

**Exit criteria:** tokens live in Paper and in the GPUI theme with a drift test;
Transactions and Overview are designed; the GPUI build of Transactions matches the Paper
screen closely enough that the designer signs off.

**Signed off** by the user on 2026-11-10. Every D item is done and every design question
is decided in `ARCHITECTURE.md`. The choices marked "review" in this track and in
`DESIGN.md` (decided while drawing, or while the user was away) are accepted as drawn.

---

## M1 — Core + import (headless) 🚧

Import real statements into a local database from the command line.

- ✅ M1.1 Convert to a Cargo workspace: `crates/core`, `crates/import`, `crates/mcp`, `crates/app`.
  Packages `coffer-core`, `coffer-import`, `coffer-mcp` (stubs for now) and `coffer-app`
  (lib `coffer_app` plus the `coffer` binary); `assets/` and `design/` stay at the root.
  Versions, edition and licence come from the root `[workspace.package]` and
  `[workspace.dependencies]`; each crate declares the dependencies of its role
- ✅ M1.2 Add `LICENSE-MIT`, `LICENSE-APACHE`; set `license = "MIT OR Apache-2.0"`
- ✅ M1.3 SQLite setup: platform data dir, WAL mode, 0600 permissions, versioned migrations.
  `coffer_core::db::Database::open` creates the data dir (0700) and `coffer.db` (0600,
  as are its `-wal`/`-shm`), sets WAL, `synchronous = FULL`, foreign keys,
  `trusted_schema = OFF`, a busy timeout and `BEGIN IMMEDIATE`, then applies the
  embedded `crates/core/migrations/` in one transaction. An existing file is checked
  before anything changes it (a SQLite database, not a newer schema, unedited migration
  history); a refused file is left byte- and mode-for-mode as it was. A newer schema or
  an edited, unknown or skipped migration is refused. `Database::open_existing` (for
  `coffer mcp`) never creates or migrates: a missing database, a pending migration, a
  newer schema or a changed history is refused. `COFFER_DATA_DIR` overrides the data dir
- ✅ M1.4 Schema: accounts, import batches, transactions, balance snapshots, categories,
  rules, transfer groups, budgets, FX rates, suggestions, reports.
  `crates/core/migrations/V1__initial_schema.sql`: `STRICT` tables, checked dates and
  currency codes, enum `CHECK`s, no `ON DELETE` actions, indexed foreign keys, triggers
  for read-only imported source fields and two-level categories, partial unique indexes
  for dedupe, FX rates as `rate_e9` fixed point (`ARCHITECTURE.md` → "Data model").
  Constraints carry the invariants: dedupe keys are source ID first (fingerprint only
  for rows without one), inserts never replace a stored row, and triggers enforce
  account currency, pot structure, valuation-only `asset`/`liability` accounts and
  read-only import batches. Triggers also enforce category keys against their parent
  (keys and parents read-only), keep a pot that holds transactions or balances under its
  parent, fix an account's currency once a rule with amount bounds names it (and keep such a rule
  in that currency), and enforce
  undo (an `undone` batch has no rows, takes none and never returns to `committed`); a
  partial unique index allows one entered balance per account and day. Tables for later
  items (splits, tags, links, duplicates, change batches, schedules, MCP clients) come
  with those items
- ✅ M1.5 Money type: integer minor units + ISO 4217 currency (no floats anywhere).
  `coffer_core::money`: `Currency` only builds from a known ISO 4217 code and carries its
  minor units (hand-written table, frozen by tests, withdrawn codes since 2014 kept);
  `Money { minor: i64, currency }` with checked arithmetic (mixing currencies or
  overflowing is an error); `Money::parse_decimal` reads bank text exactly (`.`/`,`
  decimal mark, grouped thousands, `-`/`−`/`+`/parentheses) and refuses anything it
  would have to round; `to_decimal_string` is the plain export form. `Currency` is
  `serde` and SQLite `TEXT`. Locale display formatting stays in the app; FX is M4.6
- ✅ M1.6 `Importer` trait (detect → parse → normalize).
  `coffer_import`: `Detect` (name, format, `Detection` from `No` to `Source`), `Importer`
  (`parse` into its own `Parsed` type, `normalize` into a `Statement`), `AnyImporter` for
  `&dyn` lists with `import`, and `detect` to rank candidates (the caller chooses).
  `coffer_core::statement`: `Statement` (format, preset, currency, rows in file order,
  stated balances), `StatementLine`, flagged rows as `RowProblem` at a `SourceLocation`
  (never dropped, no transaction text), `normalize_description`. Exercised with a toy
  importer in tests; CSV and OFX are M1.7 and M1.10 (`ARCHITECTURE.md` → "The
  `Importer` trait")
- ✅ M1.7 CSV importer: encoding detection, delimiter, header/skip rows, date formats
  (`DD/MM` vs `MM/DD`), separate time column, decimal style (`1,234.56` vs `1.234,56`),
  signed amount or debit/credit columns, merchant vs raw description, source ID,
  source category/type, original (local) amount + currency, optional balance column.
  `coffer_import::csv`: `CsvImporter` over a validated `CsvMapping` (the shape M1.8's
  TOML will deserialize into). BOM or stated encoding, else UTF-8 only (never guessed);
  delimiter stated or detected (ties refused); `skip_rows` and optional header, columns by
  name or position; `strftime` dates that must round-trip, with `DateFormat::candidates`
  to surface `DD/MM`/`MM/DD` ambiguity; per-row currency column; signed or debit/credit
  amounts (`Unsigned`/`Signed`). Rows with the wrong column count are flagged
  (`RowProblem::ColumnCount`), as are unreadable fields. Synthetic fixtures in
  `fixtures/csv/` (`ARCHITECTURE.md` → "The CSV importer")
- ✅ M1.8 TOML preset format + loader (`presets/<region>/<bank>.toml`), including
  source-category → category mapping.
  `coffer_import::preset`: `Preset::from_toml` reads a `CsvMapping` plus ID, name, region,
  `ignore` and `[category_map]`; unknown keys are refused and field values are checked
  where written (errors point at the line); TOML column positions count from 1.
  `crates/import/build.rs` compiles every `presets/**/*.toml` in (`Presets::bundled`);
  `Presets::from_files` checks `<region>/<bank>.toml` placement, region, an ID of
  `<region>-<bank>[-…]` and unique IDs. `Preset::category_for` maps exact source
  categories to `coffer_core::category::CategoryKey`; `Preset::unread_columns` lists header
  columns neither read nor ignored. A test requires every bundled preset to import its
  fixture in `fixtures/presets/`. Contributor reference: `presets/README.md`
  (`ARCHITECTURE.md` → "Presets"). `[sub_account_transfer]` waits for M1.17
- ⬜ M1.9 First preset: `presets/uk/monzo.toml` with a redacted fixture (built from a real export)
- ⬜ M1.10 OFX/QFX importer (SGML 1.x and XML 2.x), `FITID` IDs, `LEDGERBAL` balances
- ⬜ M1.11 Deduplication: source ID when present (OFX `FITID`, CSV ID column);
  fingerprint with occurrence index otherwise
- ⬜ M1.12 Balance reconciliation where the file has balances; failed imports flagged, not
  committed silently. Manual balance anchor for accounts whose exports have none
- ⬜ M1.13 Import batches with undo
- ✅ M1.14 Default two-level category set with stable keys. 15 groups, 50 subcategories,
  each `spending`, `income` or `transfer`, seeded once by `V2__default_categories.sql`;
  `coffer_core::category::{DEFAULT_CATEGORIES, CategoryTree}`; bundled presets may map
  only to default keys (tested) (`ARCHITECTURE.md` → "Categories")
- ⬜ M1.15 Rules engine (contains / regex / amount range / account), priority ordering.
  The validated rule types exist in `coffer_core::rule` (match field and kind, regex
  pattern with size limits, amount bounds, typed action values, cross-action checks); the
  engine, matching and ordering are still to do
- ⬜ M1.16 Transfer-pair detection (opposite amounts, own accounts, N-day window)
- ⬜ M1.17 Sub-accounts (Monzo Pots, Starling Spaces, Revolut Vaults): preset-driven detection,
  auto-created pot accounts, mirrored pot-side entries auto-paired as transfers
- ⬜ M1.18 CLI: `coffer import <file> [--account] [--preset] [--dry-run]`,
  `coffer accounts`, `coffer undo-import <batch>`
- ⬜ M1.19 Redacted fixtures + tests for every importer path
- ⬜ M1.20 Split transactions: `transaction_split` lines that sum exactly to the parent
  (checked in core); reports and budgets read the `ledger_line` view, so a split purchase
  counts in each of its categories
- ⬜ M1.21 Tags (several per transaction, private tags never leave the app) and a
  user-editable note beside the read-only imported one
- ⬜ M1.22 Manual transactions and cash accounts (`origin = manual`). Imported source fields
  stay read-only: fix a bad row by undoing its batch, or exclude it with a recorded reason
- ⬜ M1.23 Merchant clean-up: deterministic cleaning of bank text (card references,
  "CONTACTLESS", processor prefixes) into `merchant_display`, plus a merchant alias table.
  Raw fields are never changed
- ⬜ M1.24 Rule actions beyond setting a category: set merchant, add tag, set note, exclude,
  split template, my share. Each transaction records the rule that categorised it. Applying
  a rule to past transactions shows a preview and commits as an undoable change batch.
  Each action kind's `value_json` shape is typed and validated in
  `coffer_core::rule::RuleAction`; still to do: applying the actions, including
  converting basis-point shares to minor units
- ⬜ M1.25 Refunds, reimbursements and shared costs: `transaction_link`
  (`refund`/`reimbursement`/`settlement`). A refund nets against the purchase's category
  instead of counting as income; a reimbursable purchase stays outstanding until linked;
  `my_share_minor` on transactions and split lines feeds an owed-to-me balance
- ⬜ M1.26 Possible-duplicates queue: same account and amount within ±N days but different
  text (typically overlapping CSV and OFX exports). Merging excludes the duplicate, never
  deletes it, and can be undone
- ⬜ M1.27 Dated balance assertions: every balance snapshot is checked against the computed
  balance, and a failure names the window between the last passing assertion and the
  failing one. Per-account lock date (`reconciled_through`): anything that would change
  transactions before it is flagged and needs an explicit unlock
- ⬜ M1.28 Local backups: rotating `VACUUM INTO` snapshots (0600) before every import commit
  and migration; `coffer backup`, `coffer restore <file>` (integrity and schema-version
  checks before swapping)
- ⬜ M1.29 `coffer export --format csv|json`: documented schema, filters by date, account,
  category and tag; split lines keep their parent ID

**Exit criteria:** your own CSV/OFX exports import without duplicates on
re-import, reconcile where balances exist, and get categorized by rules. Splits, tags,
manual transactions and refund links work through the core API with tests, and a backup
exists from before every import.

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
- ⬜ M2.10 Schedules: confirm a detected recurring series (M2.6) as a schedule with an
  expected date, amount and tolerance; imports fulfil occurrences; missed or late payments
  are flagged
- ⬜ M2.11 MCP access control: MCP is off until a client is enabled (CLI until M3.15). Each
  client has an access level: `aggregate` (totals only), `pseudonymous` (stable keyed-hash
  merchant tokens) or `standard`. Tools above a client's level are never registered. This
  guards against over-sharing; it is not a security boundary against local processes
- ⬜ M2.12 AI access log: every tool call records the client, tool, redacted arguments,
  an optional `purpose`, row count, result hash and access level, never the returned data
- ⬜ M2.13 Untrusted-text envelope: `merchant_display` by default; raw text only as a
  length-capped, labelled `description_untrusted`, with control, zero-width and bidi
  characters stripped and URLs made inert. Text matching injection patterns is withheld,
  and the app shows how much was withheld. Setup docs (M2.8) recommend a client profile
  without web or fetch tools
- ⬜ M2.14 MCP over the new model: reads use ledger lines (splits), tags and refund links;
  new suggestion kinds `merchant_alias` and `refund_link`. The app files `rule`
  suggestions itself after repeated identical manual recategorisations (source `coffer`)

**Exit criteria:** ask "where did my money go last month?" in Claude Code and get
an answer backed by visible tool calls; approve AI-suggested categories from the inbox.
Every tool call appears in the access log.

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
- ⬜ M3.9 Split editor (the remaining amount fills in automatically; lines are locked to the
  total), tags and notes in the inspector
- ⬜ M3.10 Manual transactions and cash accounts; exclude a row with a reason, with an
  "excluded" filter so nothing disappears
- ⬜ M3.11 Link refunds, reimbursements and settlements; outstanding-claims and owed-to-me
  lists
- ⬜ M3.12 Duplicates review queue (merge or dismiss, showing why each pair was flagged) and
  reconciliation: assertion results per account, lock and unlock
- ⬜ M3.13 Rules: "Create a rule from this edit?", apply-to-history preview, merchant alias
  editor
- ⬜ M3.14 Inbox impact preview: what approving each suggestion would change (e.g. "changes
  143 past transactions; 12 have a category you set")
- ⬜ M3.15 AI access settings: enabled clients, access levels, revoke, kill switch, and the
  access log
- ⬜ M3.16 Backups (list, create, restore with the M1.28 checks) and the export dialog
- ⬜ M3.17 Manual asset and liability accounts (house, car, pension, mortgage, loan) with
  dated valuations

**Exit criteria:** day-to-day use (import, categorize, split, link refunds, review the inbox
and duplicates) without the CLI.

---

## M4 — Analytics ⬜

- ⬜ M4.1 Monthly overview: income vs spending vs net (bar/line), category breakdown, MoM change
- ⬜ M4.2 Trends: category and merchant over time, top merchants
- ⬜ M4.3 Recurring payments / subscriptions view
- ⬜ M4.4 Budgets: per-category monthly targets with progress
- ⬜ M4.5 Net worth: balances over time across accounts
- ⬜ M4.6 Multi-currency reporting in home currency; opt-in ECB rate fetch + manual rates
- ⬜ M4.7 Reviews: browse saved MCP reports
- ⬜ M4.8 Upcoming payments and a daily balance forecast per account from schedules,
  showing the lowest balance before the next income; `cash_flow_forecast` read tool
  (cached FX only)
- ⬜ M4.9 Budgets: per-category rollover; fixed, non-monthly and flex groups, with yearly
  schedules seeding the non-monthly amounts; a spending-pace line
- ⬜ M4.10 Safe report rendering: a markdown subset with no HTML or images, and links shown
  as plain text; reports are marked `ai_authored` when read back

**Exit criteria:** the dashboard answers "how am I doing?" at a glance, including what's
due before payday.

---

## M5 — Open-source release (v0.1) ⬜

- ⬜ M5.1 README (screenshots, install, MCP setup), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
- ⬜ M5.2 "Add your bank" guide: preset TOML + redacted fixture, no Rust required
- ⬜ M5.3 Presets for common UK, US, and euro-area banks
- ⬜ M5.4 Issue templates (bank preset request, bug, importer request)
- ⬜ M5.5 CI: fmt, clippy, tests; build on Linux, macOS, Windows (best-effort)
- ⬜ M5.6 Release artifacts: Linux tarball + Nix flake (+ AppImage), macOS `.dmg`
- ⬜ M5.7 Data location, backup, and privacy documentation
- ⬜ M5.8 Beancount and hledger journal export: deterministic and stably sorted; fixture
  exports are checked with `bean-check` / `hledger check` when those tools are installed
  (external tools, not dependencies)

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
- L.11 Attachments and receipts: local files stored by content hash, tax-bundle export;
  never exposed over MCP
- L.12 Savings and payoff goals: targets on pots and accounts, on-track status, avalanche vs
  snowball for debt
- L.13 Accounting reports (balance sheet, income statement), saved report definitions,
  global report exclusions, CSV export of any view
- L.14 Investments and holdings with manual or imported prices (an online price fetch would
  need its own decision, like the ECB one)
- L.15 Configurable month start (payday-aligned periods). Every report depends on it, so
  decide before M4.1 ships
- L.16 Anomaly and spike insights (median/MAD), with a read tool
- L.17 Runway, Age of Money and an in-app weekly recap
- L.18 Personal inflation index from your own recurring prices
- L.19 Calendar heatmap of daily spend
- L.20 What-if scenarios (seeded, inflation-adjusted projections)
- L.21 Pending → posted handling (needs fixtures showing which exports include pending rows)
- L.22 Match imported rows to manual entries
- 🙋 L.23 Starter merchant rule packs per region (TOML, like presets)
- 🙋 L.24 Migration presets for YNAB, Mint, Actual, Monarch and Firefly exports, including
  multi-account files
- 🙋 L.25 CSV hardening: UTF-16 via BOM, flag ambiguous `DD/MM`/`MM/DD` dates, footer rows,
  statement total shown before commit
- L.26 Statement archive: source files kept by hash and linked to their batch (pairs with L.6)
- L.27 Transfers: a queue for one-sided transfers; cross-currency pairs with the FX difference
- L.28 FX: bank-applied rate and fee per transaction; an FX fees report
- L.29 Structured search syntax shared by the GUI, exports and MCP; saved queries and
  formula cards
- 🙋 L.30 100k-transaction performance benchmark
- L.31 Account groups with subtotals and report include/exclude
- L.32 More MCP: `find_duplicates`, `preview_rule`, `reconcile_preview`, `list_suggestions`,
  `spending_pace` read tools; `duplicate_flag`, `budget_adjustment`, `recurring_review`
  suggestion kinds; tool annotations, output schemas and resources; prompts
  `categorize_inbox`, `rule_tuning`, `subscription_audit`, `year_end_review`, `weekly_review`
- 🙋 L.33 Export and import rules as TOML
