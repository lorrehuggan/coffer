-- Sub-accounts found by the bank's own ID (ROADMAP M1.17; `docs/ARCHITECTURE.md`
-- → "Decision record: sub-accounts").
--
-- A preset's `[sub_account_transfer]` recognises a transfer between an
-- account and one of its pots (Monzo Pots; later Starling Spaces and Revolut
-- Vaults) and reads the pot's stable ID from the row (Monzo's `pot_…`). The
-- import finds the pot by that ID under the account it imports into, and
-- creates it the first time it sees the ID. Pots are matched by this key
-- only, never by name: the user can rename a pot, at the bank or in Coffer.
--
-- The key is the bank's identifier for a sub-account, not for a person or a
-- payment account (no account number, sort code or IBAN), and it stays in the
-- app like every other account field.

-- The pot's ID at the bank, as the export writes it. Only pots have one;
-- pots the user creates by hand have none.
ALTER TABLE account ADD COLUMN sub_account_key TEXT
    CHECK (sub_account_key IS NULL OR (kind = 'pot' AND sub_account_key <> ''));

-- One pot per key under each parent. Another parent's pot may have the same
-- key: keys are only compared under one parent.
CREATE UNIQUE INDEX account_sub_account_key
    ON account (parent_id, sub_account_key) WHERE sub_account_key IS NOT NULL;

-- Imports find the pot by its key, so once set it never changes: a changed
-- key would send the next import's transfers to a new pot.
CREATE TRIGGER account_sub_account_key_read_only
BEFORE UPDATE OF sub_account_key ON account
WHEN OLD.sub_account_key IS NOT NULL AND NEW.sub_account_key IS NOT OLD.sub_account_key
BEGIN
    SELECT RAISE(ABORT, 'a pot''s sub-account key can''t change');
END;
