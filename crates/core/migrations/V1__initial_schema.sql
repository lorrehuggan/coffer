-- The initial schema (ROADMAP M1.4): accounts, import batches, transactions,
-- balance snapshots, categories, rules, transfer groups, budgets, FX rates,
-- suggestions and reports. `docs/ARCHITECTURE.md` → "Data model" describes
-- every table; tables for later roadmap items (splits, tags, links,
-- duplicates, change batches, schedules, MCP clients and the access log) come
-- in their own migrations.
--
-- Conventions, checked by the tests in `crates/core/src/db/schema_tests.rs`:
--
-- - Every table is STRICT, so a column holds only its declared type: money
--   columns (`*_minor`) are INTEGER and refuse a fractional value.
-- - Money is integer minor units beside an ISO 4217 code (three capital
--   letters here; the code list itself is checked in Rust).
-- - Dates are `YYYY-MM-DD`, times `HH:MM:SS`, months `YYYY-MM`, and
--   timestamps UTC `YYYY-MM-DDTHH:MM:SS.SSSZ`; each is checked to round-trip
--   through SQLite's date functions, so `2026-02-30` or `10/11/2026` is
--   refused.
-- - Booleans are INTEGER 0 or 1. Enums are TEXT with a CHECK listing the keys.
-- - IDs are `INTEGER PRIMARY KEY AUTOINCREMENT`, so an ID is never reused
--   after a delete (an undone import): a suggestion or a log that names an
--   old ID can't come to point at a different row.
-- - Foreign keys have no ON DELETE action. Deleting is always explicit in
--   core, which keeps "never silently drop data" visible in the code.
-- - Every foreign key column is indexed.
-- - Rules that span rows (category keys and levels, pots, currencies,
--   undone batches, read-only fields) are BEFORE triggers that RAISE(ABORT). Their subqueries look a
--   row up by primary key or through an index, so a bulk import stays cheap.
-- - Core never writes `"transaction"` with `REPLACE`, `INSERT OR REPLACE`,
--   `UPDATE OR REPLACE` or an upsert (`ON CONFLICT`). REPLACE deletes the
--   row in the way and inserts a new one, so no UPDATE trigger sees it: it
--   would get round the read-only source fields. The schema refuses it
--   anyway (`transaction_insert_new` below), and a test scans core's SQL for
--   such statements.
--
-- `transaction` is an SQL keyword, so the table name is always quoted.

-- Accounts --------------------------------------------------------------------

-- A bank account, card, pot, cash wallet, or a manually valued asset or
-- liability. Pots (Monzo Pots, Starling Spaces, Revolut Vaults) are children
-- of the account they belong to. `asset` and `liability` accounts hold
-- `valuation` snapshots only, never transactions.
CREATE TABLE account (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id             INTEGER REFERENCES account (id),
    name                  TEXT    NOT NULL CHECK (trim(name) <> ''),
    institution           TEXT,
    kind                  TEXT    NOT NULL CHECK (kind IN (
                              'current', 'savings', 'credit_card', 'pot', 'cash',
                              'asset', 'liability')),
    currency              TEXT    NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    -- A balance the user entered when the export has none.
    opening_balance_minor INTEGER,
    opening_date          TEXT    CHECK (opening_date IS date(opening_date)),
    -- History up to and including this date is locked (a passing check).
    reconciled_through    TEXT    CHECK (reconciled_through IS date(reconciled_through)),
    closed_on             TEXT    CHECK (closed_on IS date(closed_on)),
    created_at            TEXT    NOT NULL
                                  DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                                  CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    CHECK ((opening_balance_minor IS NULL) = (opening_date IS NULL)),
    -- Pots, and only pots, have a parent: a current or savings account in
    -- the same currency (triggers under "Account structure").
    CHECK ((kind = 'pot') = (parent_id IS NOT NULL)),
    CHECK (parent_id IS NOT id)
) STRICT;

CREATE INDEX account_parent ON account (parent_id);

-- Imports ---------------------------------------------------------------------

-- One committed import of one file into one account. Undoing it deletes its
-- transactions (including the mirrored pot-side rows) and balance snapshots,
-- and keeps this row as `undone`.
CREATE TABLE import_batch (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER NOT NULL REFERENCES account (id),
    -- The file's name, without its directory (which can contain the user's
    -- name).
    source_file TEXT    NOT NULL CHECK (source_file <> ''),
    -- QFX is OFX. Every planned format is listed, so adding an importer
    -- doesn't mean rebuilding this table.
    format      TEXT    NOT NULL CHECK (format IN ('csv', 'ofx', 'qif', 'camt053', 'mt940')),
    preset_id   TEXT    CHECK (preset_id <> ''),
    imported_at TEXT    NOT NULL
                        DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                        CHECK (imported_at IS strftime('%Y-%m-%dT%H:%M:%fZ', imported_at)),
    status      TEXT    NOT NULL DEFAULT 'committed' CHECK (status IN ('committed', 'undone'))
) STRICT;

CREATE INDEX import_batch_account ON import_batch (account_id);

-- A batch records what was imported, where and when; only its status
-- changes (undo).
CREATE TRIGGER import_batch_read_only
BEFORE UPDATE ON import_batch
WHEN NEW.id IS NOT OLD.id
  OR NEW.account_id IS NOT OLD.account_id
  OR NEW.source_file IS NOT OLD.source_file
  OR NEW.format IS NOT OLD.format
  OR NEW.preset_id IS NOT OLD.preset_id
  OR NEW.imported_at IS NOT OLD.imported_at
BEGIN
    SELECT RAISE(ABORT, 'an import batch is read-only except for its status');
END;

-- Undo deletes the batch's transactions and balances, then marks it
-- `undone` (in one DB transaction). An undone batch stays undone and holds
-- nothing: the triggers on `"transaction"` and `balance_snapshot` refuse
-- rows for it.
CREATE TRIGGER import_batch_status
BEFORE UPDATE OF status ON import_batch
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch can''t be committed again')
    WHERE OLD.status = 'undone' AND NEW.status IS NOT 'undone';
    SELECT RAISE(ABORT, 'an import batch is undone only once its transactions and balances are deleted')
    WHERE OLD.status = 'committed' AND NEW.status = 'undone'
      AND (EXISTS (SELECT 1 FROM "transaction" WHERE batch_id = OLD.id)
           OR EXISTS (SELECT 1 FROM balance_snapshot WHERE batch_id = OLD.id));
END;

-- Categories ------------------------------------------------------------------

-- Two levels. `key` is stable (`food.groceries`) and is what presets, rules
-- and MCP refer to; `label` is what the user sees and may be translated.
-- A key is one or two non-empty parts of `a-z`, `0-9` and `_` joined by a
-- dot, as `CategoryKey` in `crates/core/src/category.rs` accepts. A
-- top-level category has a one-part key and a subcategory's key is its
-- parent's key, a dot and one part, so the key names the category's place in
-- the tree. Neither the key nor the parent changes after insert (trigger
-- below); only the label does.
CREATE TABLE category (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER REFERENCES category (id),
    key       TEXT    NOT NULL UNIQUE
                      CHECK (key <> ''
                             AND key NOT GLOB '*[^a-z0-9_.]*'
                             AND key NOT GLOB '.*'
                             AND key NOT GLOB '*.'
                             AND key NOT GLOB '*.*.*'),
    label     TEXT    NOT NULL CHECK (trim(label) <> ''),

    CHECK (parent_id IS NOT id)
) STRICT;

CREATE INDEX category_parent ON category (parent_id);

-- The key matches the category's place: one part at the top level,
-- `<parent key>.<part>` below it. A parent with a two-part key would need a
-- three-part key here, which the CHECK above refuses, so this also keeps
-- categories to two levels.
CREATE TRIGGER category_key_insert
BEFORE INSERT ON category
BEGIN
    SELECT RAISE(ABORT, 'a top-level category has a one-part key')
    WHERE NEW.parent_id IS NULL AND instr(NEW.key, '.') > 0;
    SELECT RAISE(ABORT, 'categories have two levels: the parent is itself a subcategory')
    FROM category WHERE id = NEW.parent_id AND parent_id IS NOT NULL;
    SELECT RAISE(ABORT, 'a subcategory''s key is its parent''s key, a dot and one part')
    FROM category
    WHERE id = NEW.parent_id
      AND substr(NEW.key, 1, length(key) + 1) IS NOT key || '.';
END;

-- Keys are stable: presets, rules and MCP clients refer to them, so one
-- never changes and never comes to name another category. The parent is
-- part of the key, so it is fixed too (moving `food.x` under `transport`
-- would change its key).
CREATE TRIGGER category_read_only
BEFORE UPDATE ON category
WHEN NEW.id IS NOT OLD.id
  OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.key IS NOT OLD.key
BEGIN
    SELECT RAISE(ABORT, 'a category''s key and parent are read-only');
END;

-- Rules -----------------------------------------------------------------------

-- Checked in priority order (lowest first, then by ID) on every import. Each
-- field of a transaction is set by the first rule that matches and sets it.
-- Matching ignores case; `merchant` is the cleaned bank name, before any
-- merchant alias applies.
CREATE TABLE rule (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Not unique: moving a rule renumbers others, and SQLite checks
    -- uniqueness row by row during an UPDATE.
    priority    INTEGER NOT NULL,
    match_field TEXT    NOT NULL CHECK (match_field IN (
                            'merchant', 'description', 'source_category', 'source_type')),
    match_kind  TEXT    NOT NULL CHECK (match_kind IN ('is', 'starts_with', 'contains', 'regex')),
    pattern     TEXT    NOT NULL CHECK (pattern <> ''),
    -- Amount conditions in the account's minor units; NULL means no bound.
    amount_min  INTEGER,
    amount_max  INTEGER,
    account_id  INTEGER REFERENCES account (id),
    created_at  TEXT    NOT NULL
                        DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                        CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    CHECK (amount_min IS NULL OR amount_max IS NULL OR amount_min <= amount_max)
) STRICT;

CREATE INDEX rule_priority ON rule (priority, id);
CREATE INDEX rule_account ON rule (account_id);

-- A rule's amount bounds are in its account's minor units, so a rule with
-- bounds can't move to an account in another currency (100 GBP pence would
-- become 100 JPY). Narrowing a rule from every account to one, or widening it
-- to every account, keeps the numbers in each matched account's own units.
CREATE TRIGGER rule_account_currency
BEFORE UPDATE OF account_id, amount_min, amount_max ON rule
WHEN (NEW.amount_min IS NOT NULL OR NEW.amount_max IS NOT NULL)
 AND OLD.account_id IS NOT NULL
 AND NEW.account_id IS NOT NULL
 AND NEW.account_id IS NOT OLD.account_id
 AND (SELECT currency FROM account WHERE id = NEW.account_id)
     IS NOT (SELECT currency FROM account WHERE id = OLD.account_id)
BEGIN
    SELECT RAISE(ABORT, 'a rule with amount bounds can''t move to an account in another currency');
END;

-- What a rule does. Each kind at most once per rule; `set_category` and
-- `split_template` exclude each other (checked in core). An `exclude`
-- action's value is a reason key.
CREATE TABLE rule_action (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id    INTEGER NOT NULL REFERENCES rule (id),
    kind       TEXT    NOT NULL CHECK (kind IN (
                           'set_category', 'set_merchant', 'add_tag', 'set_note', 'exclude',
                           'split_template', 'my_share')),
    value_json TEXT    NOT NULL CHECK (json_valid(value_json)),

    UNIQUE (rule_id, kind)
) STRICT;

-- Transfers -------------------------------------------------------------------

-- Two (or more) transactions that move money between the user's own
-- accounts. Confirmed groups are left out of income and spending.
CREATE TABLE transfer_group (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    status     TEXT    NOT NULL CHECK (status IN ('suggested', 'confirmed', 'rejected')),
    created_at TEXT    NOT NULL
                       DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                       CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at))
) STRICT;

-- Transactions ----------------------------------------------------------------

-- One movement of money on one account. The imported source fields are
-- read-only (trigger below): a wrong row is fixed by undoing its batch or
-- excluding it. Manual rows (`origin = 'manual'`, no batch) are the user's
-- own and every field but `id`, `origin` and `created_at` can change. A
-- transaction is in its account's currency, and never on an `asset` or
-- `liability` account.
CREATE TABLE "transaction" (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id             INTEGER NOT NULL REFERENCES account (id),
    batch_id               INTEGER REFERENCES import_batch (id),

    -- Source fields, as imported.
    date                   TEXT    NOT NULL CHECK (date IS date(date)),
    -- `24:00:00` round-trips through time() but isn't a time of day.
    time                   TEXT    CHECK (time IS time(time) AND time < '24:00:00'),
    amount_minor           INTEGER NOT NULL,
    currency               TEXT    NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    -- The bank's merchant name, when the export has one (Monzo's "Name").
    merchant               TEXT,
    description_raw        TEXT    NOT NULL,
    -- `description_raw` with whitespace collapsed and uppercased.
    description_normalized TEXT    NOT NULL,
    -- The file's own ID for the row (OFX `FITID`, a CSV ID column).
    external_id            TEXT    CHECK (external_id <> ''),
    -- Dedupe key when there's no external ID: account, date, amount,
    -- normalized description and occurrence index. Required for imported
    -- rows; manual rows have neither key.
    fingerprint            TEXT    CHECK (fingerprint <> ''),
    source_category        TEXT,
    source_type            TEXT,
    -- The amount in the currency spent, when it differs (card abroad).
    original_amount_minor  INTEGER,
    original_currency      TEXT    CHECK (original_currency GLOB '[A-Z][A-Z][A-Z]'),
    -- The note from the bank export; read-only.
    note                   TEXT,
    origin                 TEXT    NOT NULL CHECK (origin IN ('imported', 'mirrored', 'manual')),
    created_at             TEXT    NOT NULL
                                   DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                                   CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- User-facing fields.
    -- Derived: cleaned bank text, then merchant aliases, then rule actions.
    merchant_display       TEXT,
    -- NULL when uncategorised or split.
    category_id            INTEGER REFERENCES category (id),
    category_source        TEXT    CHECK (category_source IN (
                                       'rule', 'preset_map', 'user', 'ai_approved')),
    -- The rule that set the category. Cleared when the rule is deleted;
    -- `category_source` stays `rule`.
    rule_id                INTEGER REFERENCES rule (id),
    transfer_group_id      INTEGER REFERENCES transfer_group (id),
    user_note              TEXT,
    -- The user's part of the amount, same sign; NULL means all of it.
    my_share_minor         INTEGER,
    -- A local label for who shares the cost; never leaves the app.
    share_label            TEXT    CHECK (share_label <> ''),
    reimbursable           INTEGER NOT NULL DEFAULT 0 CHECK (reimbursable IN (0, 1)),
    -- Excluded rows leave spending, income, budgets and reports; only
    -- `duplicate` also leaves the computed balance.
    excluded_reason        TEXT    CHECK (excluded_reason IN (
                                       'duplicate', 'bank_error', 'disputed', 'other')),
    excluded_note          TEXT,
    excluded_at            TEXT    CHECK (excluded_at IS strftime('%Y-%m-%dT%H:%M:%fZ', excluded_at)),

    CHECK ((original_amount_minor IS NULL) = (original_currency IS NULL)),
    -- Manual rows have no batch; imported and mirrored rows always do.
    CHECK ((origin = 'manual') = (batch_id IS NULL)),
    CHECK (origin <> 'imported' OR fingerprint IS NOT NULL),
    -- Manual rows aren't deduplicated. Without keys, an edit to one can't
    -- collide with (and so REPLACE) another row.
    CHECK (origin <> 'manual' OR (external_id IS NULL AND fingerprint IS NULL)),
    CHECK ((category_id IS NULL) = (category_source IS NULL)),
    CHECK (my_share_minor IS NULL
           OR (amount_minor >= 0 AND my_share_minor BETWEEN 0 AND amount_minor)
           OR (amount_minor < 0 AND my_share_minor BETWEEN amount_minor AND 0)),
    -- The exclusion fields are set and cleared together, and `other` needs
    -- a note.
    CHECK ((excluded_reason IS NULL) = (excluded_at IS NULL)),
    CHECK (excluded_reason IS NOT NULL OR excluded_note IS NULL),
    CHECK (excluded_reason IS NOT 'other' OR coalesce(trim(excluded_note), '') <> '')
) STRICT;

CREATE INDEX transaction_account_date ON "transaction" (account_id, date);
CREATE INDEX transaction_batch ON "transaction" (batch_id);
CREATE INDEX transaction_category ON "transaction" (category_id);
CREATE INDEX transaction_rule ON "transaction" (rule_id);
CREATE INDEX transaction_transfer_group ON "transaction" (transfer_group_id);
-- Dedupe, source ID first: a source ID appears once per account, and a
-- fingerprint once among the account's rows without a source ID. A row
-- with a source ID is never matched by fingerprint: two identical payments
-- on one day get their occurrence index from their order in the file, and
-- an overlapping export can order them differently.
CREATE UNIQUE INDEX transaction_external_id
    ON "transaction" (account_id, external_id) WHERE external_id IS NOT NULL;
CREATE UNIQUE INDEX transaction_fingerprint
    ON "transaction" (account_id, fingerprint)
    WHERE external_id IS NULL AND fingerprint IS NOT NULL;

-- An insert is always a new row. The unique keys above would let
-- `INSERT OR REPLACE` delete the stored row and insert a different one in
-- its place, which no UPDATE trigger sees; this trigger runs before SQLite
-- resolves the conflict, so the statement fails instead. (Deleting stays
-- possible: undoing a batch deletes its rows.) It also turns
-- `INSERT OR IGNORE` and `ON CONFLICT DO NOTHING` on a duplicate into an
-- error: core dedupes before it inserts. An upsert's `DO UPDATE` is an
-- UPDATE and meets the read-only trigger below.
CREATE TRIGGER transaction_insert_new
BEFORE INSERT ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'a transaction with this ID already exists')
    WHERE EXISTS (SELECT 1 FROM "transaction" WHERE id = NEW.id);
    SELECT RAISE(ABORT, 'a transaction with this source ID already exists on the account')
    WHERE NEW.external_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM "transaction"
                  WHERE account_id = NEW.account_id AND external_id = NEW.external_id);
    SELECT RAISE(ABORT, 'a transaction with this fingerprint already exists on the account')
    WHERE NEW.external_id IS NULL AND NEW.fingerprint IS NOT NULL
      AND EXISTS (SELECT 1 FROM "transaction"
                  WHERE account_id = NEW.account_id AND external_id IS NULL
                    AND fingerprint = NEW.fingerprint);
END;

-- The account takes transactions, in its currency. An imported row sits on
-- its batch's account; a mirrored row on a pot of it. The batch isn't
-- undone. (A row never moves to another batch: manual rows have none, and
-- an imported or mirrored row's batch is read-only.)
CREATE TRIGGER transaction_account_insert
BEFORE INSERT ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch takes no transactions')
    FROM import_batch WHERE id = NEW.batch_id AND status = 'undone';
    SELECT RAISE(ABORT, 'asset and liability accounts hold valuations, not transactions')
    FROM account WHERE id = NEW.account_id AND kind IN ('asset', 'liability');
    SELECT RAISE(ABORT, 'a transaction is in its account''s currency')
    FROM account WHERE id = NEW.account_id AND currency IS NOT NEW.currency;
    SELECT RAISE(ABORT, 'an imported row belongs to its batch''s account')
    FROM import_batch
    WHERE NEW.origin = 'imported' AND id = NEW.batch_id AND account_id IS NOT NEW.account_id;
    SELECT RAISE(ABORT, 'a mirrored row belongs to a pot of its batch''s account')
    FROM import_batch
    WHERE NEW.origin = 'mirrored' AND id = NEW.batch_id
      AND account_id IS NOT (SELECT parent_id FROM account WHERE id = NEW.account_id);
END;

-- Manual rows can move account or change currency; the same rules apply.
-- (Imported and mirrored rows can't change either: read-only trigger.)
CREATE TRIGGER transaction_account_update
BEFORE UPDATE OF account_id, currency ON "transaction"
BEGIN
    SELECT RAISE(ABORT, 'asset and liability accounts hold valuations, not transactions')
    FROM account WHERE id = NEW.account_id AND kind IN ('asset', 'liability');
    SELECT RAISE(ABORT, 'a transaction is in its account''s currency')
    FROM account WHERE id = NEW.account_id AND currency IS NOT NEW.currency;
END;

-- Imported source fields are never edited (AGENTS.md → Invariants). The
-- ID, the origin and the insert time never change, even on manual rows.
CREATE TRIGGER transaction_source_fields_read_only
BEFORE UPDATE ON "transaction"
WHEN NEW.id IS NOT OLD.id
  OR NEW.origin IS NOT OLD.origin
  OR NEW.created_at IS NOT OLD.created_at
  OR (OLD.origin <> 'manual' AND (
         NEW.account_id IS NOT OLD.account_id
      OR NEW.batch_id IS NOT OLD.batch_id
      OR NEW.date IS NOT OLD.date
      OR NEW.time IS NOT OLD.time
      OR NEW.amount_minor IS NOT OLD.amount_minor
      OR NEW.currency IS NOT OLD.currency
      OR NEW.merchant IS NOT OLD.merchant
      OR NEW.description_raw IS NOT OLD.description_raw
      OR NEW.description_normalized IS NOT OLD.description_normalized
      OR NEW.external_id IS NOT OLD.external_id
      OR NEW.fingerprint IS NOT OLD.fingerprint
      OR NEW.source_category IS NOT OLD.source_category
      OR NEW.source_type IS NOT OLD.source_type
      OR NEW.original_amount_minor IS NOT OLD.original_amount_minor
      OR NEW.original_currency IS NOT OLD.original_currency
      OR NEW.note IS NOT OLD.note))
BEGIN
    SELECT RAISE(ABORT, 'imported source fields are read-only');
END;

-- Balances --------------------------------------------------------------------

-- A balance at the end of a day. An account's first snapshot is its
-- starting point and every later one is a check against the computed
-- balance; a `valuation` sets the balance of an asset or liability and is
-- never a check. Snapshots from a file belong to its batch and go when it is
-- undone.
CREATE TABLE balance_snapshot (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER NOT NULL REFERENCES account (id),
    batch_id      INTEGER REFERENCES import_batch (id),
    date          TEXT    NOT NULL CHECK (date IS date(date)),
    balance_minor INTEGER NOT NULL,
    source        TEXT    NOT NULL CHECK (source IN (
                              'ofx', 'csv', 'statement', 'manual', 'valuation')),
    -- User text; not sent over MCP by default.
    note          TEXT,
    created_at    TEXT    NOT NULL
                          DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                          CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- Balances read from a file come with its batch; the others are entered.
    CHECK ((source IN ('ofx', 'csv')) = (batch_id IS NOT NULL))
) STRICT;

CREATE INDEX balance_snapshot_account_date ON balance_snapshot (account_id, date);
CREATE INDEX balance_snapshot_batch ON balance_snapshot (batch_id);
-- One entered balance per account and day, whatever its source: each is an
-- assertion of the same end-of-day balance, so two would either repeat or
-- contradict each other. A correction edits the one there. Balances from
-- files may repeat a date: overlapping exports both carry it, and a
-- mismatch between them is a failed check to flag, not to refuse.
CREATE UNIQUE INDEX balance_snapshot_entered
    ON balance_snapshot (account_id, date) WHERE batch_id IS NULL;

-- `asset` and `liability` accounts have `valuation` snapshots, and only
-- they do. A snapshot from a file sits on its batch's account or, like a
-- mirrored row, on a pot of it, and its batch isn't undone.
CREATE TRIGGER balance_snapshot_insert
BEFORE INSERT ON balance_snapshot
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch takes no balances')
    FROM import_batch WHERE id = NEW.batch_id AND status = 'undone';
    SELECT RAISE(ABORT, 'valuations are for asset and liability accounts, which have nothing else')
    FROM account
    WHERE id = NEW.account_id
      AND (kind IN ('asset', 'liability')) <> (NEW.source = 'valuation');
    SELECT RAISE(ABORT, 'a balance from a file belongs to its batch''s account or a pot of it')
    FROM import_batch
    WHERE id = NEW.batch_id
      AND account_id IS NOT NEW.account_id
      AND account_id IS NOT (SELECT parent_id FROM account WHERE id = NEW.account_id);
END;

CREATE TRIGGER balance_snapshot_update
BEFORE UPDATE OF account_id, batch_id, source ON balance_snapshot
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch takes no balances')
    FROM import_batch WHERE id = NEW.batch_id AND status = 'undone';
    SELECT RAISE(ABORT, 'valuations are for asset and liability accounts, which have nothing else')
    FROM account
    WHERE id = NEW.account_id
      AND (kind IN ('asset', 'liability')) <> (NEW.source = 'valuation');
    SELECT RAISE(ABORT, 'a balance from a file belongs to its batch''s account or a pot of it')
    FROM import_batch
    WHERE id = NEW.batch_id
      AND account_id IS NOT NEW.account_id
      AND account_id IS NOT (SELECT parent_id FROM account WHERE id = NEW.account_id);
END;

-- Account structure -----------------------------------------------------------

-- Here rather than with `account` because they look at transactions and
-- balances too.

-- A pot sits under a current or savings account, in its currency.
CREATE TRIGGER account_pot_insert
BEFORE INSERT ON account
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a pot belongs to a current or savings account')
    FROM account WHERE id = NEW.parent_id AND kind NOT IN ('current', 'savings');
    SELECT RAISE(ABORT, 'a pot is in its parent''s currency')
    FROM account WHERE id = NEW.parent_id AND currency IS NOT NEW.currency;
END;

CREATE TRIGGER account_pot_update
BEFORE UPDATE OF parent_id, kind, currency ON account
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a pot belongs to a current or savings account')
    FROM account WHERE id = NEW.parent_id AND kind NOT IN ('current', 'savings');
    SELECT RAISE(ABORT, 'a pot is in its parent''s currency')
    FROM account WHERE id = NEW.parent_id AND currency IS NOT NEW.currency;
END;

-- A pot's mirrored rows and file balances came with a batch on its parent,
-- so once it holds transactions or balances it stays under that parent.
CREATE TRIGGER account_pot_parent
BEFORE UPDATE OF parent_id ON account
WHEN OLD.parent_id IS NOT NULL AND NEW.parent_id IS NOT OLD.parent_id
BEGIN
    SELECT RAISE(ABORT, 'a pot with transactions or balances can''t change parent')
    WHERE EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id)
       OR EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id);
END;

-- What an account holds fixes its currency and what kind it can become. A
-- rule's amount bounds are in its account's minor units, so a rule with
-- bounds fixes the currency too.
CREATE TRIGGER account_kind_and_currency
BEFORE UPDATE OF kind, currency ON account
BEGIN
    SELECT RAISE(ABORT, 'an account''s currency can''t change once it has transactions, balances, pots or rules with amounts')
    WHERE NEW.currency IS NOT OLD.currency
      AND (EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id)
           OR EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id)
           OR EXISTS (SELECT 1 FROM account WHERE parent_id = OLD.id)
           OR EXISTS (SELECT 1 FROM rule
                      WHERE account_id = OLD.id
                        AND (amount_min IS NOT NULL OR amount_max IS NOT NULL)));
    SELECT RAISE(ABORT, 'an account with transactions can''t become an asset or liability')
    WHERE NEW.kind IN ('asset', 'liability')
      AND EXISTS (SELECT 1 FROM "transaction" WHERE account_id = OLD.id);
    SELECT RAISE(ABORT, 'an account with balances can''t move in or out of asset and liability')
    WHERE (NEW.kind IN ('asset', 'liability')) <> (OLD.kind IN ('asset', 'liability'))
      AND EXISTS (SELECT 1 FROM balance_snapshot WHERE account_id = OLD.id);
    SELECT RAISE(ABORT, 'an account with pots must stay a current or savings account')
    WHERE NEW.kind NOT IN ('current', 'savings')
      AND EXISTS (SELECT 1 FROM account WHERE parent_id = OLD.id);
END;

-- Budgets ---------------------------------------------------------------------

-- A category's budget. `month` NULL is the standing budget; a `YYYY-MM` row
-- overrides it for that month. Fixed and flex amounts are per month; a
-- non-monthly amount is per year and accrues a twelfth each month. Rollover
-- is derived from `ledger_line`, never stored.
CREATE TABLE budget (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id             INTEGER NOT NULL REFERENCES category (id),
    month                   TEXT    CHECK (month || '-01' IS date(month || '-01')),
    amount_minor            INTEGER NOT NULL CHECK (amount_minor >= 0),
    currency                TEXT    NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    rollover                INTEGER NOT NULL CHECK (rollover IN (0, 1)),
    group_kind              TEXT    NOT NULL CHECK (group_kind IN ('fixed', 'non_monthly', 'flex')),
    -- What a non-monthly budget had already set aside when it was created.
    opening_set_aside_minor INTEGER CHECK (opening_set_aside_minor >= 0),
    created_at              TEXT    NOT NULL
                                    DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                                    CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- Fixed budgets don't roll over; non-monthly ones always do.
    CHECK (group_kind <> 'fixed' OR rollover = 0),
    CHECK (group_kind <> 'non_monthly' OR rollover = 1),
    CHECK (group_kind = 'non_monthly' OR opening_set_aside_minor IS NULL)
) STRICT;

CREATE UNIQUE INDEX budget_standing ON budget (category_id) WHERE month IS NULL;
CREATE UNIQUE INDEX budget_month ON budget (category_id, month) WHERE month IS NOT NULL;

-- Exchange rates --------------------------------------------------------------

-- 1 `base` = `rate_e9` / 1_000_000_000 `quote` on `date`. A fixed-point
-- integer, so no float ever enters a conversion. ECB reference rates are
-- fetched opt-in; manual rates are the fallback.
CREATE TABLE fx_rate (
    date    TEXT    NOT NULL CHECK (date IS date(date)),
    base    TEXT    NOT NULL CHECK (base GLOB '[A-Z][A-Z][A-Z]'),
    quote   TEXT    NOT NULL CHECK (quote GLOB '[A-Z][A-Z][A-Z]'),
    rate_e9 INTEGER NOT NULL CHECK (rate_e9 > 0),
    source  TEXT    NOT NULL CHECK (source IN ('ecb', 'manual')),

    PRIMARY KEY (date, base, quote, source),
    CHECK (base <> quote)
) STRICT, WITHOUT ROWID;

-- Inbox -----------------------------------------------------------------------

-- A change proposed by an AI client over MCP, or by Coffer itself
-- (`source_client = 'coffer'`). Nothing changes until the user approves it in
-- the app or CLI. `payload_json` and `rationale` are untrusted text.
CREATE TABLE suggestion (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT    NOT NULL CHECK (kind IN (
                              'categorization', 'rule', 'transfer_pair', 'merchant_alias',
                              'refund_link')),
    payload_json  TEXT    NOT NULL CHECK (json_valid(payload_json)),
    rationale     TEXT,
    source_client TEXT    NOT NULL CHECK (source_client <> ''),
    created_at    TEXT    NOT NULL
                          DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                          CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),
    status        TEXT    NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'approved', 'rejected'))
) STRICT;

CREATE INDEX suggestion_status ON suggestion (status, created_at);

-- A review an AI client saved (`save_report`). Untrusted when read back:
-- rendered as a markdown subset, never as HTML.
CREATE TABLE report (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    period        TEXT    NOT NULL CHECK (trim(period) <> ''),
    title         TEXT    NOT NULL CHECK (trim(title) <> ''),
    body_markdown TEXT    NOT NULL,
    source_client TEXT    NOT NULL CHECK (source_client <> ''),
    created_at    TEXT    NOT NULL
                          DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                          CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at))
) STRICT;

CREATE INDEX report_created ON report (created_at);
