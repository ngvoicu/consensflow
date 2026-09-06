# Evals — does the skill change what a lead *does*?

`npm test` checks what the skill **says**. Nothing checked what a lead **does**
with it, so on 2026-08-24 three behavioural failures in one day were each
answered with more prose, and no change was ever measured.

```sh
npm run eval                                  # every scenario, once
npm run eval -- --scenario reading-is-not-writing --repeat 5
npm run eval -- --lead codex
```

**This spends real tokens.** It is deliberately not part of `npm test`, which
spawns no live CLI and reaches no network.

## How it works

The lead is a real CLI reading the real installed `SKILL.md` from your home —
that file is the artefact under test, so nothing about it is simulated. What
*is* replaced is `cf` and `cmux`: stubs that answer plausibly and record every
invocation, first on `PATH`. The lead runs in a throwaway directory.

So its choices become a log, and a log can be asserted. Nothing reaches a real
pane, a real agent, or a real conversation.

Each scenario is one lead session across several turns, because every failure
worth checking happened on turn two or later.

## Reading the result

A rate per check, not a verdict. Leads are not deterministic: a check that
passes 4/5 is a **failing** check, because the user meets it on the run it
misses. The runner exits non-zero if any check missed even once.

## The scenarios are real failures

Each one happened live, and the fix it guards is in the skill:

| Scenario | The failure |
|---|---|
| `consult-opens-a-pane` | ran `cf run` in its own pane — never opened the skill body |
| `reading-is-not-writing` | asked to READ a conversation, it SENT another request and invented a new answer |
| `look-before-you-send` | a follow-up composed against a stale view asks the wrong question |
| `answers-from-the-conversation` | answered "did she say anything else?" from memory, with the user's pane turns unread |
| `the-consult-line-is-plain` | piped the consult through `tee` and passed a `--prompt-file` beside a quoted task; could not read the result, and started a second conversation on the same work |
| `an-independent-task-gets-its-own-pane` | not live yet — the counterweight to every row above, added 2026-09-05: an unrelated task sent into a live conversation inherits a history it does not need and queues behind it |
| `a-dependent-task-stays-in-its-pane` | the guard against that rule over-correcting: "a test for the case he flagged" only means something in the conversation that flagged it |

## Baseline (2026-08-24, lead: claude)

13/13 checks held on a full pass; the two read-versus-write scenarios held
3/3 each on repeat. That is the number to compare against when the skill's
prose is next cut or rearranged — particularly if the Rules section is
tightened, since these rules were added to it while it was still short.

## What one pass is worth (2026-09-02, lead: claude)

Two full passes over the SAME skill, minutes apart, disagreed:
`reading-is-not-writing` held 4/4 then 3/4, and `look-before-you-send` 3/4
then 4/4 — each dropping a check the other pass held. So the 13/13 above is a
single sample, not a grade, and the README's own rule applies to it: use
`--repeat` before concluding anything about a prose change.

Two failures that looked like regressions were neither:

- **A SIGKILL reads as a failed scenario.** The default timeout was 180s and
  three runs were killed mid-turn; every one of them held all its checks once
  given time. The default is 420s now.
- **`a-long-answer-is-read-whole` failed 0/2 across both passes** and looked
  like the one real regression — until it was run against the skill at HEAD
  and against the changed skill, back to back: 2/2 both times. Consistent
  twice is still not consistent. A/B against HEAD is the cheap way to settle
  it, and it settles it in one pass per arm.

## Both directions of one decision (2026-09-05/06, lead: claude)

Two scenarios were added for the "continue or start fresh" decision, one per
direction, and measuring them found more wrong with the stage than with the
prose. Four stub lies were fixed, each one seen in a lead's log:

- `cf mint` handed out a name that was already taken on a second call;
- `cf run` always said `amber-tide`, so a lead that had just started a second
  conversation was sent back to read the first;
- `cf catchup` answered with jokes whatever the conversation was about, so
  "a test for the case he flagged" met a conversation that flagged nothing —
  scenarios can now supply a `transcript`;
- `cmux new-pane` returned `surface:99` every time and `cmux tree` showed it
  titled with the FIRST conversation, so a lead's new pane arrived already
  wearing the old conversation's name.

The runner now prints every command a lead ran when a check misses. What held
across every honest run: `a-dependent-task-stays-in-its-pane` 5/5 checks on
5/5 runs, and the independent scenario's decision (a fresh name, a new pane,
nothing sent into the old window) 3/3 in every round before the pane stub was
fixed. What is NOT yet measured: whether the fresh consult is then sent into
the new pane rather than run in the lead's own — every earlier miss on that
check happened under one of the stub lies above, and the first run against the
honest stage was cut short by the Claude session limit. Measure it with
`--repeat 3` when the limit resets, before reading anything into the prose.
