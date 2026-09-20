---
id: candidate-feedback-2
title: Second hands-on round on the Candidate — wide dialogs, settings gear, agent cards, named worker sessions
status: active
created: 2026-09-20
updated: 2026-09-20
priority: critical
tags: [board, team, sessions, agents, settings, usability]
---

# Second hands-on round on the Candidate

Gabriel opened the Candidate built from `891adf8` on the morning of
2026-09-20 and wrote down what he saw:

- The Project team dialog "should be bigger; a lot bigger" (a 480 px modal
  with a horizontal scrollbar and tags wrapping four lines).
- "Persistent sessions for each worker": the lead should be able to say that
  some work must be done by the same worker, with generated names like the
  old app's `diana-amber-pine` and `diana-frosty-window`, and each such
  worker gets its own lane on the board as it is generated. "I know that I
  said no" (one task per member session, CORE-19); "what do you think?"
- Your agents, the agent library and the harnesses belong under a settings
  panel behind a settings icon, not three buttons in the command bar.
- On each saved agent's card, delete our description and the "Tags: …"
  line: the card already shows chips ("T4 · Light work", "Coding",
  "Review").
- "Reviews should not be a new lane": a worker's task that gets reviewed
  shows its review on the same lane, never on another.

## Decisions and scope

- **Dialogs**: every dialog is as wide as the window allows up to 1040 px,
  scrolls vertically, never sideways; tags are chips.
- **Settings**: one gear in the command bar opens a Settings dialog that
  names the three screens and opens each in the agents window.
- **Agent cards**: a saved agent shows its tier and its tags as chips (what
  the daemon picks it by) and nothing said twice; the description line and
  the "Tags:" line go, and the editor edits model, effort, tags and tier.
  The library's offers keep the catalog's categories.
- **Named worker sessions** (the design put to Gabriel on 2026-09-20; built
  once he confirms):
  - A member (`@diana`) stays the team entry: tier, tags, roles. A
    **session** is a named window of a member, `diana-amber-pine`, created
    when a task is assigned; it holds the conversation and the task. It is a
    participant of its own, so it has its own lane, inbox and token.
  - Fresh work still goes to a **new** session of a free member of the tier
    (CORE-19 stays the default: a worker starts from nothing).
  - The lead may **continue** a session for work that needs its context:
    `cf task add --after T-3 "…"` sends the follow-up to the session that
    did T-3; the daemon resumes that session's own conversation with the
    new task in front. Never by name for fresh work: the lead still does
    not pick favourites.
  - A member may run **several sessions at once** (a per-member cap, two by
    default), so one agent can take parallel tasks; the daemon picks a
    member with a free slot as it picks one today.
  - A session **ends** when all its work is accepted or cancelled, or after
    a set idle time; its lane folds into the member's row and its window
    closes; a continued session comes back on the same conversation.
  - Reopening a task goes back to the session that did it (the same rule),
    not to a fresh one.
  - A **review** runs in a reviewer's session (a window in the dock) but on
    the board it lives under the reviewed task, on the worker's lane; a
    reviewer session never gets a lane of its own.

## Phases (each: failing tests first, suites green, one Candidate rebuild)

### Phase A: Dialogs, settings gear, agent cards [completed]

- [x] [TEST-CF2-01] Page: the team table shows tags as chips and the dialog
  is wide; the Settings dialog opens each agents screen; the saved agent's
  card shows tag chips, no description, no "Tags:" line, and its editor has
  no description field.
- [x] [IMPL-CF2-02] Satisfies TEST-CF2-01 (`57fdc74`).
- [x] [TEST-CF2-02b] Page: the reviewer's row holds no card and no note for a
  review; the review reads under the reviewed task's card, and the reviewer's
  row status says "Reviewing T-n".

### Phase B: Named worker sessions [active]

- [x] [TEST-CF2-03] Ledger: a session participant per assignment with a
  generated `agent-adjective-noun` handle unique in the project; a member's
  slot count; `--after` continuation assigns to the session that did the
  named task and resumes its conversation; a session ends when its work is
  accepted or cancelled; reopen goes to the same session.
- [x] [IMPL-CF2-04] Ledger, dispatcher (launch and resume per session,
  retire keeps a continuable conversation), API and `cf task add --after`,
  role texts (when to continue, never to pick).
- [x] [TEST-CF2-05] Board: a lane per session under its member, folding when
  ended; the dock strip per session; the drawer names the session.
- [x] [IMPL-CF2-06] Satisfies TEST-CF2-05.
- [ ] [VERIFY-CF2-07] Integration with the fake harness (two sessions of one
  member in parallel; a continuation on the same native session); the bench
  with a continuation on Claude, Codex and OpenCode; Candidate rebuilt.

## Resume context

Written 2026-09-20 mid-morning. Phase A is committed. Gabriel confirmed
Phase B at noon: "default is without name but if the lead wants the same
worker to continue something it should be able to do so; we need to improve
the lead's skill to know how to use cf and to give it good instructions about
what it should do and what it can do." Built the same day; the live
continuation run and the Candidate rebuild close it.

## TDD log

- 2026-09-20, Phase A. RED: three page tests changed (tag chips in the team
  table, the Settings dialog instead of three buttons, saved agents' tag
  chips and no description) and the harness page suite's category and
  description expectations rewritten for tag pills. GREEN: page 76/76,
  node 814/814, integration 7/7 (`57fdc74`).
- 2026-09-20, Phase B. A third migration adds `participant.member_id` (the
  one unnamed constraint: `ADD COLUMN` cannot name one). A session is a
  participant of its own, `zeus-amber-pine` (`src/ledger/names.js`, names
  injectable for tests), with its member's agent, harness, tier, tags and the
  role its task needs; `assignTask` and `createReview` start one; a member
  holds two (`SESSION_SLOTS`); a session ends when its work is accepted or
  cancelled, at a review's verdict, when its task is released, or idle for
  two hours (`SESSION_IDLE_MS`, swept each pass); an ended session's tasks
  fold into its member's lane. `createTask({ after })` continues the session
  that did T-n, alive and free, else `session-ended` or `session-busy` with
  what to do instead; reopen goes to the same session. The dispatcher's
  retire kills the window only (the conversation stays for `--after`), quota
  is charged to the member, windows of sessions that ended are closed after
  the pass. `cf task add --after T-3`, the team route without sessions, the
  lead's skill rewritten as what you do, what you never do, and the one
  exception; the PM's gains the exception. The board draws each session as a
  lane under its member ("@zeus · amber-pine", "worker session of @zeus"), the
  member's row counts its open windows, the dock names sessions the same way,
  and the composer offers tiers from members only. The fake harnesses and
  the bench find a member's newest session; the bench continues each worker's
  window with `--after` and checks the same window answers. RED: eight ledger
  tests, two dispatcher tests, one API test, role assertions, one page test.
  GREEN: ledger 74, dispatcher 43, node 825/831, integration 7/7, page
  77/77. Found on the way: the dispatcher's sweep for closed sessions must run
  after the pass's steps, or a removal racing a launch wins the lock first;
  by-name tasks stay direct to their participant (only tests use them), so
  sessions come with tiered work, reviews and continuation.
- 2026-09-20, Phase B live. Bench `claude opencode codex` on `0551371`,
  Devin reviewing: every worker's task, result and closed window as before;
  each window continued with `cf task add --after T-n` came back as the same
  session (`bench-claude-coral-pine`, `bench-opencode-gentle-canyon`,
  `bench-codex-pale-harbor`) in 2-3 s and answered `BENCH_AGAIN_*` in 6-13 s;
  the review gate and the restart green. The three question checks timed out
  because the bench matched the question's sender to the member's name
  while the session asked; the workers had asked and been answered. The
  check now matches the session; rerun pending.
