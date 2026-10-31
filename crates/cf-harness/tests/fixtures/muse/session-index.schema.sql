-- The schema of Muse Code 1.4.13's session index (`session-index.db`), as the binary
-- created it in a throwaway data folder (T-703, 2026-10-26): `sqlite_master` in its order.
-- The tests of `muse::index` build their stores from it.
CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE sessions (  session_id          TEXT PRIMARY KEY,
  session_stream_id   TEXT NOT NULL,
  session_dir         TEXT NOT NULL,
  session_log_path    TEXT NOT NULL UNIQUE,
  layout              TEXT NOT NULL,
  workspace_root      TEXT,
  workspace_key       TEXT,
  provider_id         TEXT,
  model_id            TEXT,
  git_branch          TEXT,
  title               TEXT NOT NULL,
  effective_long_title TEXT,
  title_provenance TEXT CHECK(title_provenance IN ('generated','manual')),
  title_name_fingerprint TEXT,
  title_source_fingerprint TEXT,
  first_user_prompt   TEXT,
  search_text         TEXT NOT NULL,
  created_at_us       INTEGER,
  updated_at_us       INTEGER,
  prompt_count        INTEGER NOT NULL DEFAULT 0,
  status              TEXT NOT NULL,
  status_rank         INTEGER NOT NULL,
  source_fingerprint  TEXT,
  indexed_at_us       INTEGER NOT NULL,
  latest_segment_terminated INTEGER NOT NULL DEFAULT 0,
  session_name        TEXT,
  session_name_revision INTEGER CHECK(session_name_revision >= 0),
  session_name_description_dependency TEXT,
  logged_session_name TEXT,
  logged_session_name_revision INTEGER CHECK(logged_session_name_revision >= 0),
  -- 1 while the logged name is the old store name seeded at upgrade, until a derivation replaces it.
  logged_session_name_seeded INTEGER,
  msp_created_at_us   INTEGER,
  msp_updated_at_us   INTEGER,
  msp_last_activity_at_us INTEGER,
  msp_order_key_us    INTEGER,
  msp_turn_count      INTEGER,
  msp_fork_source_session_id TEXT,
  msp_fork_cut_cursor TEXT,
  msp_fork_cut_explicit INTEGER,
  msp_fork_command_id TEXT,
  msp_provider_id     TEXT,
  msp_model_id        TEXT,
  msp_title           TEXT,
  msp_first_user_prompt TEXT,
  msp_branch          TEXT,
  msp_source_fingerprint TEXT,
  msp_kind            TEXT,
  msp_parent_session_id TEXT,
  msp_created_by_client_name TEXT,
  msp_created_by_client_version TEXT,
  CHECK ((effective_long_title IS NULL) = (title_provenance IS NULL)),
  CHECK (effective_long_title IS NULL OR title_provenance='generated' OR title_name_fingerprint IS NOT NULL),
  CHECK ((session_name IS NULL) = (session_name_revision IS NULL)),
  CHECK ((logged_session_name IS NULL) = (logged_session_name_revision IS NULL)));
CREATE INDEX idx_sessions_updated
  ON sessions(status_rank, updated_at_us DESC, created_at_us DESC, session_id ASC);
CREATE INDEX idx_sessions_created
  ON sessions(created_at_us DESC, updated_at_us DESC, session_id ASC);
CREATE INDEX idx_sessions_workspace_updated
  ON sessions(workspace_key, status_rank, updated_at_us DESC, created_at_us DESC, session_id ASC);
CREATE INDEX idx_sessions_workspace_created
  ON sessions(workspace_key, created_at_us DESC, updated_at_us DESC, session_id ASC);
CREATE INDEX idx_sessions_stream
  ON sessions(session_stream_id);
CREATE INDEX idx_sessions_provider
  ON sessions(provider_id);
CREATE INDEX idx_sessions_status
  ON sessions(status);
CREATE TABLE session_old_names (
  session_id      TEXT NOT NULL REFERENCES sessions(session_id)
                  ON DELETE CASCADE ON UPDATE CASCADE,
  normalized_name TEXT NOT NULL COLLATE NOCASE,
  PRIMARY KEY(session_id, normalized_name)
);
CREATE INDEX idx_session_old_names_name
  ON session_old_names(normalized_name);
CREATE TRIGGER session_index_description_insert_guard
BEFORE INSERT ON sessions
WHEN ((NEW.effective_long_title IS NULL) <> (NEW.title_provenance IS NULL))
  OR NEW.title_provenance NOT IN ('generated','manual')
  OR (NEW.title_provenance='manual' AND NEW.title_name_fingerprint IS NULL)
BEGIN SELECT RAISE(ABORT, 'invalid session description projection'); END;
CREATE TRIGGER session_index_description_update_guard
BEFORE UPDATE OF effective_long_title, title_provenance, title_name_fingerprint ON sessions
WHEN ((NEW.effective_long_title IS NULL) <> (NEW.title_provenance IS NULL))
  OR NEW.title_provenance NOT IN ('generated','manual')
  OR (NEW.title_provenance='manual' AND NEW.title_name_fingerprint IS NULL)
BEGIN SELECT RAISE(ABORT, 'invalid session description projection'); END;
CREATE INDEX idx_sessions_msp_order ON sessions(COALESCE(msp_order_key_us, msp_updated_at_us) DESC, session_id ASC);
CREATE INDEX idx_sessions_logged_name ON sessions(logged_session_name COLLATE NOCASE);
CREATE TRIGGER session_index_name_projection_insert_guard
             BEFORE INSERT ON sessions
             WHEN ((NEW.session_name IS NULL) <> (NEW.session_name_revision IS NULL))
               OR NEW.session_name_revision < 0
             BEGIN
               SELECT RAISE(ABORT, 'invalid session name projection');
             END;
CREATE TRIGGER session_index_name_projection_update_guard
             BEFORE UPDATE OF session_name, session_name_revision ON sessions
             WHEN ((NEW.session_name IS NULL) <> (NEW.session_name_revision IS NULL))
               OR NEW.session_name_revision < 0
             BEGIN
               SELECT RAISE(ABORT, 'invalid session name projection');
             END;
INSERT INTO schema_meta (key, value) VALUES ('schema_version', '1');
INSERT INTO schema_meta (key, value) VALUES ('logged-names-seeded', '1');
INSERT INTO schema_meta (key, value) VALUES ('session_name_snapshot_fingerprint', 'ready:0000');
INSERT INTO schema_meta (key, value) VALUES ('msp_maintenance_position', '/muse-home/data/muse/sessions/2026/10/26/01a17924-462f-7153-adbc-03a4282c89c5/session.jsonl');
