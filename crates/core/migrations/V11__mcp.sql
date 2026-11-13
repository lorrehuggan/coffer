-- MCP clients, the AI access log and settings (ROADMAP M2.1, M2.11, M2.12;
-- `docs/ARCHITECTURE.md` → "MCP surface" and "Decision record: Settings, AI
-- access and backups (2026-11-10)"). This is the whole of M2's shape for
-- these tables, so access control, the kill switch and pseudonyms need no
-- further migration.
--
-- A client is known by the name it is started with (`coffer mcp --client
-- <name>`), the same name `suggestion.source_client` and
-- `report.source_client` store. The name is the key: a client is never
-- renamed, and a later `--client` with the same name is the same client.
-- `coffer` is reserved for suggestions the app files itself.

-- MCP clients ------------------------------------------------------------------

-- A client that has asked for access, been enabled or been ignored. The
-- server creates a `requested` row (not enabled) for a name it hasn't seen;
-- only the app or the CLI enables a client, which starts at `aggregate`.
-- "Ignore" hides a request without enabling it.
CREATE TABLE mcp_client (
    name         TEXT    PRIMARY KEY
                         CHECK (length(name) BETWEEN 1 AND 64
                                AND name NOT GLOB '*[^A-Za-z0-9._-]*'
                                AND lower(name) <> 'coffer'),
    enabled      INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    access_level TEXT    NOT NULL DEFAULT 'aggregate'
                         CHECK (access_level IN ('aggregate', 'pseudonymous', 'standard')),
    status       TEXT    NOT NULL DEFAULT 'requested'
                         CHECK (status IN ('requested', 'known', 'ignored')),
    created_at   TEXT    NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                         CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at)),
    last_seen_at TEXT    CHECK (last_seen_at IS strftime('%Y-%m-%dT%H:%M:%fZ', last_seen_at)),

    -- Only a client the user has answered is enabled: a request is
    -- answered by enabling (`known`) or ignoring it.
    CHECK (enabled = 0 OR status = 'known')
) STRICT;

-- The AI access log ---------------------------------------------------------------

-- One row per tool call, written by the server itself (not a tool write:
-- `AGENTS.md` → Invariants). Never the returned data: how many rows went
-- out, a hash of the result and how many descriptions were withheld.
-- `client` is the name the server was started with, not a foreign key, so
-- a refused call from a name without a row is logged too. `access_level`
-- is the client's level at the time of the call, empty when it has none.
-- `args_redacted_json` keeps keys, dates, enum values, numbers and IDs;
-- free text only as its type and length. `purpose` is the client's own
-- words, stored and shown, never interpreted. Rows are kept for 12 months
-- by default, then deleted; they are never edited.
CREATE TABLE mcp_access_log (
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
                                                  'refused_level', 'error')),
    purpose            TEXT,
    args_redacted_json TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(args_redacted_json)),
    row_count          INTEGER NOT NULL DEFAULT 0 CHECK (row_count >= 0),
    result_hash        TEXT    CHECK (result_hash <> ''),
    withheld_count     INTEGER NOT NULL DEFAULT 0 CHECK (withheld_count >= 0),

    -- A refused or failed call returned nothing.
    CHECK (outcome = 'ok' OR (row_count = 0 AND result_hash IS NULL AND withheld_count = 0))
) STRICT;

CREATE INDEX mcp_access_log_at ON mcp_access_log (at);
CREATE INDEX mcp_access_log_client ON mcp_access_log (client, at);

-- A log row is a record of what happened: deleted when it expires, never
-- edited.
CREATE TRIGGER mcp_access_log_read_only
BEFORE UPDATE ON mcp_access_log
BEGIN
    SELECT RAISE(ABORT, 'an access log row is read-only');
END;

-- Settings ------------------------------------------------------------------------

-- Small app-wide settings as text: the MCP kill switch, the per-database
-- key for merchant pseudonyms (never exposed), and later the app's own
-- preferences. Each key's value is parsed and checked in core.
CREATE TABLE setting (
    key   TEXT PRIMARY KEY
               CHECK (length(key) BETWEEN 1 AND 64 AND key NOT GLOB '*[^a-z0-9_.]*'),
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
