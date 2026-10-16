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
  after the JavaScript (`AFTER_NODE` there: `uncommitted-work`) is in neither
  file; its verdicts in `tests/scenarios/verdicts.rs` hold it.
- `verdicts.json`: what each scenario's expectations said of each of the eleven
  recorded ledgers of `tests/fixtures/ledgers` (of the repository), judging the
  recording's own numbers (its `measure`): for each ledger and each scenario a string
  with a `1` for each expectation that held and a `0` for each that did not, in the
  scenario's order. Held by `tests/scenarios.rs`.
