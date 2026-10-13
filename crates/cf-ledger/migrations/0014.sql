-- A project may nudge its chief on a schedule: the human's wake-up calls. They
-- are off until the human turns them on, an hour apart until the human says how
-- often, and say the default words until the human writes their own (none: the
-- ledger's default text). Columns added in place, as 0011 added its own: nothing
-- of the project's row moves, and every change of the setting is in the log
-- (`project.wake_up_setting`), as is each wake-up sent (`project.wake_up`).
-- Each column is one line: the test players that hold a database this ledger
-- left to what Node's did read the lines to know what Node's schema never had.
  ALTER TABLE project ADD COLUMN wake_up_on INTEGER NOT NULL DEFAULT 0 CONSTRAINT project_wake_up_on_check CHECK (wake_up_on IN (0, 1));
  ALTER TABLE project ADD COLUMN wake_up_every_ms INTEGER NOT NULL DEFAULT 3600000 CONSTRAINT project_wake_up_every_check CHECK (wake_up_every_ms > 0);
  ALTER TABLE project ADD COLUMN wake_up_text TEXT;
