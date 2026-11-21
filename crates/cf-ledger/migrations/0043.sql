-- A closed project-window question still holds its task until new work is
-- evidenced. This boundary contains only a native record item id, never its
-- words. Existing bindings have no inferred boundary.
ALTER TABLE question_binding ADD COLUMN wait_conversation_id INTEGER REFERENCES conversation(id) ON DELETE SET NULL;
ALTER TABLE question_binding ADD COLUMN wait_last_item TEXT;
CREATE INDEX question_binding_wait ON question_binding(participant_id, task_id, closed) WHERE closed IN (2, 3);
-- A confirmation can arrive after Close for a paste begun in the old window.
-- Keep the event boundary so only a later paste releases the wait.
ALTER TABLE question_binding ADD COLUMN wait_event_id INTEGER;
