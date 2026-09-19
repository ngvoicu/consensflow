---
id: tiered-dispatch
title: Tiered dispatch, review gate and quota — the daemon picks the member
status: active
created: 2026-09-19
updated: 2026-09-19
priority: critical
tags: [board, dispatch, tiers, tags, review, quota, ledger]
---

# Tiered dispatch, review gate and quota

Gabriel, 2026-09-19, once the board page had landed: agents carry tags for
what they are good for; a coordinator names only a category of worker, never
a worker, and the daemon picks the member; the daemon knows each member's
quota, gives no work to one that has run out, and takes a task away from one
that runs out mid-work; a project may require a second review of members'
work, and of tasks a lead or PM gives itself; members never hand out tasks or
talk to each other; a session is called a project. Decided the same day: the
four saved work tiers stay as the categories (his five names, max to low,
would collide with the effort levels printed beside every agent), and a
failed review sends the work back to its author automatically, twice, before
the requester sees it.

Built on branch `stage-2/board-daemon-core` after Phase E of
`board-daemon-core` and before its Phase F, so the switch ships the final
commands once. The rename to *project* landed first (`fb24215`), so this work
is written in the final vocabulary.

## Decisions and scope

- **Tiers.** The roster's work tier (critical, complex, standard, light) is
  the category. A task for a member names a tier; the daemon picks the member.
  The tier is a hard filter and is never escalated: no free member of that
  tier → the task waits *open* on the board and the requester is told once;
  a tier with no member on the team at all is refused at creation.
- **Tags.** `tags` on the roster row, editable, defaulting to the profile's
  categories (coding, architecture, problem-solving, reviewer, …). A task may
  name tags; among the tier's free members the daemon prefers the one
  matching most of them. Tags narrow the choice, never exclude.
- **Pools.** A lead's task goes to a worker, a PM's to an advisor; the human
  chooses the pool. A reopened task stays with its author. Coordinators and
  the human stay addressable by handle (`@lead`, `@pm`), and a coordinator may
  give itself a task (`--self`). Among free members of a tier: most matching
  tags, then the fewest tasks taken in this project, then the lowest id.
- **Critical work** needs its purpose (critical-review, architecture,
  hard-problem, important-question), as the old `taskWithWorkPolicy` demanded;
  refused at creation without it; the purpose leads the delivered text.
- **Members never address each other.** Only coordinators and the human
  create tasks; a member's question goes to a coordinator or the human, and
  the API refuses any other recipient. Today `POST /api/questions` takes
  `to` unchecked; that gap closes here, test first.
- **Review policy per project:** `none`, `members` (the default) or `all`.
  Under `members` a member's finished task is reviewed; under `all`,
  coordinators' own tasks too. The reviewer is a team member in the reviewer
  role whose model identity differs from the author's (a different effort or
  harness of the same model is not independent: the old cross-model rule),
  free and with quota, tags preferred as for tasks. No such reviewer → no
  review, and the result reaches the requester with the note "unreviewed: no
  independent reviewer on the team". The verdict is the last line of the
  reviewer's result, `VERDICT: pass` or `VERDICT: changes`; `changes` sends
  the task back to its author with the findings as a follow-up, up to two
  rounds, after which the requester gets the result and both reviews and
  decides; `pass` releases the result and the review to the requester; a
  result with no verdict line is delivered as a review with the note "no
  verdict" and sends nothing back. A coordinator may also ask for a review by
  hand: `cf task review T-n`.
- **Quota** comes from each harness's own record, read by its adapter, and
  `observe()` reports it as `quota: {state: 'ok'|'low'|'exhausted',
  usedPercent?, resetsAt?}` or `null` (unknown). Found on 2026-09-19 in real
  records on this machine, to be reproduced synthetically in tests:
  - Codex: the rollout's `token_count` events carry `rate_limits`
    (`primary.used_percent`, `window_minutes`, `resets_at`,
    `rate_limit_reached_type`) — a warning ahead of failure.
  - Claude Code: an assistant item with `isApiErrorMessage: true`,
    `apiErrorStatus: 429`, `error: 'rate_limit'` — only at failure.
  - OpenCode: `message.error` with `name: 'APIError'` and
    `data.statusCode: 429`.
  - Pi: an assistant message with `stopReason: 'error'` and `errorMessage`
    beginning `429:` (its text names the reset: "Resets in 3 days").
  - Devin: the wire log's "Reached overall message rate limit … reset in N
    minutes", "Usage limit reached" and "Quota exhausted" (documented by other
    users; not yet seen here).
  Rules: `low` (Codex at or above 95 % of its window) takes no new task;
  `exhausted` puts the member out until `resetsAt` (one hour when unknown),
  sends its task in progress back to open with the note for the next member
  "reassigned from @x, which ran out of quota after starting; check the
  working tree for partial changes", and tells the requester; after the reset
  the member is simply eligible again. The lane shows "Out of quota until
  18:00".
- **Ledger:** `participant` gains `tier` and `tags` (copied from the roster
  when the member joins, so the board and `cf team` need no roster read);
  `task` gains `tier`, `tags`, `purpose`, `kind` (work or review),
  `review_of`, `round`, and a nullable `assignee_id`; `project` gains
  `review`; task states gain `open` (no assignee yet) and `review`; message
  states gain `held` (a result waiting for its review). Edited in place: no
  build has shipped the schema.
- Out of scope: per-project tier overrides, reviewer tiers, cost accounting,
  Windows (Phase H of `board-daemon-core`).

## Phases (each: failing tests first, live bench green, dead code deleted)

### Phase A: Model [completed]

- [x] [TEST-TD-01] Roster: `tags` per agent, editable, defaulting to the profile's categories; listed with the agent. `tests/roster.test.mjs`.
- [x] [IMPL-TD-02] `src/roster.js`; satisfies TEST-TD-01.
- [x] [TEST-TD-03] Ledger: open tasks with tier, tags, purpose and pool; assignment; the refusals (no member of the tier, critical without purpose, a member addressing a member, a member creating a task); the review policy and its states (held result, review task, rounds, each verdict's outcome); a task taken back from a member out of quota; the state machine and its doc diagram. `tests/ledger.test.mjs`.
- [x] [IMPL-TD-04] `src/ledger/schema.js`, `src/ledger/index.js`; satisfies TEST-TD-03.

### Phase B: Dispatcher and adapters [completed]

- [x] [TEST-TD-05] Each pass assigns open tasks (pool, tier, tag preference, free, with quota, fewest tasks); waits with one note when none is free; runs a review through two rounds and a review asked by hand; takes a task back when its member's quota runs out and blocks new work on `low`; makes the member eligible again after its reset. `tests/core-dispatcher.test.mjs`.
- [x] [IMPL-TD-06] `src/core/dispatcher.js`; satisfies TEST-TD-05.
- [x] [TEST-TD-07] Every adapter's `observe()` reports quota from synthetic records shaped like the real ones above; an unknown harness record reports `null`. `tests/adapter-*.test.mjs`.
- [x] [IMPL-TD-08] `hosts/lib/quota.js`, the shared reader `hosts/lib/completion.js` (each harness's records already pass through it), `src/adapters/*.js`; satisfies TEST-TD-07.

### Phase C: Commands and roles [planned]

- [ ] [TEST-TD-09] `cf task add --tier <t> [--tags a,b] [--purpose p] "…"`, `--self`, `@lead` and `@pm` between coordinators, `@worker` refused with the tier hint, `cf task review T-n`, a member's question to a member refused, `cf team` with tiers and tags. `tests/core-api.test.mjs`, `tests/core-cli.test.mjs`.
- [ ] [IMPL-TD-10] `src/core/api.js`, `src/core/cli.js`, `skill/core/*.md` (choose the tier, not the worker; review is the project's rule; `cf task review` by hand); satisfies TEST-TD-09.

### Phase D: Board [planned]

- [ ] [TEST-TD-11] An "Open tasks" bay (waiting for a member, per tier, saying why); the human's composer chooses Lead, PM, a worker tier or an advisor tier, with tags; member bays lose "Give a task"; strips show tier, tags and review state ("In review by @x", "Round 2"); a lane out of quota says until when; the team dialog shows each member's tier and tags and the review policy, and warns when review is on with no independent reviewer or a tier has no member; the new-project dialog sets the policy. `app/tests/core-page.spec.mjs`, `tests/core-page.test.mjs`.
- [ ] [IMPL-TD-12] `app/ui/core.html`, `app/ui/core/*.js`, `src/core/page.js`, the Rust allow-list; satisfies TEST-TD-11.

### Phase E: Live proof [planned]

- [ ] [VERIFY-TD-13] Integration through the real pane host: a fake harness whose record shows a 429 mid-task, and the task moves to the tier's other worker with the requester told; a review round trip with a fake reviewer. `tests/integration/`.
- [ ] [VERIFY-TD-14] Live bench (`npm run bench:core`) on the free models plus Codex (its quota came back on 2026-09-19 at 18:40): a lead giving tiered tasks, the daemon picking, one review round, Codex's usage read from its rollout; Claude after its reset.

## Resume context

Phases A and B are built and green (ledger 52, dispatcher 30, page 9, the
reader 75, adapters 30, the end-to-end slice). Next: Phase C (commands and
role texts), then Phase D (the board). The brain's CORE-8 ("coordinators pick the
right member") is superseded by this spec's rules (CORE-10 to CORE-15).

## TDD log

- Probes on 2026-09-19 (a one-shot `codex exec` in a clean env, then a read
  of each harness's newest records): Codex's quota had come back (5 % of its
  weekly window used), so the refusal was not captured, but its rollout showed
  the `rate_limits` field on every `token_count` event. The Claude, OpenCode
  and Pi shapes above are from records already on this machine; Devin's is
  from other users' reports.
- TEST-TD-01 RED (the roster had no tags), GREEN after IMPL-TD-02; one test
  caught a number passing as a tag, fixed in the roster and the ledger alike.
- TEST-TD-03 in two chunks. Open tasks, candidates, assignment and the
  take-back: RED 9/9, then 37/43 after the first implementation; the wrong
  ones were the tests' (a regex added tiers where a test wanted none; an
  expectation put the purpose check after the member check; an assignment
  event carried two `to`s, renamed `assignee`; a release nulled the assignee
  before moving the state and tripped the new check constraint, so it is one
  statement now). The review gate: RED 7/7, GREEN 50/50 with three old tests
  moved to the `none` policy they describe. Then `members()` and
  `withdrawReview` for the dispatcher, RED then GREEN, 52/52.
- TEST-TD-05 RED (30 tests, 8 new), GREEN 29/30 on the first implementation;
  the last one was the test's: calliope shares only zeus's model, so a task by
  diana found an independent reviewer at once. `openProject` had to pass the
  review policy through, which is why three older tests briefly held their
  results for a review.
- TEST-TD-07: the quota signal lives where each harness's record is already
  parsed, the shared reader (`hosts/lib/completion.js`), so the adapters only
  pass it on. Tests mutate the real native fixtures: a Codex `token_count`
  with `rate_limits` (ok, low at 97 %, exhausted when a limit is reached), the
  Claude and Pi `provider-429` fixtures, an OpenCode message with a 429 error.
  RED 3/4 on the first run, all three the tests' (a hand-computed epoch, a
  fixture that retries and succeeds after its 429, the wrong session id); one
  rule came out of it: the latest assistant record has the last word on quota,
  so a later success, or a later error that is not a 429, clears it. Devin has
  no reader signal, so its adapter scans its own wire log for the refusal
  phrases other users report, reading only what was appended since the last
  look; unverified against a real Devin refusal. Adapters 30/30, reader 75/75.
