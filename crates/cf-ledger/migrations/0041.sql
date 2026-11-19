-- What a question's reminders need across a restart. A question can be read
-- in the inbox while its paste is still settling, before delivered_at is set.
CREATE TABLE question_reminder (
    question_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
    first_read_at TEXT,
    reminder_id INTEGER REFERENCES message(id) ON DELETE SET NULL,
    ended INTEGER NOT NULL DEFAULT 0 CHECK (ended IN (0, 1))
);
CREATE UNIQUE INDEX question_reminder_by_message ON question_reminder(reminder_id);
CREATE INDEX message_questions_for_chief ON message(project_id, kind, state);
-- A dispatcher pass checks for an answer to every received question. Without
-- this, each check scans the entire message table, including old projects.
CREATE INDEX message_answers_by_question ON message(reply_to) WHERE kind = 'answer';
