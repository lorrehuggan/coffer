# Feature ideas: research notes

Research date: 2026-11-09. Input for roadmap planning; nothing here is a decision. Roadmap
items are referenced by their [`ROADMAP.md`](../ROADMAP.md) numbers; invariants are those in
[`AGENTS.md`](../../AGENTS.md) and [`ARCHITECTURE.md`](../ARCHITECTURE.md).

## Method and limits

Four read-only research passes, each with a separate angle: (1) baseline features of YNAB,
Monarch, Copilot, Actual, Firefly III, GnuCash, Lunch Money and Beancount/Fava; (2) user pain
points from Reddit, Hacker News and GitHub issues; (3) newer ideas from 2024–2026
products, research and plain-text accounting; (4) local-first design and the AI/MCP angle.

- Firecrawl could not be authorised in a non-interactive session, so all passes used the
  built-in web search and fetch tools (0 Firecrawl credits, about 50 requests in total).
  Much of the evidence comes from search snippets of first-party pages (help centres, docs,
  changelogs, repos), not full reads.
- Vote counts (👍 on GitHub issues) come from the GitHub API on 2026-11-09. Actual Budget
  closes feature requests on purpose and labels them "needs votes", so a closed issue there
  still shows demand and doesn't mean the feature was built.
- Not covered: app-store and Trustpilot reviews, the GnuCash bugzilla wishlist, Firefly III
  Discussions ranked by votes, and a full read of the 2026-07-28 MCP spec.
- Effort (S/M/L) and value (low/med/high) are judgement calls, not estimates.

## What was excluded as already built or planned

The research was told not to re-propose these:

- **Import and correctness:** CSV presets, the mapping screen and saving a mapping as a
  preset; OFX/QFX; dedupe; reconciliation and the manual balance anchor; undoable batches;
  `--dry-run`; encoding, decimal-style and date-order handling (M1.7); original amount and
  currency per transaction.
- **Organisation:** two-level categories, the rules engine, transfer detection, pots.
- **AI:** the full planned MCP read and inbox surface, the `monthly_review` prompt, the PII
  guard and recurring detection.
- **Views:** the transactions table, inbox, overview, trends, the recurring view with
  price-rise flags, budgets, net worth, FX, and reviews.
- **Later items:** QIF, CAMT.053, MT940, PDF import, SQLCipher, streamable-HTTP MCP, Sankey,
  translations.

`DESIGN.md` also already specifies a dark theme, the bulk-action bar for multi-select, and
"rule that categorised it" in the transaction inspector.

---

## 1. Table stakes we're missing

These are features most established apps have, with no home on the current roadmap.
Several more are listed under §3 because they extend a planned item; those are marked ⭐.

| Feature | Why it matters | Who has it | Sources |
|---|---|---|---|
| **Split transactions** | The most-cited gap. One supermarket or Amazon charge covers several categories; without splits, budgets, reports and MCP summaries are all wrong. The lines must sum exactly to the parent, which integer minor units make easy. Losing splits was a top complaint when Mint moved users to Credit Karma. | 8 of 8 | support.ynab.com/split-transactions-a-guide-SJLEKwY0q · help.copilot.money/en/articles/5325255-splitting-transactions · actualbudget.org/docs/transactions/split-transactions/ · gnucash.org/features.phtml · reddit.com/r/mintuit/comments/18brph9 · github.com/actualbudget/actual/issues/4616 (13 👍), /2234, /2872 |
| **Manual transaction entry and editing** (cash accounts, fixing a bad row) | Coffer only has imported rows. Cash spending, cash-only accounts and corrections are impossible. Needed by "matching imports to manual entries" (§2). | YNAB, Monarch, Firefly, GnuCash, Fava (others likely) | support.ynab.com/en_us/how-to-add-transactions-in-ynab-HyDwA_byi · help.monarch.com/hc/en-us/sections/32460964879380-Transactions · docs.firefly-iii.org/how-to/firefly-iii/finances/transactions/ |
| **Tags** (many per transaction, across categories) | Covers "Trip 2026", "Wedding", "Tax-deductible" without bending the category tree. Also meets much of the demand for deeper categories (Actual subcategories, 344 👍) and enables tag reports (Actual #3130, 27 👍). | Monarch, Copilot, Actual, Firefly, Lunch Money, Beancount; YNAB has flags | help.copilot.money/en/articles/9554367-tags-vs-categories · actualbudget.org/docs/transactions/tags/ · support.lunchmoney.app/setup/tags/analyzing-tags · github.com/actualbudget/actual/issues/1320 |
| **Attachments and receipts** (local files linked to a transaction) | Warranty and tax evidence. Actual #530 has 214 👍 and asks for a "zip of all attachments for tax-tagged transactions". Fits local-first: files stored by content hash next to the DB. Never exposed over MCP. | Monarch, Firefly, Lunch Money, Fava (4 of 8) | help.monarch.com (article 360056422791, "Adding Notes and Attachments") · github.com/beancount/fava/blob/main/src/fava/help/features.md · github.com/actualbudget/actual/issues/530 |
| **Data export** (CSV and JSON of any view, with split-parent IDs and a documented schema) | The local-first promise of no lock-in, and needed for tax. Users complain that Monarch caps CSV export at 10,000 rows and that YNAB's export loses the link from split lines to their parent. | 7 of 8 | support.ynab.com/en_us/how-to-export-plan-data-Sy_CouWA9 · help.copilot.money/en/articles/5944414-exporting-your-transaction-data · docs.firefly-iii.org/tutorials/firefly-iii/exporting-data/ · reddit.com/r/MonarchMoney/comments/1aenisc · reddit.com/r/ynab/comments/1hf9hvi |
| **Backup and restore** (rotating local snapshots with `VACUUM INTO`, a restore check, restore from the UI) | The SQLite file is the only copy of the user's data. M5.7 only documents where the data lives. Firefly warns that "CSV export is not a backup". Actual's backup/restore API request has 97 👍. | Actual, Firefly, GnuCash | actualbudget.org/docs/backup-restore/backup/ · docs.firefly-iii.org/tutorials/firefly-iii/exporting-data/ · github.com/actualbudget/actual/issues/826 · inkandswitch.com/essay/local-first/ |
| **Savings and payoff goals** (target amount and date; On track / Ahead / At risk; avalanche vs snowball for debt) | Among the most-requested features: Actual goals 507 👍, goal templates 210 👍. Can build on pots, but goals also apply to any account or to debt. | YNAB, Monarch, Copilot, Actual, Firefly (piggy banks) | support.ynab.com/en_us/getting-started-with-targets-ryAEP08xC · monarch.com/blog/winter-release (2025-12-18) · copilot.money/dispatch/savings-goals (2025-05-28) · docs.firefly-iii.org/explanation/financial-concepts/piggy-banks/ · github.com/actualbudget/actual/issues/496, /2965 |
| **Accounting reports** (balance sheet, income statement, cash-flow statement) and **custom/saved report definitions** | Coffer's views are fixed. Actual's custom reports request has 163 👍. Plain-text accounting users expect a balance sheet and P&L. | GnuCash, Fava, Actual, Monarch, Lunch Money | gnucash.org/features.phtml · actualbudget.org/docs/reports/ · lunchmoney.app/features/analytics · github.com/actualbudget/actual/issues/730 |
| **Investments and holdings** (quantity, cost basis, value) with **manual prices only** | Net worth is incomplete for anyone with an ISA, 401k or brokerage account. Online price fetching is rejected (§4), so value is limited to manual or imported prices. | Monarch, Copilot, GnuCash, Beancount, Lunch Money (crypto) | beancount.github.io/docs/beancount_language_syntax/ · help.monarch.com (article 41855507661076, "Investments") · gnucash.org/features.phtml |

---

## 2. Innovative ideas

Fit key: **D** = deterministic core (Rust/SQLite, no AI). **MCP** = AI-client side, with
output landing in the inbox or reports.

### Forecasting and insight

| Idea | Who does it | How it fits Coffer | Effort | Value |
|---|---|---|---|---|
| **Daily balance forecast** per account, with the lowest point before payday | PocketSmith (pocketsmith.com/tour/cash-flow-forecasts); Actual "projected balance" request (#517, 140 👍) | D. Projects from schedules (the §3 extension of M2.6/M4.3) plus budgets. Shown on the overview; exposed as a `cash_flow_forecast` read tool. Uses cached FX only. | M | high |
| **Anomaly and spike insights** (category or merchant far above its usual level) | Monarch Insights (monarch.com/blog/winter-release); open-finance anomaly docs (github.com/openfinance-app/open-finance, `docs/wiki/anomaly-detection.md`) | D, using median/MAD statistics, so it's explainable and repeatable. Gives `monthly_review` hard numbers instead of leaving the model to find outliers. | M | high |
| **Runway and Age of Money** | YNAB (support.ynab.com/en_us/age-of-money-H1ZS84W1s) | D. Age of Money matches inflows to outflows first-in-first-out; runway is liquid assets divided by median monthly spend. Two figures on the overview. | S | med |
| **Weekly recap** in the app (spent, net-worth change, what's coming up) | Monarch (Winter Release, 2025-12-18; it also emails the recap) | D, as an in-app card only. Plus a `weekly_review` MCP prompt (§3, M2.4). | S | med |
| **Personal inflation rate** from your own repeat purchases | ONS/BBC calculator (ons.gov.uk/visualisations/dvc1833/calculator) uses official CPI | D. A price index built from same-merchant recurring amounts; builds on the M4.3 price-rise flag. The official-CPI version needs a fetch (§4). | M | med |
| **Calendar heatmap** of daily spend | Actual 25.1.0 Calendar card (github.com/actualbudget/actual, release notes 2025-01-05); Lunch Money (changelog #265) | D. Cheap on the planned plot primitives; a natural Trends card. | S | med |
| **What-if scenarios** (inflation-adjusted net-worth projection, life events) | Kubera Fast Forward (help.kubera.com/article/110); Origin forecasting; ProjectionLab Monte Carlo | D, with seeded random sampling. Risks pulling Coffer into retirement planning; park until the core is solid. | M–L | low–med |

### Correctness and real-life money

| Idea | Who does it | How it fits Coffer | Effort | Value |
|---|---|---|---|---|
| **Refund and reimbursement linking.** A refund is linked to its purchase and nets against that category instead of counting as income. Expenses marked "reimbursable" go on an outstanding-claims list that clears when the payment arrives. | No app surveyed does it natively; at least 6 r/ynab threads (e.g. reddit.com/r/ynab/comments/170r1kp, /1c0m8iq, /1ixv2oc) describe workarounds | D. Modelled like a transfer group. MCP can propose links through a `refund_link` suggestion kind (§3, M2.3). Fixes the Monzo positive "Money Out" refund case at the reporting level. | M | high |
| **Shared expenses: "my share"** (a % or amount per transaction or split line, set by rules; a running "owed by X" balance that settles on repayment; reports in personal or gross terms) | Splitwise-to-YNAB bridge (github.com/danthareja/splitwise_for_ynab); reddit.com/r/personalfinance/comments/1hy9vn6, reddit.com/r/actualbudget/comments/r9y6rv | D, single-user. It meets the "couples budget" demand without multi-user (§4). | M | med–high |
| **Pending → posted handling.** The preset marks a status column; pending rows become provisional and are replaced when the posted row arrives (matched on amount and a date window), keeping the old raw text. | Reports: github.com/simonmichael/hledger/issues/1943; github.com/kramerc/honeyledger/issues/152; beancount-import's Mint source | D, in dedupe. Needs fixture evidence of which UK exports actually include pending rows before committing to it. | M | med |
| **Matching imports to manual entries.** A hand-entered cash or pending item merges with the bank row when the export arrives. | beancount-import (github.com/jbms/beancount-import) | D. Depends on manual entry (§1). Goes through the inbox or the duplicate queue rather than auto-merging. | M | med |
| **Rule suggestions from your own edits.** After you recategorise N transactions with the same normalised merchant, Coffer offers a rule. | Tiller AutoCat (help.tiller.com/en/articles/3792984) | D, no ML. The app writes a `rule` suggestion to the inbox (source = app), so it reuses the M3.6 approval flow. | S | high |
| **Starter merchant rule pack per region** (e.g. UK transport, supermarkets), shipped as data | UK users want categorisation from CSV without linking a bank (reddit.com/r/UKPersonalFinance/comments/1c7r9sw, /1e1g2w5) | D. Rules as TOML next to the presets, so the first import isn't 100% uncategorised. Contributable without Rust. | S | med |

### Plain-text accounting and local-first

| Idea | Who does it | How it fits Coffer | Effort | Value |
|---|---|---|---|---|
| **Beancount and hledger journal export** (deterministic, stably sorted, git-friendly) | Beancount/Fava, hledger; rustledger is a Rust re-implementation (GPL-3.0, so only as an external validator) | D. An exit route into the plaintext-accounting ecosystem; CI can optionally run `bean-check` on the exported fixtures. | M | high |
| **Saved queries and formula cards** (user-defined report cards over saved filters) | Actual formula cards (PR #5939, 2025; experimental); Lunch Money Query Tool | D, with a small sandboxed expression language. MCP could propose saved queries to the inbox. Overlaps the §1 custom-reports gap; do the reports first. | M–L | med |
| **AI access log.** Each MCP call records client, tool, arguments, row counts, redaction level and time, and it's shown in the app. Stores hashes of what was returned, never the data. | Monarch request records (monarch.com/AI-connector-privacy); KazeFreeze audit logger (github.com/KazeFreeze/actual-budget-mcp) | App feature in core/mcp. No finance MCP server surveyed is stricter than Coffer's inbox model; this makes the strictness visible. | M | high |
| **Per-client access levels**: `aggregate` (totals only), `pseudonymous` (stable keyed-hash merchant tokens such as `M-7f3a`), `standard`. Tools above the client's level are never registered. | hs737 hides write tools unless enabled (github.com/hs737/mcp-server-for-ynab); Monarch per-connection access | MCP config plus an app setting. Strengthens "no PII" beyond a filter. Pseudonymous mode hurts categorisation, so it's mainly for analysis. | M | high |
| **MCP connection controls**: MCP off until enabled, a list of connected clients with last-seen times, per-client revoke, and a kill switch | Monarch Settings > Integrations; Ink & Switch local-first ideals ("you retain ultimate ownership and control") | App feature. Pairs with the access log. | M | med–high |

---

## 3. Extensions to planned features

⭐ = also table stakes (most established apps have it).

### Import and core (M1)

| Roadmap item | Extension | Evidence | Effort | Value |
|---|---|---|---|---|
| **M1.7** CSV importer | Read UTF-16 when a BOM is present. When both `DD/MM` and `MM/DD` parse, check the whole file and **flag the ambiguity rather than guess**. Skip footer rows. Show the imported total next to the statement total before committing. | Firefly #4106 (€10,00 imported as 1000); github.com/mayswind/ezbookkeeping/issues/361 (UTF-16, `YYYY.MM.DD`); hledger #1158 | S | med |
| **M1.8** Presets | **Migration presets** for YNAB, Mint, Actual, Monarch and Firefly exports: map their category names onto Coffer categories (optionally creating missing ones), and route rows by an account column for files that cover several accounts. | Actual #557 (59 👍), #525 (23), #2226 (14); YNAB, Lunch Money and Origin all built Mint migration tools (ynab.com/whats-new/migrate-from-mint-with-ease) | M | high |
| **M1.11** Dedupe | A **possible-duplicates review queue**: same amount within ±N days with different descriptions (typically the same transaction in an overlapping CSV and OFX). One-click merge keeps the best fields; each row shows why it was flagged. | Actual "merge unmatched" #669 (121 👍, "show-stopper"); YNAB file-import guide; Firefly #12115 | M | high |
| **M1.12** Reconciliation ⭐ | **Dated balance assertions**: many anchors per account instead of one, taken automatically from statement closing balances (OFX `LEDGERBAL`). A failure points to the window between the last passing assertion and the failing one. **Cleared/reconciled status and a lock date**: imports before the lock date warn, and editing locked amounts needs an explicit unlock. | Beancount `balance` and `pad` directives; actualbudget.org/docs/accounts/reconciliation/; Actual #558 (48 👍); support.ynab.com/en_us/getting-started-with-reconciling-accounts-an-overview-Sy3JWx4Js | S–M | high |
| **M1.13** Import batches | **Statement archive**: keep each source file (by content hash) linked to its batch, so any transaction traces back to the file it came from. This increases the sensitive data at rest, which strengthens the case for L.6 (SQLCipher). | Beancount `document` directive (github.com/beancount/beancount/discussions/972) | M | med |
| **M1.4 / M3.3** Notes ⭐ | `note` exists in the schema for imported notes. Make it editable and searchable, and include it in exports. It stays hidden from MCP by default, as already planned. | YNAB, Monarch, Copilot, Actual, Lunch Money, Beancount (see §1 sources) | S | med |
| **M1.15** Rules ⭐ (payee clean-up) | **Actions beyond setting a category**: set a display merchant (alias), add tags, set a note, exclude from reports, apply a split template (e.g. Tesco 80% Groceries / 20% Household), set "my share". A **merchant alias table** over `description_raw` (raw text is never overwritten), plus built-in deterministic stripping of card refs, "CONTACTLESS" and "AMZN MKTP". **Apply to history with a preview**, recorded as an undoable batch. **Export and import rules as TOML.** | help.monarch.com (article 360048393372, "Transaction Rules"); actualbudget.org/docs/budgeting/rules/; ynab.com/whats-new/clean-payees; Actual #500 (101 👍), #679 (38), #3591 (22), #2433 (15) | M | high |
| **M1.16** Transfers | A queue for transfers where only one side has been imported so far. Pairs across currencies, recording the FX difference. A check that editing one side of a pair can't create a duplicate. | Actual #1628 (388 👍, the top-voted transfer issue); reddit.com/r/GnuCash/comments/1296ewz | M | med |
| **M1.17** Pots | Target amount and date per pot (the simplest form of §1 goals). | Firefly piggy banks; Monarch save-up goals (help.monarch.com, article 44373182867476) | S | med |

### MCP (M2)

All of these keep the invariants: reads are redacted, and writes go only to `suggestion`
and `report`. New suggestion kinds need the `suggestion.kind` list in `ARCHITECTURE.md`
updated, and each needs an app-side approval action.

| Roadmap item | Extension | Evidence | Effort | Value |
|---|---|---|---|---|
| **M2.1** Server | **Tool annotations** (`readOnlyHint`; inbox writes `destructiveHint=false`, `idempotentHint=true`, deduplicated by content hash; `openWorldHint=false`). Coffer enforces the rules itself, because the spec says clients shouldn't trust hints. **`outputSchema`/structured content** with integer minor units plus a currency code, and a text fallback. **Resources**: `coffer://categories`, `coffer://glossary` (sign conventions, minor units, how transfers count), `coffer://reports/{id}`. | modelcontextprotocol.io/specification/2026-07-28/server/tools; blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/; github.com/jamiew/monarch-mcp | S | med |
| **M2.2** Read tools | `find_duplicates`; `preview_rule` (match count, category changes and conflicts if a candidate rule were applied); `reconcile_preview` (statement balance and date in → gap and candidate missing items out; numbers only, no account identifiers); `list_suggestions` (status and the user's rejection reason, so the AI learns from what was turned down); `cash_flow_forecast` and `spending_pace`. An optional **`purpose` argument** on every tool, saved in the access log and shown as plain, untrusted text. | henfrydls/actual-budget-mcp (`reconcile_account`); hs737 (`reconcile_preview`, `transactions_match_statement`); auzroz/ynab-mcp (`spending_pace`, `cash_flow_forecast`); jamiew (`find_duplicate_transactions`); Monarch "request intent" | M | high |
| **M2.3** Inbox writes | New suggestion kinds: `duplicate_flag`, `merchant_alias`, `refund_link`, `budget_adjustment` (per-category amounts with reasoning; Copilot's budget rebalance is the in-app equivalent), `recurring_review` (confirm, dismiss, or mark cancelled). Each suggestion carries evidence IDs and its client name. | jamiew (merge merchants); auzroz (`budget_suggestions`, `detect_recurring`); help.copilot.money/en/articles/6206302 | M | high |
| **M2.4** Prompts | `categorize_inbox`, `rule_tuning`, `subscription_audit`, `year_end_review`, `weekly_review`. Every prompt states that transaction text is untrusted data. | KazeFreeze and jamiew prompt sets; reddit.com/r/ynab/comments/1tsu5il (what users do: categorise first, then quick questions) | S | med |
| **M2.5** PII guard | **Untrusted-text envelope**: the cleaned merchant name by default; raw description only in a labelled `description_untrusted` field, length-capped, with control, zero-width and bidi characters stripped and URLs made inert. **Deterministic injection-pattern flagging** ("ignore previous…", URLs, base64) applied when building MCP output, not in the import path. Flagged text is withheld from MCP and the withholding is shown to the user, not done silently. | simonwillison.net/2025/jun/16/the-lethal-trifecta/; arxiv.org/html/2506.08837 (design patterns); OWASP MCP Security Cheat Sheet | S | high |
| **M2.8** Setup docs | Recommend a client profile with Coffer as the only data source and no web or fetch tools, so the "lethal trifecta" can't form in the client. | simonwillison.net/2025/jun/16/the-lethal-trifecta/; ai.meta.com/blog/practical-ai-agent-security | S | med |

### GUI and analytics (M3–M4)

| Roadmap item | Extension | Evidence | Effort | Value |
|---|---|---|---|---|
| **M3.3** Transactions view | **Structured search** (`category:food amount:>50 tag:trip`), shared by the GUI, saved filters, exports and MCP `search_transactions`. **100k-row benchmark** as a tested guarantee; users of Actual and Firefly complain about slowdowns at 6k–33k rows. | docs.firefly-iii.org/references/firefly-iii/search/; Fava BQL; Actual #2985, #5619 | M | med |
| **M3.6** Inbox | **App-computed impact for each suggestion** (e.g. "this rule changes 143 past transactions; 12 currently have a user-set category"), so an injected, overly broad `propose_rule` or a fake transfer pair is visible before approval. | Integrity-attack mitigation (lethal trifecta and design-patterns sources above); Xerrion `overridden_fields` | M | high |
| **M2.6 / M4.3** Recurring ⭐ | Turn a detected series into a **schedule** with an expected date and amount. Adds an **upcoming bills** list and a **missed or late payment** flag; schedules feed the §2 forecast. | 7 of 8 apps have scheduled transactions: support.ynab.com/en_us/scheduled-transactions-a-guide-BygrAIFA9; actualbudget.org/docs/schedules/; help.copilot.money/en/articles/3760068-creating-recurrings | M | high |
| **M3.2 / M4.5** Accounts and net worth ⭐ | **Manual asset and liability accounts with dated valuations** (house, car, pension, mortgage, loans), using a last-known-value-on-or-before-date rule. **Account groups** with subtotals, and include/exclude per report. | support.ynab.com/en_us/loan-accounts-a-guide-HkNSkPHJi; Firefly #5532 (asset prices, 20 👍); Actual account groups #1683 (150 👍) | M | high |
| **M4.1 / M4.4 / M2.2** Period definition | A **configurable month start** (a fixed day, or "last working day on or before the 25th") used by the overview, budgets and `monthly_summary`. Touches every report, so decide before M4. | Actual #975 (139 👍); Lunch Money custom periods (feedback.lunchmoney.app/changelog/238) | M | med–high |
| **M4.4** Budgets ⭐ | **Per-category rollover**. **Non-monthly / flex buckets**, with detected annual payments seeding the non-monthly bucket. A **spending-pace line** (ideal spend rate against actual; bars coloured by whether they're on pace). **Targets that suggest the monthly amount.** | help.monarch.com (articles 4411119762196, 32125337244052); help.copilot.money/en/articles/3790828-budget-rollovers, /6045480; actualbudget.org/docs/experimental/goal-templates/ | M | high |
| **M4.6** Multi-currency | Record the rate the bank applied and its fee as well as the original amount (Revolut, Wise and Monzo columns). Add an **FX fees paid** report. | Actual #2373 (33 👍), #1132 (349 👍, budget currencies) | S–M | med |
| **M4.1 / M4.2** Reports | **Global exclusions** (accounts, categories, tags) and **CSV export of any view**. | Actual #730 (163 👍); news.ycombinator.com/item?id=20834864 | S–M | med |
| **M4.7 / M2.3** Reviews | **Safe report rendering**: markdown only, no HTML, no remote images, links inert. A remote image in an AI-written report would be a network call and a way to leak data. Reports read back through `list_reports` are labelled `ai_authored`. Reports can be exported as `.md`. | pharosone.ai/knowledge/data-exfiltration-through-images-and-urls.md; invariantlabs.ai/blog/mcp-github-vulnerability | S | high |

---

## 4. Rejected

Each of these conflicts with an invariant or a project rule (licence compatibility).

- **Bank/aggregator sync** (Plaid, SimpleFIN, GoCardless; Actual #898, 335 👍; Firefly #5396):
  network. Meet the need with import ergonomics instead: §3 M1.8, M1.11, and pending handling.
- **An MCP tool that triggers a bank sync or the ECB fetch:** a network call driven by the AI;
  MCP uses cached rates only.
- **Broker/crypto holdings sync and online stock quotes** (Monarch, Copilot, Lunch Money,
  GnuCash): network. Manual or imported prices only. An opt-in price fetch like the ECB one
  would need a recorded decision in `ARCHITECTURE.md`.
- **Official CPI fetch** for personal inflation: network. A user-imported CPI CSV is fine.
- **Cloud sync, multi-device and hosted server features** (OIDC, 2FA, passkeys, base URL;
  Actual #2042, Firefly #10662): cloud/server. The local equivalent is L.6 (SQLCipher).
- **Mobile apps and GPS payee suggestions** (Actual #604, #1511): desktop only, and location
  data is PII.
- **Multi-user household and couples budgets:** single user. "My share" (§2) covers the
  real need.
- **Direct create/update/delete MCP tools** (every finance MCP server surveyed has them): MCP
  writes go only to the inbox.
- **A raw SQL or query-language MCP tool** (KazeFreeze, NightSquawk, beanquery-mcp): gets
  around the PII guard and redaction.
- **Exposing household member names or credit scores** (jamiew/monarch-mcp): PII.
- **MCP sampling:** Coffer would be driving an LLM, and AI is external. Sampling is also
  deprecated in the 2026-07-28 spec.
- **Approving suggestions through elicitation or MCP Apps buttons:** the AI client could
  approve on the user's behalf. Approval stays in Coffer's own window.
- **A learned categoriser in or after import** (smart_importer, arXiv 2404.08664): AI in the
  import path, and `core` must not depend on ML. A seeded local classifier that only writes
  to the inbox would need an explicit decision in `ARCHITECTURE.md`. The deterministic
  "rule suggestions from your edits" (§2) covers most of the value.
- **In-app receipt OCR and AI splitting** (Monarch): AI in the app. Sending attachments over
  MCP would also expose PII (names, addresses). Manual splits and local attachments are fine.
- **In-app AI advisor** (Monarch, Origin, Cleo): AI is external via MCP.
- **Emailed weekly recap** (Monarch): network. The in-app version is in §2.
- **Dead man's switch** (Kubera, help.kubera.com/article/10): needs a cloud service and
  email. A local "handover pack" export is the compatible version.
- **Bill negotiation and cancellation** (Rocket Money): a third-party service acting for
  the user.
- **Linking rustledger as a dependency:** GPL-3.0 conflicts with `MIT OR Apache-2.0`. Use it
  only as an external validator.
