-- Refunds, reimbursements and shared costs (ROADMAP M1.25;
-- `docs/ARCHITECTURE.md` → Decisions, "Refunds, reimbursements, shared
-- costs", and "Decision record: refunds, reimbursements and shared costs").
--
-- A `transaction_link` ties money in (the inflow) to what it pays back:
--
-- - a `refund` to a purchase: the linked part takes the purchase's category
--   on the refund's own date, so it nets against the purchase instead of
--   counting as income;
-- - a `reimbursement` to a purchase marked `reimbursable` (claimed back): it
--   clears the claim;
-- - a `settlement` to a share label (a person): it reduces what they owe.
--
-- Links are `suggested` (Coffer's deterministic guesses, `coffer_core::link`),
-- `confirmed` (the user's) or `rejected` (a pair that is never suggested
-- again; unlinking rejects too). Only confirmed links count, in
-- `ledger_line` and in every cap below.
--
-- What one row can check is checked here; the settlement cap (at most what
-- the person owes) is checked in core, since what someone owes is a sum
-- over the whole ledger. Deleting a linked transaction is refused by the
-- foreign keys (no `ON DELETE`): whoever deletes transactions (undoing an
-- import, deleting a manual row) clears their links first.

CREATE TABLE transaction_link (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    kind         TEXT    NOT NULL CHECK (kind IN ('refund', 'reimbursement', 'settlement')),
    -- The money in.
    inflow_id    INTEGER NOT NULL REFERENCES "transaction" (id),
    -- The purchase a refund or reimbursement pays back; NULL for a
    -- settlement.
    target_id    INTEGER REFERENCES "transaction" (id),
    -- The person a settlement pays back; NULL otherwise. A local label that
    -- never leaves the app, like `"transaction".share_label`.
    share_label  TEXT    CHECK (share_label <> ''),
    -- How much of the inflow goes to the target, in their one currency:
    -- positive, at most what the inflow and the target can still take
    -- (triggers below). The rest of the inflow stays unlinked and counts by
    -- its own category.
    amount_minor INTEGER NOT NULL CHECK (amount_minor > 0),
    status       TEXT    NOT NULL DEFAULT 'suggested'
                         CHECK (status IN ('suggested', 'confirmed', 'rejected')),
    created_at   TEXT    NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                         CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    CHECK ((kind = 'settlement') = (target_id IS NULL)),
    CHECK ((kind = 'settlement') = (share_label IS NOT NULL)),
    CHECK (inflow_id IS NOT target_id)
) STRICT;

CREATE INDEX transaction_link_inflow ON transaction_link (inflow_id);
-- One link per pair, whatever its status: a rejected pair keeps its row, so
-- it is never suggested again. Also the index for the `target_id` foreign
-- key.
CREATE UNIQUE INDEX transaction_link_pair
    ON transaction_link (target_id, inflow_id) WHERE target_id IS NOT NULL;
CREATE UNIQUE INDEX transaction_link_settlement
    ON transaction_link (inflow_id, share_label) WHERE kind = 'settlement';
-- Outstanding claims are looked up by date (suggestions, Owed to you).
CREATE INDEX transaction_reimbursable ON "transaction" (date) WHERE reimbursable = 1;

-- A link's sides, checked on every insert, and on every update that leaves
-- it suggested or confirmed (a rejected link counts for nothing, and the
-- rows it names may have changed since):
--
-- - the inflow is money in, the target money out, both in one currency
--   (cross-currency links wait for L.28);
-- - the amount is at most the inflow and at most the target;
-- - a target is at most 3,037,000,499 minor units, so `ledger_line`'s
--   pro-rata products (two amounts at most the target's) stay inside a
--   64-bit integer and never turn into REAL;
-- - a reimbursement repays a purchase marked to claim back;
-- - a linked inflow isn't split and has no share of its own: its linked
--   part takes the target's category and share in `ledger_line`;
-- - confirmed links take no more, together, than the inflow has or the
--   target can take.
CREATE TRIGGER transaction_link_insert
BEFORE INSERT ON transaction_link
BEGIN
    SELECT RAISE(ABORT, 'a link''s inflow is money in')
    FROM "transaction" WHERE id = NEW.inflow_id AND amount_minor <= 0;
    SELECT RAISE(ABORT, 'a refund or reimbursement pays back money out')
    FROM "transaction" WHERE id = NEW.target_id AND amount_minor >= 0;
    SELECT RAISE(ABORT, 'both sides of a link are in one currency')
    FROM "transaction" r, "transaction" p
    WHERE r.id = NEW.inflow_id AND p.id = NEW.target_id AND r.currency IS NOT p.currency;
    SELECT RAISE(ABORT, 'a linked purchase is at most 3037000499 minor units')
    FROM "transaction" WHERE id = NEW.target_id AND amount_minor < -3037000499;
    SELECT RAISE(ABORT, 'a link is at most the money in')
    FROM "transaction" WHERE id = NEW.inflow_id AND NEW.amount_minor > amount_minor;
    SELECT RAISE(ABORT, 'a link is at most the purchase')
    FROM "transaction" WHERE id = NEW.target_id AND NEW.amount_minor > -amount_minor;
    SELECT RAISE(ABORT, 'a reimbursement repays a purchase marked to claim back')
    FROM "transaction"
    WHERE NEW.kind = 'reimbursement' AND NEW.status <> 'rejected'
      AND id = NEW.target_id AND reimbursable = 0;
    SELECT RAISE(ABORT, 'a linked inflow can''t be split')
    WHERE NEW.status <> 'rejected'
      AND EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = NEW.inflow_id);
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'confirmed links take more than the money in')
    FROM "transaction" r
    WHERE NEW.status = 'confirmed' AND r.id = NEW.inflow_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE inflow_id = NEW.inflow_id AND status = 'confirmed')
          > r.amount_minor;
    SELECT RAISE(ABORT, 'confirmed links take more than the purchase')
    FROM "transaction" p
    WHERE NEW.status = 'confirmed' AND p.id = NEW.target_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE target_id = NEW.target_id AND status = 'confirmed')
          > -p.amount_minor;
END;

-- Only the amount and the status change; a rejected pair isn't suggested
-- again (it can be confirmed: the user links it after all, or undoes an
-- unlink).
CREATE TRIGGER transaction_link_update
BEFORE UPDATE ON transaction_link
BEGIN
    SELECT RAISE(ABORT, 'a link''s kind, sides and date are read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.kind IS NOT OLD.kind
       OR NEW.inflow_id IS NOT OLD.inflow_id OR NEW.target_id IS NOT OLD.target_id
       OR NEW.share_label IS NOT OLD.share_label OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'a rejected link isn''t suggested again')
    WHERE OLD.status = 'rejected' AND NEW.status = 'suggested';
    -- The same checks as on insert, for a link that counts or may.
    SELECT RAISE(ABORT, 'a link''s inflow is money in')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id AND amount_minor <= 0;
    SELECT RAISE(ABORT, 'a refund or reimbursement pays back money out')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND amount_minor >= 0;
    SELECT RAISE(ABORT, 'both sides of a link are in one currency')
    FROM "transaction" r, "transaction" p
    WHERE NEW.status <> 'rejected' AND r.id = NEW.inflow_id AND p.id = NEW.target_id
      AND r.currency IS NOT p.currency;
    SELECT RAISE(ABORT, 'a linked purchase is at most 3037000499 minor units')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND amount_minor < -3037000499;
    SELECT RAISE(ABORT, 'a link is at most the money in')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id AND NEW.amount_minor > amount_minor;
    SELECT RAISE(ABORT, 'a link is at most the purchase')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.target_id AND NEW.amount_minor > -amount_minor;
    SELECT RAISE(ABORT, 'a reimbursement repays a purchase marked to claim back')
    FROM "transaction"
    WHERE NEW.kind = 'reimbursement' AND NEW.status <> 'rejected'
      AND id = NEW.target_id AND reimbursable = 0;
    SELECT RAISE(ABORT, 'a linked inflow can''t be split')
    WHERE NEW.status <> 'rejected'
      AND EXISTS (SELECT 1 FROM transaction_split WHERE transaction_id = NEW.inflow_id);
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own')
    FROM "transaction"
    WHERE NEW.status <> 'rejected' AND id = NEW.inflow_id
      AND (my_share_minor IS NOT NULL OR share_label IS NOT NULL);
    SELECT RAISE(ABORT, 'confirmed links take more than the money in')
    FROM "transaction" r
    WHERE NEW.status = 'confirmed' AND r.id = NEW.inflow_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE inflow_id = NEW.inflow_id AND status = 'confirmed'
                                AND id IS NOT OLD.id)
          > r.amount_minor;
    SELECT RAISE(ABORT, 'confirmed links take more than the purchase')
    FROM "transaction" p
    WHERE NEW.status = 'confirmed' AND p.id = NEW.target_id
      AND NEW.amount_minor + (SELECT coalesce(sum(amount_minor), 0) FROM transaction_link
                              WHERE target_id = NEW.target_id AND status = 'confirmed'
                                AND id IS NOT OLD.id)
          > -p.amount_minor;
END;

-- While a transaction is on either side of a suggested or confirmed link,
-- its amount and currency stay as they are, so the caps keep holding. (An
-- imported row's are read-only anyway; a manual row's links are confirmed
-- or cleared first, `manual::update`.)
CREATE TRIGGER transaction_link_amount
BEFORE UPDATE OF amount_minor, currency ON "transaction"
WHEN (NEW.amount_minor IS NOT OLD.amount_minor OR NEW.currency IS NOT OLD.currency)
 AND (EXISTS (SELECT 1 FROM transaction_link WHERE inflow_id = OLD.id AND status <> 'rejected')
      OR EXISTS (SELECT 1 FROM transaction_link
                 WHERE target_id = OLD.id AND status <> 'rejected'))
BEGIN
    SELECT RAISE(ABORT, 'a linked transaction''s amount and currency can''t change; unlink it first');
END;

-- A linked inflow's linked part takes its target's category and share, and
-- the rest is its own: it has no share of its own and isn't split.
CREATE TRIGGER transaction_link_inflow_share
BEFORE UPDATE OF my_share_minor, share_label ON "transaction"
WHEN (NEW.my_share_minor IS NOT NULL OR NEW.share_label IS NOT NULL)
 AND EXISTS (SELECT 1 FROM transaction_link WHERE inflow_id = OLD.id AND status <> 'rejected')
BEGIN
    SELECT RAISE(ABORT, 'a linked inflow has no share of its own');
END;

CREATE TRIGGER transaction_link_inflow_split
BEFORE INSERT ON transaction_split
WHEN EXISTS (SELECT 1 FROM transaction_link
             WHERE inflow_id = NEW.transaction_id AND status <> 'rejected')
BEGIN
    SELECT RAISE(ABORT, 'a linked inflow can''t be split');
END;

-- `ledger_line` with links (replaces V5's view; every V5 column keeps its
-- name, place and meaning, and six columns are added at the end).
--
-- Rows, each in its transaction's account, date and currency:
--
-- 1. A transaction without split lines: its own row. Its `amount_minor` is
--    what no confirmed link takes: the whole amount, except on an inflow,
--    where it is the unlinked rest (0 when fully linked; the row stays, so
--    every transaction keeps exactly one own row or its lines).
-- 2. Each split line, as before. (A linked inflow isn't split.)
-- 3. A confirmed refund or reimbursement against a purchase without lines:
--    one row on the inflow (its date, account, exclusion, transfer) for the
--    link's amount, in the purchase's category, with the purchase's share
--    pro rata, share label and `reimbursable`.
-- 4. The same against a split purchase: one row per purchase line, the
--    link's amount divided pro rata to the lines by largest remainder (each
--    line gets the floor of its exact part, and the units left over go one
--    each to the lines with the largest remainders, ties to the lower line
--    number), so the parts sum exactly to the link and none exceeds its
--    line. A line whose part is 0 has no row.
-- 5. A confirmed settlement: one row on the inflow for the link's amount,
--    with no category, the link's share label, and a share of 0: none of the
--    money is the user's, it repays the label's part of shared costs.
--
-- A share of a link part (rows 3 and 4) is `part * line share / line
-- amount`, truncated toward zero; the sub-unit rest is the other person's.
--
-- The rows of a transaction still sum to its amount, so balances read the
-- same rows as before.
--
-- Added columns:
--
-- - `link_id`, `link_kind`: the link a row 3–5 comes from; NULL on rows 1–2.
-- - `target_id`, `target_split_id`: the purchase (and its line) a row 3–4
--   nets against.
-- - `in_reports`: 1 when the row counts in spending, income, budgets and
--   reports: not excluded, not in a confirmed transfer, and for rows 3–4
--   the purchase isn't excluded or a confirmed transfer either (a refund of
--   an excluded double charge leaves reports with the charge).
-- - `personal_minor`: what the row counts as the user's own spending (< 0)
--   or income (> 0): `my_amount_minor`, or 0 when it isn't in reports or is
--   `reimbursable` (a purchase being claimed back, and the parts of links
--   that repay one, count as nothing). Reports read this by default
--   (decision point 3); `amount_minor` where `in_reports` is the gross view.
DROP VIEW ledger_line;

CREATE VIEW ledger_line AS
WITH
confirmed AS (
    SELECT id, kind, inflow_id, target_id, share_label, amount_minor
    FROM transaction_link
    WHERE status = 'confirmed'
),
inflow_linked AS (
    SELECT inflow_id, sum(amount_minor) AS linked_minor
    FROM confirmed
    GROUP BY inflow_id
),
split_part AS (
    SELECT l.id         AS link_id,
           s.id         AS split_id,
           s.amount_minor AS line_minor,
           s.category_id  AS category_id,
           s.my_share_minor AS line_share_minor,
           s.share_label  AS share_label,
           l.amount_minor * -s.amount_minor / -p.amount_minor
             + (row_number() OVER (
                    PARTITION BY l.id
                    ORDER BY (l.amount_minor * -s.amount_minor) % -p.amount_minor DESC,
                             s.line_no)
                <= l.amount_minor
                   - sum(l.amount_minor * -s.amount_minor / -p.amount_minor)
                         OVER (PARTITION BY l.id))
                AS part_minor
    FROM confirmed l
    JOIN "transaction" p ON p.id = l.target_id
    JOIN transaction_split s ON s.transaction_id = p.id
    WHERE l.kind IN ('refund', 'reimbursement')
),
line AS (
    -- 1. A transaction's own row.
    SELECT t.id                AS transaction_id,
           NULL                AS split_id,
           NULL                AS line_no,
           t.account_id        AS account_id,
           t.batch_id          AS batch_id,
           t.date              AS date,
           t.amount_minor - coalesce(i.linked_minor, 0) AS amount_minor,
           t.currency          AS currency,
           t.category_id       AS category_id,
           t.my_share_minor    AS my_share_minor,
           coalesce(t.my_share_minor, t.amount_minor - coalesce(i.linked_minor, 0))
                               AS my_amount_minor,
           t.share_label       AS share_label,
           t.reimbursable      AS reimbursable,
           t.merchant          AS merchant,
           t.merchant_display  AS merchant_display,
           t.excluded_reason   AS excluded_reason,
           t.transfer_group_id AS transfer_group_id,
           g.status            AS transfer_status,
           t.origin            AS origin,
           NULL                AS link_id,
           NULL                AS link_kind,
           NULL                AS target_id,
           NULL                AS target_split_id,
           (t.excluded_reason IS NULL AND g.status IS NOT 'confirmed') AS in_reports
    FROM "transaction" t
    LEFT JOIN transfer_group g ON g.id = t.transfer_group_id
    LEFT JOIN inflow_linked i ON i.inflow_id = t.id
    WHERE NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = t.id)
    UNION ALL
    -- 2. Split lines.
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
           t.origin,
           NULL,
           NULL,
           NULL,
           NULL,
           (t.excluded_reason IS NULL AND g.status IS NOT 'confirmed')
    FROM transaction_split s
    JOIN "transaction" t ON t.id = s.transaction_id
    LEFT JOIN transfer_group g ON g.id = t.transfer_group_id
    UNION ALL
    -- 3. A refund or reimbursement against a purchase without lines.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           l.amount_minor,
           r.currency,
           p.category_id,
           l.amount_minor * p.my_share_minor / p.amount_minor,
           coalesce(l.amount_minor * p.my_share_minor / p.amount_minor, l.amount_minor),
           p.share_label,
           p.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           p.id,
           NULL,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed'
            AND p.excluded_reason IS NULL AND pg.status IS NOT 'confirmed')
    FROM confirmed l
    JOIN "transaction" r ON r.id = l.inflow_id
    JOIN "transaction" p ON p.id = l.target_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    LEFT JOIN transfer_group pg ON pg.id = p.transfer_group_id
    WHERE l.kind IN ('refund', 'reimbursement')
      AND NOT EXISTS (SELECT 1 FROM transaction_split s WHERE s.transaction_id = p.id)
    UNION ALL
    -- 4. A refund or reimbursement against a split purchase, per line.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           sp.part_minor,
           r.currency,
           sp.category_id,
           sp.part_minor * sp.line_share_minor / sp.line_minor,
           coalesce(sp.part_minor * sp.line_share_minor / sp.line_minor, sp.part_minor),
           sp.share_label,
           p.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           p.id,
           sp.split_id,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed'
            AND p.excluded_reason IS NULL AND pg.status IS NOT 'confirmed')
    FROM split_part sp
    JOIN confirmed l ON l.id = sp.link_id
    JOIN "transaction" r ON r.id = l.inflow_id
    JOIN "transaction" p ON p.id = l.target_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    LEFT JOIN transfer_group pg ON pg.id = p.transfer_group_id
    WHERE sp.part_minor <> 0
    UNION ALL
    -- 5. A settlement.
    SELECT r.id,
           NULL,
           NULL,
           r.account_id,
           r.batch_id,
           r.date,
           l.amount_minor,
           r.currency,
           NULL,
           0,
           0,
           l.share_label,
           r.reimbursable,
           r.merchant,
           r.merchant_display,
           r.excluded_reason,
           r.transfer_group_id,
           rg.status,
           r.origin,
           l.id,
           l.kind,
           NULL,
           NULL,
           (r.excluded_reason IS NULL AND rg.status IS NOT 'confirmed')
    FROM confirmed l
    JOIN "transaction" r ON r.id = l.inflow_id
    LEFT JOIN transfer_group rg ON rg.id = r.transfer_group_id
    WHERE l.kind = 'settlement'
)
SELECT transaction_id,
       split_id,
       line_no,
       account_id,
       batch_id,
       date,
       amount_minor,
       currency,
       category_id,
       my_share_minor,
       my_amount_minor,
       share_label,
       reimbursable,
       merchant,
       merchant_display,
       excluded_reason,
       transfer_group_id,
       transfer_status,
       origin,
       link_id,
       link_kind,
       target_id,
       target_split_id,
       in_reports,
       CASE WHEN in_reports AND reimbursable = 0 THEN my_amount_minor ELSE 0 END
           AS personal_minor
FROM line;
