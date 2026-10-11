-- The tester joins the roles a member holds and the pools a task is for.
-- SQLite cannot widen a CHECK where it stands, so the two tables that name
-- them are rebuilt as 0006 rebuilt them, every row and column as it was, and
-- the sequences that keep a deleted id from coming back are put back as they
-- were, in the order they were.
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
    left_at TEXT, switched_from_harness TEXT, switched_from_agent TEXT, switched_from_cut INTEGER NOT NULL DEFAULT 0, designer INTEGER NOT NULL DEFAULT 0,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'tester', 'designer')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO participant
    (id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since,
     created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer)
    SELECT id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since,
           created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer
    FROM participant_rows;
  DROP TABLE participant_rows;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);

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
    CONSTRAINT task_taken_from_fk REFERENCES participant (id) ON DELETE SET NULL, held_until TEXT, deleted_at TEXT, paused_at TEXT, stop_seq INTEGER NOT NULL DEFAULT 0,
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
      CHECK (pool IS NULL OR pool IN ('worker', 'advisor', 'reviewer', 'tester', 'designer')),
    CONSTRAINT task_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO task
    (id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier, purpose,
     created_at, updated_at, taken_from_id, held_until, deleted_at, paused_at, stop_seq)
    SELECT id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier, purpose,
           created_at, updated_at, taken_from_id, held_until, deleted_at, paused_at, stop_seq
    FROM task_rows;
  DROP TABLE task_rows;
  CREATE INDEX task_assignee_index ON task (assignee_id, state);

  DELETE FROM sqlite_sequence;
  INSERT INTO sqlite_sequence (name, seq) SELECT name, seq FROM sequence_rows ORDER BY at;
  DROP TABLE sequence_rows;
