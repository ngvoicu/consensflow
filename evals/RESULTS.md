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
