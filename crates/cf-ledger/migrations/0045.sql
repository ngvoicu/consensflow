-- The fact of a hold made for a retry (D3's N35): a turn that failed holds its task and main's
-- own Resume tries it again. One row for each such hold, written with the hold and never
-- changed but for the message that carried its Resume, once. It has no state to end: the
-- tries a window has used are read from the message last pasted into it, and a row goes only
-- with its task or its session.
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
  -- The hold it belongs to: the task's pause time (`task.paused_at`) the hold wrote.
  paused_at TEXT NOT NULL,
  -- The message that carried its Resume, once main's pass resumed the task.
  message_id INTEGER REFERENCES message(id) ON DELETE SET NULL
);
CREATE INDEX turn_retry_by_task ON turn_retry (task_id, id);
CREATE INDEX turn_retry_by_message ON turn_retry (message_id);
