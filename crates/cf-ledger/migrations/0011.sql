
  ALTER TABLE task ADD COLUMN stop_seq INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE message ADD COLUMN carried_by INTEGER
    CONSTRAINT message_carried_by_fk REFERENCES message (id) ON DELETE CASCADE;
  ALTER TABLE message ADD COLUMN claimed_at TEXT;
  ALTER TABLE message ADD COLUMN door_closed_at TEXT;
  