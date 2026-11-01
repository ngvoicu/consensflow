-- The rules the human writes for a project's staff, in Project settings: plain text
-- every member window of the project is given after its role text, and the chief's
-- after its own, as what every member has (the engine's `roles::staff_rules`). A NULL
-- is no rules, which is what every project has until the human writes some. One
-- column added in place, as 0017 added its own: nothing of the project's row moves,
-- and every change of it is in the log (`project.staff_rules`). The column is one
-- line: the test players that hold a database this ledger left to what Node's did
-- read the line to know what Node's schema never had.
  ALTER TABLE project ADD COLUMN staff_rules TEXT;
-- And the size, in tokens, at which the human has the project's Claude member windows
-- compact, in Project settings: the window Claude Code is given by the launch of
-- each member (its `autoCompactWindow`), 100,000 to 1,000,000. A NULL is Claude's own
-- point, which is what every project has until the human chooses a size. Folded into
-- this migration, which is not on main yet; every change of it is in the log
-- (`project.claude_compaction`), and the column is one line, as the one above.
  ALTER TABLE project ADD COLUMN claude_compaction INTEGER;
