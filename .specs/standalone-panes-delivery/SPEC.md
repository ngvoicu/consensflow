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
can never override the human. Pull stays: `cf catchup` is unchanged in what
it prints.

**Authority.** This revision (3, 2026-09-06) and Gabriel's round-2 answers
govern implementation. Where `research-01.md` or `research-02.md` say
otherwise, they record an earlier design. Revision 4 (with astraeus's READY verdict at `d1b8ef2` and his one
pre-Phase-3 correction on part coverage applied) absorbs the third
co-lead review by astraeus (`astraeus-lilac-dune`: four narrower findings
on revision 3 plus two guards), revision 3 the second (twelve findings on
revision 2), and one scope decision: **macOS first** — Windows and Linux
packaging are deferred to a later spec, while every choice here stays
compatible with them (a PTY crate that covers ConPTY, Tauri, a JS emulator,
no Unix-only assumption added).

**Scope correction (2026-09-06, Gabriel):** the claude and pi modes go
with cmux. ConsensFlow has ONE shape after this spec — standalone, the app
owning the panes — installed into every detected harness. The mode
selector from the round-1 requirement goes with the modes; the roster
editor becomes a collapsible panel at the top of the standalone page. The
target stays macOS, Windows and Linux; this spec tests on macOS and the
other two follow in their own spec.

52 tasks across 6 phases (two independent of any phase); five live probes gate three of them.

## Team

- **Gabriel** — owner: answers, tests, starts the app to see it. Asked when
  needed, not polled.
- **Lead** (this session) — architect, project manager, keeper of this
  spec: assigns work, reviews, keeps the TDD log and the registry.
- **astraeus** — co-lead: consulted on every contract change and every
  phase exit, in `astraeus-lilac-dune`.
- **hyperion**, **zeus** — workers and reviewers of first rank (codex ultra;
  claude max — claude is rate-limited until 22:20 Europe/Athens today).
- **gefjon** — free worker (opencode), used often and for repetitive work.
- **brokkr**, **mnemosyne**, **coeus** — workers.
- **Pane policy** (the skill's own rule): one conversation per work
  stream, continued for everything that leans on it; a task independent of
  what a conversation holds gets a new conversation in a new pane; unsure
  means continue. Every consult carries the spec path, the task ids, and
  the TDD gates; nothing is applied without the lead reading it whole.

## Acceptance Criteria

- [ ] In standalone mode, `cf run @nyx "task"` from a lead's pane follows
      today's continuation rule: `--new` creates a conversation and prints
      the name the app minted; `--session` names one; otherwise the lead's
      most recent conversation with that agent continues — a live pane
      receives the task as a follow-up, a closed one is resumed. No window
      opens in the lead's pane
- [ ] `cf run`, `cf attach`, `cf say`, `cf read` in standalone mode outside
      an app pane refuse and name the app; an unavailable app never
      authorizes a second launch; a timeout after a possible launch reports
      an unknown outcome and never launches again
- [ ] A lead pane's token is scoped to its tab and to the lead's named
      operations; the `--in-pane` controller holds a single-use ticket whose
      redemption returns ownership and a capability scoped to that launch
      and generation; the harness child and a shell pane hold nothing; no
      HTTP body may carry `by`, an owner or a foreign target
- [ ] One app instance owns a state root; every `threads.json`, `tabs.json`
      and delivery write goes through one app-wide serialised queue;
      harness stores stay read-only
- [ ] Ownership and read marks in standalone mode use the app-owned lead
      identity (`tab:<id>:<generation>`); a native session is bound to a
      lead or a worker only with launch-unique evidence; an ambiguous
      binding is visibly `unbound` and never drives automatic delivery; a
      native session replaced inside a running TUI (`/new`, `/resume`, a
      fork) invalidates the binding, the readiness evidence, the receipt
      cursor and every pending decision — automatic delivery suspends, the
      page offers reopening through the app's own new/resume path with a
      new generation, and the previous transcript never authorises a write
- [ ] The page opens maximized on first launch and restores its geometry
      after; the roster editor is a collapsible panel at the top (the
      `cf ui` page in an iframe); below it the sidebar (tabs, their
      conversations, collapsible left) and the pane area; collapsing or
      expanding a panel neither stops pane services nor changes a running
      lead's `cf` semantics
- [ ] Panes tile in the user's progression counted WITH the lead: 1 alone;
      2 beside; 3 lead full-height and two stacked; 4 a 2×2; 5 lead
      full-height and a 2×2; 6 a 3×2; beyond, `rows = ceil(sqrt(n))`,
      `cols = ceil(n / rows)`; when minimum pane sizes cannot fit, every
      process is kept and a focused-pane view with navigation is offered
- [ ] Every pane has a title: conversation name · agent · effective policy
      and its source; a shell pane is titled `shell`
- [ ] The sidebar is a tree: **session** (the tab, shown by its name) →
      one level down **lead** (its name) → one level further down each
      worker (`w1 <name>`, `w2 <name>`, …) and each shell; clicking the
      session shows the grid of all its panes, clicking the lead shows the
      lead's pane alone, clicking a worker shows that worker's pane alone;
      every session ever opened stays listed, live or closed, and a closed
      one is **resumable from the sidebar**: Resume reopens the lead in a new
      generation, and clicking a closed worker conversation reopens it
      through the attach path — a session manager, not a list of what is
      currently running
- [ ] A human opens panes in a tab beside the ones consults open: a shell
      pane, or an agent pane that becomes a conversation whose lead is the
      tab's lead
- [ ] Every completed worker reply — to the lead's question or the human's
      — is delivered WHOLE into the lead's pane when the effective policy is
      `auto`: inline when its envelope fits the verified inline budget and
      is safely representable, else through `cf read <id>`, which prints
      the complete answer in numbered parts sized under the lead harness's
      verified tool-output budget, each closed by an end-of-part marker;
      a part is covered only when its complete framing appears in the
      lead's model-visible tool result and the digest of the body observed
      there equals the part on disk — printing is an attempt, a marker
      alone is not coverage. No
      cap on the answer, no truncation, no notice
- [ ] A delivery is submitted only when the lead is ready: its own
      transcript shows its last turn settled with no tool in flight, and no
      human draft is latched in that pane — a draft is cleared only by the
      observed submission that covers it (Rust stamps the epoch of the
      human's Enter; Node clears up to that epoch once the matching user
      turn appears), never by time and never past newer input; human input
      arriving between the readiness decision and the write invalidates the
      decision; PTY silence is a polling hint only. Otherwise the delivery
      waits, visibly, with its reason; **Deliver now** bypasses policy —
      never readiness, never a latched draft, and the blocked button says
      why
- [ ] Every delivery is a record: id, source answer, target session and
      generation, payload digest, pre-submission transcript cursor, state
      `pending | submitting | accepted | uncertain | failed | cancelled`;
      what is submitted is an ENVELOPE carrying the delivery id, the source
      ids and the complete answer, and its digest covers the canonical
      envelope; `accepted` needs that id and digest found after the cursor
      in the target session and generation — answer-text equality is never
      a receipt; the target stays reserved until accepted or uncertain; the
      next automatic delivery needs fresh readiness after the submitted
      turn; `uncertain` is never replayed automatically; `cancelled` is
      terminal and never recreated by polling
- [ ] Unread bookkeeping in standalone mode is by stable transcript item
      ids and delivery coverage: pull marks what it printed, an inline
      receipt covers its answer, `cf read` covers the file one, gaps stay
      unread, covered answers are not printed again
- [ ] Policy precedence holds: tab human `manual` veto > pane human setting
      > tab human `auto` > lead preference > default `auto`; set from the
      page only; `cf run --notify` records a preference; disabling cancels
      queued automatic deliveries
- [ ] Right-click **Send reply to lead…** lists the conversation's
      completed answers from the transcript; a chosen one is delivered
      through the same path; already-delivered ones need an explicit resend
- [ ] A worker's completion is reconciled when its pane exits and when the
      app restarts, so an answer finished just before exit still gets a
      record; closing a lead suspends its tab and stops its process trees;
      app exit reaps every owned tree; restart restores tabs closed; resume
      mints a new generation; held deliveries are shown with an explicit
      **Send held answers to this lead** action, never handed to a reused
      pane
- [ ] The offline integration suite drives the real `cf`, the real Node
      server, the real Rust bridge and PTYs and fake harnesses end to end;
      the packaged smoke launches the REAL app bundle on macOS in a
      self-test mode that opens a pane, renders output, takes input, acks
      and shuts down
- [ ] There are no modes: `cf use` and `cf mode` are gone (a leftover
      `mode.json` is ignored and reported once by `cf doctor`), the
      generated skill is installed into every detected harness without a
      native ConsensFlow, it names no cmux command, and the evals hold both
      directions of "continue or start fresh" and the delivery rules
- [ ] The standalone skill teaches the lead to **send and return, never
      wait**: after a consult or a follow-up it reports what is running and
      takes the user's next message; an answer arrives in its pane on its
      own when the conversation is `auto`, and when it is `manual` the human
      says when to read (`cf catchup <name> --unread`); `--wait` and polling
      are not taught, and the eval holds it
- [ ] `npm run check:all` exits 0 on macOS with no live agent CLI and no
      network: biome, Node tests, `cargo test` + clippy, page tests, the
      integration suite, the packaged smoke

## Clean install on macOS — what Gabriel gets told when it is time

Gabriel's instruction (2026-09-06): the ConsensFlow of today is removed
completely from the Mac and the new one is installed clean, with its skill
and its CLI. The lead announces the moment; the steps are these, and the
old app's own verbs do the removal so nothing is guessed:

1. Keep the roster: copy `~/.consensflow/agents.json` aside — it is shared
   with every ConsensFlow tool and `cf reset` would take it.
2. `cf off` — takes back every installed file the old ConsensFlow owns: the
   five skills, the `cf` launcher, the take-back-only leftovers.
3. `cf reset --yes` — removes the config root (`~/.consensflow`, workspaces
   and conversations included) and the app's own data directories
   (`dev.ngvoicu.consensflow`).
4. Quit the app; move `/Applications/ConsensFlow.app` to the Trash — the
   app never deletes its own bundle.
5. Install the new build's DMG; open the app once. Opening is the deliberate
   act: it claims the `cf` launcher and installs the one generated skill
   into every detected harness (`installEverywhere`, Phase 6).
6. Put `agents.json` back and open the app again so the skill regenerates
   from the roster; `cf doctor` shows one shape, no mode line, every
   harness carrying the skill, the launcher naming the app's node.

Until Phase 6 lands there is nothing to install: the announcement comes
after the switch-over commit and a green `npm run check:all`.

## Out of scope now — deferred to a later spec

Windows and Linux packaging (sidecar triples, `nsis`/`deb`/`appimage`,
`node.exe` discovery, `.cmd` launch, ConPTY probe P4, process-tree cleanup
by Job object, the OS/harness support matrix — native only, WSL never).
Every task below keeps those doors open and none locks them: `portable-pty`
covers ConPTY, the bridge is stdio, xterm is JS, paths go through Node's
`path`, and no new `#!/bin/sh` reaches shipped code. Gabriel, 2026-09-06:
"no Windows machine; later; macOS for now".

## Architecture

```
 ┌──────────────────────── ConsensFlow.app window (tauri:// origin) ─────────────────────┐
 │ [ roster editor ▾ ]  ◄ the cf ui page in an iframe, collapsible upward                 │
 │ ┌ sidebar ┐ ┌──── pane area: one tab = one directory + one lead pane + policy ───────┐ │
 │ │ tab ~/x  │ │ ┌ lead: claude ────┐ ┌ nyx-coral-lane · @nyx · auto (tab) ───────────┐ │ │
 │ │  nyx-…   │ │ │ xterm ◄─Channel  │ │ xterm (right-click: Send reply to lead…)       │ │ │
 │ │  shell   │ │ │        ack──►    │ ├ shell ────────────────────────────────────────┤ │ │
 │ │ tab ~/y  │ │ │ [1 waiting: draft open — Deliver now]                              │ │ │
 │ └──────────┘ └─┴──────────────────┴─┴───────────────────────────────────────────────┴─┘ │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        │ Tauri IPC: invoke + Channel<bytes> + acks     (the page never fetches Node)
        ▼
 ┌─ Rust: pty.rs · arbiter.rs · bridge.rs · (bin) consensflow-bridge (headless helper) ──┐
 │ pane table: (id, generation) → PtyPair, child, backlog, draft latch, input epoch       │
 │ write_paste(delivery): ESC[200~ body ESC[201~ then a separate \r, under the arbiter    │
 │ launches the bundle's ABSOLUTE node + cf.mjs; role envs (lead / controller / shell)    │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        │ JSON-lines v1 on the EXISTING stdin/stdout pipe — handle line first, then frames
        ▼
 ┌─ Node (app lifetime, independent of the displayed view) ──────────────────────────────┐
 │ src/ui.js HTTP: lead ops (consult · say · attach · read · seen · notify.lead · panes)   │
 │                 controller ops (session.bind · progress · sent.record) — per launch     │
 │ src/store.js  ONE app-wide serialised queue; instance lock on the state root            │
 │ src/tabs.js · hosts/lib/{completion,readiness,policy,deliveries}.js · src/delivery-watch│
 │ channels: pty-inline · cf-read (file) · opencode-server · pi-extension                  │
 └────────────────────────────────────────────────────────────────────────────────────────┘
        ▲ HTTP + lead token / controller capability
 cf run @nyx "task" [--new|--session X]   (requester; continuation rule as today)
 cf run … --in-pane                       (controller, under a ticket)
 cf say <name> "<words>"   cf attach <name>   cf read <deliveryId>   cf catchup <name>
```

**Identities, kept apart.** A **tab** is app-owned and persisted: id,
directory, lead `{harness, generation, nativeSession?}`, panes `[{id, kind:
lead|worker|shell, conversation?, generation, order}]`, human policy. The
**lead identity** is app-owned: `tab:<id>:<generation>`, exported to the lead
as `CONSENSFLOW_LEAD_ID`, first in `LEAD_KEYS`, and the key of ownership and
read marks in standalone mode. A **conversation** is a worker row. A
**pane** is `(id, generation)`. A **native session** is the harness's own,
bound to a lead or a worker only with launch-unique evidence: an id we
preallocated (claude `--session-id`, pi's name), an id the harness reported
on our stream, or a non-secret launch nonce in the seed's first line
(`[consensflow launch <nonce>]`, which the display reader strips) found as
the first non-empty line, after injected blocks, of one of the session's
first five user turns — never task text alone. Five, not one: on a real
codex rollout the seed is the SECOND user turn, after the `AGENTS.md`
instructions block (zeus, 2026-09-06), and a fixture that started at the
seed passed while every real session bound to nothing. Two identical
prompts in one directory bind to their own launches or stay `unbound`.

**Launch roles.** The app launches a lead directly with `CONSENSFLOW_APP`, a
tab-scoped `CONSENSFLOW_APP_TOKEN` (ops: consult, say, attach, read, seen,
notify.lead, panes), `CONSENSFLOW_LEAD_ID`, `CONSENSFLOW_TAB`,
`CONSENSFLOW_PANE_ID`, and a `PATH` whose first entry is the bundle's `bin`
so `cf` resolves to this version even with a stale global one. A worker pane
runs `cf run … --in-pane` with `CONSENSFLOW_APP` and a single-use
`CONSENSFLOW_LAUNCH` ticket; redemption returns ownership (`lead`, `tab`,
`requester`, `conversation`, `generation`) and a **controller capability**
scoped to that launch, good for `session.bind`, `progress.set`,
`sent.record` and revoked when the launch ends; the harness child gets the
`childEnv`-stripped environment. A shell pane gets nothing. Human authority
exists only on the page → Rust → Node path; the HTTP API has no `by`. The
page's UI token is not a lead token. Stripping is credential hygiene — it
stops inheritance; it is not a sandbox against code running as the same OS
user.

**Completion, readiness, delivery.** `completion.answers(kind, session)`
returns answers with stable transcript item ids and `complete` from the
harness's own markers — the vocabulary `hosts/lib/transcript-events.js`
already normalises for four engines is the starting point, and a harness is
enabled only once `findings-01.md` records, with fixtures and versions, its
completed-reply marker, its settled marker (pi's `agent_settled` versus
`agent_end` is the model), its cancellation marker and its receipt
representation. `readiness.leadReady` is `ready` only when the lead's own
transcript ends settled with no tool in flight AND the pane's draft latch is
clear; the decision carries the pane's **input epoch**, and Rust refuses the
write if human bytes arrived since. PTY silence schedules a read; it never
authorises a write. A delivery is a record whose wire form is
an **envelope**: a header line `[consensflow delivery <id> from <name>
#<item>]`, the complete answer, a trailer `[end of delivery <id>]`; the
digest covers the canonical envelope, and `accepted` needs the embedded id
and that digest found after the pre-submission cursor in the target session
and generation — two answers reading `Done.` are two envelopes. The target
is reserved until accepted or uncertain; the next automatic delivery needs
fresh readiness after the submitted turn. Channel: `pty-inline` when the
envelope fits the conservative inline budget P1 verifies (default 4 000
bytes) and contains no control byte beyond LF and TAB; otherwise `cf-read`
— the whole answer written immutable to `<workspace>/deliveries/<id>.md`
and one line pasted: `@nyx answered in <name> — run: cf read <id>  (it
prints everything; read all of it)`. `cf read` prints the answer in
numbered parts, each under the lead harness's verified tool-output budget
(pi keeps the last 2 000 lines or 50 KiB of a command's output and saves the
rest aside — the lead would see the tail and believe it read all), each
part opened by `[part k of N]` and closed by `[end of part k of N —
delivery <id>]`, and tells the lead the next `--part`. A part printed is an
attempt; a part is **covered** only when its complete framing (opening and
end marker) appears in the lead's model-visible tool result after the
cursor AND the digest computed from the part body observed there equals
the digest of the immutable part on disk — an end marker alone never
establishes coverage, because a receiver that keeps only the tail keeps
the marker and drops the text. An `EPIPE`, an early stdout close or a
truncating receiver therefore leaves ranges uncovered, and `cf`'s
deliberate exit-0 on `EPIPE` (`bin/cf.mjs:66`) is never evidence. There is
no cap on the answer.

**Drafts and clears.** Rust stamps every human `\r` with the pane's input
epoch and reports `pane.enter {epoch}`; when the matching user turn appears
in that pane's transcript, Node calls `draft.clear(pane, generation,
submittedEpoch, submissionId)` and Rust clears only the input that
submission accounted for — input typed after that epoch stays latched, a
stale clear is rejected under the same arbiter that rejects stale writes.
This holds for worker panes receiving `cf say` exactly as for the lead.

**Session replacement.** A harness can replace its native session without
replacing its process or pane (`/new`, `/resume`, a fork; pi has explicit
session-replacement events). The completion adapter reports it; the binding,
the readiness evidence, the receipt cursor and every pending decision for
that pane are invalidated; automatic delivery for it suspends with the
reason shown; the page offers reopening through the app's own new/resume
path with a new generation. This release does not follow a session switch
in place.

**Completion extraction never passes through the bounded display
normaliser.** `transcript-events.js` lends its vocabulary only: its
`adaptLine` clamps text to 8 KiB and its adapters read execution streams,
not durable session evidence. The 60 000-character assertion holds at
extraction as well as at delivery. Native channels (opencode's server, a ConsensFlow pi extension
delivering `sendUserMessage(…, {deliverAs: 'followUp'})` on
`agent_settled`) are enabled independently when P5/P6 pass their whole
path, and gate nothing else.

**The sidebar.** What the spec calls a tab, the page calls a **session**,
by name. The sidebar is a tree, always: session → lead → workers and shells,
each by name, workers numbered in the order they opened (`w1 nyx-coral-lane`,
`w2 ares-amber-moss`). Selection decides the pane area: the session node
shows the grid (`gridTemplate(n)`), the lead node shows the lead's pane
alone, a worker node shows that worker's pane alone — the same single-pane
view the focused mode uses when nothing fits, with next/previous. Sessions
are persisted (`tabs.json`) and stay listed after they close; a closed
session shows greyed with **Resume**, which reopens its lead in a new
generation, and a closed worker conversation under it reopens through the
attach path when clicked. Nothing about a closed session is forgotten by
the page: its conversations, their policies and their held deliveries are
read from the store, not from what is running.

**Layouts, counted with the lead** (totals 1–6 special; beyond: `rows =
ceil(sqrt(n))`, `cols = ceil(n / rows)`, row-major):

```
1: [lead]    2: [lead|w1]    3: [lead|w1]    4: [lead|w1]    5: [lead|w1|w2]    6: [lead|w1|w2]
                                [lead|w2]       [ w2 |w3]       [lead|w3|w4]       [ w3 |w4|w5]
```

**The lead does not wait.** In cmux mode the skill taught `cf catchup
--wait` for the moment after a question, and a lead sat blocked while a
worker thought — the user could not reach it. In standalone mode the
answer comes to the lead: under `auto` it arrives in the lead's pane as a
user turn from ConsensFlow (after the lead's own turn has settled, so it
opens the next one), and under `manual` the human decides when the lead
reads and says so. So the skill teaches send-and-return: report what is
running and in which conversation, take the user's next message, and read
only when an answer arrives or the human asks. `--wait` stays a `cf` flag
for people and scripts; the skill does not name it, and polling
(`cf catchup` in a loop, `cf sessions` every few seconds) is called out as
wrong.

## Probes — gates, recorded in `findings-01.md`

- [x] P1 — per lead harness (claude, codex, pi, opencode, kimi), through
      the REAL PTY write path (`consensflow-bridge`, not cmux): a
      multi-line body as one bracketed paste, then a separate `\r`,
      submits; the conservative inline budget holds intact — gates Phase 2
      (`cf say` is the first feature that submits text); the cmux note of
      2026-09-06 stays as evidence, not as a pass **PASSED 2026-09-06 for codex, pi, opencode, kimi**
      (brokkr, `brokkr-lilac-thicket`, through `consensflow-bridge`): 600,
      4 000 and 20 000 bytes each intact and byte-exact, all newlines kept,
      stored ~0.4 s after the `\r`; first-turn store latency up to ~3 s on pi
      and ~1 s on codex. Inline budget ≥ 20 000 B on all four; the default
      of 4 000 stays as the conservative setting. **claude PASSED too**
      (2026-09-06, same method): byte-exact at 600, 4 000 and 20 000 bytes,
      newlines kept, while the TUI showed only a `[Pasted text #1 +5 lines]`
      chip — screen ≠ store. The login was expired at the time ("Login
      expired · run /login"), which does not affect submission evidence.
- [x] P2 — kitty keyboard protocol on; a raw `\r` still submits; a
      newline inside the paste survives — Phase 2 **PASSED 2026-09-06 for codex, pi, opencode, kimi**:
      codex `ESC[>5u`, pi and kimi `ESC[>7u`, opencode only the `ESC[?u`
      query; a raw `\r` submitted everywhere; newlines survived everywhere.
      claude: no set request — it emits `ESC[<u`, the kitty clear-all-flags
      form (explicitly disengages), raw `\r` submitted, newlines survived.
      All five harnesses pass; the Phase 2 gate is open.
- [ ] P3 — an `http://localhost:<port>` iframe loads inside a `tauri://`
      page under the existing ATS exception — Phase 4
- [ ] P5 — opencode's TUI server admits a message into the running
      session, with its authentication and endpoint discovery, and the
      message reaches the transcript as a user turn — enables the
      `opencode-server` channel only
- [ ] P6 — a ConsensFlow pi extension, loaded how, delivers
      `sendUserMessage` as a follow-up on `agent_settled`, appearing as a
      user turn — enables the `pi-extension` channel only

(P4, ConPTY, moves with Windows.) Claude probes wait for the session limit
(22:20 Europe/Athens, 2026-09-06).

## Testing Architecture

### Test Framework & Tools

| Tool | Choice | Purpose |
|---|---|---|
| Node tests | `node --test tests/` | store, tabs, launch, policy, completion, readiness, deliveries, layout, bridge, `cf` side |
| Rust tests | `cargo test` in `app/src-tauri` | PTY, paste shape in raw mode, arbiter and epochs, backpressure, protocol |
| Integration | `node --test tests/integration/` | real `cf` + real Node + headless Rust bridge + fake harnesses |
| Page tests | Playwright in `app/` (`npm run test:ui`) | selector, sidebar, layouts, menus, badge — the page only |
| Packaged smoke | the real `.app` in `CONSENSFLOW_SELFTEST=1` mode | Tauri commands, Channel + acks, bundled assets, shutdown — macOS |
| Lint | biome (root), `cargo clippy` (app) | part of `check:all` |
| Release check | `npm run check:all` | everything above |
| Behaviour | `npm run eval` — real lead, stubbed `cf` | outside `check:all`; spends tokens |

### Isolation Strategy

| Layer | Approach |
|---|---|
| PTY bytes | a child in raw mode dumping hex (`stty raw -echo; od -An -tx1`) |
| Pipe protocol | Node side: the test plays the Rust parent over piped stdio; Rust side: in-process |
| Store, tabs, lock | real files under a throwaway `CONSENSFLOW_HOME`; concurrent mutations; a second instance |
| Completion, readiness, policy, deliveries | pure functions over dated fixtures copied from real stores |
| `cf` ↔ app | the REAL server with the test on the pipe as Rust; `stubApp()` for `cf`'s failure paths |
| End to end | headless `consensflow-bridge` + fake harness scripts writing real-shaped stores and echoing input as hex |
| Page | Playwright with a `window.__TAURI__` shim; no PTY |
| Packaged | the bundle's self-test mode with fake harnesses on a fake PATH |
| Network | none |

### Coverage Targets

| Metric | Target |
|---|---|
| New Node modules | every exported function, including failure branches |
| Rust `pty.rs`, `arbiter.rs`, `bridge.rs` | every public fn under `#[cfg(test)]` |
| Integration | one scenario per acceptance criterion about launch, delivery, receipt, lifecycle |
| Probes | one row per harness and probe in `findings-01.md`, with versions and fixtures |

### Test Commands

| Command | Purpose |
|---|---|
| `npm test` | Node unit suite (`tests/`, not `app/tests`) |
| `npm run check` | biome + Node tests |
| `cd app/src-tauri && cargo test && cargo clippy` | Rust |
| `npm run test:integration` | offline end-to-end |
| `cd app && npm run test:ui` | page |
| `npm run smoke` | the packaged app's self-test |
| `npm run check:all` | the release gate |
| `npm run eval` | behaviour, real lead |

## Library Choices

| Need | Choice | Alternatives | Rationale |
|---|---|---|---|
| PTY | `portable-pty` 0.9.0 (Rust, MIT) | `node-pty` | in the existing cargo build; no native addon under hardened runtime; ConPTY for later (research-01 §4.2) |
| Emulator | `@xterm/xterm` 6.0.0 + `addon-fit`, bundled into `frontendDist` | wterm | one implementation ships; wterm re-evaluated later (research-01 §4.3) |
| Bytes to page | `tauri::ipc::Channel` + acks | events | ordered, binary; acks bound the backlog |
| Directory picker | `tauri-plugin-dialog` | a text field | native |
| Native channels | opencode server API; a pi extension | PTY only | admission as a harness fact where P5/P6 prove it |
| Page tests | Playwright in `app/` | none | the root package stays zero-dependency |

## Phase 1: Bridge protocol and PTY transport [completed]

- [x] [TEST-PANE-01] `app/src-tauri/src/pty.rs` `#[cfg(test)]` — `open`
      spawns `sh -c 'printf hello'` and the reader yields `hello` then EOF;
      `resize(24, 80)` is seen by `sh -c 'stty size'`; `kill` ends the
      reader and the whole process group (`sh -c 'sleep 1000 & sleep
      1000'` leaves no orphan); an env map reaches the child; a raw-mode
      recorder child (`stty raw -echo; od -An -tx1`) echoes exactly the
      bytes written, as hex; `open` takes absolute `argv[0]` and refuses a
      bare name.
- [x] [IMPL-PANE-02] `pty.rs` — `PaneTable` keyed by `(id, generation)`;
      `open(cwd, argv, env, size)` spawning in its own process group,
      `write`, `resize`, `kill`, `list() -> [PaneInfo{id, generation, alive,
      idle_ms}]`; `Cargo.toml` gains `portable-pty = "0.9"`. -> satisfies [TEST-PANE-01]
- [x] [TEST-PANE-03] `arbiter.rs` tests — `write_paste(pane, epoch, body)`
      produces on the raw recorder `1b5b3230307e` + body + `1b5b3230317e`,
      then after `enter_delay_ms` a lone `0d`, as two writes; human bytes
      set the **draft latch** and bump the **input epoch**; a paste with a
      stale epoch is refused `Stale`; a paste while the latch is set is
      refused `Draft`; a human `\r` emits `pane.enter {epoch}`; the latch
      is cleared ONLY by `draft.clear(pane, generation, submittedEpoch,
      submissionId)` and only for input up to `submittedEpoch` — the
      counterexample is a test: the human submits A (epoch 7), types draft
      B (epoch 9), Node's delayed clear for A arrives and B stays latched;
      a clear with a stale generation is rejected; never by elapsed time (a
      fixture waits 10× any timer);
      human bytes arriving between the paste and the `\r` are queued behind
      the `\r`, never interleaved; `sanitize` normalises CRLF to LF and
      refuses any control byte beyond LF and TAB.
- [x] [IMPL-PANE-04] `arbiter.rs` (per-pane lock from paste to `\r`, latch,
      epoch, queued human bytes); `pty.rs` `write_paste`; `sanitize`.
      -> satisfies [TEST-PANE-03]
- [x] [TEST-PANE-05] `tests/bridge.test.mjs` + `bridge.rs` tests — the
      protocol: the first stdout line is the handle line (assertion reused
      from `tests/ui.test.mjs:17-50`), then frames `{v:1, id, kind:
      'req'|'res'|'evt', op, body}`; ids namespaced `n-`/`r-`; `deadlineMs`
      answered `{ok:false, error:'deadline'}` on lapse; frames over
      `maxFrameBytes` refused; EOF rejects every outstanding request;
      **nested requests** (Rust awaits Node's `consult` while Node asks Rust
      `pane.open`); handle line and first frame in ONE read keep the
      buffered bytes; stdout carries frames only; the bridge is inert when
      stdin is not a pipe.
- [x] [IMPL-PANE-06] `src/bridge.js` and `app/src-tauri/src/bridge.rs`.
      -> satisfies [TEST-PANE-05]
- [x] [TEST-PANE-07] `pty.rs` tests — backpressure: with `yes` as the
      child, unacked output is bounded at `backlogBytes`; the reader pauses
      and resumes on acks; input stays responsive; a pane whose consumer is
      HIDDEN still acks (the consumer is the page's per-pane emulator, alive
      whether or not it is displayed — asserted in Phase 4 too); `kill` ends
      everything.
- [x] [IMPL-PANE-08] `pty.rs` ack-gated reader with `seq`. -> satisfies [TEST-PANE-07]
- [x] [TEST-PANE-09] `app/src-tauri/tests/headless.rs` — the
      `consensflow-bridge` binary speaks the protocol over stdio: open the
      raw recorder, `write_paste`, observe the hex, `list`, `kill`, EOF
      reaps.
- [x] [IMPL-PANE-10] `app/src-tauri/src/bin/consensflow-bridge.rs` sharing
      the lib crate's modules. -> satisfies [TEST-PANE-09]

**Phase 1 exit evidence:** exact paste-then-separate-CR bytes through the
real PTY; stale writes and stale clears rejected; newer drafts preserved;
human input serialised behind an in-flight `\r`; bounded output with
responsive input; nested bridge requests; EOF and process-group cleanup;
P1/P2 recorded through this path for each harness being enabled.

## Phase 2: Tabs, identity, launch authority and the store [in-progress] — gates P1, P2 passed on all five harnesses

- [ ] [TEST-PANE-11] ← current (brokkr built it green, gefjon finishing the gate in `gefjon-copper-sky`) `tests/store.test.mjs` — `Store(home)`: ONE app-wide
      queue — two mutations on one row, two on different rows, two tabs
      created in different directories, 50 concurrent mixed mutations: none
      lost; an **instance lock** at `<root>/app/instance.lock` (pid, start
      time) refuses a second `Store` on the same root and reclaims a lock
      whose pid is dead; a reservation written across two files is
      recovered on restart by the documented rule (a reservation with no
      pane is released); named ops `conversation.create`, `session.bind`,
      `sent.record`, `policy.set`, `seen.set`, `delivery.upsert`,
      `tab.*`; harness stores never opened for writing.
- [ ] [IMPL-PANE-12] `src/store.js` — the queue, the lock, the ops over
      `writeJsonAtomic`. -> satisfies [TEST-PANE-11]
- [ ] [TEST-PANE-13] `tests/tabs.test.mjs` — `tab.create(dir, harness)`
      returns `{id, generation: 1, leadId: 'tab:<id>:1'}`; two tabs may
      share a directory without sharing a lead; `addPane`, `removePane`,
      order; `suspend`, `resume` (generation +1, new `leadId`); a restart
      reads every tab `closed`; a pane id with a new generation is not the
      old pane; `leadId({CONSENSFLOW_LEAD_ID:'tab:1:2'})` is `'tab:1:2'`
      and wins over `CLAUDE_CODE_SESSION_ID`.
- [ ] [IMPL-PANE-14] `src/tabs.js`; `hosts/lib/threads.js` `LEAD_KEYS`
      gains `CONSENSFLOW_LEAD_ID` first. -> satisfies [TEST-PANE-13]
- [ ] [TEST-PANE-15] `tests/launch.test.mjs` — roles and tickets: the lead
      env (`CONSENSFLOW_APP`, tab-scoped token, `CONSENSFLOW_LEAD_ID`,
      `_TAB`, `_PANE_ID`, `PATH` starting with the bundle's `bin`, no
      `CONSENSFLOW_CHILD`); the controller env (`CONSENSFLOW_APP`,
      `CONSENSFLOW_LAUNCH`, `_PANE_ID`, nothing else); the harness child's
      env through `childEnv` has none of the app variables; the shell env
      has none; `issueTicket` is single-use and expires; `redeem(ticket)`
      returns ownership plus a controller capability scoped `{launch,
      generation, ops:[session.bind, progress.set, sent.record]}`, revoked
      by `launch.end`; a lead token on another tab, on controller ops, or
      on `/api/agents|mode|reset` is refused; the UI token on `/api/panes/*`
      is refused; any body carrying `by`, `lead`, `owner` or a foreign
      `tab` is 400; Rust's launch argv names the bundle's absolute node and
      `cf.mjs`.
- [ ] [IMPL-PANE-16] `src/launch.js`, `src/ui.js` scoping middleware,
      `hosts/lib/runners.js` `childEnv` strip list, `lib.rs` absolute
      launch paths. -> satisfies [TEST-PANE-15]
- [ ] [TEST-PANE-17] `tests/ui-panes.test.mjs` — against the REAL server
      (the test on the pipe as Rust): `POST /api/panes/consult {agent, task,
      brief?, context?, handoffFile?, session?, fresh?, notify?, opId}`
      applies the continuation rule — `fresh` creates (name minted), a
      named or resolved conversation with a LIVE pane receives the task as
      a follow-up (`say`), a closed one resumes — and never opens a second
      launch for one conversation (`reserved`, refused); the same `opId`
      returns the same result; Rust deduplicates `pane.open` by launch id;
      a bridge timeout after `pane.open` was sent answers `{outcome:
      'unknown', launch}` and a retry with a new `opId` is refused while the
      launch is unresolved; `POST /api/panes/say {session, text, opId}`
      pastes on the live pane and records `sent`; `attach {session, opId}`
      resumes under a fresh ticket; `read {deliveryId, part}` returns that
      part with its markers and records an ATTEMPT, never coverage — the
      part size comes from the lead harness's verified tool-output budget;
      `seen {session, items}` by item id; `GET
      /api/panes` lists the caller's tab; controller ops `session.bind
      {evidence}`, `progress.set`, `sent.record` under a capability.
- [ ] [IMPL-PANE-18] `src/ui.js` + `src/panes.js`. -> satisfies [TEST-PANE-17]
- [ ] [TEST-PANE-19] `tests/cf-standalone.test.mjs` — the `cf` side: from a
      lead pane, `cf run @zeus "q"` (no flags) POSTs `consult` without
      `fresh`; `--new` prints `conversation: <name> (new) — pane <id>`;
      `--json`; `--in-pane` without `CONSENSFLOW_LAUNCH` refuses; with a
      ticket, ownership comes from redemption, never from `leadId(env)`,
      and later store updates (session discovery, kimi progress) go through
      the controller capability; without `CONSENSFLOW_APP` today's cmux
      behaviour is byte-identical until Phase 6; `cf attach`, `cf say`,
      `cf read`, `cf chat` refuse when the app is unreachable;
      `CONSENSFLOW_CHILD=1` refuses all; a stale global `cf` on `PATH` is
      shadowed by the bundle's.
- [ ] [IMPL-PANE-20] `bin/cf.mjs` — requester, controller, `sayVerb`,
      `readVerb`, `attachVerb`/`chatVerb` standalone branches; the kimi
      first-turn path takes ownership from redemption. -> satisfies [TEST-PANE-19]
- [x] [TEST-PANE-21] `tests/engine/session-binding.test.mjs` —
      `bindEvidence(kind, candidate, launch)`: claude and pi bind on the
      preallocated id; codex, opencode and kimi bind when the launch nonce is the first
      non-empty line of one of the candidate's first five user turns
      (fixtures copied from REAL rollout heads, including codex's
      `AGENTS.md` first turn and an attributed-tag first turn), or when the
      harness reported the id as a structured line on our own stream;
      a `replaced` result carries `bound:false` and a reason; replacement
      never fails open when liveness is unknown; two candidates in one
      directory with identical task text and different nonces bind to their
      own launches; no nonce → `unbound`, shown by `cf catchup` and
      `shouldDeliver` false; a LEAD binds the same way at `open_lead` (the
      nonce rides in the lead's first seeded line for harnesses we cannot
      preallocate, and `unbound` leaves its readiness `unknown`); a fixture
      where the process and pane stay alive but the native session changes
      (`/new`, `/resume`, a fork) yields `replaced`, which invalidates the
      binding and every dependent decision.
- [x] [IMPL-PANE-22] `hosts/lib/harness-transcript.js` discoverers take a
      `nonce`; `hosts/lib/packets.js` emits and the reader strips
      `[consensflow launch <nonce>]`; `src/store.js` `session.bind` refuses
      without evidence. -> satisfies [TEST-PANE-21]

**Phase 2 exit evidence:** real `cf` → real Node → headless Rust →
controller → fake harness; correct credentials per role and native
binding by nonce; continuation without a second process; duplicate and
timed-out launches handled; concurrent mutations preserved; session
replacement fails closed; an interrupted read creates no coverage.

## Phase 3: Completion, readiness and delivery [pending] — its first harness needs its versioned lifecycle and receipt fixtures before automatic delivery is enabled

- [ ] [TEST-PANE-23] `tests/engine/completion.test.mjs` —
      `answers(kind, session)` returns `{items: [{id, role, text, complete,
      settled, at}], inFlight, cancelled?}` from each harness's own markers,
      starting from `transcript-events.js`'s vocabulary; the per-harness
      table (completed-reply marker, settled marker, cancellation marker,
      receipt representation, unsupported states, version) is asserted
      against dated fixtures: partial text then tool work, text growing
      with an unchanged turn count (kimi), cancellation, compaction, a
      codex fork, and a session replaced in place → `{replaced: true}`;
      unreadable or unlisted → `{unknown: true}`; extraction never calls
      `adaptLine` and a 60 000-character answer comes back whole.
- [ ] [IMPL-PANE-24] `hosts/lib/completion.js`, one adapter per harness;
      `findings-01.md` gets the table per harness before that harness is
      enabled. -> satisfies [TEST-PANE-23]
- [ ] [TEST-PANE-25] `tests/engine/readiness.test.mjs` —
      `leadReady({items, inFlight, draftLatched, epoch})` is `ready` only
      when the lead's items end settled with no tool in flight and the
      latch is clear, returning the epoch the decision was made at; `busy`
      on an in-flight turn; `draft` while latched (with the reason for the
      page); `unknown` on an unreadable store; a periodically redrawing idle
      pane is `ready` — silence is not a condition.
- [ ] [IMPL-PANE-26] `hosts/lib/readiness.js`. -> satisfies [TEST-PANE-25]
- [ ] [TEST-PANE-27] `tests/engine/policy.test.mjs` —
      `effectivePolicy(tab, pane, row)`: tab `manual` vetoes; else pane
      human setting; else tab `auto`; else lead preference; else `auto`;
      `inherit` falls through; the result names its source; `--notify`
      writes only `notifyPreference`.
- [ ] [IMPL-PANE-28] `hosts/lib/policy.js`. -> satisfies [TEST-PANE-27]
- [ ] [TEST-PANE-29] `tests/engine/deliveries.test.mjs` — records: `plan`
      yields one `pending` per completed, uncovered answer under `auto`,
      with `digest`, `channel` (`pty-inline` when the serialised envelope
      ≤ `inlineBudget[kind]` and safely representable, else `cf-read` with
      the immutable file), never a truncated body (a 60 000-character
      fixture and a fixture with an ESC byte both go `cf-read`); a file
      write failure stays `pending` with the reason; `submit` stores the
      target session, generation and the pre-submission cursor; the wire
      form is the envelope (header with delivery id and source ids, the
      complete answer, trailer) and `digest` covers the canonical envelope;
      `receipt` accepts only an item after the cursor carrying THIS
      delivery id and digest (two workers both answering "Done." produce
      two distinct acceptances; an older matching turn, or an unrelated
      identical user message after the cursor, does not); for `cf-read`,
      a part is `covered` only when its full framing appears in the lead's
      tool result after the cursor and the digest of the observed body
      equals the immutable part's — a fixture that keeps every end marker
      but drops answer text covers nothing; a missing part leaves its range
      uncovered; `uncertain` after `receiptMs`, or
      on crash recovery from `submitting`; `failed` only before any byte
      was written, and only then re-planned; `cancelled` terminal and never
      re-planned; the target stays reserved until accepted or uncertain;
      `coverage` maps delivery → item ids; `seen` advances only over
      covered-or-printed items contiguous with the mark; a file delivery
      covers nothing until every part is covered by framing AND digest.
- [ ] [IMPL-PANE-30] `hosts/lib/deliveries.js`; `src/store.js`
      `delivery.upsert`; standalone `catchup` bookkeeping by item ids.
      -> satisfies [TEST-PANE-29]
- [ ] [TEST-PANE-31] `tests/delivery-watch.test.mjs` — `Watcher` runs for
      the app's lifetime, independent of the displayed view; a `pane.idle`
      event or a floor tick reads completion for every BOUND conversation,
      live or not; a `pane.exit` triggers a final reconcile for that
      conversation (an answer completed just before exit gets a record);
      restart reconciles bound conversations whose panes are closed; a
      new complete answer under `auto` plans one delivery; the write
      happens only on `ready`, passing the epoch — a `Stale` or `Draft`
      refusal from Rust leaves the record `pending` with the reason and
      `state.changed` tells the page; policy re-evaluated right before the
      write; `manual` cancels pending automatic records (`cancelled`);
      after a submission the next automatic delivery to that pane waits for
      fresh readiness after the submitted turn; bridge EOF between paste and
      `\r` → `uncertain`; a resumed lead (new generation) holds pending
      records for the old one and offers **Send held answers to this
      lead**, which creates new records; a `replaced` report from
      completion suspends automatic delivery for that pane, invalidates its
      cursor and pending decisions, and tells the page; the `--wait` grace
      is honoured.
- [ ] [IMPL-PANE-32] `src/delivery-watch.js`, started by the app process
      at launch (not by mode). -> satisfies [TEST-PANE-31]
- [ ] [TEST-PANE-33] `tests/channels.test.mjs` — `deliver(channel, target,
      record)`: `pty-inline` and `cf-read` call `pane.write_paste` with the
      epoch; `opencode-server` POSTs to the running TUI's server found and
      authenticated as P5 recorded, and reports admission from the
      response; `pi-extension` writes the record to the extension's inbox
      and reports admission from the extension's ack file; a channel not
      enabled for the lead's harness is never chosen; an enabled channel
      carries its launch configuration (extension path, endpoint discovery).
- [ ] [IMPL-PANE-34] `src/channels/pty.js` always; `src/channels/opencode.js`
      only once P5 is recorded as passed; `src/channels/pi.js` and the
      `consensflow-delivery` pi extension under `hosts/pi-extension/` only
      once P6 is recorded as passed — and P6 must include an inbox arrival
      while the agent is ALREADY idle (delivered at once, not stranded until
      the next `agent_settled`). -> satisfies [TEST-PANE-33]
- [ ] [TEST-PANE-35] `tests/ui-panes.test.mjs` — page ops: `answers.list`
      (ids, previews, `delivered`, `uncertain` marked distinctly);
      `deliver.now {answerId}` creates a `manual` record (policy bypassed,
      readiness honoured); `resend` explicit; `deliver.cancel`;
      `held.send {tab}` for held records; `tab.resume`.
- [ ] [IMPL-PANE-36] `src/panes.js` handlers. -> satisfies [TEST-PANE-35]

## Phase 4: The page — selector, sidebar, panes, layouts, menus [pending] — gated by P3

- [x] [TEST-PANE-37] `tests/layout.test.mjs` — `gridTemplate(n)` for 1–6
      matches the six pictures; beyond 6 `rows = ceil(sqrt(n))`, `cols =
      ceil(n / rows)`, row-major; `fits(n, area, minPane)` false → the page
      shows the focused-pane view (asserted in TEST-39); the lead cell is
      always `lead`.
- [x] [IMPL-PANE-38] `src/layout.js`. -> satisfies [TEST-PANE-37]
- [ ] [TEST-PANE-39] `app/tests/page.spec.mjs` (Playwright, `__TAURI__`
      shim) — first launch maximized, later launches restore geometry; the
      roster panel (the `cf ui` iframe) collapses upward and expands; the
      sidebar (collapsible left) and the pane area; four and five
      panes render their templates; the sidebar renders the tree session →
      lead → `w1 …`, `w2 …`, shells; clicking the session node shows the
      grid, the lead node the lead's pane alone, a worker node that pane
      alone; a closed session is listed greyed with **Resume**, which calls
      `tab_resume`, and a closed worker under it calls `attach` when
      clicked; a worker opening keeps focus on the lead; titles `name · @agent · policy (source)`, `shell`; right-click
      **Send reply to lead…** lists answers with delivered/uncertain marks
      and resend; **Auto / Manual / Inherit** per pane; tab policy in the
      tab header; a waiting delivery shows its reason (`draft open`, `lead
      busy`, `unbound`) and **Deliver now**; held records show **Send held
      answers to this lead**; **New conversation**, **New pane** (Shell /
      Agent); when panes cannot fit, the focused-pane view with next/prev;
      a HIDDEN tab's emulators keep consuming and acking (a canned flood on
      a hidden pane drains); no request leaves the origin except the roster
      iframe's own.
- [ ] [IMPL-PANE-40] `app/ui/` (`index.html`, `panes.js`, `term.js` with
      the `Emulator` interface and per-pane xterm instances that live while
      the pane does, `menus.js`), `app/scripts/bundle-ui.mjs`; `lib.rs`
      commands `open_lead`, `open_shell`, `open_consult`, `close_pane`,
      `pane_input`, `pane_resize`, `pane_ack`, `set_policy`, `answers_list`,
      `deliver_now`, `deliver_cancel`, `held_send`, `tab_resume`,
      `list_state`, window geometry persistence; `tauri-plugin-dialog`; the
      roster iframe. P3 recorded. -> satisfies [TEST-PANE-39]

## Phase 5: Lifecycle, the end-to-end suite and the packaged smoke [pending]

- [ ] [TEST-PANE-41] `app/src-tauri` + `tests/lifecycle.test.mjs` — closing
      a lead stops its process tree and suspends the tab; app exit reaps
      every tree; restart reads tabs closed; `tab.resume` → generation +1;
      held deliveries stay held and visible; collapsing the roster panel or
      switching tabs keeps every pane and the watcher running; an
      authenticated lead's `cf` keeps its routing while its tab exists.
- [ ] [IMPL-PANE-42] `pty.rs` process groups, `src/tabs.js` lifecycle,
      `lib.rs` exit hook, routing by tab existence. -> satisfies [TEST-PANE-41]
- [ ] [TEST-PANE-43] `tests/integration/*.test.mjs` — real `cf`, real
      `cf ui`, headless `consensflow-bridge`, fake harnesses writing
      real-shaped stores and echoing input as hex: lead and worker opened
      through the real path; `cf run --new` → worker under a ticket → the
      fake worker completes → delivery into the lead pane → the fake lead
      records the paste → `accepted` → `cf catchup --unread` shows nothing
      new for that answer; `cf run` without flags continues into the live
      pane; a 60 000-character answer goes `cf-read` and every `cf read` part's
      framing plus body digest in the fake lead's transcript records
      coverage; two concurrent `cf run --new`; two leads
      in one directory with identical tasks bind separately; a draft
      latched in the lead pane holds delivery until the submission covering
      it clears it, and a draft typed after that submission stays latched;
      a `cf read` whose stdout is closed early, one whose receiver keeps only
      the tail, and one whose receiver keeps every end marker but drops
      text, all leave ranges uncovered and the page shows them;
      a fake lead whose native session is replaced in place suspends
      delivery;
      policy switched to `manual` while queued; bridge killed between paste
      and `\r` → `uncertain`, never replayed; a worker completing and
      exiting before the next tick still delivers; lead closed and resumed
      → held; shutdown leaves no process.
- [ ] [IMPL-PANE-44] `tests/integration/harness.mjs`; `package.json`
      scripts `test:integration`, `check:all` (biome, `node --test tests/`,
      cargo test + clippy, page tests, integration, smoke). -> satisfies [TEST-PANE-43]
- [ ] [TEST-PANE-45] `tests/smoke.test.mjs` — the built `.app` launched
      with `CONSENSFLOW_SELFTEST=1` and a fake-harness `PATH`: the real
      Tauri window opens a tab, a pane renders the fake harness's output
      (the page reports rendered rows over the self-test channel), input
      typed through `pane_input` reaches the child (hex echo), acks flow,
      the app exits 0 and leaves no process; the assets loaded came from
      the bundle.
- [ ] [IMPL-PANE-46] `lib.rs` self-test mode and `app/ui/selftest.js`;
      `npm run smoke`. -> satisfies [TEST-PANE-45]

## Phase 6: Switch-over — one shape, no modes [pending]

- [ ] [TEST-PANE-47] `tests/mode.test.mjs` → `tests/install.test.mjs` —
      there is no mode: `cf use` and `cf mode` exit with "ConsensFlow has
      one shape now" and the verb list; a leftover `mode.json` (`cmux`,
      `claude`, `pi`, `standalone`) is ignored, and `cf doctor` reports it
      once as removable; `installEverywhere` (replacing `applyMode`) puts
      the one generated skill into every detected harness without a native
      ConsensFlow and claims the launcher; `cf off` still takes everything
      back; `syncCmuxSkills` stays take-back only; `cf run` without
      `CONSENSFLOW_APP` now refuses naming the app; `liveWindowElsewhere`,
      `cmux tree`, the `CMUX_SURFACE_ID` fallbacks, `cf`'s direct
      `threads.json` writes, and the claude/pi host-mode skill prose are
      gone.
- [ ] [IMPL-PANE-48] `src/mode.js` → `src/install.js` (`installEverywhere`,
      `turnOff`), `bin/cf.mjs`, `hosts/lib/threads.js`, `src/skill.js`
      (one prose), `src/ui.js` (no mode switcher; the page's system panel
      loses the three cards). -> satisfies [TEST-PANE-47]
- [ ] [TEST-PANE-49] `tests/skill.test.mjs` — the standalone skill: no
      `cmux`; the consult is `cf run @<name> "<task>"` with today's
      continuation rule, `--new` for an independent task, the name printed
      by the app; follow-up `cf say`; reading `cf catchup --unread`; the
      three acts and "continue by default, unsure means continue" verbatim;
      delivery taught: an answer that arrives in your pane is read WHOLE
      from the top, a line naming `cf read <id>` is run and its output read
      in full before anything else, a delivered answer is not re-read with
      `catchup`, a policy the human set is never changed; **send and
      return**: after a consult or a follow-up the lead reports what is
      running and takes the user's next message — under `auto` the answer
      arrives in its pane, under `manual` the human says when to read; the
      skill never names `--wait`, and says polling is wrong; the three
      cmux-only describes replaced.
- [ ] [IMPL-PANE-50] `src/skill.js`; `evals/harness.mjs` (stub `cf` only;
      `run --new` prints a minted name; a `deliver` fixture pastes into the
      lead's transcript; `read` prints a long fixture); `evals/scenarios.mjs`
      (`consult-opens-a-pane` → `cf run --new`, no harness; `look-before-
      you-send` → `cf catchup` then `cf say`; the dependent/independent
      pair; new: `a-delivered-answer-is-read-whole`, `a-delivered-file-is-
      read` (the lead runs every `cf read` part and its report contains
      content from the beginning, the middle AND the end of the file, with
      every range covered), `manual-is-the-humans`, `a-lead-sends-and-returns`
      (after `cf run --new` or `cf say` the lead runs no `cf catchup --wait`,
      no repeated `cf catchup`/`cf sessions`, and its report says the work
      is running and where); `evals/README.md`. -> satisfies [TEST-PANE-49]

### Independent of the phases — Sol at max (any time; assigned to gefjon)

- [x] [TEST-PANE-51] `tests/catalog.test.mjs` + `tests/cli.test.mjs` +
      `tests/engine/claude-core.test.mjs` + `tests/skill.test.mjs` +
      `tests/fixtures/v1-participants.json` — the `hyperion` preset is
      "Codex GPT 5.6 Sol MAX" at `effort: 'max'`; no preset in the catalog
      names `ultra`; `EFFORTS.codex` still lists `ultra` as a level the CLI
      takes (it was walked live) but the catalog test asserts no row uses
      it; the v1 fixture row for hyperion reads `max`; `cf agent sync`
      moves an existing `hyperion` row from `ultra` to `max` (label,
      effort, description — the preset-owned fields), and the generated
      skill's roster line reads "Sol MAX; max effort".
- [x] [IMPL-PANE-52] `hosts/lib/presets.js` (the Sol row: label, effort,
      description, and the ladder comment that explains why Sol sits at
      `max` below the proven `ultra` ceiling), `src/skill.js:136` ("minutes
      for max"), `tests/fixtures/v1-participants.json`. -> satisfies [TEST-PANE-51]

---

## Resume Context

> 2026-09-06 23:45 EEST — state of play. Committed: Phase 1, the binding unit
> (21–22), probes. Uncommitted and in review or in work: store + tabs (11–14)
> — asteria BLOCK twice, round 3 now with zeus in `zeus-kelp-valley` (O_EXCL
> claim, rename-based stale reclaim, one pane allocator, degraded state on
> failed compensation, release by launch id); launch roles (15–16) by hyperion,
> green, under review by zeus in `zeus-copper-island`; readiness + policy
> (25–28) by zeus, green, under review by asteria; completion model (23–24)
> by gefjon, green with the markers table in findings-01, under review by
> hyperion in `hyperion-ember-fern`, layout (37–38) by zeus queued behind it;
> deliveries (29–30) in work by gefjon in `gefjon-lilac-waves`. Open for
> astraeus at the Phase 3 exit: whether derived "settled" (claude, pi,
> opencode: complete + no open tool) may enable automatic delivery without
> a native settled signal; pi conflating cancel with provider error; kimi
> cancel reasons unobserved.
>
> 2026-09-06 22:10 EEST — **Phase 1 committed**: every unit approved by
> asteria after five hyperion batches and two gefjon rounds; the one open
> clause is B8/C7 (a hidden pane keeps acking), deferred to Phase 4's real
> emulator by decision. Phase 1 exit evidence held: exact paste-then-CR bytes
> through the real PTY (raw recorder), stale writes and stale clears rejected,
> newer drafts preserved, human input serialised behind an in-flight CR,
> bounded output with responsive input, nested bridge requests, EOF and
> process-group cleanup, interruptible transport shutdown, P1/P2 recorded on
> all five harnesses through this path. Phase 2 in progress: brokkr built
> the store and tabs green then hit his usage limit at the biome gate; gefjon
> finishes it in `gefjon-copper-sky`. gefjon's binding unit (21–22) is
> green after zeus's BLOCK (F1–F8 fixed) and awaits zeus's re-verdict in
> `zeus-copper-island`; it is committed only after that.
>
> 2026-09-06 19:40 EEST — hyperion reports both review batches GREEN (42 unit
> + 5 headless, Node bridge 26/26; lead re-ran); asteria's round-3 Rust
> re-review is running in `asteria-velvet-brook` (her window had ended —
> resumed with `cf run --session` in the same pane; the monitor now
> reports a gone window). gefjon is on the five Node findings in
> `gefjon-frosty-tide` and was told hyperion touched `src/bridge.js` and
> its test. brokkr passed P1/P2 for codex, pi, opencode, kimi (committed)
> and now holds Phase 2 tasks 11–14 (store + tabs) in `brokkr-rusty-pine`
> — Node-only, gated only by P1/P2, started to use idle capacity while
> Phase 1 is in re-review. Phase 1 stays uncommitted until asteria's
> re-verdict.
>
> 2026-09-06 17:55 EEST — Gabriel's standing instruction (goal): the lead
> checks the workers itself, tests, and continues; he is told when the new
> ConsensFlow can be installed clean (section "Clean install on macOS").
> A monitor watches each open conversation's last turn and reports a
> report/verdict turn or a turn unchanged for three minutes; the lead then
> reads it whole with `cf catchup --unread` into a file. brokkr runs probes
> P1/P2 through the headless bridge in `brokkr-lilac-thicket`.
>
> 2026-09-06 17:20 EEST — asteria's review (`asteria-velvet-brook`) returned BLOCK on
> both units with 12 findings (4 P1: a blocked PTY writer holding the table
> lock freezes `kill`; UTF-8 split across chunks corrupted in `bridge.js`;
> `maxFrameBytes` unenforced on unterminated input; an oversized handler
> response silently dropped). Findings dispatched to the authors in their
> conversations; tasks 01–06 stay unticked until the fixes land and the
> re-review passes. hyperion also delivered the Rust half of the bridge,
> backpressure and the headless binary (tasks 05–10, 23+3 tests green in his
> run; one `openpty` failure in the lead's parallel run under investigation);
> that half is under review next. The Sol unit (51/52) is green and committed.
>
> Phase 1 started 2026-09-06 16:14 EEST, delegated by the lead (PM):
> **hyperion** holds TEST-PANE-01 → IMPL-PANE-04 (Rust `pty.rs` +
> `arbiter.rs`) in conversation `hyperion-willow-orchard` (pane
> surface:66); **gefjon** holds the Node half of TEST-PANE-05 / IMPL-PANE-06
> (`src/bridge.js`, `tests/bridge.test.mjs`, the `serveUi` wiring) in
> `gefjon-frosty-tide` (surface:67). Briefs carry the verbatim task text
> and the TDD gates; both were told to paste `cargo test` / `node --test`
> output and never edit a test to pass. The Rust half of the bridge
> (`bridge.rs`, tasks 05–06) and tasks 07–10 are unassigned until these
> land. The lead verifies every claim by running the suites itself before
> ticking a box or writing a TDD-log row; a reviewer (asteria now, zeus
> after the claude limit resets at 22:20) reads each unit before it is
> committed. Claude probes P1/P2 wait for that reset.

## Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-09-06 | macOS first; Windows and Linux packaging deferred to a later spec, choices kept compatible | User's call ("no Windows machine; later; macOS for now") |
| 2026-09-06 | Push the FULL answer: inline within a verified budget, else `cf read <id>`; no cap, no truncation, no notice | User's call, twice; astraeus finding 7 closed |
| 2026-09-06 | `auto` = every completed worker reply, including replies to the human | User's call; `sent` is provenance only |
| 2026-09-06 | Two policy scopes — tab and pane — precedence tab-manual > pane-human > tab-auto > lead preference > default auto | User's call |
| 2026-09-06 | Layouts counted with the lead; 1–6 special, beyond a `ceil(sqrt(n))` grid; focused-pane view when nothing fits | User's call; astraeus finding 12 |
| 2026-09-06 | ONE shape: the claude and pi modes go with cmux; no `mode.json`, no `cf use`; the skill goes into every detected harness | User's call (2026-09-06 goal): "only ConsensFlow standalone remains, for Windows, Mac and Linux; for now we test on Mac". Supersedes "standalone replaces cmux" |
| 2026-09-06 | Roster editor stays in an iframe | User's call: "whatever is cleaner" |
| 2026-09-06 | Every pane is in the app; a tab = directory + one lead + policy; humans open shell or agent panes | User's calls |
| 2026-09-06 | PTY silence is a polling hint, never a readiness condition; readiness = lead transcript settled + no draft latched + fresh input epoch | astraeus round-2 finding 1 resolved in the direction that keeps `auto` working for redrawing TUIs; the paste is atomic under the arbiter |
| 2026-09-06 | A draft latch is cleared only by the observed submission that covers it (epoch-stamped Enter → matching user turn), never by time, never past newer input | astraeus round-2 finding 1 and round-3 finding 2; an abandoned draft holds delivery visibly until the human submits — Deliver now cannot bypass a latched draft, and the blocked button says why; there is no other honest signal for claude and codex |
| 2026-09-06 | File delivery is `cf read <id>`: prints everything in parts; a part is an attempt, coverage needs its full framing and a matching body digest in the lead's tool result — a marker alone is never coverage | Gives the file path a receipt without reading the screen (astraeus round-2 finding 8); pi keeps only the tail of a large tool output and `cf` exits 0 on EPIPE (round-3 finding 1) |
| 2026-09-06 | App-owned lead identity `tab:<id>:<generation>`; native sessions bound by preallocated id, reported id, or a launch nonce — never task text | astraeus round-2 finding 3 |
| 2026-09-06 | The `--in-pane` controller redeems a ticket for ownership plus a launch-scoped capability; Rust launches the bundle's absolute node and cf.mjs; the lead's PATH starts with the bundle's bin | astraeus round-2 finding 2 |
| 2026-09-06 | Consult follows today's continuation rule; every op carries an `opId`; a timeout after a possible launch is `unknown` and blocks a retry | astraeus round-2 finding 4 |
| 2026-09-06 | One app-wide queue and one instance per state root | astraeus round-2 finding 5; simpler than concurrency across small files |
| 2026-09-06 | Receipts identify the delivery by digest after a cursor; the target stays reserved; fresh readiness after each submitted turn | astraeus round-2 finding 7 |
| 2026-09-06 | Completion reconciled on pane exit and restart; `cancelled` terminal; held answers sent by explicit action | astraeus round-2 finding 9 |
| 2026-09-06 | Pane services live for the app's lifetime, independent of the displayed view; hidden panes keep consuming | astraeus round-2 finding 10 |
| 2026-09-06 | The packaged smoke launches the real app in a self-test mode (tauri-driver does not support macOS) | astraeus round-2 finding 11 |
| 2026-09-06 | A conservative inline budget, not a search for the largest paste; native channels enable independently | astraeus round-2 finding 11 ("over-built") |
| 2026-09-06 | P1/P2 gate Phase 2 (`cf say` is the first submission); P3 gates Phase 4; P5/P6 gate only their channels | astraeus finding 16 |
| 2026-09-06 | Codex is native on Windows; WSL out of scope | verified 2026-09-06; user's call |
| 2026-09-06 | The delivery wire form is an envelope with the delivery id; the digest covers the envelope | astraeus round-3 finding 4: identical answer text cannot identify a delivery |
| 2026-09-06 | A native session replaced in place invalidates everything for that pane; reopening goes through the app's own path | astraeus round-3 finding 3; no seamless in-place switching in this release |
| 2026-09-06 | Native adapters are built only after their probe passes; P6 covers an inbox arrival while already idle | astraeus round-3 guard |
| 2026-09-06 | The standalone skill teaches send-and-return: no `--wait`, no polling; answers arrive under `auto`, the human says when to read under `manual` | User's call (2026-09-06): "answers come automatically, or the owner asks when to read" — a lead blocked in `--wait` is a lead the user cannot reach |
| 2026-09-06 | Team: lead = architect + PM; astraeus co-lead; hyperion, zeus workers and reviewers; gefjon free repetitive worker; brokkr, mnemosyne, coeus workers; Gabriel answers, tests, runs the app; one conversation per work stream, new pane only for independent work | User's call (2026-09-06 goal) |
| 2026-09-06 | Tag `end-of-cmux-era` at `3e485ba` on origin (NAS) and upstream (GitHub) | User's call: mark where the three-mode era ends |
| 2026-09-06 | The Sol preset moves from `ultra` to `max` everywhere; `ultra` stays a codex level nobody's preset names | User's call (2026-09-06): "Sol ultra becomes max everywhere in ConsensFlow". A deliberate seat below the proven ceiling, like the DeepSeek rows — recorded so the effort-ceilings audit does not "fix" it back |
| 2026-09-06 | The launch nonce may sit in any of the session's first five user turns, as the first non-empty line after injected blocks | zeus's review of tasks 21–22: codex puts `AGENTS.md` before the seed; the fixture that began at the seed hid it |
| 2026-09-06 | The store's `session.bind` calls `bindEvidence` against its launch record; every binding decision records its generation | zeus F7: an evidence string anyone can send is vocabulary, not proof |
| 2026-09-06 | The sidebar is a session → lead → workers tree; a node click narrows the pane area to that node; closed sessions stay listed and resume from the sidebar | User's requirement (2026-09-06): "resumable like a session manager; session, then lead a level down, then w1, w2 …; click the lead to see only its pane, a worker only its pane, the session the grid" |
| 2026-09-06 | Rename, skill and evals in the LAST phase | risk 17 |

## TDD Log

| Task | Red | Green | Refactor |
|---|---|---|---|
| [TEST-PANE-01] | hyperion, `cargo test` in `app/src-tauri`: exit 101 — `PaneTable` and `portable_pty` do not exist (7 tests written first) | — | — |
| [IMPL-PANE-02] | — | `cargo test`: 7 passed, 0 failed (process-group cleanup included); lead re-ran: 13 passed after 04 | removed an unused trait import; the process-group inspection helper compiles only under test; re-ran green |
| [TEST-PANE-03] | hyperion, `cargo test`: exit 101 — every arbiter symbol absent (6 tests: wrapper+delay bytes, epoch/latch/Enter events, Stale/Draft, the epoch-7/epoch-9 delayed-clear counterexample with a 10× wait, queueing behind the automated `\r`, sanitize across every C0 byte and DEL) | — | — |
| [IMPL-PANE-04] | — | `cargo test`: 13 passed, 0 failed; lead re-ran 2026-09-06 16:40 EEST: 13 passed | fixed a paste lock left set when the Enter-event receiver disconnects mid-flush; `cargo clippy --all-targets` clean except one pre-existing `lib.rs:169` lint, fix authorised by the lead |
| [TEST-PANE-05] (Node half) | gefjon, `node --test tests/bridge.test.mjs`: 1 fail — `ERR_MODULE_NOT_FOUND src/bridge.js` (17 tests written first) | — | — |
| [TEST-PANE-51] | gefjon, `npm test` after moving the pins: 5 failed — `'ultra' !== 'max'` in catalog, cli (add + sync), claude-core, pi-core | — | — |
| [IMPL-PANE-52] | — | `npm run check`: exit 0, 461 tests, 458 pass; lead re-ran: exit 0, 458 pass | the Astra comment that named "Hyperion's ultra tier" reworded; `skill/SKILL.md` (the checked-in v0 reference) patched by the lead to say max |
| review fixes A (01–04) + B-Rust (05–10) | hyperion, one failing test per finding (18 findings over two batches; (8), B3, B5-partial, B6 moot after earlier fixes; B8 hidden consumer left unproven by design); RED captured for every non-moot fix except (9), whose RED link step died on `ENOSPC` | `cargo test`: 42 unit + 5 headless passed; Node bridge 26/26; lead re-ran 2026-09-06 19:05 EEST: 42 + 5, clippy clean | PTY tests serialised against `openpty` exhaustion; per-pane writers; bounded delimiter-aware reader; serialized writer queue |
| Phase 1 review loop | asteria: unit A BLOCK (12 findings) → hyperion batches 1–2; Rust units BLOCK (10) → batch 2; round 3 (7) → batch 3; round 4: PTY+arbiter APPROVE, backpressure APPROVE (C7 deferred), headless approve-with-edit, bridge BLOCK on C2 → batches 4–5; Node: BLOCK (5) → gefjon N1–N5, BLOCK on N2 → N2a/b, then APPROVE. Final: Rust bridge APPROVE 2026-09-06 22:05 EEST | lead re-ran after every batch; last: `cargo test` 52 unit + 6 headless, `cargo clippy -D warnings` clean, `node --test tests/bridge.test.mjs` 43/43, `npm run check` on the exact staged tree exit 0, 489 pass | per batch, under green |
| [TEST-PANE-21] | gefjon, `node --test tests/engine/session-binding.test.mjs`: `ERR_MODULE_NOT_FOUND hosts/lib/session-binding.js` (13 tests first); after zeus's BLOCK: 13 RED on the rebuilt real-rollout fixtures; after his edits E1/E2: 7 RED | — | — |
| [IMPL-PANE-22] | — | 23 binding tests green; lead re-ran 51/51 across binding, transcript and store; `npm run check` on the exact staged tree exit 0, 514 pass | zeus review: BLOCK (F1–F9: the codex seed is the SECOND user turn on real rollouts; replacement failed open without `alive`; marker matched any line) → fixed → approve with edits (E1 discovery returns the matching turn; E2 reported-vs-preallocated agreement) → **approve** 2026-09-06 22:50 EEST |
| [TEST-PANE-37] | zeus, `node --test tests/layout.test.mjs`: `ERR_MODULE_NOT_FOUND src/layout.js` (15 tests first) | — | — |
| [IMPL-PANE-38] | — | 15/15; lead re-ran 15/15 and printed the six pictures; `npm run check` on the exact staged tree exit 0 | hyperion review: APPROVE, no findings — the six literal pictures match the spec, the beyond-6 rule, `fits` at the pixel boundary, lead-first focus order |
| [IMPL-PANE-06] (Node half) | — | `node --test tests/bridge.test.mjs`: 17 passed, 0 failed; lead re-ran: 17/17, `tests/ui.test.mjs` 29/29 | biome format + two assignment-in-expression lints fixed; one self-inflicted test sizing (a 64-byte budget could not fit the refusal frame) corrected in the TEST, noted as a test bug not an assertion change |
| [TEST-PANE-15] | `node --test tests/launch.test.mjs`: exit 1, 1 test, 1 failed — `ERR_MODULE_NOT_FOUND src/launch.js` | — | — |

## Deviations

| Task | Spec Said | Actually Did | Why |
|---|---|---|---|
| review fix (9) idle tracking | RED before GREEN, always | GREEN without a captured RED | the RED build's link step failed with `No space left on device`; hyperion freed ~1.2 GB (cargo artifacts, Homebrew cache) and continued; the test exists and passes — recorded here rather than hidden |
| B8 hidden consumer | a hidden pane keeps acking | left unproven in Phase 1 | the consumer is the page's emulator (Phase 4); asteria's finding 8: crediting a fixture that asserts a local flag would prove nothing |
