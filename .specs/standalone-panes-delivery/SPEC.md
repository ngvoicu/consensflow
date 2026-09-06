---
id: standalone-panes-delivery
title: ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead
status: active
created: 2026-09-06
updated: 2026-09-06
priority: high
tags: [app, tauri, pty, panes, delivery, standalone, skill, evals]
---

# ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead

## Overview

Today a consult in `cmux` mode opens the agent's window in a pane of a
third-party terminal, and the lead reads answers only by asking
(`cf catchup`). This spec makes ConsensFlow's own desktop app the terminal:
it opens every pane itself (a PTY in Rust, drawn by xterm.js in the webview),
lays them out in the user's fixed progression, lists tabs and conversations
in a sidebar, and **delivers every completed worker answer, whole, into the
lead's pane** when policy allows — `auto` by default, `manual` where the
human says so, at conversation (tab) scope and at pane scope, and the lead
can never override the human. Pull stays: `cf catchup` is unchanged.

Revision 2 (2026-09-06) absorbs the co-lead review by astraeus
(`astraeus-lilac-dune`, sixteen findings) and Gabriel's round-2 answers:
no cap on a delivered answer, auto means every completed reply, two policy
scopes, layouts counted with the lead. The contracts that were missing —
launch authority, identity, completion, delivery receipts, lifecycle, an
offline end-to-end suite — each have tasks now. The cmux path ends in the
last phase: `standalone` becomes the mode's real name and the skill teaches
three verbs.

Evidence: `research-01.md`; consequences of the answers: `research-02.md`;
questions, answers and the review: `interview-01.md`. 50 tasks across 6
phases; six live probes gate three of them.

## Acceptance Criteria

- [ ] In standalone mode, `cf run @nyx "task" --new` run from a lead's pane
      asks the app for a pane in the lead's tab, prints the conversation
      name the app minted, and exits — no window opens in the lead's pane
- [ ] `cf run`, `cf attach` and `cf say` in standalone mode outside an app
      pane refuse and name the app; an unavailable app never authorizes a
      second launch
- [ ] A worker's harness and a shell pane hold no app credentials; a lead
      pane's token is scoped to its tab and to the named operations a lead
      may perform; no HTTP caller can supply `by`, an owner, or a target
      outside its tab
- [ ] `cf run … --in-pane` runs only with a single-use launch ticket the app
      issued, and takes `lead`, `tab` and `requester` from that ticket —
      never from its own environment
- [ ] All `threads.json` and `tabs.json` writes in standalone mode go
      through the app's Node process, serialized per workspace; harness
      stores stay read-only
- [ ] A conversation's native session is bound only with evidence that it
      is THIS launch's session; an ambiguous discovery stays unbound and
      never drives automatic delivery
- [ ] The page shows a collapsible mode selector; claude and pi show the
      roster editor; standalone shows the sidebar (tabs, their
      conversations) and the pane area, collapsible left
- [ ] Panes tile in the user's progression counted WITH the lead: 1 lead
      alone; 2 side by side; 3 lead full-height and two stacked; 4 a 2×2;
      5 lead full-height and a 2×2; 6 a 3×2 — `gridTemplate(n)` for 1–6,
      a grid beyond
- [ ] Every pane has a title: conversation name · agent · effective policy;
      a shell pane is titled `shell`
- [ ] A human opens panes in a tab beside the ones consults open: a shell
      pane in the tab's directory, or an agent pane that becomes a
      conversation whose lead is the tab's lead
- [ ] Every completed worker reply — to the lead's question or the human's
      — is delivered WHOLE into the lead's pane when the effective policy is
      `auto`: inline when the lead harness admits a paste that size, else as
      a file the lead is told to read in full; there is no cap and no
      truncation
- [ ] A delivery is written only when the lead is ready: its own transcript
      shows its turn ended, its PTY has been silent, and no human draft is
      open in that pane; otherwise it waits, visibly, and the human can
      deliver it now
- [ ] Every delivery has a record with a state; `accepted` needs
      harness-level evidence (the body, or the read instruction, appears in
      the lead's own transcript); an `uncertain` delivery is never replayed
      automatically; the lead's `seen` mark advances only across delivered
      answers contiguous with it
- [ ] Policy precedence holds: tab human `manual` veto > pane human setting
      > tab human `auto` > lead preference > default `auto`; the human sets
      it from the page only; `cf run --notify` records a preference and
      nothing more; disabling cancels queued automatic deliveries
- [ ] Right-click **Send reply to lead…** lists that conversation's
      completed answers from the transcript; a chosen one is delivered
      through the same path; already-delivered ones say so and need an
      explicit resend
- [ ] Closing a lead suspends its tab and stops its process trees; app exit
      reaps every owned tree; restart restores tabs closed, resume is
      explicit and mints a new pane generation; pending deliveries are held,
      never handed to whatever occupies a reused pane
- [ ] The offline integration suite drives the real `cf`, the real Node
      server, the real Rust bridge and PTYs, and fake harness processes
      through launch, follow-up, completion, receipt, unread and shutdown
- [ ] `standalone` is a mode; `mode.json` holding `cmux` reads as
      `standalone`; the generated skill names no cmux command; the evals
      hold both directions of "continue or start fresh" and the delivery
      rules
- [ ] `npm run check:all` exits 0 with no live agent CLI and no network:
      biome, Node tests, `cargo test` + clippy, page tests, the integration
      suite, a packaged-app smoke
- [ ] A Windows and a Linux bundle build from the same sources; the
      OS/harness support matrix is verified, not assumed, and covers native
      Windows only — WSL is out of scope; P4 recorded

## Architecture

```
 ┌──────────────────────── ConsensFlow.app window (tauri:// origin) ─────────────────────┐
 │ [ claude | pi | standalone ]  ◄ collapsible upward                                     │
 │ ┌ sidebar ┐ ┌──── pane area: one tab = one directory + one lead pane + policy ───────┐ │
 │ │ tab ~/x  │ │ ┌ lead: claude ────┐ ┌ nyx-coral-lane · @nyx · auto ─────────────────┐ │ │
 │ │  nyx-…   │ │ │ xterm ◄─Channel  │ │ xterm (right-click: Send reply to lead…)       │ │ │
 │ │  shell   │ │ │        ack──►    │ ├ shell ────────────────────────────────────────┤ │ │
 │ │ tab ~/y  │ │ │  [1 result waiting — deliver now]                                  │ │ │
 │ └──────────┘ └─┴──────────────────┴─┴───────────────────────────────────────────────┴─┘ │
 │ [ roster editor: <iframe src="http://localhost:PORT/?token=UI"> ]  ◄ cf ui, unchanged  │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        │ Tauri IPC: invoke + Channel<bytes> + acks         (the page never fetches Node)
        ▼
 ┌─ Rust: pty.rs · bridge.rs · (bin) consensflow-bridge (headless, for tests) ───────────┐
 │ pane table: id, generation → PtyPair, child, last_output_at, human_draft_at, queue     │
 │ write_paste (ESC[200~ body ESC[201~, then a separate \r)  · input arbiter · backpressure│
 │ env per ROLE: lead → APP, TOKEN(tab-scoped), TAB, PANE_ID · worker → LAUNCH ticket only │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        │ JSON-lines v1 on the EXISTING stdin/stdout pipe — handle line first, then frames
        ▼
 ┌─ Node: src/ui.js · src/bridge.js · src/store.js · src/tabs.js · src/delivery-watch.js ─┐
 │ HTTP for cf (lead tokens, named ops): consult · say · panes · attach · seen · notify.lead│
 │ the ONLY writer of threads.json + tabs.json (serialized per workspace)                   │
 │ hosts/lib/completion.js  (per-harness answer state)   hosts/lib/readiness.js (lead ready)│
 │ hosts/lib/policy.js      (precedence)                 hosts/lib/deliveries.js (records)  │
 │ channels: pty-inline · pty-file · opencode-server · pi-followup                          │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        ▲ HTTP + lead token
 cf run @nyx "task" --new        (requester; the app mints the name and returns it)
 cf run … --in-pane              (the pane's process, under a launch ticket)
 cf say <name> "<words>"         (a follow-up)      cf attach <name>  (reopen, via the app)
```

**Identities, kept apart.** A **tab** is app-owned and persisted
(`tabs.json`): id, directory, lead `{harness, sessionId?, generation}`,
panes `[{id, kind: lead|worker|shell, conversation?, generation, order}]`,
human policy. A **conversation** is a worker row in `threads.json`. A
**pane** is `{id, generation}` — a reused id with a new generation is a
different pane. A **native session** is the harness's own; it is bound to a
conversation only with evidence (§ Phase 2). Ownership (`lead`, `tab`,
`requester`) travels in the launch ticket; nothing derives it from the
environment of the process that happens to write the row.

**Launch roles.** The app launches a lead directly with its credentials
(`CONSENSFLOW_APP`, a tab-scoped `CONSENSFLOW_APP_TOKEN`, `CONSENSFLOW_TAB`,
`CONSENSFLOW_PANE_ID`). A worker pane's process is `cf run … --in-pane`
holding only a single-use `CONSENSFLOW_LAUNCH` ticket; its harness child gets
the `childEnv`-stripped environment. A shell pane gets nothing. Stripping is
credential hygiene — it stops inheritance; it is not a sandbox against code
running as the same OS user, and the spec says so. Human authority exists
only on the page → Rust → Node path; the HTTP API has no `by` field. The
page's own token (roster, mode, reset) is not a lead token.

**Completion, readiness, delivery.** `completion.answers(kind, sessionId)`
reads the native store and returns answers with stable ids and a `complete`
flag from the harness's own completion markers (never "last role is
assistant", never a turn count). `readiness.leadReady` combines three
signals: the lead's own transcript says its turn ended with no tool in
flight, the lead pane has been silent for `idleMs`, and no human keystrokes
reached that pane for `draftMs`; anything else is `busy` or `unknown`, and a
delivery then waits, visibly. Policy is evaluated immediately before
submission. A delivery is a record with a state — `pending`, `submitting`,
`accepted`, `uncertain`, `failed` — and `accepted` needs the body (or the
read instruction) to appear as a user turn in the lead's own transcript. The
channel is chosen per lead harness from the probes: `pty-inline` (the whole
body as one bracketed paste, then a separate `\r`) when the harness admits a
paste of that size, `pty-file` (the whole body written to
`<workspace>/deliveries/<id>.md` and one instruction line pasted) otherwise,
and the native channels for opencode (its TUI server API) and pi (an
extension follow-up) where P5/P6 prove them. **There is no cap**: the lead
reads everything, inline or from the file the line names, and the skill says
so. `ANSWER_LIMIT` stays where it is — a bound on `cf last`'s console
output, never on a delivery.

**Layouts, counted with the lead** (the user's progression):

```
1: [lead]        2: [lead|w1]      3: [lead|w1]      4: [lead|w1]      5: [lead|w1|w2]     6: [lead|w1|w2]
                                      [lead|w2]         [ w2 |w3]         [lead|w3|w4]        [ w3 |w4|w5]
```

Odd n ≥ 3: the lead spans both rows in column 1 and the n−1 workers form a
2-row grid beside it. Even n: a plain grid, row-major, lead first. Beyond 6:
`rows = ceil(sqrt(n))`, row-major. The lead keeps focus when a worker opens.

## Probes — gates, recorded in `findings-01.md`

- [ ] P1 — for each lead harness (claude, codex, pi, opencode, kimi): a
      multi-line body as one bracketed paste followed by a separate `\r`
      submits; the largest body that submits intact (decides
      `pty-inline` vs `pty-file`) — gates Phase 3
- [ ] P2 — the TUI enables the kitty keyboard protocol; a raw `\r` still
      submits when it has; a newline inside the paste survives — Phase 3
- [ ] P3 — an `http://localhost:<port>` iframe loads inside a `tauri://`
      page under the existing ATS exception — Phase 4
- [ ] P4 — `portable-pty` 0.9.0 reads clean bytes on Windows
      (wezterm#6783); pin 0.8.1 or patch the ConPTY flags if not — Phase 6
- [ ] P5 — opencode's TUI server accepts a message into the running
      session, and the message reaches the transcript — Phase 3
- [ ] P6 — a pi extension delivers `sendMessage` as a follow-up that runs
      when the agent is idle — Phase 3

Claude probes wait for the session limit (22:20 Europe/Athens, 2026-09-06).

## Testing Architecture

### Test Framework & Tools

| Tool | Choice | Purpose |
|---|---|---|
| Node tests | `node --test` | store, tabs, policy, completion, readiness, deliveries, layout, bridge, `cf` side |
| Rust tests | `cargo test` in `app/src-tauri` | PTY, paste shape in raw mode, arbiter, backpressure, protocol |
| Integration | `node --test tests/integration/` | real `cf` + real Node + headless Rust bridge + fake harnesses |
| Page tests | Playwright in `app/` (`npm run test:ui`) | selector, sidebar, layouts, menus, badge — the page only |
| Lint | biome (root), `cargo clippy` (app) | part of `check:all` |
| Release check | `npm run check:all` | everything above plus a packaged-app smoke |
| Behaviour | `npm run eval` — real lead, stubbed `cf` | outside `check:all`; spends tokens |

### Isolation Strategy

| Layer | Approach |
|---|---|
| PTY bytes | a child in raw mode dumping hex (`stty raw -echo; od -An -tx1`) — echo and line discipline cannot transform what is asserted |
| Pipe protocol | Node side: the test plays the Rust parent over piped stdio; Rust side: an in-process reader/writer pair |
| Store, tabs | real files under a throwaway `CONSENSFLOW_HOME` (`tempEnv()`), concurrent mutations from two callers |
| Completion, readiness, policy, deliveries | pure functions over dated fixtures copied from real stores (`tests/engine/fixtures/`) |
| `cf` ↔ app | the REAL server (`cf ui --json --no-open`) with the test on the pipe as Rust; plus `stubApp()` for `cf`'s failure paths |
| End to end | headless `consensflow-bridge` + fake harness scripts that write real-shaped stores and echo raw input as hex |
| Page | Playwright with a `window.__TAURI__` shim; no PTY |
| Network | none |

### Coverage Targets

| Metric | Target |
|---|---|
| New Node modules | every exported function exercised, including failure branches |
| Rust `pty.rs`, `bridge.rs`, `arbiter.rs` | every public fn under `#[cfg(test)]` |
| Integration | every acceptance criterion about launch, delivery, receipt, lifecycle has one scenario |
| Probes | one row per harness and per probe in `findings-01.md`, with versions |

### Test Commands

| Command | Purpose |
|---|---|
| `npm test` | Node unit suite |
| `npm run check` | biome + Node tests |
| `cd app/src-tauri && cargo test && cargo clippy` | Rust |
| `npm run test:integration` | the offline end-to-end suite |
| `cd app && npm run test:ui` | page |
| `npm run check:all` | all of the above plus the packaged smoke — the release gate |
| `npm run eval` | behaviour, real lead |

## Library Choices

| Need | Choice | Alternatives | Rationale |
|---|---|---|---|
| PTY | `portable-pty` 0.9.0 (Rust, MIT) | `node-pty` | in the existing cargo build; no native addon under hardened runtime; ConPTY (research-01 §4.2); P4 may pin 0.8.1 |
| Emulator | `@xterm/xterm` 6.0.0 + `addon-fit`, bundled into `frontendDist` | wterm | one implementation ships; wterm's open issues are TUI issues (research-01 §4.3); re-evaluate wterm in a quarter, no stub now |
| Bytes to page | `tauri::ipc::Channel` + acks from the page | events | ordered, binary; the ack is what bounds the backlog (xterm flow control) |
| Directory picker | `tauri-plugin-dialog` | a text field | native |
| Native channels | opencode server API, pi extension | PTY only | where P5/P6 hold, admission is a harness fact, not a paste |
| Page tests | Playwright in `app/` | none | the root package stays zero-dependency |

## Phase 1: Bridge protocol and PTY transport [in-progress]

- [ ] [TEST-PANE-01] `app/src-tauri/src/pty.rs` `#[cfg(test)]` — `open`
      spawns `sh -c 'printf hello'` and the reader yields `hello` then EOF;
      `resize(24, 80)` is seen by `sh -c 'stty size'`; `kill` ends the
      reader; an env map reaches the child (`sh -c 'echo $X'`); a raw-mode
      recorder child (`sh -c 'stty raw -echo; od -An -tx1'`) echoes exactly
      the bytes written, as hex. Windows variants under `#[cfg(windows)]`
      use `cmd /c`. ← current
- [ ] [IMPL-PANE-02] `pty.rs` — `PaneTable` keyed by `(id, generation)`:
      `PtyPair`, child, reader thread, `last_output_at`; `open(cwd, argv,
      env, size)`, `write`, `resize`, `kill`, `list() -> Vec<PaneInfo{id,
      generation, alive, idle_ms}>`; `Cargo.toml` gains `portable-pty =
      "0.9"`. -> satisfies [TEST-PANE-01]
- [ ] [TEST-PANE-03] `pty.rs` + `arbiter.rs` tests — `write_paste(id,
      body)` produces, on the raw recorder, `1b 5b 32 30 30 7e` + body +
      `1b 5b 32 30 31 7e`, then after `enter_delay_ms` a lone `0d`, as two
      writes; `sanitize` normalises CRLF to LF and rejects any control byte
      other than LF and TAB with `Unsafe`; the **input arbiter**: human
      keystrokes stamp `human_draft_at`, an automated paste is refused with
      `Draft` while a human draft is younger than `draft_ms`, and human
      bytes arriving between the paste and the `\r` are queued behind the
      `\r`, never interleaved.
- [ ] [IMPL-PANE-04] `pty.rs` `write_paste`; `arbiter.rs` (per-pane lock
      held from paste to `\r`, `human_draft_at`, queued human bytes);
      `sanitize`. -> satisfies [TEST-PANE-03]
- [ ] [TEST-PANE-05] `tests/bridge.test.mjs` + `bridge.rs` tests — the
      protocol: the first stdout line is the handle line (assertion reused
      from `tests/ui.test.mjs:17-50`), then frames `{v:1, id, kind:
      'req'|'res'|'evt', op, body}` one per line; ids are namespaced
      (`n-…` Node-originated, `r-…` Rust-originated) so both directions
      correlate without collision; a request carries `deadlineMs` and is
      answered `{kind:'res', ok:false, error:'deadline'}` when it lapses; a
      frame over `maxFrameBytes` is refused; EOF rejects every outstanding
      request and disables dependent operations; **nested requests** work
      (Rust awaits Node's `consult` while Node asks Rust `pane.open`); the
      handle line and the first frame arriving in ONE read keeps the
      buffered bytes; stdout carries frames only, diagnostics go to stderr;
      the bridge is inert when stdin is not a pipe.
- [ ] [IMPL-PANE-06] `src/bridge.js` (`Bridge` over stdio: `request`,
      `on`, `event`, deadlines, framing) and `app/src-tauri/src/bridge.rs`
      (reader thread, mutex writer, `request`/`reply`/`event`, dispatch
      that never blocks on a handler). -> satisfies [TEST-PANE-05]
- [ ] [TEST-PANE-07] `pty.rs` tests — **backpressure**: with a child
      running `yes`, unacked output is bounded at `backlogBytes`; the reader
      pauses and resumes on acks; `write` (input) stays responsive during
      the flood; `kill` ends everything; a paused pane still reports
      `alive`.
- [ ] [IMPL-PANE-08] `pty.rs` ack-gated reader; the Channel payload
      carries `seq`, the page acks `seq` after xterm's write callback.
      -> satisfies [TEST-PANE-07]
- [ ] [TEST-PANE-09] `app/src-tauri/tests/headless.rs` — a second binary
      target `consensflow-bridge` (no window) speaks the same protocol over
      its stdio: open a pane running the raw recorder, `write_paste`,
      observe the hex on the pane's output frames, `list`, `kill`, EOF.
- [ ] [IMPL-PANE-10] `app/src-tauri/src/bin/consensflow-bridge.rs` sharing
      `pty.rs`/`bridge.rs`/`arbiter.rs` via the lib crate; `lib.rs` uses
      the same modules behind the window. -> satisfies [TEST-PANE-09]

## Phase 2: Tabs, identity, launch authority and the store [pending]

- [ ] [TEST-PANE-11] `tests/store.test.mjs` — `ThreadStore(home)`:
      `mutate(cwd, name, fn)` serialises per workspace (two overlapping
      mutations on ONE row both land; two on DIFFERENT rows in one
      `threads.json` both land; 50 concurrent mutations lose none);
      named ops `conversation.create`, `session.bind`, `sent.record`,
      `policy.set`, `seen.set`, `delivery.upsert`; every write is atomic
      (`writeJsonAtomic`); harness stores are never opened for writing
      (asserted with a read-only fixture directory).
- [ ] [IMPL-PANE-12] `src/store.js` — a per-workspace promise queue over
      `loadThreads`/`saveThread`; the named ops; `bin/cf.mjs` in standalone
      mode submits ops over HTTP instead of writing (the cmux path keeps its
      direct writes until Phase 6). -> satisfies [TEST-PANE-11]
- [ ] [TEST-PANE-13] `tests/tabs.test.mjs` — `tabs.json` under
      `<root>/app/`: `tab.create(dir, harness)` returns `{id, generation
      1}`; two tabs may share a directory without sharing a lead;
      `tab.addPane(kind, conversation?)`, `tab.removePane`, order kept;
      `tab.suspend` (lead closed) and `tab.resume` (new generation);
      `tab.policy.set(mode)` records `human`; a restart reads every tab as
      `closed`; a pane id reused with a new generation is not the old pane.
- [ ] [IMPL-PANE-14] `src/tabs.js` — the tab store, on the same
      serialised queue as `ThreadStore`. -> satisfies [TEST-PANE-13]
- [ ] [TEST-PANE-15] `tests/launch.test.mjs` — **launch roles and
      tickets**: `issueTicket({tab, pane, lead, requester, conversation})`
      is single-use and expires in `ticketMs`; a lead launch env carries
      `CONSENSFLOW_APP`, a token scoped `{tab, ops:[consult, say, panes,
      attach, seen, notify.lead]}`, `CONSENSFLOW_TAB`, `CONSENSFLOW_PANE_ID`
      and NO `CONSENSFLOW_CHILD`; a worker launch env carries only
      `CONSENSFLOW_LAUNCH=<ticket>` plus `CONSENSFLOW_PANE_ID`, and the
      harness child's env (through `childEnv`) has none of the app
      variables; a shell launch env has none; a token for tab A used on
      tab B is refused; the page's UI token is refused on `/api/panes/*`
      and a lead token is refused on `/api/agents`, `/api/mode`,
      `/api/reset`; no request body may carry `by`, `lead` or `owner`
      (400).
- [ ] [IMPL-PANE-16] `src/launch.js` (tickets, role envs, scoped tokens),
      `src/ui.js` token scoping middleware, `hosts/lib/runners.js` `childEnv`
      strip list (`CONSENSFLOW_APP*`, `CONSENSFLOW_TAB`,
      `CONSENSFLOW_PANE_ID`, `CONSENSFLOW_LAUNCH`). -> satisfies [TEST-PANE-15]
- [ ] [TEST-PANE-17] `tests/ui-panes.test.mjs` — against the REAL server
      (`cf ui --json --no-open`, the test on the pipe as Rust): `POST
      /api/panes/consult {agent, task, brief?, context?, handoffFile?,
      session?, fresh, notify?, requestId}` with a lead token reserves the
      conversation (minting a name when `session` is absent) and a launch
      atomically, asks Rust `pane.open` with the worker role env and `cf
      run … --in-pane` argv, and answers `{name, pane, tab, launch:
      'started'}`; the same `requestId` again answers the same result
      without a second launch; a taken `session` with `fresh` is refused
      before any launch; an unknown agent names the roster; `POST
      /api/panes/say {session, text}` asks Rust `pane.write_paste` on that
      conversation's live pane, refuses when the pane is gone (naming
      `cf attach`), and records `sent`; `GET /api/panes` lists the caller's
      tab; `POST /api/panes/attach {session}` reopens a closed conversation
      in a new pane generation under a fresh ticket; `POST /api/panes/seen
      {session, turns}` applies `markRead`'s contiguity rule; `POST
      /api/panes/notify {session, mode}` records the LEAD preference only.
- [ ] [IMPL-PANE-18] `src/ui.js` + `src/panes.js` — the six handlers on the
      real server, over `ThreadStore`, `tabs`, `launch` and the bridge.
      -> satisfies [TEST-PANE-17]
- [ ] [TEST-PANE-19] `tests/cf-standalone.test.mjs` — the `cf` side: in
      standalone mode from a lead pane, `cf run @zeus "q" --new` POSTs
      `consult` with the task, prints `conversation: <name> (new) — pane
      <id>` and exits without spawning a harness; `--json` prints `{name,
      pane, tab}`; `--session <name>` continues; `cf run --in-pane` without
      `CONSENSFLOW_LAUNCH` refuses; with a ticket it takes `lead`, `tab`
      and `requester` from the ticket and never from `leadId(env)`;
      without `CONSENSFLOW_APP` today's cmux behaviour is byte-identical
      until Phase 6; `cf attach`, `cf say` and `cf chat` in standalone mode
      go through the app and refuse when it is unreachable; `CONSENSFLOW_CHILD=1`
      refuses all of them; the kimi first-turn path (`markRunning`/
      `recordTurn`) writes ownership from the ticket before streaming.
- [ ] [IMPL-PANE-20] `bin/cf.mjs` — `requestPane`, `--in-pane` with ticket
      redemption (`GET /api/launch/<ticket>` → ownership), `sayVerb`,
      `attachVerb`/`chatVerb` standalone branches; `saveWindowRow` and the
      kimi path take ownership from the redeemed ticket. -> satisfies [TEST-PANE-19]
- [ ] [TEST-PANE-21] `tests/engine/session-binding.test.mjs` —
      `bindEvidence(kind, candidate, launch)`: claude and pi bind on the
      minted id; codex, opencode and kimi bind only when the candidate
      session's FIRST user turn carries the seeded task text (dated
      fixtures from real stores); two candidates created in the same
      directory within the window with neither carrying the seed leave the
      row `unbound`; an `unbound` row reads in `cf catchup` as "not yet
      bound" and `shouldDeliver` is false for it.
- [ ] [IMPL-PANE-22] `hosts/lib/harness-transcript.js` discoverers accept a
      `seed` and return `{sessionId, evidence}`; `src/store.js`
      `session.bind` refuses without evidence. -> satisfies [TEST-PANE-21]

## Phase 3: Completion, readiness and delivery [pending] — gated by P1, P2, P5, P6

- [ ] [TEST-PANE-23] `tests/engine/completion.test.mjs` —
      `answers(kind, sessionId, env)` returns `{answers: [{id, text,
      complete, at}], inFlight}` per harness from the native store's own
      completion markers; dated fixtures for: partial text followed by tool
      work (`complete:false`), text that grows with an unchanged turn count
      (kimi), a cancelled turn, a compacted transcript, a branch change
      (codex fork); an unreadable or unsupported store answers
      `{unknown: true}` explicitly, never an empty success.
- [ ] [IMPL-PANE-24] `hosts/lib/completion.js` — one adapter per harness
      beside the display reader; `harnessTurns` stays the human-readable
      projection. -> satisfies [TEST-PANE-23]
- [ ] [TEST-PANE-25] `tests/engine/readiness.test.mjs` —
      `leadReady({kind, sessionId, paneIdleMs, humanDraftMs})` is `ready`
      only when the lead's own transcript ends in a complete answer with no
      tool in flight AND `paneIdleMs >= idleMs` AND `humanDraftMs >=
      draftMs`; `busy` when the transcript shows a turn in progress;
      `unknown` when the store is unreadable; a periodically redrawing idle
      TUI (fixture: output every 500 ms, transcript idle) is `ready` —
      silence is a hint, the transcript is the judge.
- [ ] [IMPL-PANE-26] `hosts/lib/readiness.js`. -> satisfies [TEST-PANE-25]
- [ ] [TEST-PANE-27] `tests/engine/policy.test.mjs` —
      `effectivePolicy(tab, pane, row)`: tab human `manual` vetoes
      everything; else pane human setting; else tab human `auto`; else the
      lead's `notifyPreference`; else `auto`; `inherit` at pane scope falls
      through; the result names its source (`tab-human`, `pane-human`,
      `lead`, `default`) for the pane title; `cf run --notify` can only
      write `notifyPreference`.
- [ ] [IMPL-PANE-28] `hosts/lib/policy.js`; `src/tabs.js` and
      `src/store.js` policy fields. -> satisfies [TEST-PANE-27]
- [ ] [TEST-PANE-29] `tests/engine/deliveries.test.mjs` — delivery
      records: `plan(row, answers, policy, channelFor(kind))` yields one
      `pending` record per completed answer not yet delivered, with
      `channel` `pty-inline` when `body.length <= maxPaste[kind]`,
      `pty-file` otherwise (`<workspace>/deliveries/<id>.md` + the one
      instruction line `@nyx answered in <name>; the whole answer is in
      <path> — read it in full, from the top`), `opencode-server` /
      `pi-followup` where enabled; the body is the COMPLETE answer text —
      an assertion with a 60 000-character fixture; states move `pending →
      submitting → accepted` only on `receipt` (the body or the instruction
      line found as a user turn in the lead's transcript), `→ uncertain`
      after `receiptMs` with no receipt and no error, `→ failed` on a
      write error before any byte reached the PTY (and only then is it
      re-planned); an `uncertain` record is never re-planned automatically;
      `seenAfter(row, deliveries)` advances the lead's mark only across
      accepted answers contiguous with it; a `pty-file` acceptance does not
      advance the mark.
- [ ] [IMPL-PANE-30] `hosts/lib/deliveries.js`; `src/store.js`
      `delivery.upsert`. -> satisfies [TEST-PANE-29]
- [ ] [TEST-PANE-31] `tests/delivery-watch.test.mjs` — `Watcher(bridge,
      store, completion, readiness, clock)`: a `pane.idle` event or a
      floor tick (10 s, injected clock) reads completion for every live
      conversation with a bound session; a new complete answer under an
      effective `auto` plans one delivery; the write happens only when
      `leadReady` is `ready`, else the record stays `pending` and
      `state.changed` tells the page; policy re-evaluated right before the
      write, and a switch to `manual` cancels pending automatic records;
      one in-flight write per target pane; a bridge EOF between the paste
      and the `\r` leaves the record `uncertain`; a lead replaced (new
      generation) holds pending records and delivers none to the new
      occupant; the `--wait` grace is honoured before a standing answer is
      treated as new.
- [ ] [IMPL-PANE-32] `src/delivery-watch.js`, started by `serveUi` in
      standalone mode. -> satisfies [TEST-PANE-31]
- [ ] [TEST-PANE-33] `tests/channels.test.mjs` — one interface
      `deliver(channel, target, record)`: `pty-inline` and `pty-file` call
      `pane.write_paste` with the sanitised body / instruction line;
      `opencode-server` POSTs to the running TUI's server (a stub server
      in the test) and reports admission from its response; `pi-followup`
      writes the follow-up through the extension's channel (a stub);
      a channel not enabled for the lead's harness is never chosen.
- [ ] [IMPL-PANE-34] `src/channels/{pty.js, opencode.js, pi.js}`;
      `channelFor(kind)` from `findings-01.md`'s table. -> satisfies [TEST-PANE-33]
- [ ] [TEST-PANE-35] `tests/ui-panes.test.mjs` — bridge ops for the page:
      `answers.list {conversation}` returns completed answers with ids,
      previews (first 200 characters) and `delivered` flags; `deliver.now
      {answerId}` creates a `pending` record marked `manual` (policy
      bypassed, readiness still honoured); an already-delivered answer
      needs `resend:true`; `deliver.cancel {deliveryId}`.
- [ ] [IMPL-PANE-36] `src/panes.js` handlers. -> satisfies [TEST-PANE-35]

## Phase 4: The page — selector, sidebar, panes, layouts, menus [pending] — gated by P3

- [ ] [TEST-PANE-37] `tests/layout.test.mjs` — `gridTemplate(n)` for
      total panes 1–6 matches the six pictures in Architecture (lead spans
      both rows for odd n ≥ 3; even n row-major); beyond 6, `rows =
      ceil(sqrt(n))`; `minPane` (cols × rows in cells) is respected and
      the function reports `tooMany` when the area cannot hold it; the
      lead's cell is always `lead`.
- [ ] [IMPL-PANE-38] `src/layout.js`. -> satisfies [TEST-PANE-37]
- [ ] [TEST-PANE-39] `app/tests/page.spec.mjs` (Playwright, a
      `window.__TAURI__` shim feeding canned tabs, conversations, answers
      and deliveries) — selector: three modes, collapses upward; claude/pi:
      the roster iframe, no sidebar; standalone: the sidebar (collapsible
      left) lists tabs and their conversations, a click focuses the pane;
      four panes render `gridTemplate(4)`, five render `gridTemplate(5)`;
      a worker opening does not steal focus from the lead; titles read
      `name · @agent · policy (source)`, shell panes `shell`; right-click on
      a worker: **Send reply to lead…** opens the transcript-backed list
      (ids, previews, delivered marks, resend), **Auto / Manual / Inherit**
      for the pane; the tab header offers the tab policy; a pending
      delivery shows a badge with **Deliver now**; **New conversation**
      asks directory and harness; **New pane** offers Shell and Agent
      (agent picker + message); hiding a tab keeps its panes alive
      (canned state unchanged after switching mode and back); every asset
      loads from `frontendDist` — no request leaves the origin.
- [ ] [IMPL-PANE-40] `app/ui/` — `index.html`, `panes.js`, `term.js`
      (`Emulator` interface `write`/`onData`/`resize`/`dispose`, the xterm
      implementation, acks per `seq`), `menus.js`, `layout` imported from
      `src/layout.js` at build time; xterm JS/CSS bundled by
      `app/scripts/bundle-ui.mjs`; `lib.rs` commands `open_lead(dir,
      harness)`, `open_shell(tab)`, `open_consult(tab, agent, task)`,
      `close_pane`, `pane_input`, `pane_resize`, `pane_ack(seq)`,
      `set_policy(scope, id, mode)`, `answers_list`, `deliver_now`,
      `deliver_cancel`, `list_state` — each proxied to Node where Node
      owns the fact; `tauri-plugin-dialog`; the roster iframe at the Node
      handle with the UI token. P3 recorded. -> satisfies [TEST-PANE-39]

## Phase 5: Lifecycle and the offline end-to-end suite [pending]

- [ ] [TEST-PANE-41] `app/src-tauri` tests + `tests/lifecycle.test.mjs` —
      closing a lead pane stops its whole process tree (a `sh -c 'sleep
      1000 & sleep 1000'` child leaves no orphan; process group on Unix, a
      Job object on Windows) and suspends the tab; app exit (`bridge` EOF)
      reaps every owned tree; a restart reads tabs as `closed`; `tab.resume`
      opens a new lead pane with generation +1; pending deliveries that
      targeted the old generation stay `pending` and are shown, not
      written; switching the displayed mode keeps every pane alive.
- [ ] [IMPL-PANE-42] `pty.rs` process-group / Job-object spawn and kill;
      `src/tabs.js` lifecycle ops; `lib.rs` exit hook. -> satisfies [TEST-PANE-41]
- [ ] [TEST-PANE-43] `tests/integration/*.test.mjs` — the real `cf`, the
      real `cf ui` server, the headless `consensflow-bridge`, and fake
      harness scripts on the fake PATH that (a) write real-shaped session
      stores (fixtures from Phase 3) and (b) echo raw input as hex; a lead
      pane and a worker pane are opened through the real path;
      scenarios: `cf run --new` from the lead pane → a worker pane with
      the `--in-pane` process under a ticket → the fake worker completes
      an answer → the watcher delivers into the lead pane → the fake lead
      records the paste in its store → the record is `accepted` → `cf
      catchup --unread` from the lead shows nothing new for that answer;
      `cf say` follow-up; a 60 000-character answer goes `pty-file` and the
      instruction line is received intact; two concurrent `cf run --new`
      from one lead; policy switched to `manual` while a delivery is
      queued; bridge killed between paste and `\r` → `uncertain`, never
      replayed; lead closed and resumed → pending held; shutdown leaves no
      process.
- [ ] [IMPL-PANE-44] `tests/integration/harness.mjs` (fake harnesses, the
      bridge launcher, hex recorders), `package.json` scripts
      `test:integration` and `check:all` (biome, `node --test` scoped to
      `tests/` so `app/tests` stays Playwright's, cargo test + clippy,
      page tests, integration, a packaged-app smoke that launches the
      bundle's `consensflow-bridge` and opens one pane). -> satisfies [TEST-PANE-43]

## Phase 6: Switch-over and packaging — standalone replaces cmux [pending] — gated by P4 for Windows

- [ ] [TEST-PANE-45] `tests/mode.test.mjs` — `MODES` is `['claude', 'pi',
      'standalone']`; a `mode.json` holding `cmux` reads as `standalone`;
      `cf use cmux` records `standalone` and says the name moved;
      `applyMode` installs the same generated skill; `syncCmuxSkills` stays
      take-back only; in standalone mode without `CONSENSFLOW_APP`, `cf run`
      now refuses naming the app; `liveWindowElsewhere`, the `cmux tree`
      call, the `CMUX_SURFACE_ID` fallbacks in `saveWindowRow` and
      `LEAD_KEYS` are gone.
- [ ] [IMPL-PANE-46] `src/mode.js` rename, `ALIASES = { cmux: 'standalone'
      }`, every `mode === 'cmux'` becomes `standalone`, the cmux branches
      deleted, direct `threads.json` writes from `cf` removed (the store is
      the app's). -> satisfies [TEST-PANE-45]
- [ ] [TEST-PANE-47] `tests/skill.test.mjs` — the standalone skill: no
      `cmux` anywhere; the consult is `cf run @<name> "<task>" --new`
      (the app mints the name and prints it — no `$(cf mint …)`, which
      cmd.exe cannot run); a follow-up is `cf say <name> "<words>"`;
      reading is `cf catchup <name> --unread`; the three acts and the
      "continue by default, a new conversation only for an independent
      task, unsure means continue" rule survive verbatim; delivery is
      taught: an answer that arrives in your pane is read WHOLE, from the
      top — and when the line names a file, that file is read in full
      before anything else; never re-read a delivered answer with
      `catchup`; never change a policy the human set; the three cmux-only
      describes are replaced, not softened.
- [ ] [IMPL-PANE-48] `src/skill.js` standalone section; `evals/harness.mjs`
      stubs `cf` only, with `run --new` printing a minted name and a
      `deliver` fixture that pastes an answer into the lead's own
      transcript; `evals/scenarios.mjs`: `consult-opens-a-pane` →
      `cf run --new` and no harness spawned; `look-before-you-send` →
      `cf catchup` then `cf say`; the dependent/independent pair; new:
      `a-delivered-answer-is-read-whole` (the lead's report contains the
      answer's last section), `a-delivered-file-is-read` (the lead reads
      the named file in full before acting), `manual-is-the-humans` (a lead
      told a conversation is `manual` by the human does not call
      `--notify`); `evals/README.md` updated. -> satisfies [TEST-PANE-47]
- [ ] [TEST-PANE-49] `app/tests/packaging.test.mjs` + `tests/windows.test.mjs`
      — `TRIPLES` gains `win32-x64 → x86_64-pc-windows-msvc`; the Node
      archive URL is `.zip` on Windows; `mirror(src, dst)` replaces `rsync`
      and deletes what the source lacks; bundle targets `nsis` on Windows,
      `deb` + `appimage` on Linux; `locate` finds `node.exe`; a harness
      installed as a `.cmd` launches through `cmd /c` with arguments
      preserved (Unicode and spaces in the path and the task); transcript
      discovery reads `%USERPROFILE%` paths; `cf doctor` prints the
      OS/harness support matrix from `findings-01.md` — native only:
      codex runs natively in PowerShell with the Windows sandbox (verified
      2026-09-06 against OpenAI's docs); claude natively; pi, opencode and
      kimi as the probe finds them. WSL is out of scope and is never
      offered as a path.
- [ ] [IMPL-PANE-50] `app/scripts/prepare-sidecar.mjs` (`fetch` + zip, no
      `curl`/`tar`), `app/scripts/sync-cli.mjs` (`mirror`),
      `tauri.conf.json` targets per platform, `lib.rs` (`node.exe`, no
      `login_path` on Windows), `src/harnesses.js` `.cmd` launch,
      `bin/cf.mjs` `doctor` matrix. P4 recorded. -> satisfies [TEST-PANE-49]

---

## Resume Context

> Revision 2 written 2026-09-06 after astraeus's review (`astraeus-lilac-dune`)
> and Gabriel's round-2 answers. Nothing implemented. TDD phase: RED on
> [TEST-PANE-01] — `app/src-tauri/src/pty.rs` under `#[cfg(test)]`, run
> `cargo test` in `app/src-tauri`, confirm it fails on the missing module.
> Before the first task: the revised spec goes back to astraeus for a
> second look in the same conversation; P1/P2/P5/P6 need the Claude
> session limit reset (22:20 Europe/Athens) for the claude rows.

## Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-09-06 | Push the FULL answer, no cap, no truncation, no notice fallback | User's call, twice ("the lead absolutely must read everything"). A body the harness cannot admit inline goes to a file the lead is told to read in full; a body nothing can admit stays `pending` and visible. `ANSWER_LIMIT` is a `cf last` console bound only |
| 2026-09-06 | `auto` = every completed worker reply, including replies to the human | User's call on astraeus's finding 8; `row.sent` is provenance for `cf say`, never an eligibility gate |
| 2026-09-06 | Two policy scopes — tab (conversation) and pane — with precedence tab-manual > pane-human > tab-auto > lead preference > default auto | User's call; his original words named both scopes |
| 2026-09-06 | Layouts counted with the lead: 2 beside, 3 lead full-height + 2 stacked, 4 a 2×2, 5 lead + 2×2, 6 a 3×2 | User's call ("the first, as I said") |
| 2026-09-06 | `standalone` replaces `cmux`; support ends with Phase 6 | User's call; one liveness path, one identity source |
| 2026-09-06 | Roster editor stays in an iframe | User's call: "whatever is cleaner" — keeps one implementation of the editor |
| 2026-09-06 | Every pane is in the app; a tab = directory + one lead + policy; a human opens shell or agent panes in it | User's calls; an agent pane is the same `consult` op `cf run` uses |
| 2026-09-06 | Ownership travels in a launch ticket, never derived from the writing process's env | astraeus finding 4: after the split the in-pane process sits in the WORKER's pane |
| 2026-09-06 | Human authority only on the page path; HTTP has no `by`; lead tokens are tab-scoped; lead launch keeps credentials, worker launch strips them | astraeus finding 2 |
| 2026-09-06 | Node serialises every mutation; `cf` submits named ops in standalone mode | astraeus finding 3: re-read-before-write does not serialise two processes |
| 2026-09-06 | Completion comes from each harness's own markers; readiness from the lead's transcript plus silence plus no human draft; delivery is a record with receipts | astraeus findings 1, 5, 6: silence is a hint, `harnessTurns` is a display reader, a write is not a receipt |
| 2026-09-06 | Native channels for opencode and pi where probed; PTY otherwise | astraeus finding 1: admission as a harness fact beats a paste |
| 2026-09-06 | One xterm implementation, bundled, with acks; no wterm stub | astraeus finding 15; research-01 §4.3 |
| 2026-09-06 | An offline end-to-end suite and `check:all` | astraeus finding 12: every boundary tested against a stub can pass with the product broken |
| 2026-09-06 | Codex is native on Windows (PowerShell + Windows sandbox); research-01 §7.3 was wrong | astraeus finding 11, verified 2026-09-06 against OpenAI's Windows docs |
| 2026-09-06 | Windows support is native only; WSL is out of scope | User's call (2026-09-06): "we do not take WSL into account, only native on Windows" |
| 2026-09-06 | Probes are gates with recorded findings, not a phase; P1/P2/P5/P6 before Phase 3, P3 before Phase 4, P4 before Windows | astraeus finding 16; the skill's phase format is TEST/IMPL pairs |
| 2026-09-06 | Rename, skill and evals in the LAST phase | risk 17: a half-migrated skill teaches commands that do not exist |

## TDD Log

| Task | Red | Green | Refactor |
|---|---|---|---|

## Deviations

| Task | Spec Said | Actually Did | Why |
|---|---|---|---|
