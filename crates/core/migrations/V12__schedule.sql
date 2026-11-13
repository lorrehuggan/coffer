-- Schedules: recurring payments the user confirmed (ROADMAP M2.10), and the
-- detected series the user said aren't recurring (M2.6). `docs/ARCHITECTURE.md`
-- → "Decision record: budgets and the forecast (2026-11-10)", point 2.
--
-- Core detects recurring series in the ledger (`coffer_core::recurring`);
-- a series becomes a schedule only when the user confirms it
-- (`coffer_core::schedule`). A schedule never adds a transaction: imports
-- fulfil its occurrences.
--
-- - A schedule's occurrences are not stored. Occurrence k (from 0) is its
--   `start_date` advanced k times by its cadence, each counted from the
--   start (a monthly schedule from the 31st falls on 28 or 29 February and
--   on 31 March). The next date is derived.
-- - Stored per occurrence is only what the user or an import decided: a
--   skip (`schedule_skip`) or the transaction that fulfilled it
--   (`schedule_match`). Upcoming, late and missed are derived, never
--   stored: an occurrence is missed only once the account's imports cover
--   its date plus the tolerance.
-- - A dismissed series (`recurring_dismissal`, "Not recurring") isn't
--   suggested again. It is keyed by the series key core derives.
-- - Undoing an import, or deleting a manual row, deletes the matches that
--   name its transactions first: there is no `ON DELETE`.

CREATE TABLE schedule (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id       INTEGER NOT NULL REFERENCES account (id),
    -- The name payments are matched by, compared after normalising
    -- (`recurring::normalize_merchant`).
    merchant_display TEXT    NOT NULL CHECK (trim(merchant_display) <> ''),
    category_id      INTEGER REFERENCES category (id),
    cadence          TEXT    NOT NULL CHECK (cadence IN (
                         'weekly', 'fortnightly', 'four_weekly', 'monthly', 'quarterly', 'yearly')),
    -- The first occurrence the schedule tracks, and its anchor.
    start_date       TEXT    NOT NULL CHECK (start_date IS date(start_date)),
    -- Signed: negative is money out. In the account's currency.
    amount_minor     INTEGER NOT NULL CHECK (amount_minor <> 0),
    currency         TEXT    NOT NULL CHECK (currency GLOB '[A-Z][A-Z][A-Z]'),
    -- How far a payment may be from `amount_minor` and still match.
    tolerance_minor  INTEGER NOT NULL DEFAULT 0 CHECK (tolerance_minor >= 0),
    -- How many days a payment may be from an occurrence's date and still be
    -- on time. Core keeps it under half the cadence.
    tolerance_days   INTEGER NOT NULL CHECK (tolerance_days BETWEEN 0 AND 181),
    status           TEXT    NOT NULL DEFAULT 'active'
                             CHECK (status IN ('active', 'paused', 'ended')),
    -- The detected series it was confirmed from, if any.
    series_key       TEXT    CHECK (series_key <> ''),
    created_at       TEXT    NOT NULL
                             DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                             CHECK (created_at IS strftime('%Y-%m-%dT%H:%M:%fZ', created_at))
) STRICT;

CREATE INDEX schedule_account ON schedule (account_id);
CREATE INDEX schedule_category ON schedule (category_id);

-- A schedule is in its account's currency, and never on an account that
-- holds valuations only.
CREATE TRIGGER schedule_insert
BEFORE INSERT ON schedule
BEGIN
    SELECT RAISE(ABORT, 'a schedule is in its account''s currency')
    FROM account WHERE id = NEW.account_id AND currency IS NOT NEW.currency;
    SELECT RAISE(ABORT, 'asset and liability accounts hold valuations, not schedules')
    FROM account WHERE id = NEW.account_id AND kind IN ('asset', 'liability');
END;

-- Its account, currency, cadence and start fix every occurrence's date, so
-- they never change (matches and skips name those dates). An ended
-- schedule stays ended.
CREATE TRIGGER schedule_update
BEFORE UPDATE ON schedule
BEGIN
    SELECT RAISE(ABORT, 'a schedule''s account, cadence, start and currency are read-only')
    WHERE NEW.id IS NOT OLD.id
       OR NEW.account_id IS NOT OLD.account_id
       OR NEW.cadence IS NOT OLD.cadence
       OR NEW.start_date IS NOT OLD.start_date
       OR NEW.currency IS NOT OLD.currency
       OR NEW.series_key IS NOT OLD.series_key
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'an ended schedule stays ended')
    WHERE OLD.status = 'ended' AND NEW.status IS NOT 'ended';
END;

-- An occurrence the user skipped: it is neither expected nor missed.
CREATE TABLE schedule_skip (
    schedule_id     INTEGER NOT NULL REFERENCES schedule (id),
    occurrence_date TEXT    NOT NULL CHECK (occurrence_date IS date(occurrence_date)),
    skipped_at      TEXT    NOT NULL
                            DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                            CHECK (skipped_at IS strftime('%Y-%m-%dT%H:%M:%fZ', skipped_at)),

    -- Also the index for the `schedule_id` foreign key.
    PRIMARY KEY (schedule_id, occurrence_date)
) STRICT;

-- The transaction that fulfilled an occurrence: found by an import (or when
-- the schedule was confirmed), `auto`, or chosen by the user, `user`.
CREATE TABLE schedule_match (
    schedule_id     INTEGER NOT NULL REFERENCES schedule (id),
    occurrence_date TEXT    NOT NULL CHECK (occurrence_date IS date(occurrence_date)),
    -- A transaction fulfils at most one occurrence of any schedule.
    transaction_id  INTEGER NOT NULL UNIQUE REFERENCES "transaction" (id),
    matched_by      TEXT    NOT NULL CHECK (matched_by IN ('auto', 'user')),
    matched_at      TEXT    NOT NULL
                            DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                            CHECK (matched_at IS strftime('%Y-%m-%dT%H:%M:%fZ', matched_at)),

    -- Also the index for the `schedule_id` foreign key.
    PRIMARY KEY (schedule_id, occurrence_date)
) STRICT;

-- A match is a transaction on the schedule's account, and an occurrence is
-- either skipped or matched, never both.
CREATE TRIGGER schedule_match_insert
BEFORE INSERT ON schedule_match
BEGIN
    SELECT RAISE(ABORT, 'a schedule matches transactions on its own account')
    FROM schedule s, "transaction" t
    WHERE s.id = NEW.schedule_id AND t.id = NEW.transaction_id
      AND s.account_id IS NOT t.account_id;
    SELECT RAISE(ABORT, 'a skipped occurrence isn''t matched')
    FROM schedule_skip
    WHERE schedule_id = NEW.schedule_id AND occurrence_date = NEW.occurrence_date;
END;

CREATE TRIGGER schedule_skip_insert
BEFORE INSERT ON schedule_skip
BEGIN
    SELECT RAISE(ABORT, 'a matched occurrence isn''t skipped')
    FROM schedule_match
    WHERE schedule_id = NEW.schedule_id AND occurrence_date = NEW.occurrence_date;
END;

-- Matches and skips are inserted or deleted, never edited.
CREATE TRIGGER schedule_match_read_only
BEFORE UPDATE ON schedule_match
BEGIN
    SELECT RAISE(ABORT, 'a schedule match is read-only');
END;

CREATE TRIGGER schedule_skip_read_only
BEFORE UPDATE ON schedule_skip
BEGIN
    SELECT RAISE(ABORT, 'a schedule skip is read-only');
END;

-- A detected series the user marked "Not recurring". Detection leaves it
-- out from then on.
CREATE TABLE recurring_dismissal (
    series_key   TEXT PRIMARY KEY CHECK (series_key <> ''),
    dismissed_at TEXT NOT NULL
                      DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                      CHECK (dismissed_at IS strftime('%Y-%m-%dT%H:%M:%fZ', dismissed_at))
) STRICT;
