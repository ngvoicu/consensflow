-- The size, in tokens, at which the human has every member window of a project compact,
-- whatever its harness (smaller-contexts, Phase 7, Gabriel 2026-11-06: "we must have it for
-- all"): 0026's `claude_compaction`, which was Claude's alone, renamed in place, its value
-- kept in every project. A NULL is each harness's own point. Every change of it from now on
-- is in the log as `project.compaction`; the changes logged before keep their name. The
-- number is the next free one on main when it lands (T-888 and T-1263 also take 0033).
  ALTER TABLE project RENAME COLUMN claude_compaction TO compaction;
