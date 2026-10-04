
  CREATE TABLE participant_chief (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    roles TEXT NOT NULL DEFAULT '[]',
    agent TEXT,
    harness TEXT,
    tier TEXT,
    member_id INTEGER,
    out_until TEXT,
    out_since TEXT,
    created_at TEXT NOT NULL,
    left_at TEXT,
    CONSTRAINT participant_project_fk FOREIGN KEY (project_id)
      REFERENCES project (id) ON DELETE CASCADE,
    CONSTRAINT participant_member_fk FOREIGN KEY (member_id)
      REFERENCES participant (id) ON DELETE CASCADE,
    CONSTRAINT participant_handle_unique UNIQUE (project_id, handle),
    CONSTRAINT participant_role_check
      CHECK (role IN ('human', 'chief', 'advisor', 'worker', 'reviewer', 'designer')),
    CONSTRAINT participant_tier_check
      CHECK (tier IS NULL OR tier IN ('critical', 'complex', 'standard', 'light'))
  ) STRICT;
  INSERT INTO participant_chief
    SELECT id, project_id,
           CASE handle WHEN 'lead' THEN 'chief' ELSE handle END,
           CASE role WHEN 'lead' THEN 'chief' ELSE role END,
           roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at
    FROM participant;
  DROP TABLE participant;
  ALTER TABLE participant_chief RENAME TO participant;
  CREATE INDEX participant_member_index ON participant (member_id, left_at);
  