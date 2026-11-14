-- The preset an account's next import uses (ROADMAP M3.2; `docs/ARCHITECTURE.md`
-- → "Decision record: accounts management").
--
-- The account inspector's Preset select (`DESIGN.md` → "Accounts panel")
-- remembers which CSV preset the user imports this account's exports with, so
-- the import flow (M3.4) can choose it again. It's the preset's stable `id`
-- (`uk-monzo`), the same text `import_batch.preset_id` records. Core doesn't
-- check it against the presets: they are compiled into `coffer-import` (and
-- user presets come later, M3.5), so the app offers only presets it has, and
-- an import whose preset has gone falls back to detection.
--
-- Only accounts imported from a bank have one: current, savings and credit
-- card. Cash, asset and liability accounts are kept by the user, and a pot's
-- rows arrive with its parent's imports.

ALTER TABLE account ADD COLUMN preset_id TEXT
    CHECK (preset_id IS NULL
           OR (preset_id <> '' AND kind IN ('current', 'savings', 'credit_card')));
