-- Flagged rows of an import (ROADMAP M1.11–M1.13; `docs/ARCHITECTURE.md` →
-- "Decision record: import commit, dedupe and undo").
--
-- A row the import didn't turn into a transaction, for a reason the user has
-- to see: it couldn't be read, its source ID is already taken by a different
-- transaction, it falls in a locked period, or its running balance doesn't
-- follow from the rows before it. The file's rows are never dropped
-- silently, so the batch keeps a record of each one it left out or doubts.
--
-- Only the row's place in the file and the reason are kept, never the row's
-- text: `detail` is core's own message (`RowProblem` and friends never quote
-- the file). A running-balance flag names the two balances, which are the
-- user's own figures, like `balance_snapshot`.

CREATE TABLE import_flag (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id      INTEGER NOT NULL REFERENCES import_batch (id),
    -- `SourceLocation`: a 1-based line in a text file, or entry in a
    -- structured one.
    location_kind TEXT    NOT NULL CHECK (location_kind IN ('line', 'entry')),
    location      INTEGER NOT NULL CHECK (location >= 1),
    kind          TEXT    NOT NULL CHECK (kind IN (
                              'unreadable', 'source_id_conflict', 'locked',
                              'running_balance')),
    detail        TEXT    NOT NULL CHECK (trim(detail) <> '')
) STRICT;

CREATE INDEX import_flag_batch ON import_flag (batch_id);

-- A flag records what the import found; it never changes. Undo deletes it
-- with its batch.
CREATE TRIGGER import_flag_read_only
BEFORE UPDATE ON import_flag
BEGIN
    SELECT RAISE(ABORT, 'an import flag is read-only');
END;

CREATE TRIGGER import_flag_insert
BEFORE INSERT ON import_flag
BEGIN
    SELECT RAISE(ABORT, 'an undone import batch takes no flags')
    FROM import_batch WHERE id = NEW.batch_id AND status = 'undone';
END;

-- Like its transactions and balances (`import_batch_status` in V1), a
-- batch's flags go before it is marked undone.
CREATE TRIGGER import_batch_status_flags
BEFORE UPDATE OF status ON import_batch
WHEN OLD.status = 'committed' AND NEW.status = 'undone'
BEGIN
    SELECT RAISE(ABORT, 'an import batch is undone only once its flags are deleted')
    WHERE EXISTS (SELECT 1 FROM import_flag WHERE batch_id = OLD.id);
END;
