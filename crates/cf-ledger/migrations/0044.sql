-- When each holding began and ended, on the ledger's own clock (ms), so a
-- question is placed by the moment its asking began. The holding it replaced
-- stays beside it, one hand-over back; `earlier` says there was one before
-- that. A holding recorded before this has no begin: it reaches back for ever.
ALTER TABLE question_holding ADD COLUMN began_at INTEGER;
ALTER TABLE question_holding ADD COLUMN ended_at INTEGER;
ALTER TABLE question_holding ADD COLUMN prior_task_id INTEGER;
ALTER TABLE question_holding ADD COLUMN prior_began_at INTEGER;
ALTER TABLE question_holding ADD COLUMN prior_ended_at INTEGER;
ALTER TABLE question_holding ADD COLUMN earlier INTEGER NOT NULL DEFAULT 0;
-- What the holding was before a hand-over began, bounds included, to put back.
ALTER TABLE question_handover ADD COLUMN previous_span TEXT;
