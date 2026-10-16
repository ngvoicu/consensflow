-- The summary a compaction writes in the user's place (Claude Code's
-- `isCompactSummary`) is the harness's own words, never the human's: the copy
-- marks it, so that no reader takes it for something the human typed (`cf
-- history`, the wake-up's check of what was typed, the handoff's last words)
-- and the history viewer names it. An item copied before this column is 0, as
-- it was read then. One column added in place, as 0017 added its own: nothing of
-- the transcript's rows moves. The column is one line: the test players that
-- hold a database this ledger left to what Node's did read the line to know what
-- Node's schema never had.
  ALTER TABLE transcript ADD COLUMN compaction INTEGER NOT NULL DEFAULT 0 CONSTRAINT transcript_compaction_check CHECK (compaction IN (0, 1));
