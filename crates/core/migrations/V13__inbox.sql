-- The inbox's decisions (ROADMAP M2.3, M2.7, M2.14; `docs/ARCHITECTURE.md`
-- → "MCP surface", "Decision record: Inbox impact preview (2026-11-10)").
--
-- A suggestion is filed `pending` (V1) with a payload that core has
-- checked for its kind (`coffer_core::inbox::payload`) and stored in
-- canonical form, so two identical proposals have identical
-- `payload_json`. The user approves or rejects it in the app or the CLI,
-- never over MCP. This migration records that decision:
--
-- - `decided_at`: when it was approved or rejected;
-- - `decision_reason`: the user's optional reason for a rejection (their
--   own words, never sent over MCP);
-- - `change_batch_id`: the change batch the approval recorded (a
--   categorisation, a rule applied to history), or the rule's batch that
--   settled a pending categorisation (Inbox impact preview, point 4);
-- - `applied_json`: what else the approval made, by ID (the rule, alias,
--   link or transfer group), so the app can show and undo it. No foreign
--   keys: those rows can be deleted by the user later, and a suggestion is
--   a record of what happened.

ALTER TABLE suggestion ADD COLUMN decided_at TEXT
    CHECK (decided_at IS strftime('%Y-%m-%dT%H:%M:%fZ', decided_at));
ALTER TABLE suggestion ADD COLUMN decision_reason TEXT
    CHECK (decision_reason IS NULL
           OR (trim(decision_reason) <> '' AND length(decision_reason) <= 500));
ALTER TABLE suggestion ADD COLUMN change_batch_id INTEGER REFERENCES change_batch (id);
ALTER TABLE suggestion ADD COLUMN applied_json TEXT
    CHECK (applied_json IS NULL OR json_valid(applied_json));

CREATE INDEX suggestion_change_batch ON suggestion (change_batch_id);
CREATE INDEX suggestion_source ON suggestion (source_client, status);

-- One pending suggestion per kind and payload: a second identical proposal
-- is the same suggestion (core returns the existing ID).
CREATE UNIQUE INDEX suggestion_pending_once ON suggestion (kind, payload_json)
    WHERE status = 'pending';

-- What a suggestion proposes, who proposed it and why never change. Only a
-- pending suggestion is decided, once; the decision fields are set with it
-- and kept.
CREATE TRIGGER suggestion_decision
BEFORE UPDATE ON suggestion
BEGIN
    SELECT RAISE(ABORT, 'a suggestion''s proposal is read-only')
    WHERE NEW.id IS NOT OLD.id OR NEW.kind IS NOT OLD.kind
       OR NEW.payload_json IS NOT OLD.payload_json OR NEW.rationale IS NOT OLD.rationale
       OR NEW.source_client IS NOT OLD.source_client OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'a decided suggestion stays decided')
    WHERE OLD.status <> 'pending';
    SELECT RAISE(ABORT, 'a decided suggestion has a decision time')
    WHERE (NEW.status = 'pending') = (NEW.decided_at IS NOT NULL);
    SELECT RAISE(ABORT, 'only a rejection has a reason')
    WHERE NEW.decision_reason IS NOT NULL AND NEW.status <> 'rejected';
    SELECT RAISE(ABORT, 'only an approval records what it changed')
    WHERE (NEW.change_batch_id IS NOT NULL OR NEW.applied_json IS NOT NULL)
      AND NEW.status <> 'approved';
END;

-- A suggestion is filed pending, with no decision.
CREATE TRIGGER suggestion_insert
BEFORE INSERT ON suggestion
BEGIN
    SELECT RAISE(ABORT, 'a suggestion is filed pending')
    WHERE NEW.status <> 'pending' OR NEW.decided_at IS NOT NULL
       OR NEW.decision_reason IS NOT NULL OR NEW.change_batch_id IS NOT NULL
       OR NEW.applied_json IS NOT NULL;
END;
