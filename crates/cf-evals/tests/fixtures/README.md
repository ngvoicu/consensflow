# What the JavaScript evals said

Two files, written once from the JavaScript evals (`evals/scenarios/*.mjs` and
`evals/measure.mjs`, as they were when the evals became Rust: `e30c345a` is the
last commit that changed either) and not written again: the JavaScript is what
they hold the Rust to.

- `scenarios.json`: each scenario as data, in the order of the ids: its title,
  fixture, prompt, follow-ups, the pattern (source and flags) and text of each answer,
  the fallback, the nudge, the quiet time in milliseconds and the names of its
  expectations in order. The codeword of `chief-switch`, which is another each
  time, is `@CODEWORD@` in it. Held by `tests/scenarios.rs`. A scenario written
  after the JavaScript (`AFTER_NODE` there: `uncommitted-work`, `review-loop`,
  `review-blockers`) is in neither file; the verdicts of `uncommitted-work` in
  `tests/scenarios/verdicts.rs` hold it, and those of the two review scenarios are held by a
  scripted chief on a ledger written by hand (`tests/review_loops.rs`).
- `verdicts.json`: what each scenario's expectations said of each of the eleven
  recorded ledgers of `tests/fixtures/ledgers` (of the repository), judging the
  recording's own numbers (its `measure`): for each ledger and each scenario a string
  with a `1` for each expectation that held and a `0` for each that did not, in the
  scenario's order. Held by `tests/scenarios.rs`.

**One change since, on purpose (T-972, 2026-10-31):** the `conversation` scenario's
expectations were tightened after a live run (Haiku 4.5 chief, `eval chiefs`) did every
change itself and held all three: its entry in `scenarios.json` names the four it has now,
and its strings in `verdicts.json` are what they say of the eleven recordings. Three
recordings that held "at least three changes go to the board" on tasks of other scenarios
(`chief-turns`, `parallel-unrun`, `placed`: "Write it", "Translate it", "Parser", …) now do
not, for none of those tasks carries a change the owner asked for; `chief-turns`'s chief made
two edits of its own, which the fourth expectation does not let pass. The run itself is the
recording `eval-conversation-haiku` (`tests/fixtures/ledgers`), which reads as the bug it is.
