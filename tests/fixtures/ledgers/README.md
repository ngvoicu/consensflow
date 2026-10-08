# Frozen ledgers

Ledgers the evals' tests read (`tests/evals-measure.test.mjs`) and the home the
copy tests start from (`tests/integration/home-fixture.mjs`), written down as
the rows of the ledger and nothing else: `<name>.sql` is one `INSERT` for each
row, which `tests/ledger-file.mjs` loads into a fresh ledger file made from the
native ledger's own migrations (`crates/cf-ledger/migrations`), so the schema
is always the build's and the rows are what was recorded.

`<name>.json` is what the evals' `measure()` answered on the ledger file the
rows were taken from, when it read the ledger through Node's ledger module:
the numbers that reading it with SQLite is held to, every field of the answer.
`placed.json` also holds `listed`, how many tasks the ledger's own board listed.

| ledger | what it holds |
|---|---|
| `controls` | a tell and its answer, a pause and a resume, an `--after` follow-up |
| `long-messages` | the longest result of each kind, the owner's answers, notes whole |
| `chief-turns` | tasks, side-by-side work, advice, a review, a member's question, a note, the chief's turns and its own edits |
| `parallel-unrun`, `parallel-cancelled` | which tasks ran side by side |
| `chief-switch` | a chief switched from Claude Code to Codex, which read the history (`TERN-4821` stands for the run's random codeword) |
| `open-question-asked`, `open-question-answered` | a chief's open question, before and after the owner answered |
| `devin-chief` | a Devin chief, its conversation bound to a session |
| `placed` | tasks waiting, paused, called off, failed, on a lane, of a member who left, deleted |
| `eval-round-trip` | the ledger of a real run, taken as it was left: `npm run eval -- --scenario round-trip --chief claude --staff claude` on the native daemon (2026-10-08, a Sonnet chief and Haiku staff), one task out, a question back, an answer, a result, a review, both accepted |
| `candidate-home` | a home of the Candidate's shape: open and closed projects with the history that went with them; `@WORK@` stands for the folder the projects live in |

They were recorded by `node tests/goldens/evals/record.mjs` at the last commit
that holds Node's ledger (44e1ea95, with `evals/measure.mjs` of that commit to
answer), on a clock that moves a second a reading; `eval-round-trip` is the
ledger file of the run as it was, recorded with `--file`, but for the home
folder of the machine and its user, which a recording does not carry: they are
`/home/user` and `user` in it, and `tests/ledger-file.test.mjs` holds every
recording to that. On that ledger the
run's own report, the measure that reads it with SQLite and the one that read it
through Node's ledger gave the same answer, every field. The recorder retires
with Node's ledger and the recordings stay as they are: a change to the measure
that moves a number is made on purpose, in the recording with it.
