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
