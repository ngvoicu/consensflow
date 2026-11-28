-- Leads (leads step 2): the role `lead` joins the roles a participant holds, with the table of
-- what a lead leads and a task's `added_by_id`. It writes no row.
--
-- SQLite cannot widen a CHECK where it stands, so `participant` is rebuilt as 0037 rebuilt it:
-- the text SQLite holds after 0048 (0037's table with the columns 0039 added: `backup_of`,
-- `vacation_since`, `out_kind`, and 0048's `out_form`) with `'lead'` the one word added to
-- `participant_role_check`, every row and column as it was, its indexes made again, **and the
-- three triggers of 0039** (with `'lead'` joining the two words they refuse as a backup or as the
-- owner of one: a lead, like the chief, has no backup), which dropping the table drops with it and which the foreign-key
-- check cannot see missing. The sequences that keep a deleted id from coming back are put back
-- as they were, in the order they were.

CREATE TEMP TABLE sequence_rows AS SELECT rowid AS at, name, seq FROM sqlite_sequence;

CREATE TEMP TABLE participant_rows AS SELECT * FROM participant;
DROP TABLE participant;
CREATE TABLE "participant" (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    roles TEXT NOT NULL DEFAULT '[]',
    agent TEXT,
    harness TEXT,
    tier TEXT,
    member_id INTEGER,
    out_until TEXT,
    out_since TEXT,
    created_at TEXT NOT NULL,
    left_at TEXT, switched_from_harness TEXT, switched_from_agent TEXT, switched_from_cut INTEGER NOT NULL DEFAULT 0, designer INTEGER NOT NULL DEFAULT 0, model TEXT, switched_from_model TEXT, launched_model TEXT, gone_model TEXT, effort TEXT, model_name TEXT, needs_edit INTEGER NOT NULL DEFAULT 0 CHECK (needs_edit IN (0, 1)), former_handles TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(former_handles) AND json_type(former_handles) = 'array'), backup_of INTEGER REFERENCES participant(id), vacation_since TEXT, out_kind TEXT
  CHECK (out_kind IS NULL OR out_kind IN ('quota', 'account', 'busy', 'moved')), out_form TEXT,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'tester', 'designer', 'frontend', 'architect', 'researcher', 'integrator', 'lead')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;

INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer, model, switched_from_model, launched_model, gone_model, effort, model_name, needs_edit, former_handles, backup_of, vacation_since, out_kind, out_form)
  SELECT id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer, model, switched_from_model, launched_model, gone_model, effort, model_name, needs_edit, former_handles, backup_of, vacation_since, out_kind, out_form
  FROM participant_rows;
DROP TABLE participant_rows;
CREATE INDEX participant_member_index ON participant (member_id, left_at);
CREATE INDEX participant_history_index ON participant (project_id, handle, left_at IS NULL, role, agent);
CREATE UNIQUE INDEX participant_one_live_backup ON participant(backup_of)
  WHERE backup_of IS NOT NULL AND left_at IS NULL;

CREATE TRIGGER participant_backup_insert BEFORE INSERT ON participant
WHEN NEW.backup_of IS NOT NULL
BEGIN
  SELECT CASE WHEN NEW.member_id IS NOT NULL OR NEW.role IN ('human', 'chief', 'lead')
    OR NEW.id = NEW.backup_of
    OR NOT EXISTS (
      SELECT 1 FROM participant owner WHERE owner.id = NEW.backup_of
        AND owner.project_id = NEW.project_id AND owner.member_id IS NULL
        AND owner.role NOT IN ('human', 'chief', 'lead')
        AND (NEW.left_at IS NOT NULL OR owner.left_at IS NULL)
        AND owner.backup_of IS NULL
    )
    OR EXISTS (SELECT 1 FROM participant child
      WHERE child.backup_of = NEW.id)
  THEN RAISE(ABORT, 'invalid backup relationship') END;
END;

CREATE TRIGGER participant_backup_update
BEFORE UPDATE OF backup_of, left_at, member_id, role, project_id ON participant
WHEN NEW.backup_of IS NOT NULL
BEGIN
  SELECT CASE WHEN NEW.member_id IS NOT NULL OR NEW.role IN ('human', 'chief', 'lead')
    OR NEW.id = NEW.backup_of
    OR NOT EXISTS (
      SELECT 1 FROM participant owner WHERE owner.id = NEW.backup_of
        AND owner.project_id = NEW.project_id AND owner.member_id IS NULL
        AND owner.role NOT IN ('human', 'chief', 'lead')
        AND (NEW.left_at IS NOT NULL OR owner.left_at IS NULL)
        AND owner.backup_of IS NULL
    )
    OR EXISTS (SELECT 1 FROM participant child
      WHERE child.backup_of = NEW.id)
  THEN RAISE(ABORT, 'invalid backup relationship') END;
END;

CREATE TRIGGER participant_backup_owner_update
BEFORE UPDATE OF backup_of, left_at, member_id, role, project_id ON participant
WHEN EXISTS (SELECT 1 FROM participant child WHERE child.backup_of = OLD.id)
  AND (NEW.backup_of IS NOT NULL OR NEW.member_id IS NOT NULL
       OR NEW.role IN ('human', 'chief', 'lead') OR NEW.project_id != OLD.project_id
       OR (NEW.left_at IS NOT NULL AND EXISTS (SELECT 1 FROM participant child
            WHERE child.backup_of = OLD.id AND child.left_at IS NULL)))
BEGIN
  SELECT RAISE(ABORT, 'member still has a backup');
END;

-- What a lead leads, and whether it works with its own hands. A lead's participant row has
-- `role = 'lead'`, no tier, no `member_id` and no `backup_of`.
CREATE TABLE lead (
  participant_id INTEGER PRIMARY KEY
    CONSTRAINT lead_participant_fk REFERENCES participant (id) ON DELETE CASCADE,
  scope TEXT NOT NULL CONSTRAINT lead_scope_check CHECK (length(trim(scope)) > 0),
  own_hands INTEGER NOT NULL DEFAULT 0 CONSTRAINT lead_own_hands_check CHECK (own_hands IN (0, 1)),
  -- The one note that told the chief of this lead's removal: its delivery starts the
  -- clocks of the questions and waits the chief inherited.
  removal_note_id INTEGER CONSTRAINT lead_removal_note_fk REFERENCES message (id) ON DELETE SET NULL
) STRICT;

-- Who added the task, where its requester has changed since; NULL where it never did.
ALTER TABLE task ADD COLUMN added_by_id INTEGER
  CONSTRAINT task_added_by_fk REFERENCES participant (id) ON DELETE SET NULL;

DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence (name, seq) SELECT name, seq FROM sequence_rows ORDER BY at;
DROP TABLE sequence_rows;
