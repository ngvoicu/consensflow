-- What the human says of a project, in a line under its name in the sidebar: plain
-- text of their own, for them alone (no agent is told it). A NULL is no line, which
-- is what every project has until the human writes one. One column added in place,
-- as 0014 added its own: nothing of the project's row moves, and every change of it
-- is in the log (`project.described`). The column is one line: the test players that
-- hold a database this ledger left to what Node's did read the line to know what
-- Node's schema never had.
  ALTER TABLE project ADD COLUMN description TEXT;
