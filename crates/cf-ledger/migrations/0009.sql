
  ALTER TABLE participant ADD COLUMN designer INTEGER NOT NULL DEFAULT 0;
  UPDATE participant SET harness = 'codex', designer = 1 WHERE harness = 'image';
  UPDATE conversation
  SET harness = 'codex',
      native_session = CASE
        WHEN native_session IN (SELECT native_session FROM conversation WHERE harness = 'codex')
        THEN NULL
        ELSE native_session
      END
  WHERE harness = 'image';
  