INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'u1', 0, 'user', 'Put three tasks on the board, then ask me the report name.', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'a1', 1, 'assistant', 'Reading?', 0, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'a2', 2, 'assistant', 'Tasks are out. What is the final report called?', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'u2', 3, 'user', '[ConsensFlow m-7 · T-1 · result from @worker]
Done.', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'a3', 4, 'assistant', 'T-1 is in; waiting for the rest.', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'u3', 5, 'user', 'DELTA-5530.', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO transcript (conversation_id, item_id, seq, role, text, complete, at, copied_at) VALUES (1, 'a4', 6, 'assistant', 'Noted; the note is sent.', 1, NULL, '2026-09-19T10:00:07.000Z');
INSERT INTO project (id, directory, name, state, resume_on_start, gate, created_at, updated_at) VALUES (1, '/work/site', 'site', 'open', 0, 0, '2026-09-19T10:00:01.000Z', '2026-09-19T10:00:01.000Z');
INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer) VALUES (1, 1, 'human', 'human', '[]', NULL, NULL, NULL, NULL, NULL, NULL, '2026-09-19T10:00:02.000Z', NULL, NULL, NULL, 0, 0);
INSERT INTO participant (id, project_id, handle, role, roles, agent, harness, tier, member_id, out_until, out_since, created_at, left_at, switched_from_harness, switched_from_agent, switched_from_cut, designer) VALUES (2, 1, 'chief', 'chief', '[]', NULL, 'codex', NULL, NULL, NULL, NULL, '2026-09-19T10:00:03.000Z', NULL, NULL, NULL, 0, 0);
INSERT INTO conversation (id, participant_id, harness, native_session, started_at, ended_at) VALUES (1, 2, 'codex', NULL, '2026-09-19T10:00:05.000Z', NULL);
INSERT INTO event (id, project_id, at, kind, data) VALUES (1, 1, '2026-09-19T10:00:04.000Z', 'project.created', '{"name":"site","directory":"/work/site"}');
INSERT INTO event (id, project_id, at, kind, data) VALUES (2, 1, '2026-09-19T10:00:06.000Z', 'conversation.started', '{"participant":"chief","conversation":1}');
