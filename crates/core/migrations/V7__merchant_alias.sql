-- Merchant names (ROADMAP M1.23; `docs/ARCHITECTURE.md` → Decisions, "Merchant
-- names", and "Decision record: rules", point 5).
--
-- A transaction's `merchant_display` is derived: core cleans the bank's text
-- (`coffer_core::merchant::clean`: card numbers, "CONTACTLESS", processor
-- prefixes, trailing dates and references), then the first merchant alias
-- that matches the cleaned text gives the name. Rule actions (M1.24) and the
-- user can set a name instead; such a name is never recomputed. The bank's
-- text (`merchant`, `description_raw`) is never changed.

-- A merchant name: bank text that `pattern` matches is shown as
-- `display_name`. Resolution order (core): an `exact` match (ignoring case)
-- wins, then the longest matching `prefix`, then the first matching `regex`
-- by ID, so the oldest pattern first.
CREATE TABLE merchant_alias (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    match_kind   TEXT    NOT NULL CHECK (match_kind IN ('exact', 'prefix', 'regex')),
    -- User (or, through an approved suggestion, agent) text; a `regex`
    -- pattern is validated in core like a rule's.
    pattern      TEXT    NOT NULL CHECK (trim(pattern) <> ''),
    display_name TEXT    NOT NULL CHECK (trim(display_name) <> ''
                                         AND display_name = trim(display_name)),
    created_at   TEXT    NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                         CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at))
) STRICT;

-- Who set a transaction's `merchant_display`. NULL means it is derived
-- (clean-up, then aliases) and is recomputed whenever an alias changes; a
-- name set by a rule action (`rule`) or by the user (`user`) is left alone.
-- A set name is never blank.
ALTER TABLE "transaction" ADD COLUMN merchant_display_source TEXT
    CHECK (merchant_display_source IS NULL
           OR (merchant_display_source IN ('rule', 'user')
               AND coalesce(trim(merchant_display), '') <> ''));
