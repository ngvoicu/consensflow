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

## Phases (each: failing tests first, suites green, one Candidate rebuild)

### Phase A: Dialogs, settings gear, agent cards [completed]

- [x] [TEST-CF2-01] Page: the team table shows tags as chips and the dialog
  is wide; the Settings dialog opens each agents screen; the saved agent's
  card shows tag chips, no description, no "Tags:" line, and its editor has
  no description field.
- [x] [IMPL-CF2-02] Satisfies TEST-CF2-01 (`57fdc74`).

### Phase B: Named worker sessions [planned, awaiting Gabriel's answer]

- [ ] [TEST-CF2-03] Ledger: a session participant per assignment with a
  generated `agent-adjective-noun` handle unique in the project; a member's
  slot count; `--after` continuation assigns to the session that did the
  named task and resumes its conversation; a session ends when its work is
  accepted or cancelled; reopen goes to the same session.
- [ ] [IMPL-CF2-04] Ledger, dispatcher (launch and resume per session,
  retire keeps a continuable conversation), API and `cf task add --after`,
  role texts (when to continue, never to pick).
- [ ] [TEST-CF2-05] Board: a lane per session under its member, folding when
  ended; the dock strip per session; the drawer names the session.
- [ ] [IMPL-CF2-06] Satisfies TEST-CF2-05.
- [ ] [VERIFY-CF2-07] Integration with the fake harness (two sessions of one
  member in parallel; a continuation on the same native session); the bench
  with a continuation on Claude, Codex and OpenCode; Candidate rebuilt.

## Resume context

Written 2026-09-20 mid-morning. Phase A is committed. Phase B is a design
proposal; the answer decides whether it is built as written, changed, or
dropped. Until then, one task per member session (CORE-19) stands.

## TDD log

- 2026-09-20, Phase A. RED: three page tests changed (tag chips in the team
  table, the Settings dialog instead of three buttons, saved agents' tag
  chips and no description) and the harness page suite's category and
  description expectations rewritten for tag pills. GREEN: page 76/76,
  node 814/814, integration 7/7 (`57fdc74`).
