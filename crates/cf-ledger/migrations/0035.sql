-- Whether the human has the project's members run with Claude Code's own advisor tool, in
-- Project settings ("Advisor tool"; alpha-5 A5-20, Gabriel 2026-11-10: "advisor tool for
-- workers should be disabled as a setting from the project settings ... we keep it for chief"):
-- 0, which every project has until the human ticks it, is members whose Claude Code window is
-- launched with the tool switched off (`CLAUDE_CODE_DISABLE_ADVISOR_TOOL` in the launch's own
-- settings); 1 is the owner's own setting, as every member had before. So a project that
-- exists now has the tool off from its members' next start. Every change of it is in the log
-- (`project.advisor_tool`). The number is 0035: the next one free on main after 0034. The
-- column is one line, as 0026's is: the test players read it to know what Node's schema never had.
  ALTER TABLE project ADD COLUMN advisor_tool INTEGER NOT NULL DEFAULT 0;
