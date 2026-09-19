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

- [x] [TEST-BDC-01] Ledger schema, migrations and the domain operations (sessions, participants, team, conversations, tasks, messages, events), the instance lock, crash safety and the invariants each operation keeps. `tests/ledger.test.mjs`, 26 tests.
- [x] [IMPL-BDC-02] `src/ledger/` on `node:sqlite` (`index.js`, `schema.js`); satisfies TEST-BDC-01.

### Phase B: Harness adapters [active]

- [x] [TEST-BDC-05] Adapter contract (launch, deliver, signals, collect, transcript source) with a fake harness per adapter. The contract is `prepare`, `started`, `ready`, `deliver`, `observe` (documented in `src/core/dispatcher.js`): `tests/adapter-claude.test.mjs` (7), `adapter-opencode` (5), `adapter-pi` (4), `adapter-devin` (5), `adapter-codex` (5).
- [x] [IMPL-BDC-06] Claude Code adapter (status file, hooks, peer delivery, Stop-hook turn end): `src/adapters/claude-code.js`. Live proof is VERIFY-BDC-08.
- [x] [IMPL-BDC-07] OpenCode, Pi, Devin and Codex adapters (`src/adapters/`, registered in `index.js`). Waiting signals beyond Claude's are open work for VERIFY-BDC-08; live proof per harness is VERIFY-BDC-08.
- [ ] [VERIFY-BDC-08] Live bench per adapter, as lead and as worker, with the test models. Green for the free models (run 4, 14/14: an OpenCode lead dispatching to OpenCode, Pi and Devin workers, plus the restart). Open: a Claude lead and worker (Sonnet, after the weekly limit resets), Codex (quota), and OpenCode, Pi and Devin as leads.

### Phase C: Dispatcher and inbox delivery [active]

- [x] [TEST-BDC-09] Task state machine; per-participant queue delivering one item at a time when idle; receipts; retries; the launch-liveness reaper; restore on start. `tests/core-dispatcher.test.mjs` (13) and, end to end through the real pane host, `tests/integration/core-slice.test.mjs`.
- [x] [IMPL-BDC-10] Dispatcher (`src/core/dispatcher.js`), pane host (`pane-host.js`) and the daemon entry (`daemon.js`); satisfies TEST-BDC-09.
- [x] [TEST-BDC-22] A human's Enter releases the typing latch once the harness records that submission; typing after it keeps the latch. Found by the Stage 2 harness map: no production code ever calls `clear_draft`, so one keystroke blocks every later paste into that window (Devin always pastes).
- [x] [IMPL-BDC-23] Rust `draft.clear` bridge operation and the core's use of `pane.enter`; satisfies TEST-BDC-22.

### Phase D: CLI, API and role skills [active]

- [ ] [TEST-BDC-11] `cf task add/list/get/accept/reopen`, `cf inbox`, `cf ask`, `cf answer`; team enforcement; the removed commands are gone.
- [x] [IMPL-BDC-12] CLI (`src/core/cli.js`, incl. `cf team`), daemon API (`src/core/api.js`) and new role instructions for lead, PM, advisor, worker and reviewer (`skill/core/`, `src/core/roles.js`), passed to each window through its adapter. The old commands go at the switch (TEST-BDC-11 stays open until then).

### Phase E: Board-first UI [active]

- [ ] [TEST-BDC-13] Playwright: lanes per participant, cards and markers, card detail, the human inbox with question answering, the team picker, session views kept. `app/tests/core-page.spec.mjs` (12) covers all but the PM session view; the switch adds geometry, the Agents/Library/Harnesses dialogs and the self-test hook.
- [ ] [IMPL-BDC-14] The board page on the new core's page protocol.

### Phase F: The switch [planned]

- [ ] [TEST-BDC-03] Import from alpha.62 state (tabs, threads, tasks, inbox results) is complete, idempotent and read-only on the source; replayed against a copy of the live home.
- [ ] [IMPL-BDC-04] Importer; satisfies TEST-BDC-03.
- [ ] [IMPL-BDC-21] `cf ui` runs the new core; the old core, its page code, the JSON state and `O_EXLOCK` are deleted; every suite and the live bench are green.

### Phase G: Transcript copy [planned]

- [ ] [TEST-BDC-15] Incremental copy per harness, retention, answers collected from the copy.
- [ ] [IMPL-BDC-16] `src/mirror/`.

### Phase H: Windows [planned]

- [ ] [TEST-BDC-17] Windows runner for Node and Rust suites; process-tree termination and single-instance guarantees on Windows.
- [ ] [IMPL-BDC-18] Job Objects in the PTY host, Windows updater path, path and shim handling.

### Phase I: Acceptance [planned]

- [ ] [VERIFY-BDC-19] Full live bench matrix, Playwright, a computer-use pass over the Candidate, an upgrade from a copy of the live home, hours-long CPU and memory profile.
- [ ] [VERIFY-BDC-20] Hand the Candidate to Gabriel for the live test.

## Resume context

Phases A to D are built. B, C and D wait only on live runs (a Claude lead and
worker on Sonnet, Codex when its quota allows, Pi and Devin as leads) and, for
TEST-BDC-11, on the switch that deletes the old commands. Phase E: the board
page is built and committed as `app/ui/core.html`, beside the old page. Next:
the session team (a member can leave; a new session starts with the last team;
the coordinators are told who joined or left), the PM (added from the page,
its window opened by its first message) and the Lead and PM window views that
keep the old page's session views; then Phase F, the switch. The old page's
terminal plumbing in `app/ui/panes.js` duplicates `app/ui/terminal-link.js`
until the switch deletes the old page.

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
