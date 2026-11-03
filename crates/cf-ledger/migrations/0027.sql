-- What each harness lists of its models, as the harness itself said it (CHF-03): the
-- chief's picker, the staff chains' editor and Muse's pickers choose from it. One row a
-- harness, the home's own and no project's: its last listing whole (a JSON array of
-- models, each with its efforts), when and how it was read, and why it is stale where it
-- is (NULL where it is the account's own as of then). A read that failed keeps the
-- listing read before it, stale for the failure's reason; one that never answered keeps
-- an empty listing. Written only where a list was asked for: at the first ask of a
-- daemon's life and at the human's Refresh, never in the background. Numbered 0025,
-- after the exit proof's 0024.
  CREATE TABLE model_list (
    harness TEXT PRIMARY KEY CONSTRAINT model_list_harness_check CHECK (length(harness) BETWEEN 1 AND 64),
    models TEXT NOT NULL CONSTRAINT model_list_models_check CHECK (json_valid(models) AND json_type(models) = 'array'),
    read_at INTEGER,
    how TEXT NOT NULL,
    stale_why TEXT
  ) STRICT;
