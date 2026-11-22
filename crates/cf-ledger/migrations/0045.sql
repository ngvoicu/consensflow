-- The staff as slots (architecture/staff-slots.md, section 3). It changes members' tiers and
-- nothing else: it rests nobody, removes nothing, and writes no task, message, need,
-- conversation, assignee, session link or backup link. It keeps its own copy of this
-- version's role ids, so a later change to the registry does not change what it did.
--
-- Step 1: a member whose roles all had no tier (the frontend designer, the architect, the
-- researcher, the integrator) is given a home tier where it has none (NULL, or the `light`
-- nobody chose): complex where it holds the architect role, else standard. A tier stored
-- for such a member other than `light` is kept.
-- Step 2: a Critical member that holds a worker, frontend designer or integrator role moves
-- to complex, since Critical has no coding. Any Critical slot of its other roles that no
-- other member holds is empty then; the event says which.
--
-- Each change moves the member, its sessions, its current backup and that backup's sessions (a
-- backup that was replaced, and the windows it left, are history and keep their tier), and is
-- logged as `member.settled` (handle, from, to, step and, for step 2, the slots that
-- emptied). The human is told of step 2's moves once, by the first open (`open_ledger`).
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
      ON p.id = s.id OR p.member_id = s.id OR (p.backup_of = s.id AND p.left_at IS NULL)
      OR p.member_id IN (SELECT b.id FROM participant b WHERE b.backup_of = s.id AND b.left_at IS NULL);
UPDATE participant SET tier = (SELECT r.tier FROM settled_rows r WHERE r.id = participant.id LIMIT 1)
 WHERE id IN (SELECT id FROM settled_rows);
INSERT INTO event (project_id, at, kind, data)
  SELECT project, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'member.settled',
         json_object('handle', handle, 'from', old, 'to', new, 'step', step)
    FROM settled ORDER BY rowid;
DROP TABLE settled_rows;
DROP TABLE settled;

CREATE TEMP TABLE settled AS
  SELECT p.id AS id, p.project_id AS project, p.handle AS handle, p.tier AS old,
         'complex' AS new, 2 AS step,
         (SELECT json_group_array(r.value) FROM json_each(p.roles) r
           WHERE r.value IN ('architect', 'researcher', 'advisor', 'reviewer', 'tester')
             AND NOT EXISTS (
               SELECT 1 FROM participant o
                WHERE o.project_id = p.project_id AND o.id <> p.id AND o.left_at IS NULL
                  AND o.member_id IS NULL AND o.backup_of IS NULL AND o.tier = 'critical'
                  AND EXISTS (SELECT 1 FROM json_each(o.roles) q WHERE q.value = r.value)
                  AND NOT EXISTS (SELECT 1 FROM json_each(o.roles) c
                                   WHERE c.value IN ('worker', 'frontend', 'integrator')))) AS emptied
    FROM participant p
   WHERE p.member_id IS NULL AND p.backup_of IS NULL AND p.left_at IS NULL
     AND p.role NOT IN ('human', 'chief')
     AND p.tier = 'critical'
     AND EXISTS (SELECT 1 FROM json_each(p.roles) r
                  WHERE r.value IN ('worker', 'frontend', 'integrator'))
   ORDER BY p.id;
CREATE TEMP TABLE settled_rows AS
  SELECT s.id AS settled, s.new AS tier, p.id AS id FROM settled s JOIN participant p
      ON p.id = s.id OR p.member_id = s.id OR (p.backup_of = s.id AND p.left_at IS NULL)
      OR p.member_id IN (SELECT b.id FROM participant b WHERE b.backup_of = s.id AND b.left_at IS NULL);
UPDATE participant SET tier = (SELECT r.tier FROM settled_rows r WHERE r.id = participant.id LIMIT 1)
 WHERE id IN (SELECT id FROM settled_rows);
INSERT INTO event (project_id, at, kind, data)
  SELECT project, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'member.settled',
         json_object('handle', handle, 'from', old, 'to', new, 'step', step,
                     'emptied', json(COALESCE(emptied, '[]')))
    FROM settled ORDER BY rowid;
DROP TABLE settled_rows;
DROP TABLE settled;
