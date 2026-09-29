# Chief eval results

One row per report in `evals/reports/`, oldest first; `npm run eval:summary` rewrites this file.
Judgment: how many of the scenario's expectations the chief met. Plumbing: how many of the
board's own checks held (briefs delivered, results back to the chief, members' questions
answered and the answers delivered, every task on the board). On board: questions put to the
owner on the board, none since 2026-09-29, when the chief began asking in its terminal.
Terminal: how often the owner nudged a chief that stopped without asking. Asked: the questions
the chief ended its turns with in its terminal, +Np for its own question dialog. `?` is a
report from before that column existed. A chief marked [nocard] had a one-line card naming no
board; [bare] ran without ConsensFlow.

| when | scenario | chief (model, effort) | staff | judgment | plumbing | tasks | parallel | advice | reviews | on board | notes | chief edits | files | terminal | asked | min |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-09-25T20-09 | six-decisions | claude (claude-sonnet-5) | claude | 3/7 | ? | 4 | 1 | 0 | 0 | 1 | 1 | 3 | 0 | ? | ? | 23 |
| 2026-09-25T20-26 | simple-fix | opencode (claude-opus-5) | pi | 4/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | ? | 2 |
| 2026-09-25T20-35 | simple-fix | opencode (opencode/muse-spark-1.3-contributor-free) | pi | 4/5 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 1 | ? | ? | 2 |
| 2026-09-25T20-43 | simple-fix | claude (claude-haiku-4-5-20251001) | codex | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 1 | ? | ? | 3 |
| 2026-09-25T20-58 | simple-fix | pi (pi's default) | claude | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | ? | ? | 3 |
| 2026-09-25T21-02 | simple-fix | devin (devin's default) | opencode | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | ? | ? | 2 |
| 2026-09-25T21-14 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | ? | 4 |
| 2026-09-25T21-55 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | ? | 6 | 4 | 1 | 1 | 1 | 3 | ? | 2 | ? | ? | 40 |
| 2026-09-25T22-35 | six-decisions | opencode (opencode/muse-spark-1.3-contributor-free) | claude+pi+devin | 2/7 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | ? | ? | 40 |
| 2026-09-25T22-38 | six-decisions | devin (devin's default) | claude+pi+opencode | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | ? | ? | 3 |
| 2026-09-25T22-47 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 9 | 7 | 2 | ? | 7 |
| 2026-09-25T23-27 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | ? | 5 | 2 | 1 | 1 | 1 | 1 | ? | 3 | 0 | ? | 40 |
| 2026-09-26T00-07 | six-decisions | opencode (opencode/muse-spark-1.3-contributor-free) | claude+pi+devin | 2/7 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | 0 | ? | 40 |
| 2026-09-26T00-13 | six-decisions | devin (devin's default) | claude+pi+opencode | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 2 | ? | 6 |
| 2026-09-26T04-46 | round-trip | claude (claude-opus-5) | pi+opencode+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | ? | 4 |
| 2026-09-26T04-50 | round-trip | pi (pi's default) | claude+opencode+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-26T04-55 | round-trip | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 6 |
| 2026-09-26T04-58 | round-trip | devin (devin's default) | claude+pi+opencode | 1/7 | 5/5 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 1 | 0 | ? | 3 |
| 2026-09-26T05-03 | round-trip | claude (claude-opus-5) | opencode | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | ? | 4 |
| 2026-09-26T05-28 | round-trip | claude (claude-opus-5) | devin | 6/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | ? | 25 |
| 2026-09-26T06-08 | six-decisions | claude (claude-opus-5) | pi+opencode+devin | 5/7 | 5/5 | 4 | 4 | 0 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 40 |
| 2026-09-26T06-48 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 2/7 | 5/5 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | 0 | ? | 40 |
| 2026-09-26T10-02 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | 7 | 7 | 2 | ? | 7 |
| 2026-09-26T10-09 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 5/7 | 6/6 | 4 | 2 | 0 | 1 | 1 | 1 | ? | 8 | 0 | ? | 7 |
| 2026-09-26T10-13 | six-decisions | devin (devin's default) | claude+pi+opencode | 2/7 | 7/7 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 7 | 0 | ? | 3 |
| 2026-09-26T10-16 | advice-trip | claude (claude-opus-5) | pi | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | ? | 3 |
| 2026-09-26T10-20 | advice-trip | claude (claude-opus-5) | opencode | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | ? | 4 |
| 2026-09-26T10-23 | advice-trip | claude (claude-opus-5) | devin | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | ? | 3 |
| 2026-09-26T11-03 | six-decisions | claude (claude-opus-5) | pi+opencode+devin | 3/7 | 7/7 | 1 | 1 | 1 | 0 | 1 | 1 | 0 | 0 | 0 | ? | 40 |
| 2026-09-26T11-44 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | 6/7 | 3 | 2 | 0 | 1 | 2 | 2 | ? | 3 | 0 | ? | 40 |
| 2026-09-26T11-47 | control-trip | claude (claude-opus-5) | claude | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 4 |
| 2026-09-26T11-51 | control-trip | claude (claude-opus-5) | pi | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 4 |
| 2026-09-26T12-00 | control-trip | claude (claude-opus-5) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 1 | 0 | 1 | 0 | ? | 9 |
| 2026-09-26T12-09 | control-trip | claude (claude-opus-5) | claude | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 8 |
| 2026-09-26T12-34 | control-trip | claude (claude-opus-5) | pi | 4/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 25 |
| 2026-09-26T12-44 | control-trip | claude (claude-opus-5) | claude | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 |
| 2026-09-26T12-53 | control-trip | claude (claude-opus-5) | pi | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 9 |
| 2026-09-26T12-58 | control-trip | claude (claude-opus-5) | devin | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 |
| 2026-09-26T13-09 | control-trip | claude (claude-opus-5) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 11 |
| 2026-09-26T13-14 | round-trip | claude (claude-opus-5) | claude | 7/7 | 8/8 | 2 | 1 | 0 | 1 | 0 | 1 | 0 | 1 | 0 | ? | 5 |
| 2026-09-26T13-54 | six-decisions | claude (claude-opus-5) | claude+opencode+devin | 6/7 | 7/7 | 5 | 4 | 0 | 1 | 2 | 1 | 0 | 1 | 0 | ? | 40 |
| 2026-09-26T14-21 | round-trip | claude (claude-opus-5) | codex | 4/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 25 |
| 2026-09-26T14-39 | simple-fix | codex (codex's default) | pi | 5/5 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | 0 | ? | 2 |
| 2026-09-26T15-12 | advice-trip | claude (claude-opus-5) | codex | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | ? | 4 |
| 2026-09-26T15-22 | round-trip | codex (gpt-5.6-luna) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 7 |
| 2026-09-26T15-27 | round-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 4/7 | 7/7 | 2 | 1 | 0 | 1 | 1 | 0 | ? | 1 | 0 | ? | 5 |
| 2026-09-26T15-52 | control-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 25 |
| 2026-09-26T16-11 | six-decisions | codex (gpt-5.6-luna) | opencode+devin | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 14 | 2 | ? | 19 |
| 2026-09-26T21-32 | control-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | ? | 1 | 0 | ? | 11 |
| 2026-09-26T21-36 | round-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-26T21-40 | round-trip | opencode (opencode-go/deepseek-v4-flash) | devin | 5/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-26T21-43 | round-trip | devin (devin's default) | opencode | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 2 |
| 2026-09-26T21-47 | round-trip | opencode (opencode-go/deepseek-v4-flash) | claude | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-26T21-50 | round-trip | opencode (opencode-go/deepseek-v4-flash) | devin | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 3 |
| 2026-09-27T05-27 | simple-fix | pi (pi's default) | opencode | 5/5 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | 0 | ? | 2 |
| 2026-09-27T13-19 | six-decisions | codex (gpt-5.6-sol) | opencode+devin | 6/7 | 7/7 | 9 | 3 | 1 | 2 | 9 | 5 | ? | 6 | 0 | ? | 40 |
| 2026-09-27T13-49 | six-decisions | claude (claude-opus-5) | claude+codex+pi+opencode+devin | 5/7 | 7/7 | 5 | 1 | 0 | 1 | 1 | 6 | 0 | 9 | 0 | ? | 30 |
| 2026-09-27T14-26 | six-decisions | codex (gpt-5.6-sol) | claude+codex+pi+opencode+devin | 6/7 | 7/7 | 6 | 3 | 1 | 3 | 2 | 8 | ? | 6 | 0 | ? | 37 |
| 2026-09-27T15-06 | six-decisions | pi (pi's default) | claude+codex+pi+opencode+devin | 4/7 | 6/7 | 4 | 3 | 0 | 0 | 2 | 3 | ? | 2 | 0 | ? | 40 |
| 2026-09-27T15-28 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+codex+pi+opencode+devin | 4/7 | 7/7 | 5 | 2 | 0 | 1 | 2 | 0 | ? | 8 | 0 | ? | 22 |
| 2026-09-27T15-35 | six-decisions | devin (devin's default) | claude+codex+pi+opencode+devin | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 8 | 2 | ? | 8 |
| 2026-09-27T15-39 | round-trip | opencode (opencode-go/deepseek-v4-flash) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 2 |
| 2026-09-27T15-43 | round-trip | pi (pi's default) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-27T15-46 | round-trip | pi (pi's default) | pi | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 3 |
| 2026-09-27T15-50 | round-trip | pi (pi's default) | devin | 5/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | ? | 4 |
| 2026-09-27T18-40 | long-trip | codex (gpt-5.6-sol, high) | claude (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 10 |
| 2026-09-27T18-49 | long-trip | claude (claude-opus-5, high) | claude (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 9 |
| 2026-09-27T18-57 | long-trip | claude (claude-opus-5, high) | codex (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 8 |
| 2026-09-27T19-27 | long-trip | claude (claude-opus-5, high) | pi (medium) | 2/10 | 6/7 | 3 | 3 | 1 | 1 | 1 | 0 | 0 | 0 | 0 | ? | 30 |
| 2026-09-27T19-37 | long-trip | claude (claude-opus-5, high) | opencode (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 10 |
| 2026-09-27T19-46 | long-trip | claude (claude-opus-5, high) | devin (medium) | 10/10 | 7/7 | 3 | 2 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 9 |
| 2026-09-27T19-52 | long-trip | codex (gpt-5.6-sol, high) | codex (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 6 |
| 2026-09-27T20-22 | long-trip | codex (gpt-5.6-sol, high) | pi (medium) | 3/10 | 5/7 | 7 | 5 | 1 | 2 | 1 | 0 | ? | 0 | 0 | ? | 30 |
| 2026-09-27T20-57 | long-trip | codex (gpt-5.6-sol, high) | opencode (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 6 |
| 2026-09-27T21-03 | long-trip | codex (gpt-5.6-sol, high) | devin (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 6 |
| 2026-09-27T21-08 | long-trip | pi (pi's default, high) | claude (medium) | 8/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 5 |
| 2026-09-27T21-14 | long-trip | pi (pi's default, high) | codex (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 2 | ? | 0 | 0 | ? | 7 |
| 2026-09-27T21-31 | long-trip | pi (pi's default, high) | pi (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 17 |
| 2026-09-27T21-36 | long-trip | pi (pi's default, high) | opencode (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 5 |
| 2026-09-27T21-40 | long-trip | pi (pi's default, high) | devin (medium) | 9/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 4 |
| 2026-09-27T21-46 | long-trip | opencode (opencode-go/deepseek-v4-flash, default) | claude (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 6 |
| 2026-09-27T21-52 | long-trip | opencode (opencode-go/deepseek-v4-flash, default) | codex (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 6 |
| 2026-09-27T22-07 | long-trip | opencode (opencode-go/deepseek-v4-flash, default) | pi (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 15 |
| 2026-09-27T22-12 | long-trip | opencode (opencode-go/deepseek-v4-flash, default) | opencode (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 5 |
| 2026-09-27T22-16 | long-trip | opencode (opencode-go/deepseek-v4-flash, default) | devin (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | ? | 0 | 0 | ? | 4 |
| 2026-09-27T22-20 | long-trip | devin (devin's default, default) | claude (medium) | 1/10 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 4 |
| 2026-09-27T22-50 | long-trip | devin (devin's default, default) | codex (medium) | 1/10 | 6/7 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | 0 | ? | 30 |
| 2026-09-27T22-54 | long-trip | devin (devin's default, default) | pi (medium) | 1/10 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 3 |
| 2026-09-27T23-24 | long-trip | devin (devin's default, default) | opencode (medium) | 1/10 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 30 |
| 2026-09-27T23-27 | long-trip | devin (devin's default, default) | devin (medium) | 1/10 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | ? | 3 |
| 2026-09-27T23-48 | long-trip | claude (claude-opus-5, high) | pi (medium) | 10/10 | 7/7 | 3 | 3 | 1 | 1 | 1 | 1 | 0 | 0 | 0 | ? | 19 |
| 2026-09-28T08-30 | six-decisions | claude (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 6/7 | 7/7 | 5 | 2 | 0 | 2 | 4 | 5 | 0 | 8 | 0 | ? | 40 |
| 2026-09-28T09-10 | six-decisions | codex (gpt-5.6-sol, high) | claude+codex+pi+opencode+devin (medium) | 6/7 | 7/7 | 12 | 3 | 1 | 3 | 2 | 16 | ? | 14 | 0 | ? | 40 |
| 2026-09-28T09-51 | six-decisions | pi (pi's default, high) | claude+codex+pi+opencode+devin (medium) | 4/7 | 7/7 | 4 | 3 | 0 | 0 | 1 | 1 | ? | 1 | 0 | ? | 40 |
| 2026-09-28T10-00 | six-decisions | opencode (openrouter/deepseek/deepseek-v4.1-flash, default) | claude+codex+pi+opencode+devin (medium) | 7/7 | 7/7 | 5 | 5 | 0 | 1 | 4 | 2 | ? | 2 | 0 | ? | 9 |
| 2026-09-28T10-08 | six-decisions | devin (devin's default, default) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 2 | ? | 8 |
| 2026-09-28T13-20 | conversation | claude (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 2/4 | 7/7 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 7 | 0 | ? | 12 |
| 2026-09-28T13-35 | conversation | codex (gpt-5.6-sol, high) | claude+codex+pi+opencode+devin (medium) | 4/4 | 7/7 | 5 | 1 | 0 | 2 | 1 | 9 | ? | 6 | 0 | ? | 15 |
| 2026-09-28T14-06 | conversation | claude (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 3/3 | 7/7 | 6 | 1 | 0 | 0 | 1 | 8 | 0 | 7 | 0 | ? | 32 |
| 2026-09-28T14-29 | conversation | codex (gpt-5.6-sol, high) | claude+codex+pi+opencode+devin (medium) | 3/3 | 7/7 | 12 | 1 | 0 | 6 | 0 | 8 | ? | 5 | 0 | ? | 23 |
| 2026-09-28T15-09 | conversation | pi (pi's default, high) | claude+codex+pi+opencode+devin (medium) | 3/3 | 6/7 | 5 | 1 | 0 | 0 | 1 | 4 | ? | 4 | 0 | ? | 40 |
| 2026-09-28T15-19 | conversation | opencode (openrouter/deepseek/deepseek-v4.1-flash, default) | claude+codex+pi+opencode+devin (medium) | 2/3 | 7/7 | 6 | 1 | 0 | 0 | 3 | 7 | ? | 5 | 0 | ? | 10 |
| 2026-09-28T15-28 | conversation | devin (devin's default, default) | claude+codex+pi+opencode+devin (medium) | 0/3 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 0 | ? | 8 |
| 2026-09-28T15-47 | six-decisions | claude (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 5/7 | 7/7 | 5 | 1 | 0 | 1 | 1 | 4 | 0 | 8 | 0 | ? | 19 |
| 2026-09-28T16-19 | six-decisions | codex (gpt-5.6-sol, high) | claude+codex+pi+opencode+devin (medium) | 6/7 | 6/7 | 4 | 2 | 1 | 1 | 1 | 6 | ? | 9 | 0 | ? | 32 |
| 2026-09-28T16-59 | six-decisions | pi (pi's default, high) | claude+codex+pi+opencode+devin (medium) | 5/7 | 6/7 | 9 | 7 | 0 | 4 | 1 | 1 | ? | 2 | 0 | ? | 40 |
| 2026-09-28T17-12 | six-decisions | opencode (openrouter/deepseek/deepseek-v4.1-flash, default) | claude+codex+pi+opencode+devin (medium) | 6/7 | 6/7 | 9 | 2 | 0 | 2 | 2 | 1 | ? | 8 | 0 | ? | 13 |
| 2026-09-28T17-20 | six-decisions | devin (devin's default, default) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 2 | ? | 8 |
| 2026-09-28T18-07 | conversation | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 0/3 | 7/7 | 1 | 1 | 0 | 0 | 1 | 1 | ? | 2 | 0 | ? | 40 |
| 2026-09-28T18-14 | conversation | opencode (openrouter/deepseek/deepseek-v4-pro-0813, default) | claude+codex+pi+opencode+devin (medium) | 1/3 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 0 | ? | 7 |
| 2026-09-28T18-21 | six-decisions | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 2 | ? | 7 |
| 2026-09-28T18-50 | six-decisions | opencode (openrouter/deepseek/deepseek-v4-pro-0813, default) | claude+codex+pi+opencode+devin (medium) | 5/7 | 7/7 | 6 | 2 | 0 | 1 | 1 | 2 | ? | 8 | 0 | ? | 28 |
| 2026-09-28T19-30 | conversation | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 0/3 | 6/7 | 1 | 1 | 0 | 0 | 1 | 1 | ? | 1 | 0 | ? | 40 |
| 2026-09-28T20-10 | six-decisions | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 5/7 | 6/7 | 5 | 5 | 0 | 1 | 1 | 1 | ? | 2 | 0 | 0 | 40 |
| 2026-09-28T20-28 | six-decisions | claude [bare] (claude-opus-5, high) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 8 | 1 | 1 | 18 |
| 2026-09-28T21-08 | six-decisions | claude (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 5/7 | 7/7 | 4 | 3 | 3 | 0 | 1 | 3 | 0 | 0 | 0 | 0 | 40 |
| 2026-09-28T22-22 | six-decisions | codex [bare] (gpt-5.6-luna, high) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 40 |
| 2026-09-28T23-03 | six-decisions | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 40 |
| 2026-09-28T23-43 | six-decisions | pi [nocard] (openrouter/deepseek/deepseek-v4-pro-0813, high) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 40 |
| 2026-09-29T00-23 | six-decisions | pi [bare] (openrouter/deepseek/deepseek-v4-pro-0813, high) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 40 |
| 2026-09-29T01-20 | six-decisions | opencode [bare] (openrouter/deepseek/deepseek-v4-pro-0813, default) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 0 | 0 | 23 |
| 2026-09-29T02-06 | six-decisions | devin [bare] (devin's default, default) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 0 | 0 | 40 |
| 2026-09-29T02-20 | six-decisions | opencode [bare] (openrouter/deepseek/deepseek-v4-pro-0813, default) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 2 | 1 | 10 |
| 2026-09-29T02-26 | round-trip | claude (claude-opus-5, high) | claude (medium) | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 1 | 0 | 1 | 0 | 0 | 6 |
| 2026-09-29T02-50 | six-decisions | claude [nocard] (claude-opus-5, high) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 7 | 0 | 2 | 22 |
| 2026-09-29T03-30 | six-decisions | codex (gpt-5.6-luna, high) | claude+codex+pi+opencode+devin (medium) | 5/7 | 6/7 | 7 | 4 | 0 | 3 | 1 | 1 | ? | 8 | 0 | 0 | 40 |
| 2026-09-29T03-43 | six-decisions | codex [nocard] (gpt-5.6-luna, high) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 2 | 0 | 13 |
| 2026-09-29T04-02 | six-decisions | opencode (openrouter/deepseek/deepseek-v4-pro-0813, default) | claude+codex+pi+opencode+devin (medium) | 6/7 | 7/7 | 6 | 2 | 0 | 1 | 1 | 2 | ? | 8 | 0 | 0 | 19 |
| 2026-09-29T04-07 | six-decisions | opencode [nocard] (openrouter/deepseek/deepseek-v4-pro-0813, default) | claude+codex+pi+opencode+devin (medium) | 2/7 | 7/7 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 8 | 0 | 1 | 5 |
| 2026-09-29T04-11 | six-decisions | devin (devin's default, default) | claude+codex+pi+opencode+devin (medium) | 2/7 | 7/7 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 8 | 0 | 0 | 5 |
| 2026-09-29T04-16 | six-decisions | devin [nocard] (devin's default, default) | claude+codex+pi+opencode+devin (medium) | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 0 | 3 | 5 |
| 2026-09-29T04-24 | six-decisions | pi [bare] (openrouter/deepseek/deepseek-v4-pro-0813, high) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 2 | 0 | 8 |
| 2026-09-29T04-37 | six-decisions | opencode [bare] (openrouter/deepseek/deepseek-v4-pro-0813, default) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 2 | 0 | 13 |
| 2026-09-29T04-43 | six-decisions | devin [bare] (devin's default, default) | none | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 6 | 0 | 3 | 6 |
| 2026-09-29T08-34 | terminal-questions | claude (claude-opus-5, high) | opencode (medium) | 2/5 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |
| 2026-09-29T08-38 | terminal-questions | codex (gpt-5.6-luna, high) | opencode (medium) | 5/5 | 6/6 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 2 | 4 |
| 2026-09-29T08-41 | terminal-questions | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | opencode (medium) | 3/5 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 1 | 3 |
| 2026-09-29T09-06 | terminal-questions | opencode (openrouter/deepseek/deepseek-v4-pro-0813, default) | opencode (medium) | 2/5 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 25 |
| 2026-09-29T09-31 | terminal-questions | devin (devin's default, default) | opencode (medium) | 2/5 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 0 | 25 |
| 2026-09-29T09-49 | terminal-questions | claude (claude-opus-5, high) | opencode (medium) | 5/5 | 6/6 | 3 | 1 | 0 | 1 | 0 | 0 | 0 | 3 | 0 | 6 | 13 |
| 2026-09-29T09-54 | terminal-questions | pi (openrouter/deepseek/deepseek-v4-pro-0813, high) | opencode (medium) | 5/5 | 6/6 | 2 | 1 | 0 | 0 | 0 | 0 | ? | 3 | 0 | 3 | 5 |
| 2026-09-29T09-59 | terminal-questions | opencode (openrouter/deepseek/deepseek-v4-pro-0813, default) | opencode (medium) | 5/5 | 6/6 | 1 | 1 | 0 | 0 | 0 | 0 | ? | 2 | 0 | 2 | 6 |
| 2026-09-29T10-03 | terminal-questions | devin (devin's default, default) | opencode (medium) | 4/5 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 5 | 0 | 3 | 4 |
