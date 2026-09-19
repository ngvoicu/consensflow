/**
 * The ledger's schema, as an ordered list of migrations. Migration `n` takes
 * the database from `PRAGMA user_version = n` to `n + 1`; the version is the
 * list's length. A migration is never edited once it has shipped: a change is
 * a new entry at the end.
 */
export const MIGRATIONS = [
  `
  CREATE TABLE session (
    id INTEGER PRIMARY KEY,
    directory TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    resume_on_start INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT session_state_check CHECK (state IN ('open', 'suspended')),
    CONSTRAINT session_resume_on_start_check CHECK (resume_on_start IN (0, 1))
  ) STRICT;

  CREATE TABLE participant (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    agent TEXT,
    harness TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT participant_session_fk FOREIGN KEY (session_id)
      REFERENCES session (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (session_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'lead', 'pm', 'advisor', 'worker', 'reviewer'))
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
    session_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    requester_id INTEGER NOT NULL,
    assignee_id INTEGER NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT task_session_fk FOREIGN KEY (session_id)
      REFERENCES session (id) ON DELETE CASCADE,
    CONSTRAINT task_requester_fk FOREIGN KEY (requester_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_assignee_fk FOREIGN KEY (assignee_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT task_number_unique UNIQUE (session_id, number),
    CONSTRAINT task_state_check CHECK (
      state IN ('queued', 'working', 'waiting', 'done', 'accepted', 'failed', 'cancelled')
    )
  ) STRICT;
  CREATE INDEX task_assignee_index ON task (assignee_id, state);

  CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL,
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
    CONSTRAINT message_session_fk FOREIGN KEY (session_id)
      REFERENCES session (id) ON DELETE CASCADE,
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
      state IN ('queued', 'delivering', 'delivered', 'read', 'failed', 'cancelled')
    )
  ) STRICT;
  CREATE INDEX message_queue_index ON message (recipient_id, state, id);
  CREATE INDEX message_task_index ON message (task_id, id);
  CREATE UNIQUE INDEX message_delivering_unique
    ON message (recipient_id) WHERE state = 'delivering';

  CREATE TABLE event (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    data TEXT NOT NULL,
    CONSTRAINT event_session_fk FOREIGN KEY (session_id)
      REFERENCES session (id) ON DELETE CASCADE
  ) STRICT;
  CREATE INDEX event_session_index ON event (session_id, id);
  `,
]

export const SCHEMA_VERSION = MIGRATIONS.length
