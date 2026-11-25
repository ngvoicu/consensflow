-- A question a restart or a project Close closed, while its task still waits
-- for new words, is reminded (Q2). One row per such wait:
--   after_id       the close's place in the ledger's own message order (the
--                  last message id then): the chief has acted if it sent that
--                  member words on that task with a higher id. A wait an earlier
--                  build left takes the question's own id, the conservative place.
--   note_id        the one note the chief was given (T-1859), the first linked.
--   first_read_at  the note's earliest inbox read while its paste was on its way.
--   from_update_at the clock of a wait whose note the ledger names nowhere: the
--                  update's own time.
--   reminder_id    the closed question's own reminder cursor; the open
--                  question's (question_reminder) is not reused.
CREATE TABLE question_wait (
  question_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  after_id INTEGER NOT NULL,
  note_id INTEGER REFERENCES message(id) ON DELETE SET NULL,
  first_read_at TEXT,
  from_update_at TEXT,
  reminder_id INTEGER REFERENCES message(id) ON DELETE SET NULL
);
CREATE INDEX question_wait_note ON question_wait(note_id);
CREATE INDEX question_wait_reminder ON question_wait(reminder_id);

-- Every reminder written, of an open question or a closed one: a cursor above
-- names only the last, and the inbox must know each to keep a withdrawn one.
CREATE TABLE question_reminder_sent (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);
INSERT INTO question_reminder_sent(message_id)
SELECT m.id FROM question_reminder r JOIN message m ON m.id = r.reminder_id;

-- The waits an earlier build left (`closed = 2`). Its note is known only by a
-- reference the ledger holds: a returned answer's reason, `returned in m-N`,
-- written with note m-N in one write. No note's words are read. Where there is
-- no such reference, the clock starts now.
WITH waits AS (
  SELECT b.question_id AS question_id,
         (SELECT n.id FROM message a
            JOIN message n ON n.id = CAST(substr(a.reason, 15) AS INTEGER)
              AND a.reason = 'returned in m-' || n.id
           WHERE a.reply_to = b.question_id AND a.kind = 'answer'
             AND n.kind = 'note' AND n.sender_id IS NULL AND n.project_id = q.project_id
           ORDER BY a.id LIMIT 1) AS note
  FROM question_binding b
  JOIN message q ON q.id = b.question_id AND q.kind = 'question'
  WHERE b.closed = 2)
INSERT INTO question_wait(question_id, after_id, note_id, from_update_at)
SELECT question_id, question_id, note,
       CASE WHEN note IS NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END
FROM waits;
