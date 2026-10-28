-- What each harness lists of its models, as the harness itself said it (CHF-03): the
-- chief's picker, the staff chains' editor and Muse's pickers choose from it. One row a
-- harness, the home's own and no project's: the last list read whole (a JSON array of
-- models, each with its efforts), how and when it was read, and what became of the last
-- try. A try that failed keeps the list read before it, marked stale, with the reason;
-- one that never answered keeps an empty list. Written only where a list was asked for:
-- at the first ask of a daemon's life and at the human's Refresh, never in the
-- background. Numbered 0025, after the exit proof's 0024.
  CREATE TABLE model_list (
    harness TEXT PRIMARY KEY CONSTRAINT model_list_harness_check CHECK (length(harness) BETWEEN 1 AND 64),
    models TEXT NOT NULL CONSTRAINT model_list_models_check CHECK (json_valid(models) AND json_type(models) = 'array'),
    stale INTEGER NOT NULL CONSTRAINT model_list_stale_check CHECK (stale IN (0, 1)),
    how TEXT NOT NULL,
    read_at INTEGER,
    tried_at INTEGER NOT NULL,
    failure TEXT
  ) STRICT;
