-- Possible duplicates (ROADMAP M1.26; `docs/ARCHITECTURE.md` → Decisions,
-- "Duplicates", and "Decision record: review queues", points 3–5).
--
-- Exact duplicates are refused at import (source ID, then fingerprint). A
-- near-duplicate (two rows on one account with the same amount, a few days
-- apart, that the exact dedupe can't match: overlapping CSV and OFX exports,
-- a manual row entered ahead of the bank's export) is recorded here as a
-- `pending` candidate for the user to merge or dismiss. Core finds them
-- (`coffer_core::duplicate`); the rules for a match live there.
--
-- - A pair is stored once, lower transaction ID first, so `(a, b)` and
--   `(b, a)` can't both exist. A pair recorded in any status is never raised
--   again: a dismissed pair stays dismissed.
-- - Merging excludes the duplicate (reason `duplicate`), never deletes it,
--   as one `duplicate_merge` change batch. `kept` says which side stayed and
--   `change_batch_id` names the batch; both are set exactly while the
--   candidate is `merged`. Undoing the batch puts the candidate back to
--   `pending` and clears both.
-- - Deleting a transaction (undoing its import, deleting a manual row)
--   deletes the candidates that name it first: there is no `ON DELETE`.

CREATE TABLE duplicate_candidate (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The lower transaction ID of the pair.
    transaction_a   INTEGER NOT NULL REFERENCES "transaction" (id),
    -- The higher one.
    transaction_b   INTEGER NOT NULL REFERENCES "transaction" (id),
    -- Which kind of match it is: two imported rows from different imports
    -- (`overlapping_import`), or a manual row and an imported one
    -- (`manual_entry`).
    reason          TEXT    NOT NULL CHECK (reason IN ('overlapping_import', 'manual_entry')),
    status          TEXT    NOT NULL DEFAULT 'pending'
                            CHECK (status IN ('pending', 'merged', 'dismissed')),
    -- The side that stayed when merged: `a` or `b`. The other is excluded.
    kept            TEXT    CHECK (kept IN ('a', 'b')),
    -- The `duplicate_merge` batch that merged it. One batch can merge many.
    change_batch_id INTEGER REFERENCES change_batch (id),
    created_at      TEXT    NOT NULL
                            DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                            CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),

    -- Also the index for the `transaction_a` foreign key.
    UNIQUE (transaction_a, transaction_b),
    CHECK (transaction_a < transaction_b),
    CHECK ((status = 'merged') = (kept IS NOT NULL)),
    CHECK ((status = 'merged') = (change_batch_id IS NOT NULL))
) STRICT;

CREATE INDEX duplicate_candidate_transaction_b ON duplicate_candidate (transaction_b);
CREATE INDEX duplicate_candidate_change_batch ON duplicate_candidate (change_batch_id);
CREATE INDEX duplicate_candidate_status ON duplicate_candidate (status);

-- Both rows are on one account: a duplicate is the same transaction stored
-- twice in one account. (A manual row can move to another account later;
-- merging checks the pair again.)
CREATE TRIGGER duplicate_candidate_insert
BEFORE INSERT ON duplicate_candidate
BEGIN
    SELECT RAISE(ABORT, 'a duplicate candidate''s rows are on one account')
    FROM "transaction" a, "transaction" b
    WHERE a.id = NEW.transaction_a AND b.id = NEW.transaction_b
      AND a.account_id IS NOT b.account_id;
    SELECT RAISE(ABORT, 'a new duplicate candidate is pending')
    WHERE NEW.status IS NOT 'pending';
    SELECT RAISE(ABORT, 'a duplicate candidate is merged by a duplicate_merge batch')
    FROM change_batch
    WHERE id = NEW.change_batch_id AND kind <> 'duplicate_merge';
END;

-- The pair, its reason and date never change. Only the status moves:
-- pending → merged → pending (undo), pending → dismissed → pending.
CREATE TRIGGER duplicate_candidate_update
BEFORE UPDATE ON duplicate_candidate
BEGIN
    SELECT RAISE(ABORT, 'a duplicate candidate''s pair, reason and date are read-only')
    WHERE NEW.id IS NOT OLD.id
       OR NEW.transaction_a IS NOT OLD.transaction_a
       OR NEW.transaction_b IS NOT OLD.transaction_b
       OR NEW.reason IS NOT OLD.reason
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'only a pending duplicate candidate is merged or dismissed')
    WHERE NEW.status IN ('merged', 'dismissed') AND OLD.status IS NOT 'pending';
    SELECT RAISE(ABORT, 'a duplicate candidate is merged by a duplicate_merge batch')
    FROM change_batch
    WHERE id = NEW.change_batch_id AND kind <> 'duplicate_merge';
END;
