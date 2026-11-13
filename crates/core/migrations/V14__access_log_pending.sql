-- Calls the MCP server refused because it couldn't open the database
-- (`docs/ARCHITECTURE.md` → "Decision record: M2: MCP server (2026-11-13)",
-- F.8). The server can't write the access log then, so it records each
-- refused call in a pending file next to the database
-- (`coffer_core::access_log::pending`) and moves them into the log, with
-- their own times, once the database opens again. This migration lets the
-- log hold them:
--
-- - two outcomes: `refused_restoring` (a restore held the restore lock) and
--   `refused_unavailable` (the database couldn't be opened, e.g. migrations
--   were pending, or the restore lock couldn't be checked);
-- - `pending_id`: the ID a pending entry was given when it was recorded.
--   Unique, so moving an entry in twice (a crash after the insert, before
--   the file was emptied) inserts it once. Empty for calls logged directly;
-- - `summarised_calls`: empty for a row that records one call. A row that
--   summarises the refused calls the pending file had no room for (past
--   its cap) holds how many there were: one row per client, tool and
--   outcome, timed at the first of them, without arguments or purpose.
--
-- Adding outcomes changes a `CHECK`, so the table is rebuilt (SQLite's
-- "12-step" procedure). No foreign key refers to `mcp_access_log` or from
-- it, so this needs no `foreign_keys = OFF`. IDs are kept, and so is the
-- AUTOINCREMENT counter, so an expired row's ID is never reused.

CREATE TABLE mcp_access_log_v14 (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    at                 TEXT    NOT NULL
                               DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                               CHECK (at IS strftime('%Y-%m-%dT%H:%M:%fZ', at)),
    client             TEXT    NOT NULL
                               CHECK (length(client) BETWEEN 1 AND 64
                                      AND client NOT GLOB '*[^A-Za-z0-9._-]*'),
    -- A registered tool's name. A name the server doesn't know isn't logged.
    tool               TEXT    NOT NULL
                               CHECK (length(tool) BETWEEN 1 AND 64
                                      AND tool NOT GLOB '*[^a-z0-9_]*'),
    access_level       TEXT    CHECK (access_level IN ('aggregate', 'pseudonymous', 'standard')),
    outcome            TEXT    NOT NULL
                               CHECK (outcome IN ('ok', 'refused_off', 'refused_not_enabled',
                                                  'refused_level', 'refused_restoring',
                                                  'refused_unavailable', 'error')),
    purpose            TEXT,
    args_redacted_json TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(args_redacted_json)),
    row_count          INTEGER NOT NULL DEFAULT 0 CHECK (row_count >= 0),
    result_hash        TEXT    CHECK (result_hash <> ''),
    withheld_count     INTEGER NOT NULL DEFAULT 0 CHECK (withheld_count >= 0),
    pending_id         TEXT    UNIQUE CHECK (length(pending_id) BETWEEN 1 AND 200),
    summarised_calls   INTEGER CHECK (summarised_calls >= 1),

    -- A refused or failed call returned nothing.
    CHECK (outcome = 'ok' OR (row_count = 0 AND result_hash IS NULL AND withheld_count = 0)),
    -- Only refusals the server couldn't log at once are summarised, and
    -- only from the pending file.
    CHECK (summarised_calls IS NULL
           OR (outcome IN ('refused_restoring', 'refused_unavailable')
               AND pending_id IS NOT NULL))
) STRICT;

INSERT INTO mcp_access_log_v14 (id, at, client, tool, access_level, outcome, purpose,
                                args_redacted_json, row_count, result_hash, withheld_count)
SELECT id, at, client, tool, access_level, outcome, purpose, args_redacted_json, row_count,
       result_hash, withheld_count
FROM mcp_access_log;

-- The AUTOINCREMENT counter: the copy set it to the largest ID copied, which
-- is lower than the old one when the newest rows have expired.
UPDATE sqlite_sequence
SET seq = max(seq, coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'mcp_access_log'), 0))
WHERE name = 'mcp_access_log_v14';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'mcp_access_log_v14', seq FROM sqlite_sequence
WHERE name = 'mcp_access_log'
  AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'mcp_access_log_v14');

DROP TABLE mcp_access_log;
ALTER TABLE mcp_access_log_v14 RENAME TO mcp_access_log;

CREATE INDEX mcp_access_log_at ON mcp_access_log (at);
CREATE INDEX mcp_access_log_client ON mcp_access_log (client, at);

-- A log row is a record of what happened: deleted when it expires, never
-- edited.
CREATE TRIGGER mcp_access_log_read_only
BEFORE UPDATE ON mcp_access_log
BEGIN
    SELECT RAISE(ABORT, 'an access log row is read-only');
END;
