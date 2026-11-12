-- Tags (ROADMAP M1.21; `docs/ARCHITECTURE.md` → Decisions, "Tags and notes").
--
-- A transaction has any number of tags, alongside its category. Tag names
-- are user text: MCP sees them only at `standard` access, and never for a
-- tag marked `private` (nor do exports). The user's own note needs no table:
-- it is `"transaction".user_note` (V1), beside the read-only imported `note`.
--
-- `coffer_core::tag` checks names before they're stored (`TagName`): trimmed,
-- not blank, at most 64 characters, no control characters, and unique
-- ignoring case with full Unicode case folding. The schema checks what SQL
-- can, as a backstop: no surrounding whitespace, the length, and uniqueness
-- ignoring ASCII case (`NOCASE`). SQL can't fold non-ASCII letters, so
-- `Café` and `CAFÉ` are told apart here and refused as one name in core.

CREATE TABLE tag (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT    NOT NULL COLLATE NOCASE
                    CHECK (name <> ''
                           AND name = trim(name, ' ' || char(9, 10, 11, 12, 13))
                           AND length(name) <= 64),
    -- A private tag never leaves the app: not over MCP at any access level,
    -- and not in exports.
    private INTEGER NOT NULL DEFAULT 0 CHECK (private IN (0, 1))
) STRICT;

CREATE UNIQUE INDEX tag_name ON tag (name COLLATE NOCASE);

-- A tag on a transaction. A pair appears once. Deleting a tag deletes its
-- links first, in the same DB transaction (core); undoing an import batch
-- deletes its rows' links with them. There is no `ON DELETE` (V1
-- conventions), so a forgotten link makes the delete fail rather than
-- vanish.
CREATE TABLE transaction_tag (
    transaction_id INTEGER NOT NULL REFERENCES "transaction" (id),
    tag_id         INTEGER NOT NULL REFERENCES tag (id),

    PRIMARY KEY (transaction_id, tag_id)
) STRICT, WITHOUT ROWID;

-- The primary key indexes `transaction_id`; this one finds a tag's links.
CREATE INDEX transaction_tag_tag ON transaction_tag (tag_id);

-- A link is added or removed, never changed: moving one would put a tag
-- (possibly private) on another transaction without either side noticing.
CREATE TRIGGER transaction_tag_read_only
BEFORE UPDATE ON transaction_tag
BEGIN
    SELECT RAISE(ABORT, 'a tag link is added or removed, never changed');
END;
