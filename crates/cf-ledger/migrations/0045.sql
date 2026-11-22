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
  question_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  after_id INTEGER NOT NULL,
  note_id INTEGER REFERENCES message(id) ON DELETE SET NULL,
  first_read_at TEXT,
  reminder_id INTEGER REFERENCES message(id) ON DELETE SET NULL
);
CREATE INDEX question_wait_note ON question_wait(note_id);
CREATE INDEX question_wait_reminder ON question_wait(reminder_id);

-- The waits an earlier build left (`closed = 2`): a row only where the ledger
-- itself names the note, as that build wrote it, and names it once:
--  - a returned answer's note, by the answer's own reason (`returned in m-N`),
--    whose first line is the writer's (`Your answer m-A to m-Q did not reach its
--    window.`), written by ConsensFlow itself;
--  - the closed-question note, whose first line is the writer's sentence for that
--    member, question and task (`@handle's question m-Q on T-N closed when `),
--    taskless, to the chief, from ConsensFlow or, at the gate, from the member.
-- A member's own words quoting either sentence are neither; two notes that both
-- fit name nothing.
WITH named AS (
  SELECT b.question_id AS question_id,
         COALESCE(
           (SELECT n.id FROM message a JOIN message n ON n.id = CAST(substr(a.reason, 15) AS INTEGER)
              WHERE a.reply_to = b.question_id AND a.kind = 'answer' AND a.reason LIKE 'returned in m-%'
                AND n.kind = 'note' AND n.project_id = q.project_id AND n.sender_id IS NULL
                AND n.reply_to IS NULL
                AND instr(n.body, 'Your answer m-' || a.id || ' to m-' || q.id || ' did not reach its window.') = 1
              ORDER BY a.id LIMIT 1),
           (SELECT CASE WHEN count(*) = 1 THEN min(n.id) END
              FROM message n JOIN participant c ON c.id = n.recipient_id
              WHERE n.kind = 'note' AND n.project_id = q.project_id AND c.role = 'chief'
                AND n.task_id IS NULL AND n.reply_to IS NULL AND n.urgent = 0 AND n.id > q.id
                AND (n.sender_id IS NULL OR n.sender_id = q.sender_id)
                AND instr(n.body, '@' || s.handle || '''s question m-' || q.id || ' on T-' || t.number || ' closed when ') = 1)) AS note
  FROM question_binding b
  JOIN message q ON q.id = b.question_id
  JOIN participant s ON s.id = q.sender_id
  JOIN task t ON t.id = b.task_id
  WHERE b.closed = 2)
INSERT INTO question_wait(question_id, after_id, note_id)
SELECT question_id, question_id, note FROM named WHERE note IS NOT NULL;
