# Evals — does the skill change what a lead *does*?

`npm test` checks what the skill **says**. Nothing checked what a lead **does**
with it, so behavioural failures were each answered with more prose, and no
change was ever measured.

```sh
npm run eval                                  # every scenario, once
npm run eval -- --scenario look-before-you-send --repeat 5
npm run eval -- --lead codex
```

**This spends real tokens and is not part of any automated gate.** Do not run
it in CI or as part of `check:all`: it needs a real lead CLI, a configured roster, and your approval for the spend.

## How it works

The lead is a real CLI reading the bundled `consensflow-lead` document through
the app’s role-loading code. Each evaluation has a private app root and a copy
of the roster; it does not depend on a globally installed ConsensFlow skill. What
*is* replaced is `cf`: a stub that answers plausibly and records every
invocation, first on `PATH`. The lead runs in a throwaway directory.

So its choices become a log, and a log can be asserted. Nothing reaches a real
agent or a real conversation. There is no pane tooling to stub: ConsensFlow
has one shape, the app owns the panes, and the skill never names a pane
command or a harness CLI.

The stub `cf` mints conversation names on `run --new`, continues on a bare
run or `--session`, answers `say`, `results` and `read` from a per-scenario
transcript, pastes a `deliver` fixture into the lead's transcript, and prints
a long `read` fixture in numbered parts. A turn may also carry a `delivery`
field, which the runner prefixes into what the lead receives — the envelope
or pointer arriving in lead context, the way the app pastes it into the pane.
Every invocation is logged, so a lead that invents a command the skill never
taught is caught by the log.

Each scenario is one lead session across several turns, because every failure
worth checking happened on turn two or later.

## Reading the result

A rate per check, not a verdict. Leads are not deterministic: a check that
passes 4/5 is a **failing** check, because the user meets it on the run it
misses. The runner exits non-zero if any check missed even once.

## The scenarios are the standalone contract

| Scenario | What it guards |
|---|---|
| `consult-opens-a-pane` | the consult is `cf run --new`, via `cf` only — no pane tool, no harness CLI |
| `look-before-you-send` | a follow-up rides on the answer already delivered in context — `cf say`, never a restart, no `results`/`read` round-trip |
| `an-independent-task-gets-its-own-conversation` | unrelated work starts fresh with `--new`, nothing sent into the old conversation |
| `a-dependent-task-stays-in-its-conversation` | work that leans on the conversation is a `cf say` on the delivered context where it belongs — no retrieval, no second conversation |
| `a-delivered-answer-is-read-whole` | the envelope arrives in the turn, as pasted into the pane — the lead reports its top verdict with no `catchup`, no `read` |
| `a-delivered-file-is-read` | the pointer arrives in the turn — the lead runs every `cf read` part and its report holds the beginning, the middle AND the end |
| `manual-is-the-humans` | the lead can read for its authorized task and leaves a human-set `manual` policy alone |
| `a-lead-sends-and-returns` | after `cf run --new` or `cf say` the lead reports what is running and where — no `--wait`, no polling |
| `after-dispatch-continues-independent-work` | after dispatch the lead reports what is running and does the authorized independent work in the same turn — no `--wait`, no polling, no retrieval |
| `a-delivered-result-is-used-without-asking` | an automatically delivered full result is used at once — verdict in the report, no retrieval, no read/authorize ask-back |
| `zero-runs-is-not-failure` | a `0 runs` count starts no replacement, polls nothing, and is never declared a failed dispatch or a fallback |

## History

The cmux-era scenarios (pane recipes, `mint`, `tree`, tail-pipe guards) were
retired with the switch-over: they measured a shape that no longer exists.
What they taught is kept — the honest-stage rules: a scenario whose prompt
names a file ships that file, a scenario whose follow-up refers to what the
agent said ships that transcript, and a miss prints every command the lead
ran. A check that passes because nothing was sent is no check at all.
