-- What rides in a carrier a build before this one began to paste goes again on its
-- own. This ledger freezes a carrier's set at its first paste, so the window's record
-- of any paste of it holds every row the set has; an earlier one did not. The Node
-- daemon (3.0.0-alpha.82) pasted a carrier's own words alone, and up to 3.0.4 a row
-- could join a carrier whose first paste was late and that waited again: the marker
-- of a paste that never held the row would take it for received (the reviews of
-- 2026-10-16 and 2026-10-17). A carrier began to paste where it is on its way or the
-- log says its delivery began (`delivery.begun`, which every build wrote as it began
-- one, in the step that began it; a carrier's attempts go back down when a start gives
-- it back, so they do not say). Each row of such a carrier is a queued message of its
-- own again, as the start of every earlier build left a row of a carrier on its way,
-- and the log says what was let go of, one line per carrier; the rows of a carrier
-- pasted whole go again after it. A carrier no paste of which began has no marker in
-- any record: its rows ride in its first paste, as they did. The tasks of what was
-- let go of are reconciled by the start after it (`reconcile_released`). In the step
-- that writes the version, so a start that stops between two migrations leaves nothing
-- for the next to take for this ledger's own.
  CREATE TEMP TABLE carried_rows AS
    SELECT m.id, m.carried_by AS carrier, m.project_id AS project
      FROM message m JOIN message c ON c.id = m.carried_by
     WHERE m.state = 'queued'
       AND (c.state = 'delivering'
            OR EXISTS (SELECT 1 FROM event e
                        WHERE e.project_id = c.project_id AND e.kind = 'delivery.begun'
                          AND json_extract(e.data, '$.message') = c.id));
  INSERT INTO event (project_id, at, kind, data)
    SELECT project, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'message.uncarried',
           json_object('carrier', carrier, 'released', json_group_array(id))
      FROM (SELECT id, carrier, project FROM carried_rows ORDER BY carrier, id)
     GROUP BY carrier, project
     ORDER BY carrier;
  UPDATE message SET carried_by = NULL WHERE id IN (SELECT id FROM carried_rows);
  DROP TABLE carried_rows;
