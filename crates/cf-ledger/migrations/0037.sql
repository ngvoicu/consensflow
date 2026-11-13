-- The four roles of spec new-roles (FD-04, 2026-11-11) join the roles a member holds and the
-- pools a task is for: the frontend designer (`frontend`), the architect, the researcher and
-- the integrator, after the five of 0012, so the two CHECKs name every row of the ledger's
-- table of roles (`cf_ledger::roles`). SQLite cannot widen a CHECK where it stands, so the two
-- tables are rebuilt as 0012 rebuilt them: each table's text is the one SQLite holds after
-- 0036 (the columns 0021, 0028, 0031 and 0033 added written where SQLite wrote them) with the
-- one list widened, every row and column as it was, their indexes made again (0020's
-- history index among them), and the sequences that keep a deleted id from coming back put
-- back as they were, in the order they were. The number is 0037: the next one free on main
-- after 0036.
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
    left_at TEXT, switched_from_harness TEXT, switched_from_agent TEXT, switched_from_cut INTEGER NOT NULL DEFAULT 0, designer INTEGER NOT NULL DEFAULT 0, model TEXT, switched_from_model TEXT, launched_model TEXT, gone_model TEXT, effort TEXT, model_name TEXT, needs_edit INTEGER NOT NULL DEFAULT 0 CHECK (needs_edit IN (0, 1)), former_handles TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(former_handles) AND json_type(former_handles) = 'array'),
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'tester', 'designer', 'frontend', 'architect', 'researcher', 'integrator')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO participant
    (id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until,
     out_since, created_at, left_at, switched_from_harness, switched_from_agent,
     switched_from_cut, designer, model, switched_from_model, launched_model, gone_model,
     effort, model_name, needs_edit, former_handles)
    SELECT id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until,
           out_since, created_at, left_at, switched_from_harness, switched_from_agent,
           switched_from_cut, designer, model, switched_from_model, launched_model,
           gone_model, effort, model_name, needs_edit, former_handles
    FROM participant_rows;
  DROP TABLE participant_rows;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);
  CREATE INDEX participant_history_index ON participant (project_id, handle, left_at IS NULL, role, agent);

  CREATE TEMP TABLE task_rows AS SELECT * FROM task;
  DROP TABLE task;
  CREATE TABLE task (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    requester_id INTEGER NOT NULL,
    assignee_id INTEGER,
    state TEXT NOT NULL,
    pool TEXT,
    tier TEXT,
    purpose TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL, taken_from_id INTEGER
    CONSTRAINT task_taken_from_fk REFERENCES participant (id) ON DELETE SET NULL, held_until TEXT, deleted_at TEXT, paused_at TEXT, stop_seq INTEGER NOT NULL DEFAULT 0, paused_with_project INTEGER NOT NULL DEFAULT 0 CONSTRAINT task_paused_with_project_check CHECK (paused_with_project IN (0, 1)),
    CONSTRAINT task_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT task_requester_fk FOREIGN KEY (requester_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_assignee_fk FOREIGN KEY (assignee_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_number_unique UNIQUE (project_id, number),
    CONSTRAINT task_state_check CHECK (
      state IN ('open', 'queued', 'working', 'waiting', 'paused', 'done', 'accepted', 'failed', 'cancelled')
    ),
    CONSTRAINT task_assignee_check
      CHECK (assignee_id IS NOT NULL OR state IN ('open', 'paused', 'cancelled', 'failed')),
    CONSTRAINT task_pool_check
      CHECK (pool IS NULL OR pool IN ('worker', 'advisor', 'reviewer', 'tester', 'designer', 'frontend', 'architect', 'researcher', 'integrator')),
    CONSTRAINT task_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO task
    (id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier,
     purpose, created_at, updated_at, taken_from_id, held_until, deleted_at, paused_at,
     stop_seq, paused_with_project)
    SELECT id, project_id, number, title, body, requester_id, assignee_id, state, pool,
           tier, purpose, created_at, updated_at, taken_from_id, held_until, deleted_at,
           paused_at, stop_seq, paused_with_project
    FROM task_rows;
  DROP TABLE task_rows;
  CREATE INDEX task_assignee_index ON task (assignee_id, state);

  DELETE FROM sqlite_sequence;
  INSERT INTO sqlite_sequence (name, seq) SELECT name, seq FROM sequence_rows ORDER BY at;
  DROP TABLE sequence_rows;
