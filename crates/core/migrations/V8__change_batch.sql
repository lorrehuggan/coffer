-- Change batches (ROADMAP M1.24 and M1.26; `docs/ARCHITECTURE.md` →
-- Decisions, "Bulk changes", and "Decision record: rules", point 6).
--
-- A change to user-facing fields on many existing transactions (a rule
-- applied to history, a duplicate merge, a bulk edit) is previewed, then
-- committed as one `change_batch` with a `change_item` per transaction and
-- field, recording the value before and after. Undoing the batch restores
-- a field only where it still holds the batch's `new_value`; a row changed
-- since keeps the later value.
--
-- Values are JSON written and read by core (`coffer_core::change`), one
-- shape per field. They can hold user text (a note, a merchant name), like
-- the transaction itself: never logged, never sent over MCP.
--
-- Imported source fields are never in a change: they are read-only
-- (V1 trigger), and a bad import is undone or excluded instead.

CREATE TABLE change_batch (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL CHECK (kind IN ('rule_apply', 'duplicate_merge', 'bulk_edit')),
    created_at TEXT    NOT NULL
                       DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                       CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),
    status     TEXT    NOT NULL DEFAULT 'applied' CHECK (status IN ('applied', 'undone')),
    -- When it was undone; set with `status = 'undone'`.
    undone_at  TEXT    CHECK (undone_at IS strftime('%Y-%m-%dT%H:%M:%fZ', undone_at)),

    CHECK ((status = 'undone') = (undone_at IS NOT NULL))
) STRICT;

-- One field of one transaction, before and after. Items are numbered in the
-- order they were made (`id`); undo restores them in reverse, so a split
-- made after clearing a category is removed before the category returns.
CREATE TABLE change_item (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id       INTEGER NOT NULL REFERENCES change_batch (id),
    transaction_id INTEGER NOT NULL REFERENCES "transaction" (id),
    field          TEXT    NOT NULL CHECK (field IN (
                               'category', 'merchant', 'user_note', 'tags', 'exclusion',
                               'split', 'share', 'reimbursable')),
    old_value      TEXT    NOT NULL CHECK (json_valid(old_value)),
    new_value      TEXT    NOT NULL CHECK (json_valid(new_value)),

    -- Also the index for the `batch_id` foreign key.
    UNIQUE (batch_id, transaction_id, field),
    CHECK (old_value IS NOT new_value)
) STRICT;

CREATE INDEX change_item_transaction ON change_item (transaction_id);

-- A batch is a record: only its status changes, once, from `applied` to
-- `undone`.
CREATE TRIGGER change_batch_read_only
BEFORE UPDATE ON change_batch
BEGIN
    SELECT RAISE(ABORT, 'a change batch''s kind and date are read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.kind IS NOT OLD.kind
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'an undone change batch stays undone')
    WHERE OLD.status = 'undone';
END;

-- Items are written with their batch and never changed. They're deleted
-- only with the transaction they name (undoing its import, deleting a
-- manual row), since the foreign key has no `ON DELETE`.
CREATE TRIGGER change_item_read_only
BEFORE UPDATE ON change_item
BEGIN
    SELECT RAISE(ABORT, 'a change item is read-only');
END;

-- An undone batch takes no new items.
CREATE TRIGGER change_item_insert
BEFORE INSERT ON change_item
BEGIN
    SELECT RAISE(ABORT, 'an undone change batch takes no items')
    FROM change_batch WHERE id = NEW.batch_id AND status = 'undone';
END;
