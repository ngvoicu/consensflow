-- The fact of a hold made for a retry (D3's N35): a turn that failed holds its task and main's
-- own Resume tries it again. One row for each such hold, written with the hold and never
-- changed but for the message that carried its Resume, once. The tries a task has used in a
-- row are a number on the task, `turn_retries`: it goes up by one in the write that queues a
-- numbered Resume and back to zero on exactly three things: a person's or the chief's action
-- on the task, its result, and a failure 30 minutes or more after `turn_retry_at`, the last
-- moment anything happened to a numbered retry of the task. Every task that exists reads zero.
CREATE TABLE turn_retry (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
  session_id INTEGER NOT NULL REFERENCES participant(id) ON DELETE CASCADE,
  -- What turn it was for: `failed` (`refused` is D3's R2).
  kind TEXT NOT NULL CHECK (kind IN ('failed', 'refused')),
  -- Which try the hold is for: 1 or 2.
  try INTEGER NOT NULL CHECK (try BETWEEN 1 AND 2),
  -- What the turn said and why it failed, as the engine read it.
  said TEXT NOT NULL,
  cause TEXT NOT NULL,
  -- The message that carried its Resume, once main's pass resumed the task.
  message_id INTEGER REFERENCES message(id) ON DELETE SET NULL
);
CREATE INDEX turn_retry_by_task ON turn_retry (task_id, id);
CREATE INDEX turn_retry_by_message ON turn_retry (message_id);
ALTER TABLE task ADD COLUMN turn_retries INTEGER NOT NULL DEFAULT 0 CHECK (turn_retries BETWEEN 0 AND 2);
-- The retry hold the task is in now: the `turn_retry.id` written with it, none once it ends.
-- It names the hold itself; no time says which hold a Resume belongs to.
ALTER TABLE task ADD COLUMN retry_hold INTEGER;
-- The last moment anything happened to a numbered retry of the task: one was charged (queued),
-- delivered or withdrawn. Written in the same write as its cause, and by nothing else: the
-- charge writes it in the ledger's code, a delivery of the message that carries a numbered Resume
-- writes it through the trigger below with the time of its delivery, and a withdrawal with an
-- empty mark, settled to the write's own time when the write ends, whichever path moves the message.
ALTER TABLE task ADD COLUMN turn_retry_at TEXT;
CREATE TRIGGER turn_retry_message_moved AFTER UPDATE OF state ON message
WHEN OLD.state NOT IN ('delivered', 'read') AND NEW.state IN ('delivered', 'read', 'cancelled', 'failed')
BEGIN
  UPDATE task SET turn_retry_at = CASE WHEN NEW.state IN ('delivered', 'read') THEN COALESCE(NEW.delivered_at, '') ELSE '' END
   WHERE id IN (SELECT task_id FROM turn_retry WHERE message_id = NEW.id);
END;
