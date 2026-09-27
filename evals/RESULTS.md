# Chief eval results

One row per report in `evals/reports/`, oldest first; `npm run eval:summary` rewrites this file.
Judgment: how many of the scenario's expectations the chief met. Plumbing: how many of the
board's own checks held (briefs delivered, results back to the chief, members' questions
answered and the answers delivered, every task on the board). Terminal: how often the owner
had to answer in the chief's terminal because nothing was on the board. `?` is a report from
before that column existed.

| when | scenario | chief (model) | staff | judgment | plumbing | tasks | parallel | advice | reviews | questions | notes | chief edits | files | terminal | min |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-09-25T20-09 | six-decisions | claude (claude-sonnet-5) | claude | 3/7 | ? | 4 | 1 | 0 | 0 | 1 | 1 | 3 | 0 | ? | 23 |
| 2026-09-25T20-26 | simple-fix | opencode (claude-opus-5) | pi | 4/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 2 |
| 2026-09-25T20-35 | simple-fix | opencode (opencode/muse-spark-1.3-contributor-free) | pi | 4/5 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 1 | ? | 2 |
| 2026-09-25T20-43 | simple-fix | claude (claude-haiku-4-5-20251001) | codex | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 1 | ? | 3 |
| 2026-09-25T20-58 | simple-fix | pi (pi's default) | claude | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | ? | 3 |
| 2026-09-25T21-02 | simple-fix | devin (devin's default) | opencode | 5/5 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | ? | 2 |
| 2026-09-25T21-14 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 4 |
| 2026-09-25T21-55 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | ? | 6 | 4 | 1 | 1 | 1 | 3 | ? | 2 | ? | 40 |
| 2026-09-25T22-35 | six-decisions | opencode (opencode/muse-spark-1.3-contributor-free) | claude+pi+devin | 2/7 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | ? | 40 |
| 2026-09-25T22-38 | six-decisions | devin (devin's default) | claude+pi+opencode | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | ? | 3 |
| 2026-09-25T22-47 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | 9 | 7 | 2 | 7 |
| 2026-09-25T23-27 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | ? | 5 | 2 | 1 | 1 | 1 | 1 | ? | 3 | 0 | 40 |
| 2026-09-26T00-07 | six-decisions | opencode (opencode/muse-spark-1.3-contributor-free) | claude+pi+devin | 2/7 | ? | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | 0 | 40 |
| 2026-09-26T00-13 | six-decisions | devin (devin's default) | claude+pi+opencode | 1/7 | ? | 0 | 0 | 0 | 0 | 0 | 0 | ? | 7 | 2 | 6 |
| 2026-09-26T04-46 | round-trip | claude (claude-opus-5) | pi+opencode+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | 4 |
| 2026-09-26T04-50 | round-trip | pi (pi's default) | claude+opencode+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
| 2026-09-26T04-55 | round-trip | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 6 |
| 2026-09-26T04-58 | round-trip | devin (devin's default) | claude+pi+opencode | 1/7 | 5/5 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 1 | 0 | 3 |
| 2026-09-26T05-03 | round-trip | claude (claude-opus-5) | opencode | 7/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | 4 |
| 2026-09-26T05-28 | round-trip | claude (claude-opus-5) | devin | 6/7 | 5/5 | 2 | 1 | 0 | 1 | 0 | 0 | 0 | 1 | 0 | 25 |
| 2026-09-26T06-08 | six-decisions | claude (claude-opus-5) | pi+opencode+devin | 5/7 | 5/5 | 4 | 4 | 0 | 1 | 1 | 1 | 0 | 0 | 0 | 40 |
| 2026-09-26T06-48 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 2/7 | 5/5 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 0 | 0 | 40 |
| 2026-09-26T10-02 | six-decisions | claude (claude-haiku-4-5-20251001) | pi+opencode+devin | 1/7 | 6/6 | 0 | 0 | 0 | 0 | 0 | 0 | 7 | 7 | 2 | 7 |
| 2026-09-26T10-09 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+pi+devin | 5/7 | 6/6 | 4 | 2 | 0 | 1 | 1 | 1 | ? | 8 | 0 | 7 |
| 2026-09-26T10-13 | six-decisions | devin (devin's default) | claude+pi+opencode | 2/7 | 7/7 | 0 | 0 | 0 | 0 | 1 | 0 | ? | 7 | 0 | 3 |
| 2026-09-26T10-16 | advice-trip | claude (claude-opus-5) | pi | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 3 |
| 2026-09-26T10-20 | advice-trip | claude (claude-opus-5) | opencode | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 4 |
| 2026-09-26T10-23 | advice-trip | claude (claude-opus-5) | devin | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 3 |
| 2026-09-26T11-03 | six-decisions | claude (claude-opus-5) | pi+opencode+devin | 3/7 | 7/7 | 1 | 1 | 1 | 0 | 1 | 1 | 0 | 0 | 0 | 40 |
| 2026-09-26T11-44 | six-decisions | pi (pi's default) | claude+opencode+devin | 5/7 | 6/7 | 3 | 2 | 0 | 1 | 2 | 2 | ? | 3 | 0 | 40 |
| 2026-09-26T11-47 | control-trip | claude (claude-opus-5) | claude | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 2026-09-26T11-51 | control-trip | claude (claude-opus-5) | pi | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |
| 2026-09-26T12-00 | control-trip | claude (claude-opus-5) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 1 | 0 | 1 | 0 | 9 |
| 2026-09-26T12-09 | control-trip | claude (claude-opus-5) | claude | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 8 |
| 2026-09-26T12-34 | control-trip | claude (claude-opus-5) | pi | 4/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 25 |
| 2026-09-26T12-44 | control-trip | claude (claude-opus-5) | claude | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |
| 2026-09-26T12-53 | control-trip | claude (claude-opus-5) | pi | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 9 |
| 2026-09-26T12-58 | control-trip | claude (claude-opus-5) | devin | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |
| 2026-09-26T13-09 | control-trip | claude (claude-opus-5) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 11 |
| 2026-09-26T13-14 | round-trip | claude (claude-opus-5) | claude | 7/7 | 8/8 | 2 | 1 | 0 | 1 | 0 | 1 | 0 | 1 | 0 | 5 |
| 2026-09-26T13-54 | six-decisions | claude (claude-opus-5) | claude+opencode+devin | 6/7 | 7/7 | 5 | 4 | 0 | 1 | 2 | 1 | 0 | 1 | 0 | 40 |
| 2026-09-26T14-21 | round-trip | claude (claude-opus-5) | codex | 4/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 25 |
| 2026-09-26T14-39 | simple-fix | codex (codex's default) | pi | 5/5 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | 0 | 2 |
| 2026-09-26T15-12 | advice-trip | claude (claude-opus-5) | codex | 5/5 | 7/7 | 1 | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 4 |
| 2026-09-26T15-22 | round-trip | codex (gpt-5.6-luna) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 7 |
| 2026-09-26T15-27 | round-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 4/7 | 7/7 | 2 | 1 | 0 | 1 | 1 | 0 | ? | 1 | 0 | 5 |
| 2026-09-26T15-52 | control-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 2/7 | 6/7 | 1 | 1 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 25 |
| 2026-09-26T16-11 | six-decisions | codex (gpt-5.6-luna) | opencode+devin | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 14 | 2 | 19 |
| 2026-09-26T21-32 | control-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 0 | 0 | 0 | ? | 1 | 0 | 11 |
| 2026-09-26T21-36 | round-trip | opencode (opencode-go/deepseek-v4-flash) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
| 2026-09-26T21-40 | round-trip | opencode (opencode-go/deepseek-v4-flash) | devin | 5/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
| 2026-09-26T21-43 | round-trip | devin (devin's default) | opencode | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 0 | 0 | 2 |
| 2026-09-26T21-47 | round-trip | opencode (opencode-go/deepseek-v4-flash) | claude | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
| 2026-09-26T21-50 | round-trip | opencode (opencode-go/deepseek-v4-flash) | devin | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 3 |
| 2026-09-27T05-27 | simple-fix | pi (pi's default) | opencode | 5/5 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 1 | 0 | 2 |
| 2026-09-27T13-19 | six-decisions | codex (gpt-5.6-sol) | opencode+devin | 6/7 | 7/7 | 9 | 3 | 1 | 2 | 9 | 5 | ? | 6 | 0 | 40 |
| 2026-09-27T13-49 | six-decisions | claude (claude-opus-5) | claude+codex+pi+opencode+devin | 5/7 | 7/7 | 5 | 1 | 0 | 1 | 1 | 6 | 0 | 9 | 0 | 30 |
| 2026-09-27T14-26 | six-decisions | codex (gpt-5.6-sol) | claude+codex+pi+opencode+devin | 6/7 | 7/7 | 6 | 3 | 1 | 3 | 2 | 8 | ? | 6 | 0 | 37 |
| 2026-09-27T15-06 | six-decisions | pi (pi's default) | claude+codex+pi+opencode+devin | 4/7 | 6/7 | 4 | 3 | 0 | 0 | 2 | 3 | ? | 2 | 0 | 40 |
| 2026-09-27T15-28 | six-decisions | opencode (opencode-go/deepseek-v4-flash) | claude+codex+pi+opencode+devin | 4/7 | 7/7 | 5 | 2 | 0 | 1 | 2 | 0 | ? | 8 | 0 | 22 |
| 2026-09-27T15-35 | six-decisions | devin (devin's default) | claude+codex+pi+opencode+devin | 1/7 | 7/7 | 0 | 0 | 0 | 0 | 0 | 0 | ? | 8 | 2 | 8 |
| 2026-09-27T15-39 | round-trip | opencode (opencode-go/deepseek-v4-flash) | opencode | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 2 |
| 2026-09-27T15-43 | round-trip | pi (pi's default) | codex | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
| 2026-09-27T15-46 | round-trip | pi (pi's default) | pi | 7/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 3 |
| 2026-09-27T15-50 | round-trip | pi (pi's default) | devin | 5/7 | 7/7 | 2 | 1 | 0 | 1 | 0 | 0 | ? | 1 | 0 | 4 |
