import { LedgerError } from './model.js'

/**
 * The ledger's schema, as an ordered list of migrations. Migration `n` takes
 * the database from `PRAGMA user_version = n` to `n + 1`; the version is the
 * list's length. A migration is SQL run inside one transaction, and it is
 * never edited once it has shipped: a change is a new entry at the end.
 *
 * The list starts afresh on 2026-09-21: every home was cleaned for the
 * simplified model (reviews are ordinary tasks), and going live starts from a
 * clean home too, so no earlier schema needs a way up.
 */
export const MIGRATIONS = [
  `
  CREATE TABLE project (
    id INTEGER PRIMARY KEY,
    directory TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    resume_on_start INTEGER NOT NULL DEFAULT 0,
    gate INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT project_state_check CHECK (state IN ('open', 'suspended')),
    CONSTRAINT project_resume_on_start_check CHECK (resume_on_start IN (0, 1)),
    CONSTRAINT project_gate_check CHECK (gate IN (0, 1))
  ) STRICT;

  CREATE TABLE participant (
    id INTEGER PRIMARY KEY,
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
    left_at TEXT,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'designer')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);

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
    purpose TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
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
      CHECK (pool IS NULL OR pool IN ('worker', 'advisor', 'reviewer', 'designer')),
    CONSTRAINT task_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  CREATE INDEX task_assignee_index ON task (assignee_id, state);

  CREATE TABLE task_need (
    task_id INTEGER NOT NULL,
    needs_id INTEGER NOT NULL,
    CONSTRAINT task_need_pk PRIMARY KEY (task_id, needs_id),
    CONSTRAINT task_need_task_fk FOREIGN KEY (task_id) REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT task_need_needs_fk FOREIGN KEY (needs_id) REFERENCES task (id) ON DELETE CASCADE,
    CONSTRAINT task_need_self_check CHECK (task_id != needs_id)
  ) STRICT;
  CREATE INDEX task_need_needs_index ON task_need (needs_id);

  CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    recipient_id INTEGER NOT NULL,
    sender_id INTEGER,
    kind TEXT NOT NULL,
    task_id INTEGER,
    reply_to INTEGER,
    body TEXT NOT NULL,
    questions TEXT,
    choices TEXT,
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
      state IN ('queued', 'delivering', 'delivered', 'read', 'gated', 'failed', 'cancelled')
    )
  ) STRICT;
  CREATE INDEX message_queue_index ON message (recipient_id, state, id);
  CREATE INDEX message_task_index ON message (task_id, id);
  CREATE UNIQUE INDEX message_delivering_unique
    ON message (recipient_id) WHERE state = 'delivering';

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
  // The member a task was last taken back from (out of quota, or reassigned
  // by the human), so the daemon gives it to another member of its tier.
  `
  ALTER TABLE task ADD COLUMN taken_from_id INTEGER
    CONSTRAINT task_taken_from_fk REFERENCES participant (id) ON DELETE SET NULL;
  `,
  // A task held with its window while its member is out of quota: when it
  // goes on by itself. Cleared by any move.
  `
  ALTER TABLE task ADD COLUMN held_until TEXT;
  `,
  // A question put to a task's window with the task paused for it (the
  // chief's `cf tell`); the delivered text says so.
  `
  ALTER TABLE message ADD COLUMN urgent INTEGER NOT NULL DEFAULT 0;
  `,
  // The lead is the Chief of Staff (2026-09-25): the role and the handle say
  // `chief`. A ledger written before then carries `lead` rows and a check
  // that refuses `chief`, so the table is rebuilt, ids and references kept.
  `
  CREATE TABLE participant_chief (
    id INTEGER PRIMARY KEY,
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
    left_at TEXT,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'designer')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO participant_chief
    SELECT id, project_id,
           CASE handle WHEN 'lead' THEN 'chief' ELSE handle END,
           CASE role WHEN 'lead' THEN 'chief' ELSE role END,
           roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at
    FROM participant;
  DROP TABLE participant;
  ALTER TABLE participant_chief RENAME TO participant;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);
  `,
]

export const SCHEMA_VERSION = MIGRATIONS.length

/** Takes a ledger from the version it was written at to SCHEMA_VERSION; a newer one is refused. */
export function migrate(db) {
  const version = db.prepare('PRAGMA user_version').get().user_version
  if (version > SCHEMA_VERSION) {
    throw new LedgerError(
      'ledger-newer',
      `this home was written by a newer ConsensFlow (schema ${version}; this build knows ${SCHEMA_VERSION})`,
      409,
    )
  }
  if (version === SCHEMA_VERSION) return
  // A migration may rebuild a table others refer to; with foreign keys on,
  // dropping it would cascade through them. Off for the migrations, every
  // reference checked before each one commits, then on again: a start
  // refused here leaves the version as it was, so the next start checks too.
  db.exec('PRAGMA foreign_keys = OFF')
  try {
    for (let from = version; from < SCHEMA_VERSION; from += 1) {
      db.exec('BEGIN IMMEDIATE')
      try {
        db.exec(MIGRATIONS[from])
        db.exec(`PRAGMA user_version = ${from + 1}`)
        const broken = db.prepare('PRAGMA foreign_key_check').all()
        if (broken.length > 0) {
          throw new LedgerError(
            'ledger-broken',
            `the ledger's references do not hold after migration: ${JSON.stringify(broken[0])}`,
            500,
          )
        }
        db.exec('COMMIT')
      } catch (cause) {
        db.exec('ROLLBACK')
        throw cause
      }
    }
  } finally {
    db.exec('PRAGMA foreign_keys = ON')
  }
}
