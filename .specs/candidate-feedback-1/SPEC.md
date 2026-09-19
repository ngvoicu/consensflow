---
id: candidate-feedback-1
title: First hands-on round on the Candidate — kanban, team, agents window, questions
status: active
created: 2026-09-20
updated: 2026-09-20
priority: critical
tags: [board, kanban, team, roles, questions, agents, dock, usability]
---

# First hands-on round on the Candidate

Gabriel opened ConsensFlow Candidate on the night of 2026-09-19 with a real
project (a poker engine), a Claude lead, a Codex worker and an OpenCode
reviewer, and wrote down what he saw. His words, in the order they came:

- Talking to the lead in the dock makes no task ("ok...").
- A worker's question: who answers it? And the harnesses' own question
  dialogs: "don't close the door"; the questions and their options should go
  to the lead or PM's inbox, the answer back to the worker's inbox, and a lead
  or PM's question straight to the human.
- The lead "was able to put task for a particular team member".
- The Agents button shows a blank page.
- The New project dialog has no way to add the team.
- The lead's window shows the "@diana joined" note twice, wrapped in Claude's
  peer-message warning.
- The lead struggled to find how to put a task; "doesn't it have a skill?"
- A member's docked terminal disappears when switching to the lead, and
  vanishes when its task ends, "even from memory?"
- A finished task is gone from the board: "how can I read the answer?"
- The lead receives notes about team changes and "could not get reviewed".
- A review policy should require at least one reviewer.
- Agents should be able to be reviewers and workers, not one role.
- The team dialog "looks kinda bad; you don't understand what you have to do".
- The lead reasoned about which member would get a task from the tags:
  remove the descriptions from the team listing, keep the tags.
- "We should have tasks as a kanban board, persisted and updated with the
  results."

## What the facts said (2026-09-20 morning)

- The lead never named a member. It ran `cf task add --tier light --tags
  review,poker`; diana was the only light worker. The problem is the lead
  reasoning about members from `cf team`'s descriptions, not the daemon.
- The note reached the lead once (one attempt, confirmed in the same second).
  Claude Code shows a peer message as a preview line and again when the turn
  starts; the warning text is Claude Code's framing of every peer message.
- The Agents dialog loads the daemon's page in an iframe over
  `http://127.0.0.1` inside the app's secure page; WebKit blocks that as
  mixed content, hence the blank dialog.
- "T-1 unreviewed" was correct at the time: the reviewer joined five minutes
  after the result. Nothing refused a review policy with no reviewer.
- The lead's transcript: `cf task add --help` prints one usage line, `cf
  --help` says "unknown command"; it found the way after `cf team`.
- Munder Difflin keeps questions on the task card with an "Ask me" board;
  Omnigent routes Claude's native prompts, `AskUserQuestion` included, through
  Claude's `PermissionRequest` hook to a web card and returns the verdict to
  the hook (brain: `research/omnigent-and-munder-difflin.md`).
- Claude Code drops identical peer messages arriving within a short window
  and rate-limits per sender (its docs): our 60 s re-send of a message its
  inbox had already queued made a duplicate or a silent drop. Fixed first,
  before this spec: a natively queued delivery waits for the record.

## Decisions and scope

- **The board is a kanban of tasks** (CORE-1 restated): columns by state,
  rows by participant (human, lead, PM, members), every task a card that
  stays after it is done, with its result readable from the card. States:
  Backlog (open), Queued, Working, Waiting (a question), In review, Done,
  Accepted, Failed and Cancelled folded at the end. Member status (the lamp)
  lives on the row head. The drawer stays for the thread and the actions.
- **A member may hold several roles**: worker, advisor, reviewer, as a set on
  the participant. Assignment asks "members with this role"; one task at a
  time per member still holds; a member never reviews its own task (same
  participant) and never a task whose author shares its model (already the
  rule).
- **A review policy other than none requires at least one reviewer role on
  the team**, checked when a project opens with a policy, when the policy
  changes, and when the last reviewer would leave (the ledger refuses with a
  named reason; the dialogs say it).
- **The Agents, Library and Harnesses screens open in their own app window**
  at the daemon's address with the UI token, no iframe. The board refreshes
  its agents when that window closes.
- **The dock is a horizontal strip of every window**, the lead first, then
  the PM, then the members, scrolling sideways as the old app did (Gabriel,
  2026-09-20), not one docked window with tabs. A finished window's terminal
  stays in the strip, marked ended and readable, until the member's next
  window or the human closes it.
- **Coordinators get no team-change notes.** A task cancelled by a member
  leaving is still reported to its requester. An unreviewed result carries
  its reason in the result delivery, not as a separate note.
- **`cf team` and the skill's team table show name, roles, tier and tags**,
  no descriptions. The role texts say tags are what a coordinator may prefer
  with, never a way to pick a member.
- **Role texts start with a command card** (six lines), and `cf --help` and
  `cf task --help` print the full usage.
- **The New project dialog has the team step**: the last team pre-filled,
  editable, with the review policy beside it.
- **Native questions are relayed to the board** where a harness lets us see
  them: Claude through its `PermissionRequest` hook (`AskUserQuestion` with
  its options), Codex through the app-server the supervisor already runs
  (`request_user_input`), OpenCode and Pi through their plugins if their
  question tools expose a hook. Devin has no such door; a Devin question
  shows as waiting with "open the terminal". A relayed question becomes a
  board question with its options to the task's requester (a lead's or PM's
  to the human); the answer is fed back to the dialog and the task continues.
  Researched per harness before building (Phase E).
- **Reviews sit under the task they review** on the board (reviewer, round,
  verdict), never as separate cards; the reviewer's row shows only that it is
  reviewing (Gabriel, 2026-09-20: "hierarchical"). The ledger keeps reviews
  as tasks of kind review; the board groups them.
- **One finished task, one delivery** to its requester: the result with its
  review verdicts under it, in plain words; no separate note, no message
  numbers in what the human reads. The reviewer's findings stay readable on
  the board under the task. (Gabriel saw three deliveries and a note with
  numbers for one task: "wtf is this?", "everything is so confusing".)
- **The result stands apart** from the brief: its own block on the card and
  in the drawer; a review brief's "the task" and "the result" as labelled
  sections.
- Not in scope: chat with the lead making tasks by itself (a role-text line
  telling coordinators to put chat work on the board with `--self` is enough
  for now, and is part of the command card).

## Phases (each: failing tests first, suites and the packaged smoke green, one Candidate rebuild per phase)

### Phase A: Agents window, one delivery per task, the command card [completed]

- [x] [TEST-CF1-01] Rust: an `open_agents_window` command opens (or focuses) a window labelled `agents` at the daemon's URL with the token and the page path; refused without a daemon handle. Page: the three buttons call it; the board refreshes agents when the window closes. Smoke: the window exists after the page asks for it.
- [x] [IMPL-CF1-02] The command, the page wiring, the dialogs removed; satisfies TEST-CF1-01.
- [x] [TEST-CF1-17] Ledger: a reviewed task's release is one delivery to the requester, the result with the review verdicts under it in plain words (pass, changes twice, unreviewed), no separate note; the reviewer's findings are on the review task. Bench: `review-received-by-lead` reads the verdict from that delivery.
- [x] [IMPL-CF1-18] Satisfies TEST-CF1-17.
- [x] [TEST-CF1-03] `cf --help` and `cf task --help` print the usage; the lead, PM, worker, advisor and reviewer texts open with the command card; `cf team` and the skill table carry no descriptions; the coordinators' texts say tags prefer, names are not theirs to pick.
- [x] [IMPL-CF1-04] Satisfies TEST-CF1-03.

### Phase B: The kanban [completed]

Design, 2026-09-20, before the code:

- **Grid**: one row per participant (lead, PM, then members; the PM's team
  after the lead's), one column per state: Backlog (open, in the requester's
  row), Queued, Working, Waiting, In review, Done, Accepted; Failed and
  Cancelled fold into one last column, collapsed until opened. The row head
  carries the lamp, the name, tier and tags, the status line and the tools
  (Terminal; Give a task for a coordinator). A row scrolls sideways when the
  columns do not fit; the header row stays.
- **For you** stays above the grid: questions to answer in place, results
  addressed to the human, and the New task button that opens the composer.
- **Card**: number and title, requester to assignee, age; a done or accepted
  card shows the first line of its result (`board.get` gives each task its
  `result` line); a card's reviews sit under it as one line each (reviewer,
  round, verdict); review tasks are never cards of their own, and a
  reviewer's row shows "reviewing T-3" while it works. Clicking a card opens
  the drawer.
- **Drawer**: the brief first, then the result as its own block, then each
  review as a block with its findings, then the rest of the thread; the
  actions as today. A review brief's "the task" and "the result" sections are
  rendered as labelled sections.
- **Dock**: `#stage` is a horizontal strip of window cards, lead first, PM,
  then members in row order; it scrolls sideways; the Terminal button on a
  row scrolls its card into view; an ended window's card stays with an
  "ended" badge and a close button until the member's next window replaces
  it or the human closes it. The dock tabs go.


- [x] [TEST-CF1-05] Page: columns by state, rows by participant, cards that stay when done with the result's first line, the drawer from a card, the failed and cancelled fold, the lamp on the row head, the composer where the Open tasks bay was. Data: `board.get` returns every task of the project, not the active ones.
- [x] [IMPL-CF1-06] Satisfies TEST-CF1-05.
- [x] [TEST-CF1-07] Dock: a horizontal strip of every window, lead first, scrolling sideways; an ended window's terminal stays in it, readable and marked ended, until the member's next window or the human closes it.
- [x] [IMPL-CF1-08] Satisfies TEST-CF1-07.

### Phase C: The team [completed]

- [x] [TEST-CF1-09] Ledger: `roles` as a set per member; `members(project, role)` by role; a worker-reviewer under review of its own work is busy and takes no review; `addMember` and `removeMember` and `setReview` refuse a policy with no reviewer role, with a named reason; `lastTeam` carries roles.
- [x] [IMPL-CF1-10] Schema (in place; nothing shipped it), ledger, API (`roles`), CLI `cf team`, page operations; satisfies TEST-CF1-09.
- [x] [TEST-CF1-11] Page: the team dialog as a table (agent, roles as checkboxes, tier, tags) with an add row and the review policy with its reviewer rule shown; the New project dialog with the team step; the join notes gone from the lead's lane.
- [x] [IMPL-CF1-12] Satisfies TEST-CF1-11; the dispatcher launches a member with the text of the role its task needs.

### Phase D: Live proof [active]

- [ ] [VERIFY-CF1-13] Bench green on the free models and on Claude and Codex after Phases A to C; Candidate rebuilt; Gabriel's second round.

### Phase E: Questions relayed to the board [planned]

- [ ] [TEST-CF1-14] Research note per harness in the brain: how a pending question is seen and answered (Claude hook, Codex app-server, OpenCode and Pi plugins, Devin none), with the exact fields.
- [ ] [TEST-CF1-15] Ledger and dispatcher: a relayed question with options becomes a board question to the requester (human for coordinators); the answer returns through the harness's own door; the task waits meanwhile; a question nobody answers is shown in the human's row after a set time.
- [ ] [IMPL-CF1-16] Claude first, then Codex, then OpenCode and Pi; each proven with the fake harness and once live.

## Resume context

Written 2026-09-20 morning after Gabriel's first hands-on round. The
delivery re-send bug is fixed and committed first. Phase A starts with the
agents window (a blank dialog today) and the command card. The kanban is
Phase B, the team model Phase C, the question relay Phase E after its
research. The `board-daemon-core` spec keeps Phases G to I (transcript copy,
Windows, acceptance) and the importer. Phases A to C are committed; Phase D
(the bench on every harness, the Candidate rebuild) is next, and the rebuild
waits for the running Candidate to be quit.

## TDD log

- 2026-09-20: a natively queued delivery waits for the harness record instead
  of being sent again after 60 s (dispatcher test: queued at 150 s confirmed,
  attempts 1; a paste at 61 s sent again, attempts 2). The adapters mark the
  peer, broker and plugin channels as queued; three adapter tests learned the
  shape. Found through Claude Code's docs while reading up on peer messages.
- 2026-09-20, Phase A. Agents window: Rust `open_agents_window` (a second
  window at the daemon's address with the token, reused and turned to the
  asked page; refused without a daemon), one Rust test on the URL, the page
  calls it from the three buttons and refreshes its agents when it is back in
  front (page test), the framed dialogs and their styles gone, a smoke step
  that opens it twice. One delivery per finished task: the reviewer's
  findings stay on the review task as a `read` message, the result goes to
  the requester with `Reviewed by @x, round n: verdict` and the findings under
  it, "The reviewer asked for changes twice. Accept it, or send it back with
  what to change." after the last round, "Unreviewed: reason." when skipped
  (the task keeps the reason in a new `unreviewed` column); no notes. Seven
  ledger and dispatcher tests moved to that shape, the integration test and
  the bench read the findings from the review task. Command help: `cf
  --help`, `cf help`, bare `cf`, `cf task --help` (two API tests); a command
  card opens every role text (roles tests); `cf team` and the skill's team
  table show name, role or tier, and tags only (API and roles tests). The old
  core's skill generator (`generateSkill`, its `skill/roles` texts, the `cf
  run` line on the agents page) was dead since the switch and went with this:
  every window gets its role text from the core, and a launch without one is
  refused (role-skills tests rewritten, 18 cases across five harnesses and
  five roles).
- 2026-09-20, Phase B. The board is a kanban: a table with a row per
  participant (the human's row holds its backlog) and a column per state,
  Failed and Cancelled sharing the last one; cards stay where they end and
  a done card shows the first line of its result (`board.get` gives every
  task its `result` line); a task's reviews sit under its card, one line
  each, and a reviewer's row shows "Reviewing T-n" while it works; "For
  you" above the grid keeps the questions, results and the composer. The
  drawer shows the brief, the result as its own block, each review with its
  findings (`task.get` carries `reviews`), then the rest of the thread. The
  dock is a horizontal strip of every window in row order; a row's Terminal
  button brings its card into view; an ended window stays, marked ended,
  until the member's next window or its Close. RED: nine page tests
  rewritten and one added (an ended window kept, then closed), a ledger
  test for the result line and the reviews. GREEN: 70/70 page tests, 55/55
  ledger. Found on the way: a stray brace left by the tab-styles removal
  silently disabled every rule after it (the board lost its overflow and the
  dock intercepted clicks on cards); a brace count is now part of the
  checklist for stylesheet edits.
- 2026-09-20, Phase C. A member holds a set of roles (`roles` JSON column,
  `role` stays its first): `members(project, role)` reads the set, the
  reviewer check for a review task reads it, a worker-reviewer under review
  of its own work is busy and takes no review. A review policy needs a
  reviewer: `createProject` defaults to `members` when one is on the team,
  else `none`; `setReview`, `removeMember` and `setRoles` refuse to leave a
  policy without one (`no-reviewer`, `last-reviewer`). Joining leaves no note
  for the lead. The dispatcher launches a member with the text of the role
  its task needs (`#roleFor`: a review task opens the reviewer text). Page:
  `team.last` operation; the team dialog is a table (member, three role
  boxes, tier, tags, Remove with its confirm) with an add row of role boxes;
  the review choices are held at `none` with a warning until a reviewer is
  ticked; the New project dialog lists every saved agent with the last team's
  roles ticked and sends the team explicitly. RED: ledger tests for the role
  set and the reviewer rule, a dispatcher test for the role text, protocol
  tests for `roles`, seven page tests (two rewritten, five added), the CLI
  `cf team` line `worker+reviewer`. GREEN: 797/797 node, 73/73 page. Found on
  the way: Playwright's `uncheck` counts a tick the page puts straight back
  as a failure, so a refused change is tested with a plain click.
