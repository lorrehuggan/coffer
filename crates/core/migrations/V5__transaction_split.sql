-- Split transactions (ROADMAP M1.20; `docs/ARCHITECTURE.md` → Decisions,
-- "Splits", and "Data model").
--
-- A transaction can be split into two or more lines, each with its own
-- category and share, that sum exactly to the transaction's amount. Lines are
-- in the parent's currency (there is no currency column) and in integer
-- minor units. Reports, budgets and MCP read `ledger_line` (below): one row
-- per split line, or the transaction itself when it isn't split.
--
-- What one row can check is checked here; the rest is checked in core
-- (`coffer_core::split`), in the same DB transaction as the edit:
--
-- - here: a line is not zero and has its parent's sign, a line's share is
--   part of the line, line numbers are unique per transaction, a split
--   parent has no category or share of its own (its lines do) and isn't in a
--   suggested or confirmed transfer, and its amount and currency don't
--   change while it has lines;
-- - in core: at least two lines, numbered from 1, that sum exactly to the
--   parent's amount (a sum can't be checked line by line while the lines are
--   being written), and the categories they name exist.
--
-- Deleting a transaction with lines is refused by the foreign key (no
-- `ON DELETE`): whoever deletes transactions (undoing an import, deleting a
-- manual row) deletes their lines first.

CREATE TABLE transaction_split (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id INTEGER NOT NULL REFERENCES "transaction" (id),
    -- The line's place in the split, from 1.
    line_no        INTEGER NOT NULL CHECK (line_no >= 1),
    -- In the parent's currency; not zero, and the parent's sign (trigger
    -- below).
    amount_minor   INTEGER NOT NULL CHECK (amount_minor <> 0),
    -- NULL leaves the line uncategorised.
    category_id    INTEGER REFERENCES category (id),
    -- The user's part of the line, same sign; NULL means all of it. Like
    -- `"transaction".my_share_minor`.
    my_share_minor INTEGER,
    -- A local label for who shares the cost; never leaves the app.
    share_label    TEXT    CHECK (share_label <> ''),
    -- The user's note on this line; not sent over MCP.
    user_note      TEXT,

    -- Also the index for the `transaction_id` foreign key.
    UNIQUE (transaction_id, line_no),
    CHECK (my_share_minor IS NULL
           OR (amount_minor >= 0 AND my_share_minor BETWEEN 0 AND amount_minor)
           OR (amount_minor < 0 AND my_share_minor BETWEEN amount_minor AND 0))
) STRICT;

CREATE INDEX transaction_split_category ON transaction_split (category_id);

-- A line has its parent's sign: a purchase splits into amounts spent, a
-- refund into amounts back. A zero transaction can't be split. The parent
-- has no category or share of its own once split (core clears them in the
-- same DB transaction), and a transfer isn't split: it is neither spending
-- nor income, so there is nothing to divide between categories. A row in a
-- `rejected` group is an ordinary transaction (the group only stops the pair
-- being suggested again) and can be split.
CREATE TRIGGER transaction_split_insert
BEFORE INSERT ON transaction_split
BEGIN
    SELECT RAISE(ABORT, 'a split line has its transaction''s sign')
    FROM "transaction"
    WHERE id = NEW.transaction_id
      AND (amount_minor = 0 OR (amount_minor > 0) <> (NEW.amount_minor > 0));
    SELECT RAISE(ABORT, 'a split transaction has no category of its own: its lines do')
    FROM "transaction"
    WHERE id = NEW.transaction_id AND category_id IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction has no share of its own: its lines do')
    FROM "transaction"
    WHERE id = NEW.transaction_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    FROM "transaction" t JOIN transfer_group g ON g.id = t.transfer_group_id
    WHERE t.id = NEW.transaction_id AND g.status <> 'rejected';
END;

-- A line stays with its transaction. Core replaces a split's lines as a
-- whole; an edit to one line's amount keeps the parent's sign.
CREATE TRIGGER transaction_split_update
BEFORE UPDATE ON transaction_split
BEGIN
    SELECT RAISE(ABORT, 'a split line''s ID and transaction are read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.transaction_id IS NOT OLD.transaction_id;
    SELECT RAISE(ABORT, 'a split line has its transaction''s sign')
    FROM "transaction"
    WHERE NEW.amount_minor IS NOT OLD.amount_minor
      AND id = NEW.transaction_id
      AND (amount_minor > 0) <> (NEW.amount_minor > 0);
END;

-- While a transaction has lines: no category or share of its own, no
-- suggested or confirmed transfer, and its amount and currency stay as they
-- are, so the lines keep summing to it. (An imported row's amount and
-- currency are read-only anyway; a manual row's are editable, so the split
-- is removed or replaced first.) Clearing a category or a share, and
-- leaving a transfer group, stay possible.
CREATE TRIGGER transaction_split_parent
BEFORE UPDATE OF category_id, my_share_minor, share_label, amount_minor, currency,
                 transfer_group_id ON "transaction"
WHEN EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'a split transaction has no category of its own: its lines do')
    WHERE NEW.category_id IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction has no share of its own: its lines do')
    WHERE NEW.my_share_minor IS NOT NULL OR NEW.share_label IS NOT NULL;
    SELECT RAISE(ABORT, 'a split transaction''s amount and currency can''t change; remove the split first')
    WHERE NEW.amount_minor IS NOT OLD.amount_minor OR NEW.currency IS NOT OLD.currency;
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    FROM transfer_group
    WHERE NEW.transfer_group_id IS NOT OLD.transfer_group_id
      AND id = NEW.transfer_group_id AND status <> 'rejected';
END;

-- A rejected group can hold a split row; it can't then become a transfer
-- again until the split is removed.
CREATE TRIGGER transfer_group_split
BEFORE UPDATE OF status ON transfer_group
WHEN NEW.status <> 'rejected'
BEGIN
    SELECT RAISE(ABORT, 'a transfer can''t be split')
    WHERE EXISTS (SELECT 1 FROM "transaction" t
                  JOIN transaction_split s ON s.transaction_id = t.id
                  WHERE t.transfer_group_id = NEW.id);
END;

-- What reports, budgets and MCP read: one row per split line, or the
-- transaction itself when it has no lines. Each row is in its transaction's
-- account, date and currency.
--
-- The view doesn't filter: excluded rows and transfers are in it, with the
-- columns to leave them out by (`excluded_reason`, `transfer_status`).
-- Reports skip excluded rows and confirmed transfers (`docs/ARCHITECTURE.md`
-- → "Data model"), but the balance, the excluded filter and the transfer
-- views read the same rows, and each decides for itself. Only `duplicate`
-- also leaves the computed balance.
--
-- `split_id` and `line_no` are NULL on a transaction's own row.
-- `my_share_minor` is as stored (NULL means all of it); `my_amount_minor`
-- is the user's part with that resolved. Notes are left out: nothing that
-- reports on amounts needs them.
CREATE VIEW ledger_line AS
SELECT t.id                                     AS transaction_id,
       NULL                                     AS split_id,
       NULL                                     AS line_no,
       t.account_id                             AS account_id,
       t.batch_id                               AS batch_id,
       t.date                                   AS date,
       t.amount_minor                           AS amount_minor,
       t.currency                               AS currency,
       t.category_id                            AS category_id,
       t.my_share_minor                         AS my_share_minor,
       coalesce(t.my_share_minor, t.amount_minor) AS my_amount_minor,
       t.share_label                            AS share_label,
       t.reimbursable                           AS reimbursable,
       t.merchant                               AS merchant,
       t.merchant_display                       AS merchant_display,
       t.excluded_reason                        AS excluded_reason,
       t.transfer_group_id                      AS transfer_group_id,
       g.status                                 AS transfer_status,
       t.origin                                 AS origin
FROM "transaction" t
LEFT JOIN transfer_group g ON g.id = t.transfer_group_id
WHERE NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id)
UNION ALL
SELECT t.id,
       s.id,
       s.line_no,
       t.account_id,
       t.batch_id,
       t.date,
       s.amount_minor,
       t.currency,
       s.category_id,
       s.my_share_minor,
       coalesce(s.my_share_minor, s.amount_minor),
       s.share_label,
       t.reimbursable,
       t.merchant,
       t.merchant_display,
       t.excluded_reason,
       t.transfer_group_id,
       g.status,
       t.origin
FROM transaction_split s
JOIN "transaction" t ON t.id = s.transaction_id
LEFT JOIN transfer_group g ON g.id = t.transfer_group_id;
