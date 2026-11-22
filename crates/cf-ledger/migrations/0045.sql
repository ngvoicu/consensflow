-- The staff as slots (architecture/staff-slots.md, section 3). It changes members' tiers and
-- the lead line of a Critical brief not yet handed over, and nothing else: it rests nobody,
-- removes nobody, and writes no task, need, conversation, assignee, session link or backup
-- link. It keeps its own copy of this version's role ids, so a later change to the registry
-- does not change what it did.
--
-- A member whose roles all had no tier (the frontend designer, the architect, the researcher,
-- the integrator) is given a home tier where it has none (NULL, or the `light` nobody chose):
-- complex where it holds the architect role, else standard. A tier stored for such a member
-- other than `light` is kept. No Critical member is moved: every role offers every tier.
--
-- Each change moves the member, its open windows, its current backup and that backup's open
-- windows (a closed window, and a backup that was replaced with the windows it left, are
-- history and keep their tier), and is logged as `member.settled` (handle, from, to, step 1).
CREATE TEMP TABLE settled AS
  SELECT p.id AS id, p.project_id AS project, p.handle AS handle, p.tier AS old,
         CASE WHEN EXISTS (SELECT 1 FROM json_each(p.roles) r WHERE r.value = 'architect')
              THEN 'complex' ELSE 'standard' END AS new,
         1 AS step, NULL AS emptied
    FROM participant p
   WHERE p.member_id IS NULL AND p.backup_of IS NULL AND p.left_at IS NULL
     AND p.role NOT IN ('human', 'chief')
     AND json_array_length(p.roles) > 0
     AND NOT EXISTS (SELECT 1 FROM json_each(p.roles) r
                      WHERE r.value NOT IN ('frontend', 'architect', 'researcher', 'integrator'))
     AND (p.tier IS NULL OR p.tier = 'light')
   ORDER BY p.id;
CREATE TEMP TABLE settled_rows AS
  SELECT s.id AS settled, s.new AS tier, p.id AS id FROM settled s JOIN participant p
      ON p.id = s.id OR (p.member_id = s.id AND p.left_at IS NULL)
      OR (p.backup_of = s.id AND p.left_at IS NULL)
      OR (p.left_at IS NULL
          AND p.member_id IN (SELECT b.id FROM participant b WHERE b.backup_of = s.id AND b.left_at IS NULL));
UPDATE participant SET tier = (SELECT r.tier FROM settled_rows r WHERE r.id = participant.id LIMIT 1)
 WHERE id IN (SELECT id FROM settled_rows);
INSERT INTO event (project_id, at, kind, data)
  SELECT project, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'member.settled',
         json_object('handle', handle, 'from', old, 'to', new, 'step', step)
    FROM settled ORDER BY rowid;
DROP TABLE settled_rows;
DROP TABLE settled;
-- A Critical brief that an earlier version wrote and has not handed over (queued for its window,
-- or held at the human's gate) leads with the rule of now, as an edit of its task rewrites it
-- (`rewrite_pending_briefs`): the worker's, the frontend designer's and the integrator's say
-- their purpose alone, the architect's what it writes. Every earlier version led every Critical
-- brief with "It changes no project file", which would forbid these roles their work. A brief
-- that was handed over, or is being handed, keeps what it said.
UPDATE message
   SET body = (SELECT 'Critical work: ' || t.purpose || '.' ||
                      CASE WHEN t.pool = 'architect'
                           THEN ' It writes nothing that runs (documents, contracts, the code''s frame): return them and its findings to the chief.'
                           ELSE '' END
                 FROM task t WHERE t.id = message.task_id)
              || substr(body, length((SELECT 'Critical work: ' || t.purpose || '. It changes no project file: return analysis, evidence and recommendations to the chief.'
                                        FROM task t WHERE t.id = message.task_id)) + 1)
 WHERE kind = 'task' AND state IN ('queued', 'gated')
   AND task_id IN (SELECT id FROM task WHERE purpose IS NOT NULL
                      AND pool IN ('worker', 'frontend', 'integrator', 'architect'))
   AND substr(body, 1, length((SELECT 'Critical work: ' || t.purpose || '. It changes no project file: return analysis, evidence and recommendations to the chief.'
                                 FROM task t WHERE t.id = message.task_id)) + 2)
       = (SELECT 'Critical work: ' || t.purpose || '. It changes no project file: return analysis, evidence and recommendations to the chief.'
            FROM task t WHERE t.id = message.task_id) || char(10) || char(10);
