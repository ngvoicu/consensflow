-- A project no longer carries the mark that it comes back by itself after a
-- restart: the daemon suspends what was open when it starts, and the human's
-- Resume brings it back, so nothing reads the mark. SQLite drops a column only
-- where nothing refers to it, and the tables of a project refer to its row, so
-- it is rebuilt as 0006 rebuilt it, every row and column as it was but that
-- one, and the sequences that keep a deleted id from coming back are put back
-- as they were, in the order they were.
  CREATE TEMP TABLE sequence_rows AS SELECT rowid AS at, name, seq FROM sqlite_sequence;

  CREATE TEMP TABLE project_rows AS SELECT * FROM project;
  DROP TABLE project;
  CREATE TABLE project (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    directory TEXT NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    gate INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT project_state_check CHECK (state IN ('open', 'suspended')),
    CONSTRAINT project_gate_check CHECK (gate IN (0, 1))
  ) STRICT;
  INSERT INTO project (id, directory, name, state, gate, created_at, updated_at)
    SELECT id, directory, name, state, gate, created_at, updated_at
    FROM project_rows;
  DROP TABLE project_rows;

  DELETE FROM sqlite_sequence;
  INSERT INTO sqlite_sequence (name, seq) SELECT name, seq FROM sequence_rows ORDER BY at;
  DROP TABLE sequence_rows;
