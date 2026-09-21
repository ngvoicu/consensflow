/**
 * The ledger's schema, as an ordered list of migrations. Migration `n` takes
 * the database from `PRAGMA user_version = n` to `n + 1`; the version is the
 * list's length. A migration is never edited once it has shipped: a change is
 * a new entry at the end. A migration is SQL run inside one transaction, or a
 * function that manages its own (a table rebuild must switch foreign keys off
 * first, which no transaction allows).
 */
export const MIGRATIONS = [
  `
  CREATE TABLE project (
    id INTEGER PRIMARY KEY,
    directory TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    resume_on_start INTEGER NOT NULL DEFAULT 0,
    review TEXT NOT NULL DEFAULT 'members',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT project_state_check CHECK (state IN ('open', 'suspended')),
    CONSTRAINT project_resume_on_start_check CHECK (resume_on_start IN (0, 1)),
    CONSTRAINT project_review_check CHECK (review IN ('none', 'members', 'all'))
  ) STRICT;

  CREATE TABLE participant (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    agent TEXT,
    harness TEXT,
    created_at TEXT NOT NULL,
    left_at TEXT,
    tier TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    out_until TEXT,
    out_since TEXT,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'lead', 'pm', 'advisor', 'worker', 'reviewer')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;

  CREATE TABLE conversation (
    id INTEGER PRIMARY KEY,
    participant_id INTEGER NOT NULL,
    harness TEXT NOT NULL,
    native_session TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CONSTRAINT conversation_participant_fk FOREIGN KEY (participant_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT conversation_native_session_unique UNIQUE (harness, native_session)
  ) STRICT;
  CREATE UNIQUE INDEX conversation_current_unique
    ON conversation (participant_id) WHERE ended_at IS NULL;

  CREATE TABLE task (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    requester_id INTEGER NOT NULL,
    assignee_id INTEGER,
    state TEXT NOT NULL,
    pool TEXT,
    tier TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    purpose TEXT,
    kind TEXT NOT NULL DEFAULT 'work',
    review_of INTEGER,
    round INTEGER NOT NULL DEFAULT 0,
    verdict TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT task_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT task_requester_fk FOREIGN KEY (requester_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_assignee_fk FOREIGN KEY (assignee_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_number_unique UNIQUE (project_id, number),
    CONSTRAINT task_review_of_fk FOREIGN KEY (review_of)
      REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT task_state_check CHECK (
      state IN ('open', 'queued', 'working', 'waiting', 'review', 'done', 'accepted', 'failed', 'cancelled')
    ),
    CONSTRAINT task_assignee_check
      CHECK (assignee_id IS NOT NULL OR state IN ('open', 'cancelled', 'failed')),
    CONSTRAINT task_pool_check CHECK (pool IS NULL OR pool IN ('worker', 'advisor')),
    CONSTRAINT task_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light')),
    CONSTRAINT task_kind_check CHECK (kind IN ('work', 'review')),
    CONSTRAINT task_verdict_check CHECK (verdict IS NULL OR verdict IN ('pass', 'changes'))
  ) STRICT;
  CREATE INDEX task_assignee_index ON task (assignee_id, state);

  CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    recipient_id INTEGER NOT NULL,
    sender_id INTEGER,
    kind TEXT NOT NULL,
    task_id INTEGER,
    reply_to INTEGER,
    body TEXT NOT NULL,
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    receipt TEXT,
    created_at TEXT NOT NULL,
    delivered_at TEXT,
    CONSTRAINT message_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT message_recipient_fk FOREIGN KEY (recipient_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT message_sender_fk FOREIGN KEY (sender_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT message_task_fk FOREIGN KEY (task_id)
      REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT message_reply_to_fk FOREIGN KEY (reply_to)
      REFERENCES message (id) ON DELETE CASCADE,
    CONSTRAINT message_kind_check
      CHECK (kind IN ('task', 'result', 'question', 'answer', 'note')),
    CONSTRAINT message_state_check CHECK (
      state IN ('queued', 'delivering', 'delivered', 'read', 'held', 'failed', 'cancelled')
    )
  ) STRICT;
  CREATE INDEX message_queue_index ON message (recipient_id, state, id);
  CREATE INDEX message_task_index ON message (task_id, id);
  CREATE UNIQUE INDEX message_delivering_unique
    ON message (recipient_id) WHERE state = 'delivering';

  CREATE TABLE event (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    data TEXT NOT NULL,
    CONSTRAINT event_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE
  ) STRICT;
  CREATE INDEX event_project_index ON event (project_id, id);
  `,
  `
  -- A member holds a set of roles (Phase C); a task remembers why it went
  -- unreviewed; a question may carry its options and its answer the choices.
  ALTER TABLE participant ADD COLUMN roles TEXT NOT NULL DEFAULT '[]';
  UPDATE participant SET roles = json_array(role) WHERE role IN ('worker', 'advisor', 'reviewer');
  ALTER TABLE task ADD COLUMN unreviewed TEXT;
  ALTER TABLE message ADD COLUMN questions TEXT;
  ALTER TABLE message ADD COLUMN choices TEXT;
  `,
  `
  -- A session is a member's named window (Phase B of candidate-feedback-2):
  -- it points at its member. ADD COLUMN cannot name a constraint in SQLite,
  -- so this foreign key is the one unnamed constraint in the schema.
  ALTER TABLE participant ADD COLUMN member_id INTEGER REFERENCES participant (id) ON DELETE CASCADE;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);
  `,
  `
  -- The PM and tags go (candidate-feedback-3): the lead is the only
  -- coordinator, and a task finds its member by tier alone. A PM's row goes
  -- with everything it asked for and was told (the cascades). The role
  -- constraint keeps 'pm' in its list: changing it means rebuilding the
  -- table, and dropping a table under foreign keys cascades into every other.
  DELETE FROM participant WHERE role = 'pm';
  ALTER TABLE participant DROP COLUMN tags;
  ALTER TABLE task DROP COLUMN tags;
  `,
  `
  -- The "all work" review policy goes (candidate-feedback-3): a project reviews
  -- its workers' work or nothing; the lead asks for a review of its own by
  -- hand. The review constraint keeps 'all' in its list for the reason the
  -- role constraint keeps 'pm'.
  UPDATE project SET review = 'members' WHERE review = 'all';
  `,
  // The image designer joins the roles (candidate-feedback-3), and the
  // constraints catch up with everything that left: no 'pm' role, no 'all'
  // policy, and a task pool may be 'designer'.
  rebuilding(`
        CREATE TABLE project_next (
          id INTEGER PRIMARY KEY,
          directory TEXT NOT NULL,
          name TEXT NOT NULL,
          state TEXT NOT NULL,
          resume_on_start INTEGER NOT NULL DEFAULT 0,
          review TEXT NOT NULL DEFAULT 'members',
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          CONSTRAINT project_state_check CHECK (state IN ('open', 'suspended')),
          CONSTRAINT project_resume_on_start_check CHECK (resume_on_start IN (0, 1)),
          CONSTRAINT project_review_check CHECK (review IN ('none', 'members'))
        ) STRICT;
        INSERT INTO project_next SELECT id, directory, name, state, resume_on_start, review, created_at, updated_at FROM project;
        DROP TABLE project;
        ALTER TABLE project_next RENAME TO project;

        CREATE TABLE participant_next (
          id INTEGER PRIMARY KEY,
          project_id INTEGER NOT NULL,
          handle TEXT NOT NULL,
          role TEXT NOT NULL,
          agent TEXT,
          harness TEXT,
          created_at TEXT NOT NULL,
          left_at TEXT,
          tier TEXT,
          out_until TEXT,
          out_since TEXT,
          roles TEXT NOT NULL DEFAULT '[]',
          member_id INTEGER,
          CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
            REFERENCES project (id) ON DELETE CASCADE,
          CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
            REFERENCES participant (id) ON DELETE CASCADE,
          CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
          CONSTRAINT participant_role_check
            CHECK (role IN ('human', 'lead', 'advisor', 'worker', 'reviewer', 'designer')),
          CONSTRAINT participant_tier_check
            CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
        ) STRICT;
        INSERT INTO participant_next
          SELECT id, project_id, handle, role, agent, harness, created_at, left_at, tier,
                 out_until, out_since, roles, member_id
          FROM participant;
        DROP TABLE participant;
        ALTER TABLE participant_next RENAME TO participant;
        CREATE INDEX participant_member_index ON participant (member_id, left_at);

        CREATE TABLE task_next (
          id INTEGER PRIMARY KEY,
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
          kind TEXT NOT NULL DEFAULT 'work',
          review_of INTEGER,
          round INTEGER NOT NULL DEFAULT 0,
          verdict TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          unreviewed TEXT,
          CONSTRAINT task_project_fk FOREIGN KEY (project_id)
            REFERENCES project (id) ON DELETE CASCADE,
          CONSTRAINT task_requester_fk FOREIGN KEY (requester_id)
            REFERENCES participant (id) ON DELETE CASCADE,
          CONSTRAINT task_assignee_fk FOREIGN KEY (assignee_id)
            REFERENCES participant (id) ON DELETE CASCADE,
          CONSTRAINT task_number_unique UNIQUE (project_id, number),
          CONSTRAINT task_review_of_fk FOREIGN KEY (review_of)
            REFERENCES task (id) ON DELETE CASCADE,
          CONSTRAINT task_state_check CHECK (
            state IN ('open', 'queued', 'working', 'waiting', 'review', 'done', 'accepted', 'failed', 'cancelled')
          ),
          CONSTRAINT task_assignee_check
            CHECK (assignee_id IS NOT NULL OR state IN ('open', 'cancelled', 'failed')),
          CONSTRAINT task_pool_check CHECK (pool IS NULL OR pool IN ('worker', 'advisor', 'designer')),
          CONSTRAINT task_tier_check
            CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light')),
          CONSTRAINT task_kind_check CHECK (kind IN ('work', 'review')),
          CONSTRAINT task_verdict_check CHECK (verdict IS NULL OR verdict IN ('pass', 'changes'))
        ) STRICT;
        INSERT INTO task_next
          SELECT id, project_id, number, title, body, requester_id, assignee_id, state, pool, tier,
                 purpose, kind, review_of, round, verdict, created_at, updated_at, unreviewed
          FROM task;
        DROP TABLE task;
        ALTER TABLE task_next RENAME TO task;
        CREATE INDEX task_assignee_index ON task (assignee_id, state);
  `),
  // Human approval required (candidate-feedback-3, Phase C): a project may
  // gate every message between two agents, and such a message waits in a
  // state of its own until the human passes it on or declines it.
  rebuilding(`
        ALTER TABLE project ADD COLUMN gate INTEGER NOT NULL DEFAULT 0
          CONSTRAINT project_gate_check CHECK (gate IN (0, 1));

        CREATE TABLE message_next (
          id INTEGER PRIMARY KEY,
          project_id INTEGER NOT NULL,
          recipient_id INTEGER NOT NULL,
          sender_id INTEGER,
          kind TEXT NOT NULL,
          task_id INTEGER,
          reply_to INTEGER,
          body TEXT NOT NULL,
          state TEXT NOT NULL,
          attempts INTEGER NOT NULL DEFAULT 0,
          reason TEXT,
          receipt TEXT,
          created_at TEXT NOT NULL,
          delivered_at TEXT,
          questions TEXT,
          choices TEXT,
          CONSTRAINT message_project_fk FOREIGN KEY (project_id)
            REFERENCES project (id) ON DELETE CASCADE,
          CONSTRAINT message_recipient_fk FOREIGN KEY (recipient_id)
            REFERENCES participant (id) ON DELETE CASCADE,
          CONSTRAINT message_sender_fk FOREIGN KEY (sender_id)
            REFERENCES participant (id) ON DELETE CASCADE,
          CONSTRAINT message_task_fk FOREIGN KEY (task_id)
            REFERENCES task (id) ON DELETE CASCADE,
          CONSTRAINT message_reply_to_fk FOREIGN KEY (reply_to)
            REFERENCES message (id) ON DELETE CASCADE,
          CONSTRAINT message_kind_check
            CHECK (kind IN ('task', 'result', 'question', 'answer', 'note')),
          CONSTRAINT message_state_check CHECK (
            state IN ('queued', 'delivering', 'delivered', 'read', 'held', 'gated', 'failed', 'cancelled')
          )
        ) STRICT;
        INSERT INTO message_next
          SELECT id, project_id, recipient_id, sender_id, kind, task_id, reply_to, body, state,
                 attempts, reason, receipt, created_at, delivered_at, questions, choices
          FROM message;
        DROP TABLE message;
        ALTER TABLE message_next RENAME TO message;
        CREATE INDEX message_queue_index ON message (recipient_id, state, id);
        CREATE INDEX message_task_index ON message (task_id, id);
        CREATE UNIQUE INDEX message_delivering_unique
          ON message (recipient_id) WHERE state = 'delivering';
  `),
  `
  -- A plan on the board: a task may need other tasks accepted before the
  -- daemon gives it out. One row per need; both ends go with their task.
  CREATE TABLE task_need (
    task_id INTEGER NOT NULL,
    needs_id INTEGER NOT NULL,
    CONSTRAINT task_need_pk PRIMARY KEY (task_id, needs_id),
    CONSTRAINT task_need_task_fk FOREIGN KEY (task_id) REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT task_need_needs_fk FOREIGN KEY (needs_id) REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT task_need_self_check CHECK (task_id != needs_id)
  ) STRICT;
  CREATE INDEX task_need_needs_index ON task_need (needs_id);
  `,
  `
  -- ConsensFlow's own copy of each window's conversation (Phase G), kept in
  -- the home with everything else: what the agent was told, wrote and got
  -- back from its tools, one row per item, updated while an item is still
  -- being written. It lives and dies with its conversation's project.
  CREATE TABLE transcript (
    conversation_id INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    complete INTEGER NOT NULL DEFAULT 1,
    at TEXT,
    copied_at TEXT NOT NULL,
    CONSTRAINT transcript_pk PRIMARY KEY (conversation_id, item_id),
    CONSTRAINT transcript_conversation_fk FOREIGN KEY (conversation_id)
      REFERENCES conversation (id) ON DELETE CASCADE,
    CONSTRAINT transcript_role_check CHECK (role IN ('user', 'assistant', 'tool', 'custom')),
    CONSTRAINT transcript_complete_check CHECK (complete IN (0, 1))
  ) STRICT;
  CREATE INDEX transcript_order_index ON transcript (conversation_id, seq);
  `,
]

/**
 * A migration that changes what a CHECK allows: SQLite cannot alter one in
 * place, so the table is rebuilt the way SQLite documents it: foreign keys
 * off, copy into a new table, drop the old, rename, check, foreign keys on.
 */
function rebuilding(sql) {
  return (db, version) => {
    db.exec('PRAGMA foreign_keys = OFF')
    try {
      db.exec('BEGIN IMMEDIATE')
      try {
        db.exec(sql)
        const dangling = db.prepare('PRAGMA foreign_key_check').all()
        if (dangling.length > 0) {
          throw new Error(`the rebuild left ${dangling.length} rows without their parent`)
        }
        db.exec(`PRAGMA user_version = ${version}`)
        db.exec('COMMIT')
      } catch (cause) {
        db.exec('ROLLBACK')
        throw cause
      }
    } finally {
      db.exec('PRAGMA foreign_keys = ON')
    }
  }
}

export const SCHEMA_VERSION = MIGRATIONS.length
