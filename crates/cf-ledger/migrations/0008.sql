
  ALTER TABLE task ADD COLUMN paused_at TEXT;
  ALTER TABLE participant ADD COLUMN switched_from_harness TEXT;
  ALTER TABLE participant ADD COLUMN switched_from_agent TEXT;
  ALTER TABLE participant ADD COLUMN switched_from_cut INTEGER NOT NULL DEFAULT 0;
  UPDATE task SET paused_at = (
    SELECT MAX(e.at) FROM event e
    WHERE e.project_id = task.project_id AND e.kind = 'task.state'
      AND json_extract(e.data, '$.task') = task.number
      AND json_extract(e.data, '$.to') = 'paused'
  );
  UPDATE participant
  SET switched_from_harness = json_extract(e.data, '$.from.harness'),
      switched_from_agent = json_extract(e.data, '$.from.agent'),
      switched_from_cut = json_extract(e.data, '$.cut')
  FROM event e
  WHERE participant.role = 'chief'
    AND e.id = (SELECT MAX(id) FROM event
                WHERE project_id = participant.project_id AND kind = 'chief.switched');
  