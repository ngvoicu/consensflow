-- A question a restart or a project Close closed, while its task still waits
-- for new words, is reminded (Q2). One row per such wait:
--   after_id     the close's place in the ledger's own message order (the
--                last message id then): the chief has acted if it sent that
--                member words on that task with a higher id. A wait an earlier
--                build left takes the question's own id, the conservative place.
--   note_id      the one note the chief was given (T-1859), the first linked.
--   first_read_at the note's earliest inbox read while its paste was on its way.
--   reminder_id  the closed question's own reminder cursor; the open question's
--                (question_reminder) is not reused.
CREATE TABLE question_wait (
  question_id INTEGER PRIMARY KEY REFERENCES message(id),
  after_id INTEGER NOT NULL,
  note_id INTEGER REFERENCES message(id),
  first_read_at TEXT,
  reminder_id INTEGER REFERENCES message(id)
);
CREATE INDEX question_wait_note ON question_wait(note_id);
CREATE INDEX question_wait_reminder ON question_wait(reminder_id);

-- The waits an earlier build left (`closed = 2`): a row only where the ledger
-- itself names the note, by the answer's own reason (`returned in m-N`), or by
-- ConsensFlow's own sentence for that question and task, to the chief.
WITH named AS (
  SELECT b.question_id AS question_id,
         COALESCE(
           (SELECT n.id FROM message a JOIN message n ON n.id = CAST(substr(a.reason, 15) AS INTEGER)
              WHERE a.reply_to = b.question_id AND a.kind = 'answer' AND a.reason LIKE 'returned in m-%'
                AND n.kind = 'note' AND n.project_id = q.project_id
              ORDER BY a.id LIMIT 1),
           (SELECT n.id FROM message n JOIN participant c ON c.id = n.recipient_id
              WHERE n.kind = 'note' AND n.project_id = q.project_id AND c.role = 'chief'
                AND instr(n.body, 'question m-' || b.question_id || ' on T-' || t.number || ' closed when ') > 0
              ORDER BY n.id LIMIT 1)) AS note
  FROM question_binding b
  JOIN message q ON q.id = b.question_id
  JOIN task t ON t.id = b.task_id
  WHERE b.closed = 2)
INSERT INTO question_wait(question_id, after_id, note_id)
SELECT question_id, question_id, note FROM named WHERE note IS NOT NULL;
