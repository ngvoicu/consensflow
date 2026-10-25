-- Whether a paused task was paused by its project's suspension: its window closed
-- with the project (the human's Close, or the chief's window ending), so the
-- human's Resume of the project resumes it. Every other pause leaves it 0, and
-- so does every task a ledger before this column paused: its Resume leaves it
-- paused, as every Resume did then. A move of the task's state sets it back to
-- 0, as it ends a hold, and it is read only of a paused task.
-- Added in place, as 0018 added its own column: nothing of the table moves.
-- The column is one line: the test players that hold a database this ledger left
-- to what Node's did read the line to know what Node's schema never had.
  ALTER TABLE task ADD COLUMN paused_with_project INTEGER NOT NULL DEFAULT 0 CONSTRAINT task_paused_with_project_check CHECK (paused_with_project IN (0, 1));
