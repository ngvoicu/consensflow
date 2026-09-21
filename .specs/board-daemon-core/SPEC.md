---
id: board-daemon-core
title: Board, daemon and inboxes — the Stage 2 rewrite of the daemon core
status: active
created: 2026-09-19
updated: 2026-09-19
priority: critical
tags: [rewrite, board, daemon, inbox, ledger, adapters, windows]
---

# Board, daemon and inboxes (Stage 2)

The rewrite of ConsensFlow's daemon core that Gabriel authorized on 2026-09-19
("if you need to rewrite old consensflow please do so, we need clean code and
coherence"). Started on 2026-09-19 on branch `stage-2/board-daemon-core`, while
the Claude and Codex runs of `.specs/reliable-delivery`'s live bench wait for
their model quotas. Requirements,
research and decisions live in the `consensflow-sme` brain: requirements
(core model, platform and quality, engineering principles), decisions (Stage 2
direction, inbox queue and transcript copy, yolo everywhere), research (Omnigent
and Munder Difflin lessons, harness launch audit, reliability diagnosis).

## What the product becomes

A board is the main screen. Every participant — lead, PM, each advisor, each
worker, and the human — has a lane and an inbox. A coordinator runs one command
to create and dispatch a task; the daemon opens or resumes the right pane,
delivers the task, watches the agent through its harness's own signals, collects
the answer, attaches it to the task and queues it in the requester's inbox, from
which the daemon delivers it one item at a time when the requester is idle.
Questions travel the same way, to a coordinator or to the human. Nothing else
opens panes or types into them.

## Decisions and scope

- **Keep the shell, rebuild the core.** The Tauri app (window, Rust PTY host,
  input arbiter, updater) and the bridge protocol stay; the Node daemon core is
  rebuilt.
- **Built beside the old core, switched once.** The new core is new modules
  with its own daemon entry. From Phase B on, the integration harness and the
  live bench start it with the real Rust pane host, so it is proven end to end
  before the app uses it. The app changes over in one phase (the switch), which
  deletes the old core: `store.js`, `tabs.js`, `panes.js`, `delivery-watch.js`,
  the inbox, delivery and receiver modules it replaces, `page.js`, `tasks.js`,
  `launch.js`, `requester.js`, the JSON state files and the `O_EXLOCK` lock. The
  branch merges only when the new core is the only core, so no shipped build
  ever has two sources of truth (ENG-2).
- **One ledger:** SQLite through the built-in `node:sqlite` at
  `<home>/consensflow.db`, WAL mode, one writer (the daemon). Schema versions and
  migrations are code. The board and every inbox are views of it.
- **The ledger's lock is the instance lock.** The daemon opens the file with
  `locking_mode=EXCLUSIVE` and takes the lock at once; a second process gets
  "database is locked", and the operating system releases the lock when the
  holder dies (probed on 2026-09-19). It works on Windows too. Nothing but the
  daemon opens the file: `cf`, the page and the tests go through the daemon.
- **The model.** A *session* is one board: a lead with its workers and,
  optionally, a PM with its advisors, plus the human. A *participant* is one
  member with a lane and an inbox (human, lead, PM, advisor, worker) and at most
  one pane at a time; parallel work goes to different participants. A *task*
  has a requester, an assignee and a state. A *message* is one inbox item
  (task, result, question, answer, note) with a delivery state and a receipt.
  A *conversation* is a participant's native harness session, kept as history.
  An *event* is an append-only line of the session's log.
- **Adapters own the harnesses:** one module per harness (Claude Code, Codex,
  OpenCode, Pi, Devin; Kimi kept but paused) with one interface: launch
  (full-permission mode, settings, extensions), deliver (native input first),
  signals (working, idle, waiting with a reason, turn ended), collect (the answer
  written after a delivery), transcript source.
- **Signals before transcripts:** status comes from each harness's own signals
  (Claude's sessions status file and hooks, Codex app-server events, OpenCode
  server events, Pi extension events, Devin hooks), one publisher per pane.
  Transcript parsing is only for collecting answers, from ConsensFlow's own copy.
- **Inbox queue, one by one:** every result, question, answer and follow-up is a
  message queued for its recipient; the daemon delivers the head of each queue
  when the recipient is idle, proves arrival from a native signal, and keeps a
  receipt. A long result is delivered as a short notice plus `cf inbox read`.
- **Daemon-only dispatch:** `cf task add --to @agent` creates and dispatches;
  `cf run`, `cf say`, `cf attach`, `cf read`, `cf results`, `cf lead send/read`
  and every way for an agent to open a pane are removed.
- **Session team:** the human picks the agents a session may use (Agent library
  → Your agents → Session team, with uses: worker, advisor, reviewer); the
  daemon refuses assignments outside the team; the previous team is reused.
- **Transcript copy:** the daemon copies every transcript it launched into
  `<home>/transcripts/` incrementally, with a retention rule.
- **Windows:** everything above runs on Windows; the automated suites run on a
  Windows runner.
- Out of scope: cloud or cross-machine sessions; a separate background daemon
  process (revisit only if real crashes appear).

## Phases (each: failing tests first, live bench green, dead code deleted)

### Phase A: Ledger [completed]

- [x] [TEST-BDC-01] Ledger schema, migrations and the domain operations (sessions, participants, team, conversations, tasks, messages, events), the instance lock, crash safety and the invariants each operation keeps. `tests/ledger.test.mjs`, 34 tests (the session team among them).
- [x] [IMPL-BDC-02] `src/ledger/` on `node:sqlite` (`index.js`, `schema.js`); satisfies TEST-BDC-01.

### Phase B: Harness adapters [active]

- [x] [TEST-BDC-05] Adapter contract (launch, deliver, signals, collect, transcript source) with a fake harness per adapter. The contract is `prepare`, `started`, `ready`, `deliver`, `observe` (documented in `src/core/dispatcher.js`): `tests/adapter-claude.test.mjs` (7), `adapter-opencode` (5), `adapter-pi` (4), `adapter-devin` (5), `adapter-codex` (5).
- [x] [IMPL-BDC-06] Claude Code adapter (status file, hooks, peer delivery, Stop-hook turn end): `src/adapters/claude-code.js`. Live proof is VERIFY-BDC-08.
- [x] [IMPL-BDC-07] OpenCode, Pi, Devin and Codex adapters (`src/adapters/`, registered in `index.js`). Waiting signals beyond Claude's are open work for VERIFY-BDC-08; live proof per harness is VERIFY-BDC-08.
- [ ] [VERIFY-BDC-08] Live bench per adapter, as lead and as worker, with the test models. Green for the free models (run 4, 14/14: an OpenCode lead dispatching to OpenCode, Pi and Devin workers, plus the restart). Open: a Claude lead and worker (Sonnet, after the weekly limit resets), Codex (quota), and OpenCode, Pi and Devin as leads.

### Phase C: Dispatcher and inbox delivery [active]

- [x] [TEST-BDC-09] Task state machine; per-participant queue delivering one item at a time when idle; receipts; retries; the launch-liveness reaper; restore on start. `tests/core-dispatcher.test.mjs` (22) and, end to end through the real pane host, `tests/integration/core-slice.test.mjs`.
- [x] [IMPL-BDC-10] Dispatcher (`src/core/dispatcher.js`), pane host (`pane-host.js`) and the daemon entry (`daemon.js`); satisfies TEST-BDC-09.
- [x] [TEST-BDC-22] A human's Enter releases the typing latch once the harness records that submission; typing after it keeps the latch. Found by the Stage 2 harness map: no production code ever calls `clear_draft`, so one keystroke blocks every later paste into that window (Devin always pastes).
- [x] [IMPL-BDC-23] Rust `draft.clear` bridge operation and the core's use of `pane.enter`; satisfies TEST-BDC-22.

### Phase D: CLI, API and role skills [active]

- [ ] [TEST-BDC-11] `cf task add/list/get/accept/reopen`, `cf inbox`, `cf ask`, `cf answer`; team enforcement; the removed commands are gone.
- [x] [IMPL-BDC-12] CLI (`src/core/cli.js`, incl. `cf team`), daemon API (`src/core/api.js`) and new role instructions for lead, PM, advisor, worker and reviewer (`skill/core/`, `src/core/roles.js`), passed to each window through its adapter. The old commands go at the switch (TEST-BDC-11 stays open until then).

### Phase E: Board-first UI [completed]

- [x] [TEST-BDC-13] Playwright: lanes per participant, cards and markers, card detail, the human inbox with question answering, the team picker, session views kept. `app/tests/core-page.spec.mjs` (16), with the data side in `tests/core-page.test.mjs` (9). What the old page has and this one does not yet (window geometry, the Agents, Library and Harnesses dialogs, the self-test hook) moves over at the switch, IMPL-BDC-21.
- [x] [IMPL-BDC-14] The board page on the new core's page protocol: bays grouped by team, the Lead and PM window views, the session team with removal, the PM.

### Phase F: The switch [active]

Decided on 2026-09-19 with Gabriel's "when can we open the app": the first
hands-on build comes before the importer. The Candidate has its own home
(`~/.consensflow-candidate`), so it starts with an empty board and he creates
a project; the live app and its JSON state are never read. There is no
importer: Gabriel, 2026-09-21, "we will delete everything when we have to go
live and start from clean, everything from the old ConsensFlow will go away."
TEST-BDC-03 and IMPL-BDC-04 are dropped; go-live is a clean home.

- [x] [TEST-BDC-24] The agents screens keep working on the new core: the roster editor (with the new tags field), the agent library and the harness diagnostics are served by the new daemon behind the UI token the app already checks; their routes' tests move with them. `tests/core-agents-server.test.mjs`, `app/tests/core-page.spec.mjs`.
- [x] [IMPL-BDC-25] `src/core/agents-server.js` (the pages and `/api/agents…` routes out of `src/ui.js`), mounted on the new core's API server; the new daemon's handle carries the UI token; the board page gains the three dialogs; satisfies TEST-BDC-24.
- [x] [IMPL-BDC-21] `cf ui` runs the new core and `core.html` becomes `index.html`; window geometry and the update flow keep working; the packaged smoke drives the new page; every suite and the live bench are green. Every suite, the packaged smoke and the bench (`npm run bench:core -- opencode pi devin --reviewer devin`, 19/19) green on the switched tree, 2026-09-19 evening.
- [x] [IMPL-BDC-26] The old core is deleted: `src/ui.js`, `store.js`, `tabs.js`, `panes.js`, `delivery-watch.js`, the inbox, delivery and receiver modules it replaces, `page.js`, `tasks.js`, `launch.js`, `requester.js`, the old page (`app/ui/index.html`, `panes.js`, `sidebar.js`, `tasks.js`, `menus.js`), the old Rust commands, the JSON state and `O_EXLOCK`, and their tests; the Devin hook text stops naming `cf results`. Done 2026-09-19. Still to prune in a follow-up: the one-shot runner the old `cf run` used (`runAgent`, the engines, image runs, packets, harness transcripts, session binding, threads, Codex auth, transcript events) and their tests; image agents have no adapter in the new core.
- [x] [VERIFY-BDC-27] `npm run candidate` builds, smoke-tests and installs ConsensFlow Candidate; Gabriel is told it is ready. Installed 2026-09-19 21:07 (`~/Applications/ConsensFlow Candidate.app`, 3.0.0-alpha.62, smoke 2/2, the live app unchanged).
- [x] [TEST-BDC-28] One task per member session (Gabriel, 2026-09-19 evening: "make all workers one task only"; the lead cannot know a worker, so the session rule is the daemon's). Ledger: a member holds its work from assignment to the verdict and is busy meanwhile; only its task's messages reach it. Dispatcher: a worker, advisor or reviewer window closes and its conversation ends once it holds no task; a send-back lands in the author's still-open session; the reviewer closes after its verdict; coordinators never close; a fresh session whose first message is a reopening gets the brief in front; after a restart a member task with no window is given up and one with an answer due resumes its own session; low quota outlives the window until its reset. Page: a member between tasks reads as free. Integration: two tiered tasks, one worker, two native sessions. Bench: the worker's window closes after its task.
- [x] [IMPL-BDC-29] `holdsWork`, `HELD_TASK_STATES` in `members()` and `nextDelivery`; `#retire`, `retiring`, `#launchText`, the restart give-up and `lowUntil` in the dispatcher; the role texts (workers, advisors and reviewers start from nothing; the lead and PM put everything in the task); the composer placeholders; satisfies TEST-BDC-28.
- [x] [TEST-BDC-03] *Dropped 2026-09-21:* no import from the old app's state; go-live starts from a clean home.
- [x] [IMPL-BDC-04] *Dropped with TEST-BDC-03.*

### Phase G: Transcript copy [done]

Gabriel, 2026-09-21: "when the task is in done I can't open the terminal to
see what the agent wrote?" and, on the fix, "we don't copy anything into the
project's home but in .consensflow-candidate / .consensflow (the home)."

- [x] [TEST-BDC-15] The daemon keeps its own copy of every window's conversation in the ledger (the home's `consensflow.db`, a `transcript` table that lives and dies with its conversation's project): each look at a window copies the new items and the one still being written, an item is cut at 64 000 characters, a record that shrank is copied over. `ledger.transcript(project, task)` reads the assignee's conversations in order, the last 300 items by default; a continued window shows the same copy; a task on the board shows nothing. The page reads it with `task.transcript`; the drawer shows "What the agent did" under the thread with each item's role, text and whether it is still being written. Retention: with the project. Answers are still collected from the live record, as before.
- [x] [IMPL-BDC-16] `copyTranscript` and `transcript` in the ledger (migration 8), `#copyTranscript` in the dispatcher, `task.transcript` in the page and the Rust allow-list, the drawer section; satisfies TEST-BDC-15. Done 2026-09-21.

### Phase H: Windows [planned]

- [ ] [TEST-BDC-17] Windows runner for Node and Rust suites; process-tree termination and single-instance guarantees on Windows.
- [ ] [IMPL-BDC-18] Job Objects in the PTY host, Windows updater path, path and shim handling.

### Phase I: Acceptance [planned]

- [ ] [VERIFY-BDC-19] Full live bench matrix, Playwright, a computer-use pass over the Candidate, an upgrade from a copy of the live home, hours-long CPU and memory profile.
- [ ] [VERIFY-BDC-20] Hand the Candidate to Gabriel for the live test.

## Resume context

The switch is made and installed (2026-09-19 evening): `cf ui` starts the new
daemon, the board page is `app/ui/index.html`, the old core, page, commands
and tests are gone, and ConsensFlow Candidate (`~/Applications`, home
`~/.consensflow-candidate`) carries it; the live app is untouched. The app
talks to the daemon only through `core_request` and the allow-list in
`commands.rs`; the agents screens (roster with tags, library, harnesses) are
served by the daemon at its URL behind the UI token the handle carries. The
human closes a project from the list (`project.close`: suspended, every window
killed, tiered work back in the backlog) and resumes it later. Members run one
task per session (TEST-BDC-28): their windows close with their task and the
next task starts fresh, so coordinators are told to put everything in the
task. Live on all four harnesses (2026-09-19 night): the bench passes
11/11 with a Codex worker, with a Codex reviewer (10/11: its verdict line was
wrapped in emphasis, now read), and with a Claude lead and worker on Sonnet;
the Codex socket falls back to the user's temporary directory when the home
path is too long, and every roster agent carries default tags read off its
profile. Next: Gabriel
tests the Candidate (empty board; he creates a project); a follow-up commit
prunes the one-shot runner code the old `cf run` left behind (`runAgent` in
`hosts/lib/runners.js`, `codex-auth`, `harness-transcript`, `image-run`,
`packets`, `session-binding`, `threads` and their tests; image agents have no
adapter in the new core); no importer is built (dropped 2026-09-21: go-live
starts clean). Not built yet:
removing a PM (the ledger refuses it as `not-a-member`).

## TDD log

- TEST-BDC-01 RED: the module did not exist (suite failed to load). GREEN after
  IMPL-BDC-02: 24/25, the one failure a wrong test (it expected a native
  session id to be unique across harnesses; it is unique within one harness).
  Review added three cases: a file that is not a database is refused as
  `ledger-unreadable`, a question needs its asker, and WAL is asserted. 26/26,
  three runs. The crash test kills a child writing tasks in a loop after
  150 ms; every task it left has exactly its one task message, and
  `integrity_check` is ok.
- The Stage 2 harness map (a read-only survey of today's per-harness launch,
  delivery, signals and collection, with file:line references) decided the
  slice: every harness already has a proven native way to push into a live
  window, so one delivery path per harness serves every participant and the
  five lead receivers retire at the switch.
- TEST-BDC-09 RED: the module did not exist. GREEN after IMPL-BDC-10: 13/13 on
  the first full run (the tests were traced by hand against the design first).
  The dispatcher needed two ledger reads, `activeTask` and `message`, each added
  test-first (ledger 27/27).
- Claude adapter RED: module missing; then 4 failures, all in the tests'
  assumptions (the answer parser marks user items complete; `harnesses.js`
  names Claude's CLI `claude`, not `claude-code`). GREEN 7/7. Claude's live
  status reader moved from the old watcher into the adapter; the old watcher
  imports it (160/160 old pane and watcher tests, six runs).
- The slice's integration test passed on its first run (0.8 s). To prove it
  can fail, result delivery was broken on purpose: it timed out, and passed
  again once restored. The API and `cf` commands: 7/7.
- TEST-BDC-22: Rust first. `draft.clear` now reaches the arbiter's existing,
  epoch-checked `clear_draft` (Rust 106/106, clippy clean); its test shows a
  submitted draft released once, and text typed after the Enter kept. Then the
  core: RED 14/15 (no clear was ever sent), GREEN 15/15. The end-to-end test
  failed twice more for two different reasons. First, the Enter arrived before
  the core's first look at the window, so the baseline included the human's
  own message; the core now counts the human's messages as it opens a window.
  Second, the test typed before the fake agent was in raw mode and its keys
  were lost, which a person cannot do; the test now types once the lead reads
  idle. 2/2 three times; without the release, the typed test fails.
- IMPL-BDC-07: the OpenCode and Pi adapters were written before their tests
  (a slip). Their tests (9) were then checked against four deliberate breaks
  (the TUI's server arguments, seeding a window with no task, Pi's settled
  marker, Pi's extension argument): each was caught. Devin and Codex went
  test-first: RED (modules missing), GREEN 10/10. The dispatcher gained one
  rule on the way: a harness that cannot take its first message after the
  window opens (OpenCode's server never answering) fails that delivery at once
  and ends the window, instead of waiting out the launch timeout (RED, GREEN
  16/16). Shared rule for the new adapters: a conversation with no messages and
  nothing in flight reads idle, so a window opened without a task can receive.
- VERIFY-BDC-08, first live runs of the new core (`npm run bench:core`, free
  models, an OpenCode lead). Run 1: the human's task reached the lead's window
  about a second after it opened, before OpenCode had loaded ConsensFlow's
  plugin; the fetch failed, the core counted it uncertain and gave up after
  one silent minute. Two fixes, test-first: OpenCode is ready only once its
  plugin reports that its TUI shows the conversation, and a handover that
  never shows in the harness record is tried again whether it was admitted or
  uncertain. Run 2: still nothing. Reading a copy of the ledger showed three
  attempts; sending the same text by hand through the plugin worked. The
  adapters passed the pane as `{id, generation}` while OpenCode's channel reads
  the generation beside the id, so every call threw, and the dispatcher had
  turned the throw into "uncertain". The adapter tests had replaced the
  channel with a stand-in and could not see it. Fixes: the adapters pass the
  pane's id and generation side by side (as the old code did), the OpenCode,
  Pi and Codex adapter tests now run the real channel against a fake plugin,
  extension or broker (the OpenCode one failed before the fix), and an adapter
  that throws fails its attempt with the error as the reason.
- The gate then failed on an old Pi extension test (a probe answered "native
  editor unavailable" instead of "lead busy"): the probe gives the extension
  1 s, which the full suite plus a live bench exceeded. The Stage 2 harness map
  had noted that `probeEditor` and `currentSession` in `src/channels/pi.js`
  have no caller, so the extension's branch answering them served nothing.
  Removed all three and their four tests rather than stretching a dead
  timeout (flagged to Gabriel, since his global rule is to report
  pre-existing dead code; his ConsensFlow rule is "never dead code").
- Run 3 (in progress when committed): the real OpenCode lead ran
  `cf task add` itself (6 s), the OpenCode worker answered (10 s), the result
  reached the lead (11 s) and the worker read idle.
- Run 3 then failed at Pi: the lead had answered its first task without
  `cf task done`, and the ledger held every later task for it behind that
  open one. One task at a time is right for a worker, wrong for a coordinator,
  whose tasks end only when it says so. The rule now applies to workers,
  advisors and reviewers only (ledger test first, RED then GREEN).
- Run 4: 14/14. The OpenCode lead ran each `cf task add` itself (2-4 s); the
  OpenCode, Pi and Devin workers answered (6-22 s); each result was in the
  lead's window within a second of the answer; every worker read idle; after
  the restart the lead came back on the same session.
- Phase E groundwork: the page protocol moved into `src/core/page.js` (tests
  first: RED, module missing; GREEN 5/5) with the human's new operations
  (agents list, accept, reopen, cancel), and the Rust app forwards exactly
  those names through one allow-listed command, `core_request` (its routes and
  refusals added to the forwarding contract test first; Rust 106/106, clippy
  clean). The new page is built beside the old one as `app/ui/core.html` and
  becomes `index.html` at the switch.
- The board page (`app/ui/core.html`, `app/ui/core/`, `app/ui/terminal-link.js`):
  one bay per participant with its lamp, each task a strip (a control room's
  flight strip: segmented boxes, the number block in its state's colour), the
  human's bay first with questions to answer in place, a composer per bay, a
  drawer with a task's thread and accept/reopen/cancel, the team picker, the
  new-session flow and the live terminals behind every bay. Written test-first
  against a stand-in app (12 Playwright tests; one expectation was wrong about
  strip order and was corrected: the live task first, then the queue, then
  what waits for a decision). Screenshots at 1440 and 390 px checked by eye;
  the drawer's task number took its state's colour after that review. The old
  page's 138 tests still pass.
- The board page's gate failed in integration TEST-PANE-75 with
  `bridge writer queue is full`. The headless helper sends pane output over
  the bridge through the same 32-slot writer queue as responses, and an event
  that met a full queue closed the whole transport, so a peer pausing for a
  few milliseconds during an output burst killed the helper and its panes.
  That the filler was pane output is inferred: the unit test proves the
  mechanism, not the incident. Rust test first, on a gated writer: three
  queues' worth of stream events with a request arriving mid-burst. RED
  against the old `event` (the burst died at the full queue), RED again with
  streams allowed the whole queue (the mid-burst response closed the bridge),
  GREEN with `stream_event`: it waits for room and fills at most half the
  queue. Review then found the count runs one ahead of the channel while the
  writer holds a job it has taken but not yet counted, so a frame allowed the
  whole queue now asks the channel itself. Rust 107/107 and 16/16, clippy
  clean, integration 22/22 on three runs, two gates green. Only the headless
  helper streams; the app's window draws pane output itself.
- The session team and the PM, after review found `addPm` and `lastTeam` with
  no production caller. Ledger first (RED 6/7, GREEN 34/34): a member who
  leaves has its open tasks cancelled with the messages still on their way to
  it and its unread questions, is refused as a recipient (`member-left`),
  including answers and reopened tasks, and rejoins on the same row in its new
  role and harness; a new session takes the team it is given; a coordinator
  whose window runs is told who joined or left and which tasks went with them
  (one whose window has not started reads the team at launch). The join note
  broke five dispatcher tests whose fixture added the worker after the lead's
  window opened; the fixture now builds the team at launch, as a real session
  does. Dispatcher (22/22): removal waits for the member's step in progress,
  so a window still opening is closed too (the naive version, checked on
  purpose, left it open), and that window's exit fails nothing; a PM's window
  opens with its first task. Page protocol (9/9): `member.remove`, `pm.add`,
  and `session.open` passing the last team as the saved agents are now (a
  deleted agent drops out, a changed harness is used). A new contract test
  reads the Rust allow-list and fails when it and the page operations differ
  (it failed until the two new names were added). The page (16/16): Board,
  Lead and PM views, the PM's team grouped after the lead's, a Remove that asks
  first, and a PM section; one more test caught the team dialog's redraw
  dropping a pending removal (it did, twice over: the other rows' redraw reset
  it), fixed by changing that state only on the Remove and Keep buttons.
  Screenshots at 1440 and 390 px: the confirmation now takes its own line.
- Gabriel renamed the concept: a *session* is now a *project* (2026-09-19,
  "we can call sessions Projects"). Done as one mechanical change across the
  new core only, before the tiered-dispatch work, so that work is written in
  the final vocabulary: the ledger table and columns, the events, the page
  operations (`projects.list`, `project.open`, `project.resume`), the Rust
  allow-list, the pane ids (`p<project>-<handle>`), `CONSENSFLOW_PROJECT`, the
  role texts and every test. The harnesses' own sessions keep the word:
  `nativeSession`, `--session-id`, Claude's `sessions/<pid>.json`, OpenCode's
  and Codex's session channels. The schema could change in place because no
  build has shipped it. Node 108/108 core and adapter tests, page 154/154,
  slice 2/2, Rust 107 + 16. Earlier entries in this file keep the old word.
- The switch (TEST-BDC-24, IMPL-BDC-25, IMPL-BDC-21, IMPL-BDC-26), 2026-09-19.
  RED: `tests/core-agents-server.test.mjs` failed to load (no module); the
  page spec's agents dialogs found no `roster_handle`. GREEN: the pages and
  `/api/agents…` routes moved out of `src/ui.js` into
  `src/core/agents-server.js` (7/7), mounted on the core's API server before
  the token check, the daemon's handle line carries a UI token, and the board
  opens the three screens as framed dialogs at the daemon's URL. `cf ui` runs
  `startCore`; the old verbs, modules, page files, Rust commands and their
  tests were deleted in one cut (a reachability pass over `src/`, `hosts/`
  and `bin/` decided what was dead; it missed `bin/cf.mjs` once and deleted
  `host-payloads.js`, restored from HEAD). What the cut broke and how it was
  fixed: clippy's `-D warnings` on the runtime's never-read `launches`
  field (removed); `tests/bridge.test.mjs` importing `stdinIsPipe` from the
  deleted `src/ui.js` (its two tests covered code nothing shipped any more,
  deleted); the harness page spec starting the old UI server (now `startApi`
  with `agentsUi`, as the daemon does); the updater page spec faking the old
  `list_state` and waiting on `#app` (now answers `core_request` with a
  project whose worker window is the blocker, waits on `body`); the roster
  card's tags line sharing the description's class (its own class now). The
  updater self-test needed a way to close its two windows without the old
  `close_pane`: `project.close` (dispatcher, page, allow-list, a Close button
  on the project list), tested at the dispatcher (a tiered task returns to
  the backlog and is taken again after Resume), the page protocol and the
  page. The Devin hook now names `cf inbox` and `cf task get`. Gate on the
  staged tree: 783 pass, 6 skipped of 789; integration 5/5; Playwright
  68/68; Rust 107 + 16, clippy clean.
- VERIFY-BDC-27, 2026-09-19 evening. The first `npm run candidate` failed its
  packaged smoke twice, each time on something real. (1) The smoke's catalog
  probe still started the old UI server (now `startApi` with `agentsUi`, as
  the daemon does). (2) The self-test's board step never saw its task: the
  core delivers only to a settled window whose typed draft is released, and
  both come from the harness record, which the smoke's shell stand-in did not
  keep. It now keeps what a Claude window keeps: `sessions/<pid>.json` (idle
  or busy) and a transcript where every line it reads is a user turn answered
  by an assistant turn, so the latch releases and the core confirms the
  delivery from the record (the self-test now waits for that confirmation and
  the smoke asserts it). (3) With that status file in place the packaged app,
  which uses Claude's peer inbox on macOS, refused the delivery outright:
  `claude-peer.js` treated a status row without the messaging identity as an
  inconsistent inbox (a hard refusal, three attempts, task failed) instead of
  an unregistered one (paste instead). That is a product bug for a Claude
  build without peer messaging; a row with no `messagingSocketPath` is now
  `native-session-unavailable`, with a test that failed first
  (`peer-refused`). (4) `/bin/sh` printf sign-extends bytes above 0x7f, so the
  stand-in's hex of `·` was wrong; masked. Smoke 2/2 in 4 s, then
  `npm run candidate` end to end: built, smoke 2/2, installed, live app and
  roster unchanged.
- After the advisor's review of the switch: the bench reran on the switched
  tree, 19/19 (OpenCode lead; OpenCode, Pi and Devin workers answered in
  7-21 s; a Devin reviewer passed an OpenCode result in 19 s; restart on the
  same session). The peer-inbox rule widened to a `null` socket path (a
  `null` would have fallen through to the hard refusal), with the test
  covering both shapes; the later identity check lost its now-redundant type
  test. The skill file the packaged daemon writes for the lead was checked:
  it is the new role text (tiers, `--self`, `cf inbox`), no deleted verb.
  Known and untested: `app/ui/update-selftest.js` on `project.open` and
  `project.close` (the updater smoke is opt-in and needs two bundles).
- TEST-BDC-28 / IMPL-BDC-29, 2026-09-19 evening, one task per member
  session. Gabriel's rule, after the lead's first answer (reuse with a context
  budget) contradicted the tier rule: a lead never sees a worker, so a
  session policy must be the daemon's. RED: 5 dispatcher tests, 1 ledger, 1
  integration, the bench's idle checks renamed to window-closed. GREEN with
  four decisions the tests fix: the author under review keeps its window
  (the send-back lands where the work was done) and is busy to the assigner;
  a fresh session's first message that is not the brief (a reopening) gets
  the brief in front, while an answer due after a restart resumes the
  member's own session; a member's working task with no window and nothing
  due is given up; low quota is kept in the daemon until its reset, since the
  window that reported it is gone. Found on the way: the fake pane host never
  exited a killed window, so every test chaining two tasks through one
  member hung; it exits at once now, and one test holds the exit to prove
  the gap between kill and exit is handled (a task assigned meanwhile waits
  for the fresh window). Also: only a member's own task's messages reach it
  (a stray note never opens a window). Board: a member between tasks reads
  "Free: a window opens with its next task"; the composer says the member
  starts from nothing. Unit 791 pass, 6 skipped of 797; integration 6/6 (two
  tiered tasks, one worker, two native sessions, the first pid dead before
  the second opened); page 69/69; bench 19/19 (the three workers' windows
  closed after their task, a Devin reviewer passed an OpenCode result in
  22 s, restart on the same session).
- Live proof on Codex and Claude, 2026-09-19 night, after Gabriel's
  checklist ("delivery must work flawless; yolo; markers; quota;
  transcripts; skills; deterministic") and his word that every harness has
  quota. Codex worker and reviewer: the window closed at once with
  "ConsensFlow home makes the Codex socket path too long" (the bench's and
  integration's homes are long temporary paths; the Candidate's
  `~/.consensflow-candidate` fits). The supervisor now falls back to
  `$TMPDIR/consensflow/codex-XXXXXX` (still 0700) when the home does not
  fit, and refuses only when nothing fits; the socket is a runtime endpoint,
  not state. Bench: Codex worker 11/11; Codex reviewer 10/11 because its
  verdict came as `**VERDICT: pass**` and the ledger read only a bare line;
  `verdictOf` now reads through markdown emphasis and dashes (exported,
  tested on six shapes). Claude lead and worker: two runs lost to
  first-run onboarding on the lead's screen, found with a 10 s probe through
  the harness (the harness now keeps every pane's output and exits, and a
  failed bench check prints them): with `CLAUDE_CONFIG_DIR` set, Claude keeps
  its global config inside that directory, so the harness's sandbox default
  (and, before it, the bench's `~/.claude`, where only a stub exists) showed
  no completed onboarding. A null override now clears the harness default;
  the app never sets the variable for panes. Claude bench 11/11: the
  human's task to a Sonnet lead, a fresh Sonnet worker, the result to the
  lead through the peer inbox in 11 s, a Devin review, restart on
  `--resume`. Tags: every catalog preset and every roster agent without
  hand-set tags now carries tags read off its own "good for" text and
  categories (`defaultTags`: coding, review, planning, architecture,
  debugging, analysis, hard-problems, questions, small-changes, long-tasks,
  second-opinion, images); the human's tags still win. The PM's description
  line said it hands work to the lead; the body and CORE-12 say the human
  does; aligned.
