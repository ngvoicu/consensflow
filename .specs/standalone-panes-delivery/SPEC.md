---
id: standalone-panes-delivery
title: ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead
status: active
created: 2026-09-06
updated: 2026-09-10
priority: high
tags: [app, tauri, pty, panes, delivery, standalone, skill, evals]
---

# ConsensFlow owns the panes — standalone mode in the app, results delivered to the lead

**Latest confirmed amendment:** detect every known harness, show its installed
version, newer official release availability and evidence-based integration
status. Version checks are informational, never admission/read/delivery gates.
Automatically prepare the bundled Pi extension only when Pi is installed;
missing/failed extension is red with an explanation and retry. Preserve active
sessions. Skills ship with each ConsensFlow release; remove separate Update
skills/install skills actions. See plan sections 8–9 for acceptance criteria.
This supersedes prior no-extension/no-version-inspection statements below.
The latest instruction authorizes reinstall and native checks after the user closed
the app; preserve histories and profiles.

**Role-skill planning decision, Gabriel, 2026-09-09:** the app lead receives
`consensflow-lead`; its PM receives `consensflow-pm`. Neither is installed
globally or given to workers. **Gabriel's subsequent clarification:** we delete
the old generic global `consensflow` skill manually during the controlled
transition. The app/installer/updater must not delete, move or rewrite it, or
implement a global-skill cleanup/migration feature. They manage only the new
private role skills and must not recreate the global one. This supersedes older
global-skill installation/retirement requirements below; their test records
remain historical. The five owned global files were removed manually after hash verification. See the
[skill and stack review](review-role-skills-and-stack.md) and the two linked
drafts. Keeping Tauri/xterm.js while repairing the identified state and delivery
defects is the technical recommendation; a full rewrite or framework migration
has not been approved. These documents do not change installed skills or add
completed implementation tasks.

**Pi integration decision (revised):** the app-managed Pi extension is now
explicitly approved, with automatic preparation conditional on Pi detection.
Use app-private files and process-scoped loading; preserve native settings,
existing extensions and active sessions. See plan section 8. Earlier prohibition
and extension-free feasibility notes are historical and superseded.

## Overview

Today a consult in `cmux` mode opens the agent's window in a pane of a
third-party terminal, and the lead reads answers only by asking
(`cf catchup`). This spec makes ConsensFlow's own desktop app the terminal:
it opens every pane itself (a PTY in Rust, drawn by xterm.js in the webview),
lays them out in the user's fixed progression, lists tabs and conversations
in a sidebar, and **delivers every completed worker answer, whole, into the
lead's pane** when policy allows — `auto` by default, `manual` where the
human says so, at conversation (tab) scope and at pane scope, and the lead
can never override the human. The 2026-09-07 refinement retires `cf catchup`: `cf results` discovers
completed worker results and `cf read` reads them whole through the same
framed receipts used by automatic delivery.

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
selector from the round-1 requirement goes with the modes. Gabriel's
2026-09-07 refinement puts the roster editor in a full-window Agents dialog
instead of the initially proposed collapsible strip. The
target stays macOS, Windows and Linux; this spec tests on macOS and the
other two follow in their own spec.

175 tasks across 19 phases; release stabilization covers reliable dispatch/delivery, terminal appearance, pane closure, skill semantics and signed in-app updates.

## Team

Historical release team (Gabriel, 2026-09-07): this lead owns architecture,
product decisions, integration, the spec and clean installation; **zeus**
is co-leader and release reviewer, **diana** implements bounded work,
**gefjon** handles repeat work and checks. **calliope** is unavailable.
The earlier assignments below are historical handoffs. The lead reads and
reconciles completed answers and continues autonomously through the release
gates; no new approval is needed for the already-authorized implementation
or clean installation. Gabriel subsequently directed that no backups be retained.

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

- [x] A fresh Pi lead discovers the generated ConsensFlow skill through Pi's
      actual resource loader and its default `~/.pi/agent/skills` directory,
      honoring `PI_CODING_AGENT_DIR`. The old owned `~/.pi/harness` skill is
      retired only after the new destination is installed successfully;
      unowned files and the current override destination are preserved.
      A live fresh lead asked to greet Gefjon consults the roster agent
      through `cf run`, creating an app-owned worker pane.
- [x] The main workspace consistently calls its top-level unit a session:
      **New session**, **Start session**, and the empty-workspace guidance
      agree with **Sessions / panes**. Worker conversation names keep their
      existing meaning and internal route/element identifiers stay stable.
- [x] In standalone mode, `cf run @nyx "task"` from a lead's pane follows
      today's continuation rule: `--new` creates a conversation and prints
      the name the app minted; `--session` names one; otherwise the lead's
      most recent conversation with that agent continues — a live pane
      receives the task as a follow-up, a closed one is resumed. No window
      opens in the lead's pane
- [x] `cf run`, `cf attach`, `cf say`, `cf read` in standalone mode outside
      an app pane refuse and name the app; an unavailable app never
      authorizes a second launch; a timeout after a possible launch reports
      an unknown outcome and never launches again
- [x] A lead pane's token is scoped to its tab and to the lead's named
      operations; the `--in-pane` controller holds a single-use ticket whose
      redemption returns ownership and a capability scoped to that launch
      and generation; the harness child and a shell pane hold nothing; no
      HTTP body may carry `by`, an owner or a foreign target
- [x] One app instance owns a state root; every `threads.json`, `tabs.json`
      and delivery write goes through one app-wide serialised queue;
      harness stores stay read-only
- [x] Ownership and read marks in standalone mode use the app-owned lead
      identity (`tab:<id>:<generation>`); a native session is bound to a
      lead or a worker only with launch-unique evidence; an ambiguous
      binding is visibly `unbound` and never drives automatic delivery; a
      native session replaced inside a running TUI (`/new`, `/resume`, a
      fork) invalidates the binding, the readiness evidence, the receipt
      cursor and every pending decision — automatic delivery suspends, the
      page offers reopening through the app's own new/resume path with a
      new generation, and the previous transcript never authorises a write
- [x] The page opens maximized on first launch and restores its geometry
      after; the roster editor remains the authenticated `cf ui` page in an
      iframe, opened only by the **Agents** button as a
      full-window dialog with Close and Escape; no roster strip takes space
      above the workspace. The sidebar remains collapsible left. Opening or
      closing Agents keeps every pane, output acknowledgement and watcher
      running, preserves the iframe, and returns focus to the opener
- [x] Panes tile in the user's progression counted WITH the lead: 1 alone;
      2 beside; 3 lead full-height and two stacked; 4 a 2×2; 5 lead
      full-height and a 2×2; 6 a 3×2; beyond, at most three columns and
      two visible rows (six panes), with further rows reached by vertical
      scroll. Narrow windows reflow to two or one columns. Explicit pane
      selection focuses one pane; pane count never forces focused mode.
- [x] Every pane has a title: conversation name · agent · **Replies: Automatic**
      or **Replies: Manual**, with a readable tooltip naming the setting
      source (session, worker, lead preference or default); a shell pane is
      titled `shell`
- [x] The sidebar is a tree: **session** (the tab, shown by its name) →
      one level down **lead** (its name) → one level further down each
      worker (`w1 <name>`, `w2 <name>`, …) and each shell; clicking the
      session shows the grid of all its panes, clicking the lead shows the
      lead's pane alone, clicking a worker shows that worker's pane alone;
      every session ever opened stays listed, live or closed, and a closed
      one is **resumable from the sidebar**: Resume reopens the lead in a new
      generation, and clicking a closed worker conversation reopens it
      through the attach path — a session manager, not a list of what is
      currently running
- [x] A human opens panes in a tab beside the ones consults open: a shell
      pane, or an agent pane that becomes a conversation whose lead is the
      tab's lead
- [x] Every completed worker reply — to the lead's question or the human's
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
- [x] A delivery is submitted only when the lead is ready: its own
      transcript shows its last turn settled with no tool in flight, and no
      human draft is latched in that pane. The human sends or erases input,
      then confirms **Resume replies** in the app. Rust requires the exact
      pane generation, input epoch and page sequence captured when opening
      confirmation, with no pending writes. Native text/Enter observations
      never authorize a clear. Human input
      arriving between the readiness decision and the write invalidates the
      decision; PTY silence is a polling hint only. Otherwise the delivery
      waits, visibly, with its reason; **Deliver now** bypasses policy —
      never readiness, never a latched draft, and the blocked button says
      why
- [x] Every delivery is a record: id, source answer, target session and
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
- [x] Unread bookkeeping in standalone mode is by stable transcript item
      ids and delivery coverage: pull marks what it printed, an inline
      receipt covers its answer, `cf read` covers the file one, gaps stay
      unread, covered answers are not printed again
- [x] Policy precedence holds: tab human `manual` veto > pane human setting
      > tab human `auto` > lead preference > default `auto`; set from the
      page only; `cf run --notify` records a preference; disabling cancels
      queued automatic deliveries
- [x] Right-click **Send reply to lead…** lists the conversation's
      completed answers from the transcript; a chosen one is delivered
      through the same path; already-delivered ones need an explicit resend
- [x] A worker's completion is reconciled when its pane exits and when the
      app restarts, so an answer finished just before exit still gets a
      record; closing a lead suspends its tab and stops that lead's process
      tree; sibling workers may finish into held answers;
      app exit reaps every owned tree; restart restores tabs closed; resume
      mints a new generation; held deliveries are shown with an explicit
      **Send held answers to this lead** action, never handed to a reused
      pane
- [x] The offline integration suite drives the real `cf`, the real Node
      server, the real Rust bridge and PTYs and fake harnesses end to end;
      the packaged smoke launches the REAL app bundle on macOS in a
      self-test mode that opens a pane, renders output, takes input, acks
      and shuts down
- [x] There are no modes: `cf use` and `cf mode` are gone (a leftover
      `mode.json` is ignored and reported once by `cf doctor`), the
      generated skill is installed into every detected harness without a
      native ConsensFlow, it names no cmux command, and the evals hold both
      directions of "continue or start fresh" and the delivery rules
- [x] The standalone skill teaches the lead to **send and return, never
      wait**: after a consult or a follow-up it reports what is running and
      takes the user's next message; an answer arrives in its pane on its
      own when the conversation is `auto`, and when it is `manual` the human
      says when to read (`cf results`, then `cf read`); waiting and polling
      are not taught, and the eval holds it
- [x] `npm run check:all` exits 0 on macOS with no live agent CLI and no
      network: biome, Node tests, `cargo test` + clippy, page tests, the
      integration suite, the packaged smoke

## Clean install on macOS — what Gabriel gets told when it is time

Gabriel's instruction (2026-09-06): the ConsensFlow of today is removed
completely from the Mac and the new one is installed clean, with its skill
and its CLI. The lead announces the moment; the steps are these, and the
old app's own verbs do the removal so nothing is guessed:

1. Back up the installed app, launcher, owned skills and complete
   `~/.consensflow` (including `agents.json` and any `mode.json`), plus the
   app's OS data directories. Restore only the roster after reset. Finish
   and reconcile active ConsensFlow workers in other projects before taking
   back their shared launcher or coordination state. Keep the old bundled
   Node and CLI paths explicitly: `cf off` removes the global launcher.
2. `cf off` — takes back every installed file the old ConsensFlow owns: the
   five skills, the `cf` launcher, the take-back-only leftovers.
3. `cf reset --yes` — removes the config root (`~/.consensflow`, workspaces
   and conversations included) and the app's own data directories
   (`dev.ngvoicu.consensflow`).
4. Quit the app; archive `/Applications/ConsensFlow.app` and obsolete probe
   bundles outside the active installation — the app never deletes its own
   bundle. Do not remove a probe bundle while a worker still uses it.
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
 cf say <name> "<words>"   cf attach <name>   cf results [<name>]   cf read <name|deliveryId>
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

**Drafts and recovery (2026-09-07 refinement).** Typing latches incoming
messages so an automatic paste cannot overwrite the human's composer.
Native user text, timestamps and Enter epochs cannot prove causality in an
opaque terminal: Enter may act on a menu while an older identical prompt is
flushed to the transcript. Node therefore never clears drafts by matching
text. After sending or erasing input, the human uses **Resume replies** and
confirms that the terminal input is empty. App-only Tauri commands capture
and check the exact pane generation, input epoch and page input sequence;
any intervening input or pending write refuses recovery. No text is erased,
no reply policy changes, and there is no lead HTTP/CLI recovery operation.
This applies equally to worker follow-ups. `cf read` needs no input recovery
because it returns complete result parts through the lead's tool channel.
The old `draft.clear` Node bridge endpoint is removed. Knowing an Enter
epoch, including the current one, never authorizes a Node-side clear.

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
delivering `sendUserMessage(…)` from an inbox when the agent is idle, or on
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

**Layouts, counted with the lead** (totals 1–6 special where width permits;
beyond: up to three columns, row-major, at most two visible rows and
vertical scroll for the rest):

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
only when an answer arrives or the human asks. Manual discovery uses
`cf results` and whole reading uses `cf read`; repeated polling is
unnecessary. `cf catchup`, including its wait mode, is retired.

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
- [x] P3 — an `http://localhost:<port>` iframe loads inside a `tauri://`
      page under the existing ATS exception — Phase 4
- [x] P5 — opencode's TUI server admits a message into the running
      session, with its authentication and endpoint discovery, and the
      message reaches the transcript as a user turn — enables the
      `opencode-server` channel only
- [x] P6 — a ConsensFlow pi extension, loaded how, delivers
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

## Phase 2: Tabs, identity, launch authority and the store [completed] — gates P1, P2 passed on all five harnesses

- [x] [TEST-PANE-11] `tests/store.test.mjs` — `Store(home)`: ONE app-wide
      queue — two mutations on one row, two on different rows, two tabs
      created in different directories, 50 concurrent mixed mutations: none
      lost; an **instance lock** at `<root>/app/instance.lock` (pid, start
      time) refuses a second `Store` on the same root and reclaims a lock
      whose pid is dead; a reservation written across two files is
      recovered on restart by the documented rule (a reservation with no
      pane is released); named ops `conversation.create`, `session.bind`,
      `sent.record`, `policy.set`, `seen.set`, `delivery.upsert`,
      `tab.*`; harness stores never opened for writing.
- [x] [IMPL-PANE-12] `src/store.js` — the queue, the lock, the ops over
      `writeJsonAtomic`. -> satisfies [TEST-PANE-11]
- [x] [TEST-PANE-13] `tests/tabs.test.mjs` — `tab.create(dir, harness)`
      returns `{id, generation: 1, leadId: 'tab:<id>:1'}`; two tabs may
      share a directory without sharing a lead; `addPane`, `removePane`,
      order; `suspend`, `resume` (generation +1, new `leadId`); a restart
      reads every tab `closed`; a pane id with a new generation is not the
      old pane; `leadId({CONSENSFLOW_LEAD_ID:'tab:1:2'})` is `'tab:1:2'`
      and wins over `CLAUDE_CODE_SESSION_ID`.
- [x] [IMPL-PANE-14] `src/tabs.js`; `hosts/lib/threads.js` `LEAD_KEYS`
      gains `CONSENSFLOW_LEAD_ID` first. -> satisfies [TEST-PANE-13]
- [x] [TEST-PANE-15] `tests/launch.test.mjs` — roles and tickets: the lead
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
- [x] [IMPL-PANE-16] `src/launch.js`, `src/ui.js` scoping middleware,
      `hosts/lib/runners.js` `childEnv` strip list, `lib.rs` absolute
      launch paths. -> satisfies [TEST-PANE-15]
- [x] [TEST-PANE-17] `tests/ui-panes.test.mjs` — against the REAL server
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
- [x] [IMPL-PANE-18] `src/ui.js` + `src/panes.js`. -> satisfies [TEST-PANE-17]
- [x] [TEST-PANE-19] `tests/cf-standalone.test.mjs` — the `cf` side: from a
      lead pane, `cf run @zeus "q"` (no flags) POSTs `consult` without
      `fresh`; `--new` prints `conversation: <name> (new) — pane <id>`;
      `--json`; `--in-pane` without `CONSENSFLOW_LAUNCH` refuses; with a
      ticket, ownership comes from redemption, never from `leadId(env)`,
      and later store updates (session discovery, kimi progress) go through
      the controller capability; after Phase 6, without `CONSENSFLOW_APP`
      the app is required and direct cmux launching is retired; `cf attach`, `cf say`,
      `cf read`, `cf chat` refuse when the app is unreachable;
      `CONSENSFLOW_CHILD=1` refuses all; a stale global `cf` on `PATH` is
      shadowed by the bundle's.
- [x] [IMPL-PANE-20] `bin/cf.mjs` — requester, controller, `sayVerb`,
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

## Phase 3: Completion, readiness and delivery [completed] — versioned lifecycle and receipt fixtures gate automatic delivery

- [x] [TEST-PANE-23] `tests/engine/completion.test.mjs` —
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
- [x] [IMPL-PANE-24] `hosts/lib/completion.js`, one adapter per harness;
      `findings-01.md` gets the table per harness before that harness is
      enabled. -> satisfies [TEST-PANE-23]
- [x] [TEST-PANE-25] `tests/engine/readiness.test.mjs` —
      `leadReady({items, inFlight, draftLatched, epoch})` is `ready` only
      when the lead's items end settled with no tool in flight and the
      latch is clear, returning the epoch the decision was made at; `busy`
      on an in-flight turn; `draft` while latched (with the reason for the
      page); `unknown` on an unreadable store; a periodically redrawing idle
      pane is `ready` — silence is not a condition.
- [x] [IMPL-PANE-26] `hosts/lib/readiness.js`. -> satisfies [TEST-PANE-25]
- [x] [TEST-PANE-27] `tests/engine/policy.test.mjs` —
      `effectivePolicy(tab, pane, row)`: tab `manual` vetoes; else pane
      human setting; else tab `auto`; else lead preference; else `auto`;
      `inherit` falls through; the result names its source; `--notify`
      writes only `notifyPreference`.
- [x] [IMPL-PANE-28] `hosts/lib/policy.js`. -> satisfies [TEST-PANE-27]
- [x] [TEST-PANE-29] `tests/engine/deliveries.test.mjs` — records: `plan`
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
- [x] [IMPL-PANE-30] `hosts/lib/deliveries.js`; `src/store.js`
      `delivery.upsert`; standalone `catchup` bookkeeping by item ids.
      -> satisfies [TEST-PANE-29]
- [x] [TEST-PANE-31] `tests/delivery-watch.test.mjs` — `Watcher` runs for
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
- [x] [IMPL-PANE-32] `src/delivery-watch.js`, started by the app process
      at launch (not by mode). -> satisfies [TEST-PANE-31]
- [x] [TEST-PANE-33] `tests/channels.test.mjs` — `deliver(channel, target,
      record)`: `pty-inline` and `cf-read` call `pane.write_paste` with the
      epoch; `opencode-server` POSTs to the running TUI's server found and
      authenticated as P5 recorded, and reports admission from the
      response; `pi-extension` writes the record to the extension's inbox
      and reports admission from the extension's ack file; a channel not
      enabled for the lead's harness is never chosen; an enabled channel
      carries its launch configuration (extension path, endpoint discovery).
- [x] [IMPL-PANE-34] `src/channels/pty.js` always; `src/channels/opencode.js`
      only once P5 is recorded as passed; `src/channels/pi.js` and the
      `consensflow-delivery` pi extension under `hosts/pi-extension/` only
      once P6 is recorded as passed — and P6 must include an inbox arrival
      while the agent is ALREADY idle (delivered at once, not stranded until
      the next `agent_settled`). -> satisfies [TEST-PANE-33]
- [x] [TEST-PANE-35] `tests/ui-panes.test.mjs` — page ops: `answers.list`
      (ids, previews, `delivered`, `uncertain` marked distinctly; latest
      file attempt exposes total and unconfirmed part numbers, displayed
      in the answer menu);
      `deliver.now {answerId}` creates a `manual` record (policy bypassed,
      readiness honoured); `resend` explicit; `deliver.cancel`;
      `held.send {tab}` for held records; `tab.resume`.
- [x] [IMPL-PANE-36] `src/panes.js` handlers. -> satisfies [TEST-PANE-35]

## Phase 4: The page — selector, sidebar, panes, layouts, menus [completed] — P3 passed; user refinement 53–54 verified

- [x] [TEST-PANE-37] `tests/layout.test.mjs` — `gridTemplate(n)` for 1–6
      matches the six pictures; beyond 6 `rows = ceil(sqrt(n))`, `cols =
      ceil(n / rows)`, row-major; `fits(n, area, minPane)` false → the page
      shows the focused-pane view (asserted in TEST-39); the lead cell is
      always `lead`.
- [x] [IMPL-PANE-38] `src/layout.js`. -> satisfies [TEST-PANE-37]
- [x] [TEST-PANE-39] `app/tests/page.spec.mjs` (Playwright, `__TAURI__`
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
      answers to this lead**; **New session**, **New pane** (Shell /
      Agent); when panes cannot fit, the focused-pane view with next/prev;
      a HIDDEN tab's emulators keep consuming and acking (a canned flood on
      a hidden pane drains); no request leaves the origin except the roster
      iframe's own.
- [x] [IMPL-PANE-40] `app/ui/` (`index.html`, `panes.js`, `term.js` with
      the `Emulator` interface and per-pane xterm instances that live while
      the pane does, `menus.js`), `app/scripts/bundle-ui.mjs`; `lib.rs`
      commands `open_lead`, `open_shell`, `open_consult`, `close_pane`,
      `pane_input`, `pane_resize`, `pane_ack`, `set_policy`, `answers_list`,
      `deliver_now`, `deliver_cancel`, `held_send`, `tab_resume`,
      `list_state`, window geometry persistence; `tauri-plugin-dialog`; the
      roster iframe. P3 recorded. -> satisfies [TEST-PANE-39]

### User refinement, 2026-09-07 — clear delivery controls and more pane space

- [x] [TEST-PANE-53] `app/tests/page.spec.mjs`: Agents is closed initially,
      opens a dialog covering the app window, retains its authenticated
      iframe across Close/Escape, returns focus, and keeps pane output/ACKs
      running while open. The header says **Reply delivery: Automatic** or
      **Reply delivery: Manual**; an accessible information button explains
      complete replies, lead readiness, human typing, manual delivery and
      the session manual veto. Session and single-pane views share the same
      pane top edge for one and multiple panes; navigation belongs inside
      the focused pane title bar and creates no blank strip above it.
- [x] [IMPL-PANE-54] `app/ui/index.html`, `panes.js`, `menus.js`: existing
      palette/type, one Agents button, native full-window dialog, explicit
      delivery labels and help, consistent grid/focused alignment. No new
      dependency or terminal recreation. -> satisfies [TEST-PANE-53]

## Phase 5: Lifecycle, the end-to-end suite and the packaged smoke [completed]

- [x] [TEST-PANE-41] `app/src-tauri` + `tests/lifecycle.test.mjs` — closing
      a lead stops its process tree and suspends the tab; app exit reaps
      every tree; restart reads tabs closed; `tab.resume` → generation +1;
      held deliveries stay held and visible; opening or closing Agents or
      switching tabs keeps every pane and the watcher running; an
      authenticated lead's `cf` keeps its routing while its tab exists.
- [x] [IMPL-PANE-42] `pty.rs` process groups, `src/tabs.js` lifecycle,
      `lib.rs` exit hook, routing by tab existence. -> satisfies [TEST-PANE-41]
- [x] [TEST-PANE-43] `tests/integration/*.test.mjs` — real `cf`, real
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
- [x] [IMPL-PANE-44] `tests/integration/harness.mjs`; `package.json`
      scripts `test:integration`, `check:all` (biome, `node --test tests/`,
      cargo test + clippy, page tests, integration, smoke). -> satisfies [TEST-PANE-43]
- [x] [TEST-PANE-45] `tests/smoke.test.mjs` — the built `.app` launched
      with `CONSENSFLOW_SELFTEST=1` and a fake-harness `PATH`: the real
      Tauri window opens a tab, a pane renders the fake harness's output
      (the page reports rendered rows over the self-test channel), input
      typed through `pane_input` reaches the child (hex echo), acks flow,
      the app exits 0 and leaves no process; the assets loaded came from
      the bundle.
- [x] [IMPL-PANE-46] `lib.rs` self-test mode and `app/ui/selftest.js`;
      `npm run smoke`. -> satisfies [TEST-PANE-45]

## Phase 6: Switch-over — one shape, no modes [completed]

- [x] [TEST-PANE-47] `tests/mode.test.mjs` → `tests/install.test.mjs` —
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
- [x] [IMPL-PANE-48] `src/mode.js` → `src/install.js` (`installEverywhere`,
      `turnOff`), `bin/cf.mjs`, `hosts/lib/threads.js`, `src/skill.js`
      (one prose), `src/ui.js` (no mode switcher; the page's system panel
      loses the three cards). -> satisfies [TEST-PANE-47]
- [x] [TEST-PANE-49] `tests/skill.test.mjs` — the standalone skill: no
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
- [x] [IMPL-PANE-50] `src/skill.js`; `evals/harness.mjs` (stub `cf` only;
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

## Phase 7: Post-install discovery and terminology [completed]

- [x] [TEST-PANE-55] `tests/install.test.mjs`: Pi's real default directory,
      absolute/tilde/empty config overrides, native-integration detection in
      the same directory, owned legacy migration, destination conflict and
      preservation of unowned files. Independently exercise the installed
      Pi loader and confirm the skill enters its model prompt.
- [x] [IMPL-PANE-56] `src/harnesses.js`, `src/install.js`: resolve Pi's
      actual agent directory, honor its override, and retire the obsolete
      owned skill after successful installation. -> satisfies [TEST-PANE-55]
- [x] [TEST-PANE-57] `app/tests/page.spec.mjs`: New session opens the
      directory/harness flow, the dialog and submit use session terminology,
      and the empty workspace says session. -> user-reported label mismatch
- [x] [IMPL-PANE-58] `app/ui/index.html`, `app/ui/panes.js`: align visible
      workspace labels while preserving existing identifiers and behavior.
      Build/update the installed app and verify fresh native Pi delegation.
      -> satisfies [TEST-PANE-57]
- [x] [TEST-PANE-59] `tests/integration/harness.mjs`, `task44.test.mjs`:
      real Rust PTY host inherits a Finder-only PATH while the Node editor
      knows the harness directory. A real `cf run --in-pane` must start the
      test harness and produce its native transcript. -> live worker exits
- [x] [IMPL-PANE-60] `src/panes.js`: carry the editor's discovered PATH
      into worker controllers, retaining their existing ticket-only authority.
      Rebuild and repeat fresh Pi/Gefjon delegation and Zeus review.
      Alpha.24 installed and workers running; Zeus reviewed the change.
      -> satisfies [TEST-PANE-59]

## Phase 8: Safe input recovery and native completion [completed]

Alpha.24 exposed latched input after a human submission. The initial
transcript-matching proposal and its passing test were insufficient: Zeus
found permanent poisoning after editor controls; Diana demonstrated delayed
identical native text plus a phantom Enter can falsely clear an unsent draft.
The lead accepts the smaller safe design below under Gabriel's architecture
and implementation authorization. This refinement supersedes earlier claims
that native text alone proves a submitted input epoch.

- [x] [TEST-PANE-61] Diana's real Node/Rust/PTY reproduction is retained and
      refined to require zero automatic clears after native user observation,
      with newer input still latched and no app-only recovery on the Node
      bridge. Rust unit and real Tauri IPC cases require exact epoch,
      generation and page sequence for a human confirmation.
- [x] [DESIGN-PANE-62] Zeus and Diana: review Enter/transcript races, editor
      controls, generation changes and delayed identical native records.
      Decision: no heuristic auto-clear and no raw keystroke capture. Only
      explicit human confirmation in the app can resume an opaque composer.
- [x] [IMPL-PANE-63] App-only **Resume replies** captures the live input
      state, explains send/erase/confirm, and refuses stale confirmations.
      Cancel and Escape do nothing; input policy and terminal contents remain
      unchanged. Manual `cf read` is independent of the input latch.
- [x] [VERIFY-PANE-64] Full checks, rebuilt installed app, actual Pi receives
      and reports Gefjon's completed response after human input confirmation;
      independent review and final release record. No retained backups.
- [x] [TEST-PANE-65] Root: Pi native settlement is read from the target
      lead/worker launch, without editor-global extension environment.
      Evidence for a different launch must never authorize delivery.
- [x] [IMPL-PANE-66] Root: pass launch-scoped Pi settlement paths to the
      completion adapter at every watcher read. Existing native extension
      fixtures must carry the production channel fields, not inject globals.
- [x] [TEST-PANE-67] Diana: both Pi transcript readers honor agent/session
      directory overrides, native tilde/empty semantics and precedence;
      overridden reads cannot pick up a decoy from the default directory.
- [x] [IMPL-PANE-68] Root: share Pi directory resolution between installation
      and both transcript readers. -> satisfies [TEST-PANE-67]
- [x] [TEST-PANE-69] Gefjon: frozen native Claude 2.1.263 records prove
      version admission, incomplete-prefix guards and actual final boundary.
      Preserve structural records and document source provenance.
- [x] [IMPL-PANE-70] Root: admit the verified Claude version and address any
      demonstrated lifecycle difference without weakening unknown-version
      or incomplete-turn guards. -> satisfies [TEST-PANE-69]


## Phase 9: Session names and complete result reading [completed]

Gabriel's 2026-09-07 clarification: every live pane stays interactive; a
lead's instruction not to edit files is a task constraint, not a pane mode.
Sessions can be named independently of their directory, and every open
session continues running while another is displayed. Selecting a session
must neither suspend processes nor stop draining its output.

A lead discovers completed worker results with `cf results` and requests
a whole result with `cf read <conversation>`. This replaces `cf catchup`
completely; no legacy catchup execution or read-mark behavior remains.
The daemon and manual reads share result extraction and receipt evidence.
An unfinished manual part-read stays `reading` until every part is observed;
resume it with the listed immutable delivery ID. It does not expire into a
false receipt or trigger an automatic duplicate.
Manual reads neither inject into the terminal nor require an idle lead or
an empty composer. An unfinished fragment or clipped tool output is never
accepted as a complete result.
Reuse the existing framed parts and native receipt evidence for large
results; result-only reads must not pretend that omitted discussion was read.
Historical complete Pi/Kimi answers stay readable while a later turn runs.
A manual part-read continues through a daemon restart on the same bound lead;
it never reserves the input channel. An automatic copy already being submitted
is not duplicated, while a pending draft-held answer can be claimed by the
manual reader. Follow-up parts always use the immutable delivery ID.

- [x] [TEST-PANE-71] Diana: persisted session renaming (trimmed nonempty
      label, bounded length, invalid names rejected, identity/directory/
      generation unchanged); browser rename/save/cancel and reload coverage.
- [x] [IMPL-PANE-72] Diana: session rename through the existing store and
      page architecture, available from session actions. Root wires the
      minimal Rust command bridge after her Node/UI implementation.
- [x] [TEST-PANE-73] Diana: two real Node/Rust/PTY sessions remain alive
      concurrently; page selection drains hidden-session output and never
      sends a close/suspend command. All live panes retain keyboard input.
- [x] [VERIFY-PANE-74] Root: verify concurrent sessions in the installed app,
      including renaming and switching while worker tasks continue.
- [x] [TEST-PANE-75] Root: result-only reading excludes partial turns;
      complete long output uses all framed parts; unread result bookkeeping
      requires full native receipt and does not consume omitted discussion.
- [x] [IMPL-PANE-76] Root and Gefjon: `cf results` discovery, dedicated
      complete `cf read` result retrieval and generated skill guidance;
      retire catchup execution and its obsolete workflow.
- [x] [TEST-PANE-77] Diana: 10 and 20 pane session grids stay within three
      columns and at most two visible rows (six panes on a wide screen),
      scroll to the last row, and reflow to two/one columns at
      smaller widths; many pending results never widen a pane or page.
- [x] [IMPL-PANE-78] Diana: maximum three columns, unlimited rows with
      vertical scrolling. Each row is at least half the available viewport
      height, so extra rows stay below the fold even in tall windows. This
      follows Gabriel's later correction: maximum SIX panes on screen.
      Keep explicit pane
      selection focused. Replace repeated delivery badges with one compact
      per-pane pending-result count and on-demand details/actions.

---

## Phase 10: Visible worker failures and session removal [completed]

User report 2026-09-08: empty lead branch, no session deletion, worker p-13
vanishes seconds after successful `cf run` admission. Treat launch admission and
native startup as separate states; retain actionable failure evidence. Existing
session-grid selection and six-pane limit remain unchanged.

- [x] [TEST-PANE-79] Diana: browser RED for a leaf lead without a child group or
  expanded state; worker children retain their hierarchy.
- [x] [IMPL-PANE-80] Diana: render groups only when populated; satisfies 79.
- [x] [TEST-PANE-81] Root/Diana: RED deletion tests: page modal confirmation and
  cancel, active/closed sessions, own processes stopped, another same-directory
  session survives, stale generation rejected, deleted tab ID never reused.
- [x] [IMPL-PANE-82] Root/Diana: page-only session deletion, explicit process
  shutdown and scoped app bookkeeping removal; preserve project files and native
  harness history. Monotonic tab identity. Satisfies 81.
- [x] [TEST-PANE-83] Root: reproduce exact failed worker startup; native harness
  failure remains inspectable in app with exit status and the current page’s terminal output, until an explicit retry replaces the failed pane.
- [x] [IMPL-PANE-84] Root: fix diagnosed startup cause and retain failure state;
  browser and real-process checks satisfy 83.
- [x] [VERIFY-PANE-85] Root: full gates, rebuilt alpha.26, actual installed
  Gefjon answer visible, deletion of disposable session and unaffected other
  session; replace old install/artifact without backups.

- [x] [TEST-PANE-86] Root/Diana: RED for runtime draft-latch visibility, lead
  versus worker recovery labels, and old-generation replies excluded from current
  badge; previous-session replies stay explicitly identified.
- [x] [IMPL-PANE-87] Root/Diana: expose draft latch and delivery generation;
  show recovery only when input blocks, label worker action Allow lead messages,
  keep one exact human-confirmed snapshot; satisfies 86.

## Phase 11: Full-height terminals and ordinary automatic reply flow [complete]

Gabriel's 2026-09-08 screenshot exposes two gaps in alpha.26 acceptance:
terminal content occupies only the upper part of a full-height pane; a completed
Gefjon result remains pending after an ordinary Pi prompt because human input
stays latched. The prior native gate explicitly clicked Resume replies and did
not verify the normal automatic question-to-answer flow.

- [x] [TEST-PANE-88] Diana: reproduce actual terminal-host/xterm height and PTY
  resize dimensions in tall grid/focused panes, with hidden/visible error banner.
- [x] [IMPL-PANE-89] Diana: terminal fills all space below visible pane chrome;
  preserve failed diagnostics, six-pane scrolling, input and output lifecycle.
- [x] [DESIGN-PANE-90] Root/Zeus: inspect the native Pi editor API and define a
  bounded automatic-delivery contract that preserves unsent input and exact
  generation/epoch authority. No transcript/Enter guessing or policy override.
- [x] [TEST-PANE-91] Root: RED for ordinary Pi send-to-worker-to-lead automation,
  authoritative empty-editor evidence and newer/unsent input race refusals.
- [x] [IMPL-PANE-92] Root: implement the reviewed native Pi path and honest
  recovery UI/docs; opaque terminal routes retain their existing protection.
- [x] [VERIFY-PANE-93] Root: full gates and actual installed alpha.28: terminal
  allocation fills its pane; Pi receives the complete worker result after a normal
  question without Resume clicks; a real unsent draft is preserved. Replace
  prior app/installer without backups after verification.

2026-09-08 user decision after seeing installed alpha.27: keep Pi's normal
inline layout, including the space below a short transcript; the same behavior
occurs in cmux. Do not enable Pi fullscreen mode or reposition its native
composer. The CSS fix still ensures xterm receives the entire pane height;
installed OpenCode was observed with its native composer at the pane bottom.

Diana's final bounded review is PASS, no material findings. Her independent
native suites were 150/150 and actual Xterm probe 1/1. Root's complete
`npm run check:all` exited 0: 1131 Node total, 1127 pass, 4 expected skips;
70 Rust unit + 12 real headless; 63 UI; 25 integration; 1 packaged smoke;
Clippy with warnings denied. Installed alpha.27 matches built/mounted DMG
(51 files/links), deep strict codesign passes. Alpha.26 installer removed;
no backup created. Normal installed Pi -> Gefjon -> Pi accepted d-35 once,
whole native user-envelope equality, manual=false, no Resume click. User
deleted the old t-6/t-7 sessions while this work ran; they were not restored.
The existing replacement t-8 session was resumed after reinstall. Draft
preservation/automatic continuation is the remaining native acceptance check.

Additional native gate blocker (d-35 succeeded, second GF failed before delivery):
`gefjon-velvet-harbor` p-29 created its native session metadata before the nonce
user turn. `openAndDiscover` selected the id too early and permanently returned
from `bindDiscovered` on missing evidence; the transcript later completed but
row.sessionId remained null. This is distinct from draft readiness. Alpha.28
will keep discovery pending for unavailable native evidence and select
OpenCode candidates by the existing launch nonce scanner, not newest id.

- [x] [TEST-PANE-94] Root: spawned CLI/server regression for session metadata
  preceding first user turn, a newer unrelated native session, and message
  parts preceding their completion events. RED: 3 failed, exit 1.
- [x] [IMPL-PANE-95] Root: nonce-bound discovery with transient evidence retry;
  explicit app binding refusals remain final, unknown evidence never binds.
- [x] [VERIFY-PANE-96] Diana: bounded independent review of this discovery fix.
  `diana-olive-valley`, native `01a07fc6-d6da-74a0-837e-1244e8aeaa75`,
  complete result read: APPROVE, no material findings; static review only.

Discovery GREEN: 8/8 spawned CLI/server probes, exit 0, including missing
marker refusal, native process exit outcome and explicit controller refusals.
The existing nonce scanner selects across OpenCode sessions; incomplete native
text/events keep discovery alive within its existing lifecycle deadline.

Alpha.28 release gates: build and codesign exit 0; installed app, built app
and mounted DMG match across 51 files/links. Initial check:all stopped at two
documentation tests because README.md disappeared during the run. Restored
the tracked documentation with current Pi delivery and session/grid behavior;
both documentation probes passed 2/2. Final `npm run check`: exit 0, Node
1134 total / 1130 passed / 4 expected skips. Remaining full-gate chain exited
0: Rust 70 unit + 12 real headless, Clippy warnings denied, 63 UI,
25 integration and 1 packaged smoke. No runtime change followed the build.
Native alpha.28 acceptance passed in resumed test session t-9, generation 2.
Gefjon `gefjon-dusty-glade` bound by nonce to native OpenCode
`ses_f8030eb28ffelZ7J1bp7QBVxS4`. Its entire three-line answer with
CF28-DRAFT-START/END stayed pending as d-37 while Pi visibly retained the
unsent CF28-UNSENT-KEEP-THIS draft. Two native snapshots 34,832 ms apart
confirmed the same pending id, zero native receipts, and no submittedAt or
expiresAt. After Ctrl-U erased only the probe draft, d-37 was accepted with
one attempt and exactly one complete canonical user-envelope receipt. Pi
then relayed all three lines and settled. No Resume click, manual read,
policy change or manual delivery. Native OpenCode editor reached the pane
bottom; Pi's accepted inline behavior remains unchanged.

Release: `/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.28/ConsensFlow_3.0.0-alpha.28_aarch64.dmg`,
SHA-256 `16ebc079e009cc215df9a841403e1ad69e644a8f27927b202ffb74b1634fbcec`.
The prior alpha.27 app and installer were replaced without backups. Only
previously active t-8/t-9 leads were resumed; user-deleted sessions stayed
deleted. Evidence: `/tmp/cf-alpha28-check-green.log`,
`/tmp/cf-alpha28-remaining.log`, `/tmp/cf-alpha28-discovery-red.log`,
`/tmp/cf-alpha28-discovery-green.log`, plus `release28-proof.json` and
`native28-draft*.json` in `/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-alpha27-ae2xud65`.

Phase 11 design (Root, reviewed by Zeus `zeus-lilac-reef`): the current Pi
SDK's `ctx.mode === 'tui'`, `hasUI`, `ui.getEditorText`, `isIdle`, and
`hasPendingMessages` are the native editor authority. A launch-scoped,
nonce/session/expiry-bound request avoids submitting while a draft exists.
The extension checks the editor again synchronously at `sendUserMessage`,
with no await between check and send. Pi's API preserves editor contents.
RPC and headless empty-string fallbacks are rejected. No raw editor text is
persisted. The new internal `pane.claim_native_epoch` keeps epoch, generation,
input failure and busy checks; it never clears the opaque latch or permits a
PTY paste. Readiness names this explicit Pi delegation rather than inventing
a clear latch. Only launches with the new `editorGuard: 1` capability use it.

Zeus approved the core design and requested honest readiness/UI wording
(integrated), plus pending rather than failed state for send-boundary refusals.
Root retains the established affirmative-zero-byte failure/replan contract:
a rare raced refusal gets a fresh delivery id, so stale ack files/deadlines
cannot validate a retry. Ordinary draft waiting stays one pending record with
no admission expiry. Failed records are excluded from the pending badge;
unknown or possibly admitted outcomes are never retried automatically.

Diana handed back the two UI files: real xterm height RED exit 1 (89px gap),
focused GREEN 1/1 and full UI 62/62 exit 0. Root's native-editor refusal RED
was 5 failures exit 1; native-claim RED was a missing-method compile failure
exit 101. Native flow/epoch/probe gates are green: `npm run check` exit 0, Node 1131 total / 1127 passed / 4 expected skips; Rust 70 unit + 12 headless exit 0; UI 63 passed exit 0. Final `check:all` and installed gates continue.

## Phase 12: Clear prompts and efficient result delivery [completed]

2026-09-08 screenshots: OpenCode shows the launch nonce and a redundant
Message from the user heading for a bare task. Pi ran two results listings
and a read to retrieve one known conversation. Live evidence: d-38 was
automatically planned while Pi processed a repeated user request; manual
cf read cancelled that pending record and accepted d-39. The daemon did
not lose the worker answer. Generated src/skill.js still carried the older
opaque-Pi instructions even though skill/SKILL.md had been updated.

- [x] [DESIGN-PANE-97] Root/Zeus: choose a supported way to keep launch
  correlation separate from visible task text; retain exact session binding.
  Zeus's whole msg_011CeqbogpTfVLxYbwF4pDYe confirms the heading bug and
  recommends probing native support. Root verified OpenCode v1.18.29 source:
  chat.message runs before native persistence, synthetic hides a text part,
  ignored excludes it from model input, and PartID accepts the prt prefix.
  Chosen: a launch-scoped plugin splits the marker into a hidden/ignored
  native part, preserving nonce discovery and completion events. Actual task,
  briefs and attachments remain intact. No native database edits, guessed IDs,
  global plugin installation or new delivery transport. Unmergeable inline
  JSONC keeps the original launch configuration and visible marker.
  Sources: https://github.com/anomalyco/opencode/blob/v1.18.29/packages/opencode/src/session/prompt.ts,
  https://github.com/anomalyco/opencode/blob/v1.18.29/packages/tui/src/routes/session/index.tsx,
  https://github.com/anomalyco/opencode/blob/v1.18.29/packages/opencode/src/session/message-v2.ts,
  https://github.com/anomalyco/opencode/blob/v1.18.29/packages/opencode/src/session/schema.ts.
- [x] [TEST-PANE-98] Root: RED for generated guidance using one direct read
  of a known conversation, automatic delivery by default, current native Pi
  authority and parity with the shipped sample skill.
  Focused generator tests: 3 failed, exit 1, before production changes.
- [x] [IMPL-PANE-99] Root: update the generator and sample; optional discovery
  only for unknown/multiple conversations, no status/read polling after run.
  Skill and installer suites passed 84/84, exit 0. Existing cf read already
  retrieves the whole first part in one invocation; no new reader API needed.
- [x] [TEST-PANE-100] Root: native prompt regression for the chosen design,
  including simultaneous identical tasks and whole result/receipt preservation.
  RED: 5 failed before production (heading, launch wiring, plugin missing).
  Focused plugin, interactive and binding suites pass 53/53, exit 0;
  standalone CF boundary suite passes 98/98, including the actual plugin
  through spawned CF and native-shaped SQLite parts/events. Installed native
  acceptance below proves two identical tasks, plugin loading and whole receipts.
- [x] [IMPL-PANE-101] Root: implement the minimal reviewed prompt correction.
  Zeus complete msg_011Cer272WR3RZ1wvn1Cq6Jp confirms worker env wiring,
  native visibility/model exclusion and configuration merge. Native plugin
  loading remains an explicit acceptance gate. His latent lead-seed comment
  is outside the current path: leads carry marker-only seeds, so the plugin
  is intentionally not configured there; any future lead task seed needs env
  wiring and a test. No --pure flag is used in app launches or this probe.
- [x] [VERIFY-PANE-102] Root/Zeus: bounded final review, relevant complete
  checks, rebuilt installed app and native ordinary automatic/manual read probes.
  Final npm run check:all exit 0: Node 1144 total / 1140 pass / 4 expected
  skips, Rust 70 unit + 12 real headless, Clippy warnings denied, UI 63/63,
  integration 25/25, packaged smoke 1/1. Build exit 0. Built, installed and
  mounted DMG match across 52 files/links; deep strict codesign passes.
  Five installed harness skills equal generateSkill(actual roster) exactly.

Installed alpha.29 native acceptance, 2026-09-08:

- Own isolated Pi lead t-12 launched two real Gefjon/OpenCode 1.18.29 workers:
  gefjon-amber-harbor (ses_f7edcf9fdffep9mtgpH75b3FJI) and
  gefjon-ember-harbor (ses_f7edcf2ffffedJEqKOila5CnGn). Both received the
  identical three-line task and bound to their distinct launch nonces.
  Each first native user turn has the original marker part with
  synthetic=true, ignored=true plus an ordinary visible task part. Neither
  visible task contains the launch marker or the redundant user heading.
  These stored native parts prove the packaged plugin loaded; no --pure,
  injected native DB rows or guessed binding were used. CUA AX confirms
  both app-owned workers. Post-install screenshot capture was unavailable
  (ScreenCaptureKit -3801), so no fresh pixel-level claim is made.
- Automatic d-42 and d-43: complete 60-byte answers, manual=false, accepted,
  exact native user envelope once each, current lead generation. Pi relayed
  every line with attribution. Zero cf results/read calls. The initial
  technical test did include nine other pre-dispatch exploratory tool calls;
  this is not evidence that every agent setup always costs only one call.
- Explicit manual re-read of the known ember result: exactly one
  cf read gefjon-ember-harbor --answer msg_0812310fc00155JDLSqrfUNTUF.
  d-44 accepted, manual=true, cf-read, one complete part, one whole native
  tool receipt, no results listing. Pi quoted the complete answer.
- Ordinary request "Ask gefjon-amber-harbor for a joke.": one tool call
  total, cf say, followed by automatic d-45; zero result/read calls, one
  whole native user receipt. Gefjon answered "Why do programmers prefer
  dark mode? Because light attracts bugs." Pi relayed that answer whole.
- Old alpha.28 installer and root-owned packaging copies removed, including
  temporary alpha.27/28/29 DMG roots. No backups, commits or session deletion.
  Existing user sessions remain available in the sidebar after restart.

Evidence: /var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-efficiency-nzh965nw/
(native29-auto-proof.json, native29-tool-proof.json, native29-manual-proof.json,
native29-joke-proof.json, installed-skill-proof.json, release29-proof.json,
release29-manifest.json, cleanup29.json, zeus-implementation-result.json).
Gate/build logs: /tmp/cf-efficiency-check-all.log and
/tmp/cf-efficiency-build.log. Installer:
/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.29/ConsensFlow_3.0.0-alpha.29_aarch64.dmg
SHA256 ba712b20de1edd7c98b7aaf1306c5f3032b86facc95c4923fbf156defd2feba3.

Acceptance: Automatic short answers need no follow-up tool call. If the user
explicitly asks to read a known conversation, one cf read call returns its
complete result (further parts remain required for large answers). User drafts
and policy remain authoritative. Correlation metadata must not be removed
without replacement evidence; native transcripts are never rewritten.

## Phase 13: Wait for delivery or an explicit read request [complete]

Gabriel's clarification: the lead does not need to know whether delivery is
Automatic or Manual. After delegation it returns control and waits for an
answer. Automatic delivery supplies it; under Manual the human asks the lead
to read. Do not add policy snapshots, mode queries or a new authorization UI.

- [x] [TEST-PANE-103] Root: RED for generated guidance that requires an
  explicit human result-read request, removes the unconditional pre-follow-up
  read, forbids polling/native-store bypass, and preserves whole delivered
  file-part reads. The UI explanation must say the lead reads only when asked.
  RED: 4 generated-skill failures and 1 focused UI failure, both exit 1.
  GREEN: skill/installer 86/86 and focused UI 1/1, both exit 0.
- [x] [IMPL-PANE-104] Root: update the skill generator, exact generated sample,
  README and existing Manual help. One requested read covers the requested
  results and their parts, not future polling; completing an already delivered
  result remains allowed. This is an agent instruction, not a claim that the
  CLI can prove natural-language user intent. No daemon or CLI API change.
- [x] [VERIFY-PANE-105] Root/Gefjon: read the bounded native review, run the
  relevant regression gates, rebuild/install the app and all generated skills,
  then verify an isolated Manual session waits without reading and an explicit
  request retrieves the whole answer in one call. Recheck ordinary Auto
  delivery without result calls. No backups or changes to user session policy.
  Gefjon whole msg_0814d7a2d001GbYXmTHBN2QsJZ confirms the implemented
  skill gate and allows shipping it alone. Its policy-snapshot discussion is
  superseded; no snapshot or mode-query guidance was added.
  npm run check:all exit 0: Node 1146 total / 1142 pass / 4 expected skips,
  Rust 70 unit + 12 headless, Clippy warnings denied, UI 63/63, integration
  25/25, packaged smoke 1/1. Initial gate caught one formatter issue in the
  new test; corrected before the passing full run.
  First native Manual probe exposed a behavior failure: t-14 waited after
  dispatch, but interpreted "Continue the existing task" as permission to
  cf read (d-47). Not accepted as release proof. Added explicit negative
  examples (continue/carry on/finish the task) and a return-control rule.
  New assertion RED: 1/36 fails, exit 1; fresh native acceptance required.

Installed alpha.30 final-skill acceptance, 2026-09-08:

- Fresh Pi t-15 / gefjon-velvet-lagoon, native OpenCode
  ses_f7ea65004ffeP3yLENJu0DoFLf. Ordinary delegation on Manual loaded the
  generated skill and ran one cf run; the complete worker answer remained
  undelivered. The exact previously failing request, "Continue the existing
  task", then produced zero new tools. The lead explicitly returned control
  and waited. Both native harnesses were settled; no result was fetched.
- Explicit "Read the result from gefjon-velvet-lagoon and report the whole
  reply" used one cf read, no results listing. Complete 57-byte d-48 accepted
  as manual=true, cf-read, with one exact whole native tool receipt and an
  attributed complete lead reply. A transient provider 429 recovered without
  duplicate result tools.
- The human page switched only this own session to Auto, without informing
  the lead. Ordinary "Ask gefjon-velvet-lagoon for another short joke" used
  one cf say. Daemon d-49 accepted as manual=false, pty-inline, with one exact
  complete native user envelope; the lead reported the whole joke. Zero new
  result-read or result-list tools, no mode queries and no Resume replies click.
- After the final instruction-only refinement, npm run check passed again:
  1146 total / 1142 pass / 4 expected skips. The earlier full gate covers the
  unchanged daemon, Rust, UI and integration paths. Final build passed; native
  acceptance used the final generated skill, SHA256
  03cc30b1123c2603a03bbab1146f3dae84763965f1ca902cb532ae3047c0c080.
  Final bundled generator and all five installed skills produce those same
  bytes. Built, installed and mounted DMG match across 52 files/links; deep
  strict codesign and installed version checks pass. A Finder .DS_Store was
  removed from the package before resealing; no production behavior changed.
- Alpha.29 installer and temporary packaging copies removed. Alpha.30 app
  reopened successfully; no backup or session deletion/restoration.

Evidence: /var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-manual-l3l63wn5/
(native30-acceptance.json, native30-v2-manual-before.json,
native30-v2-manual-continue.json, native30-v2-manual-requested.json,
native30-v2-auto.json, installed-skill-proof.json, release30-proof.json,
release30-manifest.json, cleanup30.json, gefjon-initial-review.json).
The initial failing native30-manual-continue.json is retained as test evidence.
Logs: /tmp/cf-manual-check-all.log, /tmp/cf-manual-continue-check.log,
/tmp/cf-manual-build-final.log and /tmp/cf-manual-package-final.log.
Installer:
/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.30/ConsensFlow_3.0.0-alpha.30_aarch64.dmg
SHA256 18a55aa7b8d9407a889893f1e9438e4215207bf6bc52f6c1a4939f08b7888eb2.

## Phase 14: Native terminal style and automatic input handling [complete]

User direction, 2026-09-08: remove Resume replies for everyone and clean its
obsolete code; preserve the original style of each native TUI. This supersedes
the earlier human-confirmation workflow, but never authorizes overwriting a
draft. Auto delivery remains daemon-owned; Manual still waits for an explicit
user read request. Large-result references remain as accepted in Phase 13.

- [x] [DESIGN-PANE-106] Root/Zeus: establish supported native ingress or exact
  editor authority for each harness, including workers. No inference from Enter
  or transcript silence; prove that native delivery preserves unsent drafts.
- [x] [TEST-PANE-107] Gefjon: reproduce missing terminal color capabilities in
  a real child PTY and app palette overrides in real Xterm rendering.
- [x] [IMPL-PANE-108] Gefjon: advertise correct terminal capabilities and let
  each TUI retain its own colors and formatting; preserve explicit user choices.
- [x] [TEST-PANE-109] Root: test automatic delivery without confirmation,
  draft preservation, concurrent typing, stale launches and duplicate receipts.
- [x] [IMPL-PANE-110] Root: implement the proven native delivery/readiness path.
- [x] [TEST-PANE-111] Root: replace obsolete recovery UI and skill expectations
  with the new behavior for every lead and worker.
- [x] [IMPL-PANE-112] Root: remove recovery controls and their unused commands,
  dialog state and documentation; retain independent delivery and input guards.
- [x] [VERIFY-PANE-113] Root/Zeus: run focused and full gates, review the final
  diff, verify native colors and draft-safe delivery in the actual app.
- [x] [RELEASE-PANE-114] Root: build, install and verify the new app and installer;
  remove superseded release artifacts, without backups or restoring sessions.
- [x] [TEST-PANE-115] Gefjon: reproduce a new app session incorrectly opening
  an earlier native conversation in the same directory.
- [x] [IMPL-PANE-116] Gefjon/Root: make New session create a fresh native
  conversation for every supported harness; preserve explicit session resume.
- [x] [VERIFY-PANE-117] Root: verify two new installed-app sessions in one
  directory have distinct native identities and no inherited conversation.

Implementation and review evidence: real child-PTY color regression failed before capability defaults;
Xterm now matches the default terminal's foreground/background and all 16 ANSI
color query responses, and retains exact 24-bit SGR colors. Codex native queue
and Claude MCP channel probes both preserved an unsent draft. OpenCode's HTTP
message through the real Rust PTY and Xterm displayed the answer while preserving
the draft and keeping Rust's latch set. Native queue epoch-race and real native
receipt regressions pass; full release evidence is recorded below.

Ownership: Gefjon completed terminal colors and Pi raw-message support. Diana
completed Codex adapter and owns obsolete recovery UI/Rust API cleanup. Zeus
owns Claude channel server/adapter/tests. Root owns launch/store/watcher wiring,
spec, documentation, integration and release. No shared-file stashes or commits.

Cleanup evidence: TEST-PANE-111 first failed on all 4 supported lead harnesses;
Diana's final UI suite passed 61/61, Rust 69 unit + 13 headless; generator tests
36/36. Production Claude channel/hook through Rust PTY and real Xterm preserved
the draft and displayed the native result. /clear rotated its native session ID;
an old-target follow-up was refused with zero bytes and native-session-changed.
Zeus's final review identified a Claude session-transition gap and an admission
expiry incorrectly reused for native receipts. Root added SessionEnd closure
markers scoped to each native UUID, explicit verified-version gating, and raw
channel-text refusal; their regressions are green. Diana owns receipt-window
separation and visible lead invalidation. Native receipt was accepted 3908 ms after submit with the unsent draft
preserved. The real Claude receiving hook rejected an old-target message even
after the probe deliberately restored stale SessionStart evidence and removed
the SessionEnd marker; no worker prompt or answer entered the new transcript.
UserPromptSubmit is the independent live-session guard; SessionEnd tombstones
are scoped to the old UUID, so delayed closure cannot erase a new session.
Diana's receipt and invalidation fixes passed 281 focused Node tests. The user's new same-folder session bug is
completed by Gefjon as TEST/IMPL-PANE-115/116. Pi reused a tab-derived native
session name across app-state resets; fresh names now include the launch UUID,
while explicit resume uses the recorded native identity. The regression failed
before the change and passed afterward; 127 pane-server tests and 28 runner tests
passed. The reported harness is still unconfirmed, so installed verification
covered both Pi and Codex in the same directory.

Diana's final review (automatic delivery d-61) found no material blockers in the
Claude receiving guard, hook wiring, version gate, or native proof. The page
projection regression failed before suspended records were excluded from current
pending results and passed afterward. Native unknown-session states now explain
that the Claude reply channel is unavailable. Rust verification passed 69 unit
and 13 headless tests, and Clippy with warnings denied.

The rebuilt alpha.31 smoke exposed a startup race: an unresolved lead appeared
as alive:false and the page retired it before its first PTY output. The original
smoke failed with two arrivals and no acknowledgements; the page now receives an
explicit starting:true state and retains that terminal. The state and real-Xterm
regressions failed before the fix; 129 pane-server tests, 63 UI tests, and the
rebuilt packaged smoke passed after it. Diana's final read-only review d-64 found
no material blocker. The initial candidate installer was removed and regenerated from the corrected bundle.
Installed acceptance found a separate New session navigation defect: the native
conversation was fresh but the page kept the old tab selected. The new-session
result now selects its returned tab ID and focuses that terminal after refresh.
The same-directory Pi/Codex regression failed with the old header still shown;
all 64 UI tests passed after the correction. The installed candidate was updated with this UI correction before native acceptance.
The native Codex worker also exposed inherited TERM=dumb from a noninteractive
launcher. Rust now replaces absent, empty, or dumb inherited TERM with
xterm-256color; explicit pane overrides and dropEnv remain authoritative, and
NO_COLOR/FORCE_COLOR are untouched. The new real-PTY regression failed before
the change (dumb:end), then all 69 Rust unit and 14 headless tests passed, as did
Clippy with warnings denied. Native folder trust was acknowledged only for this
run's isolated acceptance directory. No user project settings were changed. Evidence: /tmp/cf-starting-state-red31.log,
/tmp/cf-startup-red31.log, /tmp/cf-starting-state-green31.log,
/tmp/cf-startup-ui-green31.log, /tmp/cf-native-packaged-smoke31-final.log.

Installed alpha.31 acceptance confirmed four separate new sessions in one
directory (Pi t-20/t-21, Codex t-22/t-23), distinct native identities, no marker
from another conversation, and immediate selection/focus of each new terminal.
Gefjon's exact final token arrived automatically as d-68 with one native receipt,
one cf run and zero result-read commands. Diana's automatic d-67 review passed the
stale-refresh/unknown-outcome correction. Deleting all five acceptance sessions
succeeded but exposed a page read racing tab removal: Watcher.held threw unknown
tab after Page.state had captured the old tab list. The real Page/Watcher/Store
regression failed there, then all 63 watcher tests passed after deleted tabs
returned an empty held list. sendHeld still refuses an absent tab. The final
candidate includes this cleanup fix; prior acceptance remains valid
for unchanged fresh identity, selection, terminal and delivery behavior.


Final release evidence, 2026-09-08:

- Gefjon's complete automatic review d-69 passed the deletion-race fix and
  confirmed that deleted sessions cannot acquire send authority.
- Every verification component passed: Node 1201/1205 (4 expected skips),
  Rust 69 unit + 14 headless, UI 66/66, integration 25/25, packaged smoke 1/1,
  Biome 100 files and warning-denied Clippy. Total: 1376 passed, 4 skipped.
  The components were rerun individually after the earlier fixture corrections;
  this does not claim a later single check:all invocation.
- Alpha.31 was installed and restarted from the final verified bundle. Deep,
  strict codesign verification passed; the mounted DMG, staged app and installed
  app matched the built bundle's 55-file/link manifest.
- Final installed cleanup resumed only test t-24 explicitly, preserving its
  native identity at generation 2, then deleted it and confirmed its Pi process
  stopped. No internal_error appeared after deletion. Own t-19 through t-24 and
  the temporary native-workspace directory are gone; user t-16/t-17 remain.
- Alpha.30 and superseded candidate installers/staging copies were removed.
  No backup or commit was made. Native histories were not reset or restored.

Installer:
/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.31/ConsensFlow_3.0.0-alpha.31_aarch64.dmg
SHA256 964ed55bcb8dc88c172acb8d0f502dce8c6dc774e2f4258ea692792aefe07d2d.
Evidence directory:
/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-native-style-ryaplurd/
(installed-native-acceptance31.json, fresh-sessions31.json,
native31-diana-final.json, native31-cleanup-review.json, final-gates31.json,
release31-proof.json, release31-manifest.json, cleanup31.json).
Final logs: /tmp/cf-delete-refresh-{node,integration,smoke,lint,build,package}31.log,
/tmp/cf-new-selection-ui-final31.log, /tmp/cf-inherited-term-rust-green31.log,
and /tmp/cf-inherited-term-clippy31.log.

## Phase 15: Quiet fresh sessions and removal of the Claude development channel [completed]

User direction, 2026-09-08: do not use the Claude development channel.
A new Codex session must remain idle until the human sends the first message;
an internal launch identifier is not a task and must never be submitted as one.
The removal includes the channel implementation, launch arguments and hooks,
registrations, tests specific to the retired adapter, and current documentation.
Claude results remain readable on request. Do not replace the rejected channel
with a permission bypass or write over the native editor. Preserve fresh native
identities, explicit resume, worker task dispatch and existing user settings.

- [x] [DESIGN-PANE-118] Root/Gefjon: choose and verify native session binding
  that does not invoke the model; confirm the removal boundary for Claude.
- [x] [TEST-PANE-119] Root: reproduce unsolicited Codex startup work and
  the rejected Claude launch configuration at real process boundaries.
- [x] [IMPL-PANE-120] Root: start fresh leads without a synthetic task;
  preserve exact binding/resume and intentionally seeded worker launches.
- [x] [TEST-PANE-121] Root: prove Claude opens without development channels
  and retired configurations cannot send; explicit result reads still work.
- [x] [IMPL-PANE-122] Root: remove the Claude development adapter and its
  owned hooks/configuration/docs; accurately show unavailable automatic delivery.
- [x] [VERIFY-PANE-123] Root/Gefjon: review the final change, run relevant
  suites, install and verify new idle sessions and Claude startup in the app.

Alpha.32 acceptance, 2026-09-08:

- Installed app: fresh Codex 0.153.4 sessions t-28 and t-29 in the same
  repository remained idle until their first human message. Each native
  transcript contains its own test prompt and one final answer, with no
  synthetic launch prompt. Native IDs are respectively
  `01a0821b-879d-7de0-89fe-0580c8a20f22` and
  `01a0821c-ed12-7d93-a3f5-de12ee9a3d11`. Closing and explicitly resuming t-29
  retained its exact native ID and answer, with no extra model turn.
- Installed Claude 2.1.263 session t-30 opened at its normal empty editor,
  with native colors and 0 tokens. No development-channel confirmation;
  its launch reservation has no channel. Native ID:
  `941a56d8-4d8a-4747-87d5-6cef51f3363f`.
- Native Codex queue probe also delivered the requested answer while retaining
  an unsent draft. New Codex binding requires the verified 0.153.4 native
  metadata, original CLI user thread and exact launch-specific originator;
  absent, ambiguous, forked or unsupported metadata remains unbound. This
  internal metadata mechanism must be reverified when upgrading Codex.
- Claude's retired adapter, launch flags, registrations, owned implementation
  files and channel-specific framing rule are removed. Historical reservations
  cannot reactivate it. Existing guarded terminal delivery and explicit result
  reads remain; removal does not prove automatic delivery through opaque input.
- Gefjon d-70 implemented and tested the removal; Diana d-71 reviewed native
  identity options. Gefjon d-72's final review found one stale test fixture
  missing the newly required native metadata. The fixture was corrected without
  weakening expectations, and the full Node check passed afterward.
- Final gates: Node 1185 passed / 4 expected skips, UI 66/66, Rust 83/83,
  integration 25/25, packaged smoke 1/1; 1360 passed total. Lint, Clippy with
  warnings denied and diff whitespace checks passed. One initial integration
  teardown check transiently saw an exiting fake process; it was absent on
  inspection, and both isolated and full reruns passed without code changes.
- The 53-entry built, DMG and installed app manifests match; strict deep
  signature verification passed. All five installed skills match the current
  generator and user roster. DMG SHA-256:
  `86c8e43ef7f35a537bd618e9f271710ff9eef031fc1372d46a56cb4f74b33f01`.
- Removed own review/acceptance sessions t-27–t-30 and temporary installer
  staging; superseded alpha.31 installer removed. No backup or commit. User
  sessions t-16/t-26 remain closed for the user to manage; native histories
  and user settings were not deleted.

Evidence: `/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-startup32-1cp5ur_z/`
(`installed-idle32-proof.json`, `codex-native32-proof.json`,
`claude-quiet32-proof.json`, `gates32.json`, `release32-proof.json`,
`release32-manifest.json`, `skills32-proof.json`, `cleanup32-proof.json`).
Installer: `/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.32/ConsensFlow_3.0.0-alpha.32_aarch64.dmg`.

## Phase 16: Quiet fresh OpenCode sessions [pending]

User report, 2026-09-08: a fresh OpenCode lead receives an internal launch
marker as a task and starts thinking before the first human message. New leads
must open idle. User explicitly rejects the plugin approach and modifications
to Claude Code, Codex, Pi or OpenCode. The implementation stays in ConsensFlow,
using ordinary native CLI/API operations without editing harness binaries,
source or global settings. Existing native styles remain unchanged.

Create an empty OpenCode session through its authenticated loopback API, stop
the temporary server, and launch the normal TUI with the exact returned session
ID. No synthetic prompt, guessed newest session or plugin injection. Native
creation is bounded; failures prevent pane launch and release its reservation.
Same-folder sessions must have distinct identities; explicit resume preserves
its existing identity. Retire ConsensFlow's OpenCode launch plugin, using the
same native identity for workers so their actual task can travel without a
launch marker. Preserve automatic result delivery and other harness behavior.

- [x] [TEST-PANE-124] Root: reproduce the unsolicited OpenCode lead prompt at
  the real server/pane boundary; verify distinct launches and resume.
- [x] [IMPL-PANE-125] Root: create and bind native OpenCode identities before
  opening leads/workers, send only real worker tasks, remove plugin injection.
  -> satisfies [TEST-PANE-124]
- [x] [TEST-PANE-126] Gefjon: test native empty-session creation with real
  subprocess/API boundaries, bounded failures and child cleanup.
- [x] [IMPL-PANE-127] Gefjon: add the native session-creation helper in
  ConsensFlow. -> satisfies [TEST-PANE-126]
- [x] [TEST-PANE-129] Root: reproduce ignored OpenCode `--session --prompt` with the installed app and a real spawned CLI fixture; native task API readiness, model, cancellation and no-retry regressions.
- [x] [IMPL-PANE-130] Root: submit first/reopened OpenCode worker tasks through the native API after its TUI starts; preserve exact session, initial model and bounded cancellation. -> satisfies [TEST-PANE-129]
- [x] [VERIFY-PANE-128] Root: final review incorporating earlier independent findings, relevant full
  gates, native and installed idle/first-message/binding/resume acceptance;
  package and reinstall without backups, clean only own test artifacts.

## Phase 17: Reliable everyday operation [completed]

Gabriel's 2026-09-08 installed report reopens acceptance. Preserve the live
fortuna-advisor and poker-bot sessions, their panes and native histories. Work
in an isolated ConsensFlow state root; no app restart while user work runs.
No native harness binary/source/global-setting edits, Claude development
channel, backups or commits. This phase is authorized with the prior release
fixes and cleanup. A new stack is an option only if diagnosed boundaries need it.

Acceptance: a fresh/reopened worker receives its exact task once after slow
startup; dispatch output distinguishes a pane opening from task admission;
zero historical runs never means a live worker did no work. The lead uses a
complete auto-delivered answer immediately and continues independent work
while waiting, without polling or asking the user to read an already delivered
answer. Manual result retrieval remains user-requested. Native Claude/Codex
colors survive the app launch environment. Every pane offers a clear close
action preserving conversation history; closing one worker leaves its siblings
and lead running. Automatic complete results from Claude, Pi and OpenCode reach
real native leads, preserve drafts and do not replay uncertain submissions.
Pending indicators show unique current results and their actual blocker.

- [x] [TEST-PANE-131] Root/Gefjon: tests/opencode-launch.test.mjs and tests/cf-standalone.test.mjs: reproduce slow first/resumed seeding and misleading launch status.
- [x] [IMPL-PANE-132] src/channels/opencode.js and bin/cf.mjs: bounded readiness and truthful dispatch/session status. -> satisfies [TEST-PANE-131]
- [x] [TEST-PANE-133] Root: tests/skill.test.mjs and evals: automatic-arrival, independent-work, manual-read and zero-runs scenarios.
- [x] [IMPL-PANE-134] src/skill.js and skill/SKILL.md: simplify the lead workflow, preserve default conversation continuation and manual retrieval rule; regenerate owned installations. -> satisfies [TEST-PANE-133]
- [x] [TEST-PANE-135] Diana: app/tests/page.spec.mjs and relevant real bridge lifecycle tests: worker/shell close, lead/session semantics, stale-generation refusal, surviving sibling.
- [x] [IMPL-PANE-136] Diana: app/ui/panes.js, menus.js and minimal existing bridge paths: visible pane close action with history retained. -> satisfies [TEST-PANE-135]
- [x] [TEST-PANE-137] Root: real PTY/terminal tests reproduce lost native colors under inherited launcher environment; retain ANSI/dim/bold styles.
- [x] [IMPL-PANE-138] Root: minimal app terminal environment/rendering correction, no native theme/config edits. -> satisfies [TEST-PANE-137]
- [x] [TEST-PANE-139] Root/Zeus: native-process delivery matrix reproduces Claude lead and Pi completion failures, draft/interruption/no-replay boundaries.
- [x] [IMPL-PANE-140] Root: correct completion/readiness/delivery at the diagnosed boundary with native evidence and no retired Claude channel. -> satisfies [TEST-PANE-139]
- [x] [TEST-PANE-141] Root: page/watcher tests reproduce stale/duplicate/hidden blocked result indicators.
- [x] [IMPL-PANE-142] Root: current unique result counts and actionable truthful waiting reasons. -> satisfies [TEST-PANE-141]
- [x] [VERIFY-PANE-143] Final integration review, relevant full gates, real native cross-harness acceptance, packaged isolated acceptance, safe install and superseded release/staging cleanup; close VERIFY-PANE-128 only on its actual evidence.

## Phase 18: Signed updates with user-controlled restart [completed]

Gabriel authorized this addition on 2026-09-09. Implement in the existing
Tauri app; keep the protected installed sessions running. No remote release
publication or native-harness modification is part of local implementation.

Contract:

- A quiet check ten seconds after startup and every six hours discovers a
  newer release; background failures do not interrupt terminal work. A
  permanent native menu item and an Updates button provide explicit checks,
  including clear offline/error/up-to-date feedback. Self-tests skip scheduling.
- The update dialog shows installed version, selected channel, candidate
  version, plain-text release notes, download progress and Download / Later.
  Downloading is explicit and may proceed while agents work. Installation and
  restart are a separate explicit action, never triggered by a timer or download.
- Stable admits only stable semantic versions. Alpha admits alpha prereleases
  and stable graduation releases. Both require a strictly newer version;
  switching channels never downgrades. Channel choice persists in app-owned
  preferences; a first run derives its channel from its installed version.
- Use the official Rust Tauri updater and mandatory signatures. Metadata is
  public static JSON per channel on GitHub, with immutable versioned release
  archive URLs. Only the app's guarded update commands are exposed to the UI;
  direct plugin install commands are not granted. Release notes remain text.
  On macOS, after official signature verification, installation stages beside
  the current app and uses atomic RENAME_SWAP. The official 2.11.0 installer
  moves the old app away before its second rename; a failed rename can drop
  the temporary old copy. The atomic swap removes that failure window.
  The old bundle is deleted after a successful exchange; no backup is retained.
  A post-swap filesystem cleanup error records the staging path on Console,
  without claiming that the already-installed update was rolled back.
- The signed archive contains the app, bundled Node, cf, generated skill and
  adapters as one matching version. Prepare release metadata only after all
  artifacts and signatures exist; publishing assets precedes publishing the
  channel pointer. Signing private material stays outside the checkout and
  never enters logs or workers. No backups are created.
- The backend refuses installation with ANY open native pane, across all
  sessions. The user must close/suspend those sessions first: opaque unsent
  native-editor drafts cannot be saved or inferred safely. No automatic
  stopping of agents and no claiming that idle means a draft is absent.
  The install gate and every native pane launch share one atomic admission
  boundary; a racing launch either precedes and blocks installation, or is
  refused before spawning. On installation failure, admission reopens.
- Historical alpha.36 contract, superseded by the latest no-native-version-check
  decision above: candidate compatibility metadata lists only exact native CLI versions with
  recorded release evidence, separately from transcript schema versions.
  Detected unverified versions are explained; an available fix is advertised
  only when that newer release explicitly verifies the detected version.
  Unknown compatibility stays unknown. During this phase Claude 2.1.266 and
  OpenCode 1.18.30 arrived; captured native fixtures and a real Claude peer
  delivery preserving an unsent draft verify them. A real OpenCode worker
  also delivered automatically to that Claude lead without disturbing the
  draft. Future 2.1.267 remains
  unverified in the diagnostics regression.
- Tests exercise the real updater's signature/download boundary, rejected
  tampering, channel/version policy, failure/retry and real-PTY installation
  admission races. Test installation/restart only in an isolated throwaway
  app copy. The protected installed app and its sessions remain untouched.

- [x] [TEST-PANE-144] Root: update state, signed download, channel/version rejection and retry regression tests.
- [x] [IMPL-PANE-145] Root: guarded official Rust updater controller, persisted channel and native menu integration. -> satisfies [TEST-PANE-144]
- [x] [TEST-PANE-146] Root: real PTY tests for open-pane refusal, launch/install race and admission recovery.
- [x] [IMPL-PANE-147] Root: atomic PaneTable installation admission guard, explicit install/restart only. -> satisfies [TEST-PANE-146]
- [x] [TEST-PANE-148] Diana: UI tests for quiet/manual checks, notes, download/later, progress, channel changes and protected install.
- [x] [IMPL-PANE-149] Diana: concise update dialog, permanent entry and scheduled check lifecycle. -> satisfies [TEST-PANE-148]
- [x] [TEST-PANE-150] Gefjon/Diana: release-feed tests for bundle versions, signatures, HTTPS immutable assets and channel semantics.
- [x] [IMPL-PANE-151] Diana after bounded handoff from Gefjon: deterministic local metadata preparation and documented publish order. -> satisfies [TEST-PANE-150]
- [x] [TEST-PANE-152] Root: native-version compatibility diagnostics, unknown future versions and candidate fix evidence.
- [x] [IMPL-PANE-153] Root: report verified compatibility and available fixes without changing native harnesses. -> satisfies [TEST-PANE-152]
- [x] [VERIFY-PANE-154] Zeus review, full applicable gates, signed package and isolated real update/install/restart evidence; publication and protected-install boundaries documented in the alpha.36 release validation.

## Phase 19 — accepted role/session plan [completed]

Implementation is isolated in `consensflow-next-ib1cg0zs`. The user's latest
2026-09-09 instruction authorizes reinstall and native verification after closing
the installed app. Preserve histories and native profiles. No commit or publication.

- [x] [TEST-PANE-155] Native result and binding tests ignore absent/arbitrary harness versions; launch never probes versions.
- [x] [IMPL-PANE-156] Remove native version decisions from readers, binding and delivery selection. -> satisfies [TEST-PANE-155]
- [x] [TEST-PANE-157] Complete worker results remain readable without a readable lead transcript or receipt cursor.
- [x] [IMPL-PANE-158] Separate result access from receiver receipt observation. -> satisfies [TEST-PANE-157]
- [x] [TEST-PANE-159] Closed workers and whole results persist across restart/resume, without redispatch or cross-session leakage.
- [x] [IMPL-PANE-160] Restore durable worker navigation and same-session result access. -> satisfies [TEST-PANE-159]
- [x] [TEST-PANE-161] Pi extension integration preserves drafts and complete automatic results; installation occurs only when Pi is detected.
- [x] [IMPL-PANE-162] Implement conditional app-private Pi extension preparation and verify launch, editor, settlement and delivery without changing active processes. -> satisfies [TEST-PANE-161]
- [x] [TEST-PANE-163] Real terminal geometry: fixed double-width lead and two-row workers scrolling horizontally beyond pane five.
- [x] [IMPL-PANE-164] Implement fixed lead layout, scrolling and persistent selection without recreating terminals. -> satisfies [TEST-PANE-163]
- [x] [TEST-PANE-165] Private lead/PM skills are scoped correctly; setup/update/roster leave global skills untouched.
- [x] [IMPL-PANE-166] Implement private role skills and remove global installation/healing, with no automatic global cleanup. -> satisfies [TEST-PANE-165]
- [x] [TEST-PANE-167] PM has separate identity/window, manual same-session lead send/read and no worker authority or push.
- [x] [IMPL-PANE-168] Add optional PM role/window and complete manual lead result reads. -> satisfies [TEST-PANE-167]
- [x] [TEST-PANE-169] Updater and release metadata never inspect native versions; signed ConsensFlow version checks remain.
- [x] [IMPL-PANE-170] Remove compatibility probes, matrices, feed requirements and stale documentation. -> satisfies [TEST-PANE-169]
- [x] [VERIFY-PANE-171] Applicable full gates, isolated candidate acceptance, then authorized reinstall and installed verification with user histories preserved.

### Harness administration amendment

- [x] [TEST-PANE-172] In tests/ui.test.mjs and app/tests/page.spec.mjs cover each detected/missing harness, version/latest-release failures, evidence-based status, Pi extension red/retry and non-disruptive checks. Network/CLI boundaries use isolated fixtures.
- [x] [IMPL-PANE-173] Update src/install.js, src/ui.js, src/page.js and app administration UI with per-harness detection, informational version/update checks, status/actions and Pi installer status. No native version gating or active-session mutation. -> satisfies [TEST-PANE-172]
- [x] [TEST-PANE-174] Packaging, CLI and UI tests prove role skills are build-owned, no separate install/update skills action exists and global files remain untouched across app upgrades.
- [x] [IMPL-PANE-175] Remove separate skill-management actions from bin/cf.mjs and UI, update bundle/README to report included role skills; retain private roster data refresh and manual legacy cleanup. -> satisfies [TEST-PANE-174]

Decision log amendment: user now authorizes Pi extension auto-preparation only
when Pi exists and asks for version/update/status controls for every harness.
This supersedes the no-extension feasibility blocker and blanket diagnostics
version ban, not the no-version-gating contract. Completed historical tasks
155/169 retain their recorded evidence; new diagnostics are covered by 172/173.
VERIFY-PANE-171 runs after these new tasks as the final integrated gate.

### Phase 19 evidence

Implementation started in the isolated checkout; baseline protected file hashes
are recorded in `implementation-isolation.json`. Alpha.37 is installed; companion-window, updater and final acceptance passed.

## Resume Context

2026-09-10 current: Phase 23 is complete. Alpha.43 is installed; the duplicate
Updates header button is removed and the native-menu event remains tested.
UI 89/89, Rust updater 8/8, Node updater 16/16, packaged smoke 1/1; all exit 0.
Installed bytes and existing state/preferences were verified. Both remote
feeds remain HTTP 404 because signed updater releases have not been published.
No GitHub release, commit, GitHub push or NAS push was performed. See Phase 23.

Previous Phase 22: Alpha.42 was installed and running;
both saved conversation identities survived. Full assigned role instructions
are supplied on launch/resume for all four supported lead/PM harnesses. Role
tests 21/21, packaged smoke 1/1, session-concurrency 3/3; native fresh PM probes
passed for all four harnesses after explicitly selecting Gefjon's free Muse
model for OpenCode/Pi. The earlier default-model limits are superseded. Full Node
1221 passed, one existing lifecycle assertion failed, five skipped; integration
24/25 with a timing-sensitive failed-worker assertion that passes in isolation.
See Phase 22 evidence below. No commit or publication.

Previous Phase 20 context: at 183/184, both reported regressions were fixed
and tested; alpha.38 DMG and signed updater archive are prepared. Fortuna is
open with eight panes, so installation awaits the user's restart decision.
Next: install alpha.38 when approved, verify bundled version and preserved
session rows, then close VERIFY-PANE-178. Do not stop active panes meanwhile.

Previous completed release context:

Latest authorization, 2026-09-09: the user closed ConsensFlow and explicitly
asked to finish implementation, reinstall and check the installed application.
Continue source work in the isolated checkout; installation and native acceptance
are now authorized. Preserve user history and profiles; do not publish or commit.
Completed: 175/175 tasks. Alpha.37 is installed and running; the final
installed bundle matches all 55 files of the tested build. Source changes are
integrated into the original repository without committing. PM and updater
packaged acceptance both passed. Automated core/UI/process gates and native automatic
result receipt have passed for Claude Code, Codex, Pi and OpenCode. Pi draft
holding and delivery after explicit editor clearing were observed. Model-provider
quota/authentication errors are recorded separately from delivery acceptance.

Current product contract: Tauri/xterm.js, fixed double-width lead, horizontal
worker columns, full-size PM view above the lead in the main window, private `consensflow-lead` and
`consensflow-pm` documents for Claude Code/Codex/OpenCode/Pi only. Workers receive
neither role document; Kimi remains worker-only. PM send/read is manual and
same-session. Native versions are informational diagnostics, never gates.
Pi's process-scoped extension is explicitly approved and prepared only when Pi
is installed. No harness profile changes, development channels or global skill
installation. The five manifest-owned generic global skills were manually
removed after matching their stored hashes. The application does not delete them.

Historical defect/recovery record follows; it describes alpha.34, not the
installed alpha.37 implementation.

**Historical product blockers, 2026-09-09; recovery alone was not a product fix:**

- The running installed app and bundled CLI are still **alpha.34**, whose
  completion reader rejects Claude Code **2.1.266**. Alpha.36 admits that
  version, but `DeliveryWatcher.readResult` still couples worker-result access
  to parsing the lead transcript for a receipt cursor. Decouple authenticated
  access to a complete result from receiver-version/receipt availability;
  retain unknown receipt status rather than refusing the already-available text.
  Adding versions to an allowlist is no longer an acceptable fix: remove all
  native version checks per the latest planning decision, including the ones
  already present in alpha.36. Keep only ConsensFlow's own release comparison.
- After restart/resume, Poker `t-16` generation 6 has only lead `p-45` in its
  pane list; Astraeus/Calliope remain in `threads.json` under `tab:t-16:5`.
  Astraeus `d-105` is held for the previous lead generation. Both native
  reviews are complete: Astraeus 24949 bytes and Calliope 26168 bytes.
  The lead's claim that Calliope never started was false. Both were exported
  byte-for-byte to Gabriel's requested `pluribus/reviews/astraeus-2026-09-09.md`
  and `pluribus/reviews/calliope-2026-09-09.md`; no native task was restarted.
- Fortuna `t-41` generation 2 likewise has only lead `p-104`; Zeus, Diana,
  Apollo and Gefjon remain linked to generation 1. Apollo (10548 bytes) and
  Gefjon (2902 bytes) have complete native results. Zeus and Diana have no
  confirmed final result; their last public progress concerns tests in flight,
  which does not prove a process remains alive. Finals and explicitly incomplete
  progress exports are in `fortuna-advisor/runtime-archive/reviews/consensflow-recovered-2026-09-09/`.
  Recovery did not change Fortuna code/specs, tests, native stores, read marks
  or processes. The draft requires persistent worker navigation, readable old
  results and resumable native conversations without redispatch or cross-session
  reassignment. Investigate process-exit pane removal as well as restart recovery.

No installed restart was executed in this planning/recovery turn. These are
newly observed acceptance blockers; historical green test counts below do not
close them. The existing VERIFY-PANE-128/143 gates remain open.

Historical Phase 18 local implementation and verification: 152/154,
2026-09-09. The alpha.36 local candidate is prepared at
`/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.36/ConsensFlow_3.0.0-alpha.36_aarch64.dmg`
(SHA256 `1d2ac00748a28752e366c8de95322f0aaa8eaa06b068a8797b62b6d4bc17eff9`).
The matching signed archive, alpha `latest.json`, manifest, full validation and
sanitized evidence are in that directory. It supersedes the alpha.35 installer;
its earlier stabilization evidence is retained as historical evidence only.

Implemented: ten-second startup/six-hour checks, native menu and update dialog,
explicit download and later/install actions, stable/alpha channels, exact native
compatibility, official signature verification, atomic macOS bundle exchange
and restart. ALL open/hidden/idle panes and incomplete cleanup block install;
no draft is inferred and no working agent is stopped by the updater. A racing
launch cannot pass the same install admission boundary. Unknown cleanup stays
visible, ECHILD is treated as already reaped, and post-swap shutdown is bounded.
The complete app/Node/cf/skill/adapters update as one matching version.

Zeus reviewed final architecture and cleanup; Diana completed UI, release-feed
binding and packaged acceptance work; Gefjon supplied the initial release RED
suite. Their results were delivered through ConsensFlow and read whole. Root
integrated and verified. No native binary/global-setting edits, development
channels, plugins, commits or backups.

Final gates: Node1254 passed/1259 total (5 intentional skips), Rust102, UI83,
integration25, ordinary packaged smoke1 and actual packaged updater1: 1466
passing tests. Lint, warning-denied Clippy and formatting exit0. Final release
subset33/33. Mounted DMG matches all55 tested application files; deep strict
ad-hoc signature, archive/bundle byte-and-mode equality and actual release-key
verification pass. Altered archive bytes fail signature verification.

The isolated actual update ran alpha.35 PID3389, downloaded and verified the
signed test archive over HTTPS, refused installation with two real PTY panes
still alive, then atomically installed alpha.36 and restarted as PID3516. The
new UI and bundled Node were live, old processes were reaped, no staging stayed,
and the target application was unchanged. Test-driver path/TLS/barrier/FIFO
failures were corrected before that passing run; no rejected run was counted
as success. This uses an isolated test signing key; the separate distribution
archive was verified against the real configured release public key.

Native Claude2.1.266/OpenCode1.18.30 fixtures and real automatic worker-to-lead
delivery passed: Gefjon result CF_OC130_AUTO_DONE arrived as d-3, the Claude
lead acknowledged it and its unsent CF_UNSENT_DRAFT_266 remained visible.
Alpha.35's earlier seven-route matrix, skill behavior and original-colour
receipts remain in the historical evidence. Future native versions still need
verification; native permission rules can hold an agent's result-file tool call.

OPEN operations: read-only verification on 2026-09-09 found the installed
alpha.34 app at PID66854, started at 17:31:08 local time. Preserve Fortuna
t-41 and Poker t-16; their latest generations and recovery are recorded above.
VERIFY-PANE-128 and VERIFY-PANE-143 stay
unchecked until safe replacement/restart and installed acceptance are allowed.
The versioned GitHub release and rolling alpha/stable feeds have not been
published; online discovery is not activated. Older builds need one manual DMG
install to acquire the updater. The app is ad-hoc signed, not Apple-notarized.
There is no remaining local Phase18 code/test work. All owned worker labs and
scratch app copies are closed/cleaned; the release signing key is outside both
app state roots at ~/.tauri/consensflow-updater.key.

Previous release:

Current: Phase 15 complete, 123/123. Alpha.32 is installed and restarted.
Fresh Codex leads open without a model prompt and bind by verified native
metadata; same-folder separation and exact explicit resume passed installed
acceptance. Claude's development channel is removed and normal idle startup
passed. All final gates passed: 1360 tests, 4 expected skips. Own app test
sessions and superseded installer staging are removed; user sessions t-16/t-26
remain closed and retained. No commit or backup.

Previous release: Phase 14 complete, 117/117, alpha.31.
Native TUI styles and automatic draft-safe delivery are verified; reply recovery
controls are removed. New Pi/Codex conversations in the same directory have
separate native identities and histories, and the newly created terminal is
selected and focused. All verification components passed (1376 tests plus
4 expected skips); Diana and Gefjon reviews passed. Own acceptance sessions,
workspace and superseded release artifacts were removed without backups or a
commit. User sessions t-16/t-17 remain closed after the normal app restart;
resuming or deleting them remains the user's choice.

Previous release:

Current: Phase 13 complete, 105/105. Alpha.30 installed and restarted;
all five harness skills match the final generator and the native-tested skill
hash. The lead waits without querying delivery mode; generic continuation is
not a request to read. Explicit reading and Auto delivery both passed native
acceptance. No backup, commit, session deletion or restoration. Existing
sessions are suspended by the normal app restart and remain the user's to manage.

Previous release:

Current: Phase 12 complete, 102/102. Alpha.29 installed and release artifact
verified. Generated skills updated on all five harnesses; bare worker prompts
are clean through the native OpenCode launch plugin. Two identical tasks
bind separately and deliver whole automatically with zero result tools; an
explicit known-result read uses one call; ordinary joke follow-up uses one
send and zero result calls. Zeus's whole review is recorded above. No backup
or commit. Existing t-10/t-11 are suspended after installation; own t-12 is
active. User-deleted t-8/t-9 were not restored. Pi's accepted inline terminal
layout is unchanged.

Historical alpha.26 native acceptance (limited to explicit recovery):

- Fresh Pi lead t-4 loaded the generated skill and used `cf run @gefjon`.
  Gefjon `gefjon-amber-brook`, p-19, native OpenCode
  `ses_f808fec32ffe1or1SrmSXZ6CPP`, stayed visible and replied completely.
- After one app confirmation of the visually empty lead input, automatic
  delivery d-27 was accepted and Pi quoted the entire joke with AUTO-DELIVERY-OK.
  Direct worker input also worked: WORKER-STILL-LIVE and CONCURRENT-WORKER-OK
  arrived automatically as accepted d-28/d-29. Native Pi agent_settled evidence
  confirmed the lead's complete reports. No manual read, polling or policy change
  was used by the test lead.
- Two actual Pi sessions in the same directory stayed active. t-4 was renamed
  to Alpha.26 verificat at 05:20:28 UTC while Gefjon's 20-second task continued;
  its final answer completed at 05:20:37 UTC. Selection and rename retained
  generation 1, native identities and all six app/controller/harness processes.
- Native Delete removed t-4 and stopped Pi 12600, controller 13581 and
  OpenCode 13582; Pi 16808 in t-5 survived and answered AFTER-DELETE-OK.
  Both disposable sessions were removed through the app; the three user
  sessions remain available to resume or delete.
- Full gate exit 0: Node 1111 passed / 1115 total (four expected skips),
  Rust 69 unit + 12 headless, UI 61, real integration 25, packaged smoke 1.
  Final dialog wording distinguishes worker replies at the lead from lead
  messages at workers; its follow-up browser RED cases are recorded below.
  Final rebuilt bundle passed `npm run check:all` again, exit 0, with the same counts.
  The final installed and mounted DMG applications match all 51 files/links;
  codesign and DMG verification pass. Installed doctor confirms alpha.26,
  11 roster agents and one generated skill in each of five harnesses.
- Startup: four real-process RED/GREEN cases cover local PATH, retained exit 7,
  missing binary before admission and live continuation without a second lookup.
  Deletion modal races and interrupted deletion retry are covered. Failed pane
  scrollback stays in the page emulator until retry; no native re-subscription
  or failure-history archive is added. Unresolved launches are checked before
  any pane is stopped. Tab counter corruption fails closed.
- Zeus confirmed the final scope/lifecycle design. Generated immutable
  `/deliveries/d-*.md` bodies are now ignored by Git, preserving complete
  result receipts without committing inter-agent output. Session deletion
  preserves conversation records, inert coordination files and native history.

Final release artifact: `~/ConsensFlow-Releases/3.0.0-alpha.26/ConsensFlow_3.0.0-alpha.26_aarch64.dmg`.
SHA-256: `aa351fa767fd8c45e62d08cc0657fbe878e4092c41400bb39c0bd9b5b3c567d0`.
The alpha.25 installer and temporary QA folders were removed; no backup directory
or duplicate installed app remains. The three pre-existing sessions are retained.
Final native UI check resumed the existing repository session: its 18 historical
replies appear separately as **18 previous-session replies**, with no current
pending-result badge and no Resume replies button on the fresh unlatched lead.
All 87 tasks and all acceptance criteria are complete for the scoped macOS release.

Historical alpha.25 release record (superseded by alpha.26):

> Alpha.25 is installed at `/Applications/ConsensFlow.app`, without a backup.
> The DMG in `~/ConsensFlow-Releases/3.0.0-alpha.25/` matches the built and
> installed app across all 51 files/links. Its SHA-256 is
> `dc39f079001fc12dbca88ba665248c441064e2e9cb36666ad18ababfa48b06ea`.
> Codesign, DMG verify, installed `cf doctor` and all five installed skills
> pass. Old alpha.23/24 local installers were removed.
>
> Final `npm run check:all` exited 0 after the last source fixes: Node
> 1109 passed / 1113 total (four expected default skips); Rust 69 unit +
> 12 headless; page 52; real integration 19; packaged smoke 1. The packaged
> smoke runs the built app's own page, Node, CLI and a real PTY child.
>
> Zeus verified the main fixes and reported no remaining blockers, with
> two narrower findings subsequently fixed and tested RED then GREEN:
> the old Node `draft.clear` endpoint is removed and Kimi history extraction
> preserves the latest turn's queued-admission guard. Diana's UI passed
> 52/52; Gefjon's CLI/skill passed 125/125 before final copy alignment.
>
> 76/78 tasks are complete. VERIFY-PANE-64 (installed Pi/Gefjon automatic
> delivery after human input confirmation) and VERIFY-PANE-74 (installed
> rename/session-switching while workers continue) remain unverified live.
> The UI tool returns `cgWindowNotFound` for the running installed app;
> Gabriel has been asked to bring its window onto the current desktop.
> Automated production IPC/concurrency/result-reader tests pass; they are
> not recorded as substitutes for those two native operator checks.

> 2026-09-07 post-install regression reopened at `955b54d`, clean tree.
> Gabriel reported Sessions / panes versus New conversation and a Pi lead
> spawning `pi -p --name gefjon` instead of a roster consult. The installed
> Pi's `getAgentDir()` returns `~/.pi/agent`; ConsensFlow wrote its skill to
> `~/.pi/harness/skills`. The real `loadSkills`/`formatSkillsForPrompt` probe
> exited 1: no ConsensFlow skill and no model-prompt entry. Existing installer
> tests mirrored the incorrect path. Default/override discovery is fixed and
> a fresh Pi uses cf run. The live test also exposed missing worker PATH;
> both Gefjon and Zeus exited before their harness started. TEST-PANE-59
> reproduces spawn claude ENOENT through the real PTY and passes after
> forwarding the editor PATH. Alpha.24 is installed: doctor and codesign pass, native Pi discovers
> the skill and the legacy file is absent. Real Gefjon and Zeus panes are
> running; Zeus is reviewing, Gefjon is retrying provider rate limits. No backups are
> retained: Gabriel explicitly requested deleting them after alpha.23.
>

> 2026-09-07 **clean installation completed** after Gabriel explicitly
> directed the reinstall to proceed regardless of other projects' sessions.
> The temporary reset backup and all archived old bundles were subsequently
> deleted at Gabriel's explicit direction; no backup is retained.
> The old bundled CLI's `off --force` and `reset --yes` both exited 0;
> the old app and P3 bundles were retired, the verified alpha.23 DMG installed
> to `/Applications/ConsensFlow.app`, and only `agents.json` restored.
> `cf doctor` exits 0 with 11 agents, five installed skills, no mode line,
> and `/Applications/ConsensFlow.app/Contents/MacOS/node`. All five skill
> files match the installed generator and their manifest hashes. The actual
> running app executable is also under `/Applications`; a stale macOS
> registration of the build copy was removed before the verified launch.
> Native UI checks show an empty session tree, the full-window Agents
> dialog with the restored roster, and Close returning to the fresh workspace.
> Native harness credentials/history and the other projects' terminal
> windows were left in place. Release summaries were recorded beside the
> DMG; the raw reset evidence was deleted with the backup. No code changed
> after the tested release commit `3567314`.
>
> 2026-09-07 release candidate **3.0.0-alpha.23: all 54 tasks and all
> acceptance criteria passed**. Final `npm run check:all` exited 0: Biome
> 87 files, Node 1,048 passed / 4 skipped, Rust 68 unit + 12 headless,
> clippy with `-D warnings`, Playwright 40/40, real-process integration
> 15/15, real packaged-app smoke 1/1. The four default Node skips include
> the opt-in smoke, which passed separately inside the aggregate. Real-model
> evals are explicitly unrun and outside this offline gate.
> Zeus's final architecture/receipt review and Gefjon's Phase 6/UI review
> found no material blocker. Diana's real-PTY fault tests prove incomplete
> reads stay visibly uncovered and bridge death persists `uncertain` before
> restart, with no replay after a real resume and two watcher intervals.
> The final signed app's 43 bundled CLI source files match the working tree;
> the verified DMG's 50 bundle files match that tested app. DMG SHA-256:
> `7763caa8d36860ddc5db97ceae3eaff03402de145a342910c073b03e6405fc01`.
> Artifact: `/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.23/ConsensFlow_3.0.0-alpha.23_aarch64.dmg`.
> The local commit is gated on `npm run check` from an export of its exact
> staged tree; that result and the resulting commit id belong in the release
> record beside the DMG. This candidate was subsequently installed as recorded
> above. Native harness history and authentication remain outside cleanup scope.
>
> 2026-09-07 release implementation: root owns Phase 6 installer/CLI/settings,
> package scripts, and the final installation. Zeus implements 45–46 and
> adjudicates lifecycle with a real editor present; Diana repairs manual
> delivery identity/settlement and closed-tab held visibility, then 43–44;
> Gefjon implements the standalone skill/evals (49–50). Current candidate
> is `3.0.0-alpha.23`. Installed runtime remains untouched until workers
> finish and the candidate passes the release gate.
> Phase 6 root regressions: 126/126 passed, exit 0, after replacing retired
> mode/direct-cmux execution assertions. Reset/off, unowned-file protection,
> native integrations, PATH narrowing, roster edits and launcher ownership
> remain covered. The authenticated standalone CLI suite carries run/say/
> attach/read/bind/receipt coverage. Scripts `test:integration`, `smoke`,
> `check:all` now exist; missing integration or smoke artifacts are still
> release blockers. Do not infer task completion from script presence.
>
> 2026-09-07 15:17 EEST — release audit resumed at `3dd52bd` with all
> preexisting dirty changes preserved under
> `/Users/gabrielvoicu/ConsensFlow-Backups/20260907-151242` (patch, HEAD,
> status, untracked files). Zeus audits architecture in
> `zeus-copper-thicket` (surface:114), Diana audits 19–20/35–36/41–42 in
> `diana-misty-orchard` (surface:115), Gefjon runs baseline checks and
> legacy-install inventory in `gefjon-rusty-moss` (surface:116).
> Lead baseline: `cargo test --offline` exit 0, 66 unit + 9 headless;
> `cargo clippy --offline --all-targets -- -D warnings` exit 0.
> Page baseline: 34/35, stale Kimi lead picker and expectation conflict
> with the recorded lead-harness withdrawal. Corrected expectation first,
> RED captured (1 failed, exit 1), then removed the stale picker option;
> focused GREEN 1/1 exit 0; full page regression 35/35 exit 0.
> Read the complete prior `zeus-copper-tide` and `asteria-amber-fern`
> handoffs: the final exact-contract lifecycle review was APPROVE after
> transmitted-unknown protection and pre-transmission shell cleanup.
> Next: reconcile current test/audit evidence, finish 43–50, run the full
> release gate on the candidate, then back up app state and clean install.
> Installation has not started; old runtime stays available for workers.
>
> 2026-09-07 06:20 EEST — **channels (33–34) committed f9e606c**, registry
> 36/52. In review: the `cf` side round 3 (hyperion, `hyperion-nutmeg-harbor`),
> the watcher round 2 (hyperion, `hyperion-nutmeg-cloud`), the bridge
> operations 35–36 with the lead's session (asteria). In work: lifecycle
> 41–42 (hyperion, `hyperion-rusty-thicket`, with the new `dropEnv` field
> on `pane.open`). Waiting on zeus after asteria: `dropEnv` sent for the
> lead launch, kimi withdrawn from the lead harnesses. Then 43–46, the
> Phase 3 exit consult with astraeus, Phase 6.
>
> 2026-09-07 06:05 EEST — **page (39–40) committed f3943b8, Phase 4 complete**,
> registry 34/52. In work: `cf` side round 3 (apollo, hyperion's five),
> channels round 4 (diana, apollo's two), watcher round 2 (phoebus,
> hyperion's six), bridge operations 35–36 with the lead's preallocated and
> resumable session (zeus). Next: Phase 5 lifecycle 41–42 to hyperion,
> integration 43–44 and smoke 45–46 after 19–20 and 31–36 land, then the
> Phase 3 exit consult with astraeus and Phase 6 (47–50).
>
> 2026-09-07 05:25 EEST — **handlers (17–18 with slice B) committed e4634bd**,
> registry 32/52. In review: page round 4 (asteria, the last ordering
> finding), the `cf` side round 2 (hyperion, `hyperion-nutmeg-harbor`),
> the watcher (hyperion, `hyperion-nutmeg-cloud`), channels round 2 (apollo,
> `apollo-quartz-fern`). In work: zeus on tasks 35–36 widened to every bridge
> request Rust sends (the lead launch `tab.open` with binding evidence and
> channel configuration, `tab.resume`, `shell.open`, `pane.close`,
> `notify.set`, `state.list`, `answers.list`, `deliver.now`,
> `deliver.cancel`, `held.send`), the watcher wiring into `src/ui.js`,
> `state.changed` once per store mutation, 500 for unmodelled throws.
> Landing order: page, cf side, channels, watcher, then 35–36. Then Phase 5
> (41–46) and the Phase 3 exit consult with astraeus.
>
> 2026-09-07 03:30 EEST — **completion (23–24) committed f9bb3a9, deliveries
> (29–30) committed 94790a0**, registry 30/52. In work: slice A round 2 of
> the pane handlers (zeus, nine asteria findings, then slice B read/seen in
> the same conversation); page round 2 (hyperion, `hyperion-jade-willows`);
> the `cf` side 19–20 (apollo, `apollo-olive-reef`); the watcher 31–32
> (phoebus, `phoebus-pebble-spring`); channels 33–34 with probes P5/P6
> (diana, `diana-olive-bloom`, records go to findings-01.md). Page ops
> 35–36 wait for slice B and page round 2 (same file, zeus). Reviewers:
> asteria (`asteria-velvet-brook`), hyperion (`hyperion-nutmeg-cloud`).
> Phase 3 exit consult with astraeus after 31–34.
>
> 2026-09-07 02:50 EEST — state of play. Committed: Phase 1, binding, readiness
> + policy, layout, store + tabs + launch (13142ab). In review: completion
> round 4 (hyperion, 44/44 + readiness 33/33, `settledAfter` owns cursor
> freshness) waits for asteria; deliveries round 3 (apollo, 114/114, red-first
> artifacts kept) with hyperion in `hyperion-nutmeg-cloud`; pane handlers
> slice A (zeus, 96/96, held guard on consult/say/attach, `elsewhere`,
> store allocator) with asteria in `asteria-velvet-brook`. In work: page
> round 2 (hyperion, `hyperion-jade-willows`, asteria's 11 findings), the
> `cf` side 19–20 (apollo, `apollo-olive-reef`). Waiting: slice B (read,
> seen) after the deliveries verdict; Phase 3 exit consult with astraeus.
> Biome now lists the five spec modules under hosts/lib explicitly.
>
> 2026-09-07 01:40 EEST — **store, tabs, launch committed** (11–16) after
> asteria's round-6 approve (95/95 across store, tabs, binding, launch).
> In work: completion round 3 by hyperion in `hyperion-ivory-sky` (asteria's
> CM1–CM10; decisions: Pi settlement derived from Pi's own retry backoff
> unless a post-run record exists, OpenCode claims no cancellation until a
> supported-version fixture, cursors opaque with `itemsAfter(cursor)` on
> every adapter); deliveries round 2 by apollo in `apollo-hazy-lagoon`
> (gefjon's window died after two silent hours on the rate-limited free
> model; the eight DEL findings and decisions went to apollo in a prompt
> file); page unit by hyperion in `hyperion-jade-willows` (RED confirmed,
> GREEN in progress). Next tasks: 17–20 (`tests/ui-panes.test.mjs` against
> the real server, then the `cf` side). Registry 26/52.
>
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

2026-09-08, Phase 16: user rejects harness modifications/plugins. Use only native OpenCode CLI/API session creation inside ConsensFlow; retire its launch plugin. Diana conditional GO requires canonical directory and exact ID evidence, durable empty session, and reaped bootstrap child. Native probe proves idle TUI and draft-preserving queue; implementation gates remain open.

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
| 2026-09-07 | A unit is committed only when `npm run check` on an export of the exact staged tree exits 0, and the commit is gated on that exit code, never chained after it | The launch unit was committed while its `src/ui.js` imported the uncommitted store; `cf ui` at that HEAD could not load. Reverted in `1cc0b13`; the unit returns with the store |
| 2026-09-07 | The store's ownership is a kernel-held exclusive lock: the lock file opened with `O_RDWR|O_CREAT|O_EXLOCK|O_NONBLOCK` and the fd held for the store's lifetime; no pid, start time, tombstone or reclaim | asteria's S1 survived four rounds of file-primitive designs (a third contender, an admitted mutation writing after displacement) and S6 fell to time zones and DST; a live probe on this Mac showed Node honours the raw `O_EXLOCK` flag (EAGAIN in-process and cross-process, released with the fd, so released on a crash). BSD/macOS only; Linux and Windows get their lock in the packaging spec |
| 2026-09-06 | Rename, skill and evals in the LAST phase | risk 17 |
| 2026-09-07 | A transcript cursor is an opaque token minted by the completion adapter, and only that adapter compares it; every adapter exposes an items-after-cursor function, and no consumer (deliveries included) indexes an array with a cursor | asteria, completion round 2 (CM4): two real OpenCode tool results share one timestamp while their native completion events are sequence 47 and 48, so a timestamp is not a total order — and `deliveries.js` was reading numeric cursors as array indexes, which matches no adapter's native positions |
| 2026-09-07 | The adapter's items-after-cursor function returns an array when the transcript was read and the cursor positioned (an empty array is a verified empty boundary) and a non-array when it could not be read or the cursor is unknown; `cf read` prints a part's complete text verbatim, next-part line included, so it takes `--part <k>` | apollo, deliveries round 2: the receipt logic separates a verified empty transcript (may time out to uncertain) from an unreadable one (decides nothing); an adapter that answered an empty array for an unknown cursor would collapse that distinction |
| 2026-09-07 | Pi settles by a derived 120-second quiet window after the last append with no open tools, never labelled native; `stopReason:stop` is only a completion candidate | asteria CM1: Pi persists messages before retries, compaction and queued continuations, and the real `triton-jade-fern` session shows two 429 records then success with no user turn; Pi emits `agent_settled` only in memory (`agent-session.js:347`, after the post-run work at `:772`) and its maximum provider backoff is 60 s (`settings-manager.js:610`), so the window is twice that. Cost accepted: a Pi worker's answer reaches the lead about two minutes after it finishes in auto mode |
| 2026-09-07 | The argv Rust receives in `pane.open` carries two flags the `cf` side (tasks 19–20) must accept: `--launch <nonce>` for codex, opencode and kimi, placed as the seed's first line so `bindEvidence` finds it, and `--native-session <id>` for claude (a UUID the app mints) and pi (the conversation name), the session id the harness must take | zeus, pane handlers slice A: the launch nonce and the preallocated id are the two evidence paths the binding module already accepts (Phase 2 tasks 21–22); the flags name them on the command line instead of in the environment, which `childEnv` strips |
| 2026-09-07 | A new tab's policy is unset at creation; only a human's explicit choice is recorded as tab-auto or tab-manual. The reservation is the launch lock: taken before `pane.open` goes out, resolved only when Rust answers, so an unanswered open is `{outcome:'unknown', launch}` and every later launch under a new opId is refused until `pane.exit`. One launch identity: `issueTicket` returns it, and it serves the reservation, the `pane.open` dedupe, the lead's answer and `endLaunch`. `sent.record` takes an `opId` | zeus, slice A: writing `policy:'auto'` on every tab made the lead's `--notify` preference dead under the precedence table; two identities for one launch needed a map that a one-line return removes; a replayed `sent.record` appended twice |
| 2026-09-07 | Page unit, after asteria's round-1 block: two input paths into a pane (human keystrokes, paste and IME through `write_human`; emulator-generated replies such as DSR/DA answers through a reply path serialized on the same writer that never touches draft or Enter accounting); every Tauri command that waits on Node or the PTY is async and off the main thread with per-pane ordered admission; Node's `state.changed` is forwarded by Rust as a page event and the page reconciles new panes without changing the selection; GUI shutdown uses the headless ordering (close admission, drain launches, reap, drain handlers, transport); the page never computes policy precedence itself, it bundles `hosts/lib/policy.js` as it bundles `layout.js`; root Biome covers authored `app/ui` and `app/tests` files, vendor excluded | asteria: terminal queries with no typing latched a draft; blocked synchronous commands stall the page's acks; a worker opened by the lead never appeared; the shutdown race Phase 1 had fixed was reintroduced; a tab-manual veto displayed as `auto (pane)` |
| 2026-09-07 | Delivery ids come from a store-owned atomic allocator (`allocateDeliveryId()`, `d-<digits>`, persisted, never repeating across restarts); the deliveries module validates the pattern, refuses an id any visible record carries and a resend with its predecessor's id, and keys every map with `Object.create(null)`. Part framing is injective: the header records the body's exact byte length and one parsed body is compared. Unicode is normalised once at plan time (lone surrogates to U+FFFD, `normalized: true` on the record). `seenAfter` takes the conversation explicitly and scans from the transcript beginning over marked, covered or printed items. `channelFor` removed. Time is injected, never `Date.now()` | hyperion, deliveries round 2 review: two conversations shared `d-shared` and one file path, `abc` and `abc\n` framed identically so a changed body was accepted, an accepted `w1/msg_01` marked `w2/msg_01` seen, a mark at `a-3` skipped unread `a-1`/`a-2`, and the optional callback forced an ESC-bearing answer inline |
| 2026-09-07 | Root Biome covers the five modules this spec created under `hosts/lib` (completion, deliveries, readiness, policy, session-binding), listed explicitly in `biome.json`; it does NOT cover the rest of `hosts/lib`, because a safe `--write` there rewrites fourteen committed files (presets.js alone 1,590 lines) and leaves ten manual lint fixes outside any unit, a drive-by reformat that waits for its own change. In-flight modules are formatted by their authors, the two committed ones by the lead. A conversation live in another tab answers code `elsewhere`, `reserved` is only a launch that has not come back. Settlement freshness against a receipt cursor is the adapter's judgement (a function beside `itemsAfterCursor` returning true, false or null) and readiness maps null to unknown | asteria found completion.js and hyperion found deliveries.js failing the repository rules unseen; zeus's `#target` used one code for two situations; readiness compared opaque branded cursors numerically, so a Claude cursor against a settled Pi result read as ready |
| 2026-09-07 | Pane handlers, after asteria's slice A block: launch admission is one queued store operation that reads current pane and reservation state and reserves in the same step, nothing is released on a cached snapshot; a conversation with a bound native session is resumed on that session with the harness's own resume argument and binding is validated against the resume target, a UUID or nonce is minted only for a fresh conversation; an `opId` is one operation whose result is final once decided, refusals included, recorded before any external effect, so a replay never resubmits bytes; controller mutations and exit releases carry the expected launch, pane and generation and are compared inside the store queue; a named consult whose agent or harness differs from the row is refused (`agent-mismatch`); `live` requires a resolved reservation, an unresolved one reports `status: unresolved`; the delivery counter must be a positive safe integer on read and write | asteria reproduced two launches for one conversation from a stale tab read, a bound Claude session replaced by a fresh UUID on attach, a same-opId replay that pasted twice after a recording failure, an old controller's write landing on its replacement, a stale exit releasing a successor, a consult for one agent pasted into another's window, and `null` or `2^53` counters repeating ids |
| 2026-09-07 | The `cf` side: the bundle carries a three-line POSIX `bin/cf` shim that shadows a stale global `cf` and runs `cf.mjs` with the node named by `CONSENSFLOW_NODE`, which `leadEnv` sets to the app's `process.execPath`; `cf read` sends `{tab, deliveryId, part, opId}` and prints the part's text verbatim, `cf catchup` sends `seen {tab, session, items, opId}`; every seen walk (cf, the seen operation, the watcher) excludes tool items; in ConsensFlow `--new --session <name>` is refused with a message that teaches (`--new` mints the name), the old mint-then-name idiom dies with cmux in Phase 6; standalone `cf catchup --wait` and standalone `cf attach --print` are refused, `cf chat` points at `say`/`attach`, `CONSENSFLOW_CHILD=1` refuses the five conversation verbs; standalone binding reads the seed turn through the completion adapters and so fails closed on a harness version outside `SUPPORTED` (codex 0.153.4 today), a table to maintain at each harness update | apollo, tasks 19–20: nothing named `cf` existed in the bundle to shadow a global with; a pane's PATH is whatever the app gave it; slice B answered 501 so the request shapes had to be fixed by the client; a tool item is never covered nor printed, so it breaks the contiguous walk; `readConsult` refuses `session` with `fresh` |
| 2026-09-07 | The launch marker (nonce) is placed by the packet module in one place: `createPacket` takes a `nonce` like `createWindowSeed`, byte-identical output without one; kimi's first turn is a packet in argv, not a window seed, so this is the only way a kimi worker can ever bind | apollo, tasks 19–20: the marker never reached kimi's store, a kimi worker could never bind, and the earlier test hid it by asserting the honest-unbound refusal for the wrong reason |
| 2026-09-07 | Page unit, Tauri commands: per-pane input ordering is established synchronously at the command entry point before any blocking work is dispatched; a queued input is a message to the pane's single writer task and callers await a completion signal, never a blocking-pool worker; acks never share a pool with PTY writes; pending input per pane is bounded and an overflow is refused with a visible code; a provisional emulator is retired on confirmed closure or generation replacement after its parser work drains | asteria, page round 2: 1,000 sequential `pane_input` futures reached the PTY as 0000, 0009, 0010, 0011, 0002; 512 inputs behind one blocked PTY starved every other pane's input and acks with the production pool limit; a pane marked dead in the first snapshot kept its card and xterm alive |
| 2026-09-07 | Pane handlers, slice A round 2 shape: `store.admit` is the single admission mutation and `launchEvidence` is evaluated inside it from the row it read; `expect {launchId, tab, pane, generation}` on controller mutations and the lead's own paste is required in effect (a reserved row refuses a write that does not name its launch) and permitted absent on an unreserved row; `GET /api/panes` reports `status` as `live`, `unresolved` or `ended` and `live` needs a resolved reservation; a body naming a conversation on a controller op is `400 names-a-conversation`; `issueTicket` accepts the admission's launch id so ticket and reservation share one identity | zeus, slice A round 2: an unreserved row has no launch to compare, and 36 approved store tests use `sentRecord` as a generic queue write; the alternative was a shape requirement that buys nothing |
| 2026-09-07 | Pane handlers, slice A residuals: `store.admit` rejects a closed tab before changing anything, a closed tab is never evidence that its pane exited, only `pane.exit` releases; everything checkable before transmission (bridge available, inputs valid, roster agent present) is checked before admission, a failure after admission but before `pane.open` was transmitted releases exactly that launch inside the queue, a failure after transmission keeps the reservation; the opId ledger is consulted before any mutable-state validation, roster included; `agent-mismatch` is refused on attach as on consult and the conversation's agent and harness are validated inside queued admission | asteria, slice A round 2: suspending a tab mid-attach opened a second pane with no exit for the first; a missing bridge or a failed notify write stranded a reservation with no child to release it; a replay after roster removal answered 400 where the original was 200; attach after a roster name changed harness recorded a claude id on a codex session |
| 2026-09-07 | A closed conversation is RESUMED by the harness's own continuation path (the one `cf attach` already uses), for consult as for attach, with the `--native-session` the server passes; kimi continues non-interactively on `-S <id>` with the task before its TUI takes over; only `--new` preallocates. The pane-open response is validated: release only on exactly `{ok:false, error}`, any other shape keeps the reservation as uncertain; `transmitted` is set only when bytes were handed to the transport. App-only `cf` flags are validated before the image, app and cmux split, evidence flags require `--in-pane`, exactly one is present and `--launch` must match the redeemed launch; the shim refuses an unset `CONSENSFLOW_NODE` | hyperion's cf review: a closed `cf run` started codex and opencode cold and gave claude `--session-id` instead of `--resume`; asteria's handlers round 3: Rust answering `null` released a launch and a serialisation failure stranded one; the evidence flags fell through to cmux behaviour and a fake PATH `node` ran |
| 2026-09-07 | Page input admission is a synchronous Tauri command whose body only enqueues into the pane's bounded queue on the IPC thread and returns a ticket, so admission order is arrival order by construction; completion is awaited by a separate async command on the ticket; the page stamps a per-pane monotonic sequence on every input and Rust refuses a gap or regression with a visible code | asteria, page round 3: Tauri schedules the whole async command body, so work placed before the first await still runs in scheduling order; 1,000 sequential IPC requests through the production handler arrived as 0000, 0005, 0001, 0003 |
| 2026-09-07 | Tasks 35–36 cover every bridge request Rust sends to Node, not only the six page ops named: `tab.open` (the lead launch: tab created, lead reserved on the tab record with a minted launch id, the harness command with the lead's binding evidence, `leadEnv` plus the channel configuration, `pane.open`, the lead bound through the same path as a worker), `tab.resume`, `shell.open`, `pane.close`, `notify.set`, `state.list`, `answers.list`, `deliver.now`, `deliver.cancel`, `held.send`; `state.changed` is emitted once per store mutation that changes what the page shows, from the mutation queue; `/api/tabs` is the HTTP twin of `tab.open` | the lead launch had no task: Rust's `open_lead` asks Node for `tab.open` and Node handled only `ping`, so every page operation rendered as not available; the handlers' owner (zeus) builds them in one unit |
| 2026-09-07 | Channels: every native-channel delivery has a deadline, the opencode adapter defaults one inside the module; `launchConfiguration('pi')` mints one ack timeout and derives the extension's strictly shorter one from it, both carried in the channel object the adapter reads; an inbox record with an invalid id is quarantined once beside the inbox, never deleted or retried; the opencode username default `opencode` is confirmed against the real binary | apollo, channels round 2: a delivery without a deadline hung indefinitely, and the extension's honest negative ack landed about 10 ms after the adapter's identical 30 s timeout, so it was never read |
| 2026-09-07 | Page input: a correctly numbered message is consumed under the sequence lock before any refusal about its content (size included) is returned, with no bytes enqueued, so a refusal answers that sequence number and the next one is accepted; the page never rolls its counter back | asteria, page round 4: a 65,537-byte paste as sequence 1 was refused for size before its number was recorded, and every later keystroke answered sequence-gap, the pane dead to input |
| 2026-09-07 | Watcher: a native channel sends the pointer for a `cf-read` record and the envelope otherwise, the user envelope is never file coverage; `admitted: false` from a native channel is affirmative non-admission, the record fails with zero bytes and stays replayable; planning for a conversation waits while a newer sent question may still be landing, until its user turn appears or the wait grace expires; `close` drains every admitted operation through the transport before it resolves; the watcher emits no `state.changed` of its own | hyperion's watcher review: a large record on a native channel stayed submitting then uncertain with the real adapter, a pi negative ack read as success, a standing old answer was written 100 ms after a new question was sent, and a close between admission and transport stranded a zero-byte delivery as uncertain |
| 2026-09-07 | `cf` side: one evidence validator shared by `run` and `attach`; a kimi launch that captures no session is a visible failure with a nonzero exit; binding returns a status and never mutates the global exit, the final exit is decided once from the window outcome; the fake harness in the tests persists the prompt it was actually given so a marker test proves the marker travelled | hyperion's cf round 2: `attach` ignored bogus evidence flags and handed over with a wrong launch; a missing kimi binary exited 0 in silence; the same unbound transcript exited 0 or 1 depending on when discovery finished; a synthetic opencode fixture passed with the nonce removed from the real prompt |
| 2026-09-07 | Channels: the pi extension accepts exactly two wire shapes, the envelope and the pointer line rebuilt from the record's fields, compared byte for byte; the pi ack timeout minted at launch is 30 s (a turn of arbitrary length is what it waits on) while the opencode HTTP deadline keeps its own 3 s default, the two never share a constant; the pi cf-read channel test runs the real extension code | apollo, channels round 3: the adapters sent the pointer but the extension still demanded the envelope, so a file delivery through real pi timed out with the record left in the inbox; a delivery arriving 1.2 s into a 5 s turn was admitted on disk while the app recorded uncertain against a 3 s timeout |
| 2026-09-07 | Bridge operations (35–36): the lead is a session like a worker, preallocated at `tab.open` for claude and pi and bound at open through `session.bind`, and `tab.resume` resumes the BOUND session with the harness's own resume argument so the lead's context survives; closing the lead pane is the suspend, there is no `tab.suspend` verb; a lead pane ending releases its reservation and suspends the tab; `state.changed` fires on every successful store mutation and never on a failed one; `state.list` carries no answers, `answers.list` is per conversation; bridge operations are not idempotent by opId (the page has no credential, a second click is a second action); an unmodelled throw on a pane route is 500 with the cause's message as `reason`, store refusals carry codes and stay 400; `deliver.now` marks a pending record manual in place and resends an accepted or cancelled one under a new id | zeus, tasks 35–36: a lead's native session was null so every resume started the lead cold, losing its context; a lead exit left an unrecoverable tab; `deliver.now` wrote pending onto a cancelled record and reported success; a bare `internal_error` would have thrown away the one diagnostic a person needs on their own machine |
| 2026-09-07 | `pane.open` carries `dropEnv`, a list of names Rust removes from the child environment before spawning, after applying `env`; the lead launch sends the harness's billing guards in it (`interactiveGuards`) | zeus, task 21: a lead pane is spawned by Rust and the frame could only add keys, so a lead's harness ran without the guard that keeps a subscription login from switching to API-key billing; workers go through `cf`, which strips them |
| 2026-09-07 | Channels landed with two recorded follow-ups, neither blocking: the pi adapter does not withdraw the inbox record on `ack-timeout`, so past a 30 s turn the extension may still deliver after the app recorded uncertain (bounded, and readiness-gated on both paths; the clean close is to unlink the record on timeout); the pi extension imports `hosts/lib/deliveries.js`, so Phase 6 packaging must carry that dependency beside the extension | apollo, channels round 4 approve |
| 2026-09-07 | Bridge operations, after asteria's round 1: a lead's seed opens with the launch marker exactly as a worker's and discovered or reported lead evidence binds through the launch-fenced `leadBind`; a lead exit's release and the tab's suspend are one queued operation compared on pane, generation and launch; page delivery actions choose their transition against the current record inside the queue and never overwrite a terminal state; an unknown lead launch and its pane are preserved until a matching exit; `deliver.now` under manual creates the plan itself from the native answer; a refused resume returns the generation to suspended; a resume re-stamps the binding; `/api/tabs` is the lead-launch operation over HTTP; only explicit refusal types or codes are 400 | asteria, 35–36 round 1: codex and opencode leads could never bind (raw UUID seed), a late lead exit closed its replacement, a page action parked across an acceptance wrote pending over acceptedAt, a manual-policy answer had no way to be sent, a corrupted store file answered 400 |
| 2026-09-07 | User UI refinement: Agents opens full-window instead of an always-visible roster strip; reply delivery has explicit labels and explanatory help; selecting a single pane preserves the grid top edge | Gabriel's live screenshots and instructions at 16:26; this supersedes the earlier collapse-up roster layout |
| 2026-09-07 | Phase 3 exit, with the co-lead: cancellation, seen ownership, opaque cursors, separate channel budgets and the lead as a session all stand. Revised: the pi ack after a send with no observed `message_start` is `admitted: null` (unknown, mapped to uncertain), `admitted: false` only for a refusal before any send; one absolute per-delivery expiry stamped on the record, honoured by adapter and extension alike, expired inbox entries moved aside and never sent; native adapters call a new bridge request `pane.claim_epoch` right before their send, so a native admission has the same draft and epoch protection as a paste; the pi inbox follow-up is required Phase 5 work; the packaged smoke (45–46) must load and deliver through the packaged pi extension without resolving anything from the checkout and exercise the kernel lock with the bundled Node | astraeus: missing admission evidence is not proof of rejection (two successive sends negatively acked while both were delivered); `24 < 30` does not order two timers that start at different moments; `isIdle()` does not establish that the human has no draft |
| 2026-09-07 | `cf` side, after hyperion's round 3: every failure inside the binding operation, a controller refusal included, is `{bound:false, reason}`, handover always completes and the window decides the exit once; a spawn failure of the harness is a one-line refusal; a resumed kimi turn is a continuing packet; controller-side `--notify` is refused before redemption; `cf help` is the one allowed delta from byte identity; a pi worker launched through `cf` under the app carries the pi extension from `launchConfiguration('pi')`. Server: attaching a closed unbound conversation is refused before admission (`unbound-conversation`) with guidance to a new consult | hyperion's cf round 3: a real 409 on bind left codex with a raw stack and a live window and kimi with no window; a missing codex binary crashed with an unhandled error; the attach mismatch test never reached the check it claimed to test |
| 2026-09-07 | Readiness takes the caller's purpose: the derived-only Pi gate applies to AUTOMATIC delivery, a manual `deliver.now` proceeds on the derived settlement; the opencode HTTP deadline is the smaller of its 3 s default and the time left to the record's expiry; a caller-contract error throws and is never reported as uncertain; a pre-send epoch claim refusal is a zero-byte replayable failure. Follow-ups recorded: 30 ms slack at the pi expiry boundary, `claimEpoch` duplicated between two adapters | apollo's approve of the Pi evidence unit: `deliver.now` on a Pi lead without native evidence was refused, contradicting the decision that manual delivery stays possible; a malformed target reported "may have been delivered" when nothing was written |

## TDD Log

| Task | Red | Green | Refactor |
|---|---|---|---|
| [TEST-PANE-144] updater policy/state/download | Actual cargo runner exit101: absent validator/controller/download and candidate compatibility helper; root policy/state/signature/fix-metadata RED logs. | Official updater signature/download and policy tests pass; actual signed bytes accepted, tampering refused; full Rust102 exit0. | Fixed HTTPS channel feeds, strict newer-version policy, matching exact native compatibility and no direct UI plugin install authority. |
| [TEST-PANE-146] / [IMPL-PANE-147] install/lifecycle | Missing admission API; missing-runtime installer assertion failed; absent bounded-drain helper failed compilation, exits101. | Real PTY launch/install races, teardown, codesigned bundle failure cases and bounded drain pass; full Rust102 exit0. | Atomic same-volume exchange; ECHILD is reaped, unconfirmed cleanup blocks, post-swap drain cannot prevent restart indefinitely. |
| [TEST-PANE-148] / [IMPL-PANE-149] updater UI | Diana initial missing-surface RED plus two actual notes/blocker failures, exit1. | Nine worker UI cases plus cleanup-label integration case; updater10 and complete UI83/83, exits0. | Later dismisses the same-version banner, channel change checks immediately, failed pre-install refresh refuses installation. |
| [TEST-PANE-150] / [IMPL-PANE-151] release metadata | Gefjon actual absent prepare-update module failures, exit1. | Diana implemented helper:17/17 release tests, final related subset33/33, exits0. Real DMG/archive/source metadata preparation exit0. | Bind exact source/bundle/archive versions and native compatibility; compare safe archive bytes/modes without extraction; retain notes as text. |
| [TEST-PANE-152] / [IMPL-PANE-153] compatibility | Missing module/candidate helper and real bridge-wrapper mismatch failed tests. Native266/1.18.30 initially rejected; Claude266 peer version initially unsupported. | Compatibility/full Node1254 pass; native fixture/regression102 pass; real Claude266 peer and OpenCode30 automatic result with draft preserved. | Exact native versions only; future2.1.267 remains unknown. No native plugin or configuration edits. |
| [VERIFY-PANE-154] packaged acceptance | Isolated driver failed on path guard, generated public-key filename, macOS symlink path, TLS extensions, observation ordering and inherited stdin lifetime. Finder cosmetic DMG step was interrupted after stalling. | Final HTTPS/signature/two-real-PTY-block/install/restart test1/1; native oldPID3389 -> newPID3516 and exact app bytes/modes, no staging. CI DMG build, mount/hash/codesign and real distribution-signature checks pass. All distinct gates1466 pass. | Driver uses canonical scratch paths, valid TLS, an explicit selftest barrier and external FIFO writer; production verification stays enabled. Published feed and protected installed acceptance remain separate. |
| [IMPL-PANE-132] dispatch wording | Spawned CLI first-open assertion failed, exit1: only a conversation/pane line. | Full CLI95/95, exit0; startup combined109/109 earlier. | Keep first line stable; explicitly state that task startup continues in the app. |
| [TEST-PANE-139] / [IMPL-PANE-140] native delivery | Real native matrix exposed UTC process-identity mismatch, failed-record replan flood and unbound active Codex lead. Diana expiry-race regression2/3: deleted expiresAt was restored by store merge. | Native7/7 whole receipts and lead answers; same-ID/UUID epoch-race expiry clearing uses null. Node full1230 pass; Rust85; integration25; peer serialized-frame boundary10/10. | No retries after uncertain writes; exact native Claude UUID plus complete envelope; failed attempts stay terminal, explicit resend gets fresh metadata. |
| [TEST-PANE-141] / [IMPL-PANE-142] page indicators | Diana added duplicate/stale-generation failure and pending indicators regressions. | Current UI73/73 and full server/CLI suite pass. | Count current unique worker results and show source/reason; startup failure remains visible even if a native TUI is open. |
| [TEST-PANE-131] | Gefjon slow native startup: 1/1 failed at 15 seconds. Root spawned CLI status: 1/1 failed; progress admission: 1/1 failed, exits 1. | Gefjon helper 14/14; root real CLI startup/resume/uncertain/status 3/3, exits 0. | Shared 60-second budget, exactly one task POST. |
| [TEST-PANE-133] | Skill suite 39 tests, 3 failed, exit 1. | Text contract39/39; real Claude independent-work5/5, delivered-use3/3, manual-read3/3, exits0. Gefjon updated11 scenarios. Eval stream capture now keeps all assistant messages; fixtures include named input files. | Independent work and automatic arrivals are not manual retrieval. |
| [TEST-PANE-135] / [IMPL-PANE-136] | Diana 5/5 UI close regressions failed, exit 1. | Diana automatic delivery d-2: focused5, UI71, related bridge138 all pass, exits0. Root reviewed visible generation-bound action and existing close bridge. | No session deletion on pane close; lead action says Suspend session. |
| [TEST-PANE-137] / [IMPL-PANE-138] | Real Rust PTY inherited NO_COLOR=1, FORCE_COLOR=0 and empty COLORTERM; 1 failed, exit101. | Actual PTY pane tests6/6, exit0, including ANSI bytes and explicit overrides. | Remove only inherited color suppression; preserve explicit pane overrides. |
| [TEST-PANE-139] (completion subset) | Native Claude2.1.265 fixture3/4 failed because adapter rejected new version. | Claude263/265 suites15/15, exit0. | Captured own direct/tool turns and checked installed native finalizer; native seven-route matrix subsequently passed (see Resume Context). |
| [TEST-PANE-129] | Installed alpha33: zero native worker messages with --session/--prompt. Native helper 6 failures; spawned CLI zero task POSTs, exits 1 (`cf-native34-seed-red.log`, `cf-native34-cli-red.log`). | Focused107/107, runners28/28 and native model/task/queue/draft proof pass. | Native task API must be exercised, not just argv recorded. |
| [TEST-PANE-124] | Original unsolicited prompt: 3/3 RED, exit 1. Revised native-only contract: 6 failures / 7 tests, exit 1 (`cf-native33-red.log`), showing absent native identity/creation. Auth configuration RED: missing explicit native username, exit 1. | Runner 28/28, launch/auth 6/6, and focused native pane 9/9 pass, all exit 0. Native production-helper probe passed first-human/queue/draft checks. | Plugin approach rejected by user; native API replaces plugin identity. |
| [TEST-PANE-126] | Gefjon: missing createSession export, exit 1. | Gefjon: native helper 6/6, channel regression 43/43, exit 0; parent-exit/canonical-path review corrections included. | No harness plugin or native app edits. |
| [VERIFY-PANE-123] | Gefjon d-72 identified a stale native-metadata fixture; first integration teardown had a transient process-exit check. | Corrected fixture, unchanged expectations: full Node 1185 passed / 4 skips; integration rerun 25/25; all gates 1360 passed. Installed Codex idle/separate/resume and Claude no-channel startup passed. | Alpha.32 installed with matching signed artifact manifests; own app test sessions and old installer removed, no backups or commit. |
| [TEST-PANE-121] / [IMPL-PANE-122] | Gefjon d-70: 3 retirement tests failed. Root closing-tag regression failed because text was forced into a file. | Gefjon: channels 43/43, delivery watcher 62/62, skill 36/36; Root delivery planner suite exit 0. | Removed Claude adapter/hooks/flags/registry/files, obsolete closing-tag rule and stale documentation. Retired stored channels retain existing guarded terminal fallback. |
| [DESIGN-PANE-118] | Diana d-71: native app-server creates empty thread ID but no resumable rollout before first real user turn; hooks require trust and cannot be bypassed. | Root native Codex 0.153.4: no synthetic task, launch-specific originator persisted with first human turn, queue accepted and draft preserved. | Root accepts only verified cli_version 0.153.4 / thread_source user metadata; no future-version, fork, recency, or prompt-marker fallback for new metadata-bound leads. The originator mechanism is internal, like the transcript reader, not claimed as a public API. |
| [IMPL-PANE-120] | Quiet startup regression: 1 failed. Metadata binding/discovery regressions: 2 failed, including a false match from a copied prompt marker. | Native Codex probe: idle for 8 seconds, then answered only after human input; native session_meta.originator retained the launch marker. Binding/transcript suites: 41/41 pass. Pane server suite: exit 0. | No model prompt, hook or terminal scraping for Codex lead identity. Legacy marker launches still resume; metadata-only launches reject prompt-based fallback, forks and ambiguity. |
| [TEST-PANE-119] | `node --test --test-name-pattern="opens a new Codex lead without submitting a user prompt" tests/ui-panes.test.mjs`: exit 1, synthetic launch marker present in the native pane command | Focused regression passed; final full Node gate and installed idle startup passed. | Worker prompts remain intentional; fresh lead carries identity only through native metadata. |

2026-09-08 verification adjustments: the spawned server fixture offered a Codex
worker but installed only a Claude shim; preflight correctly refused it. Added
its missing harness fixture. Missing executable is now refused before launch;
the retained-failure test uses an executable that exits 7, testing runtime startup
failure. HTTP deletion expects existing page-only 403 behavior. No assertions
were weakened to permit lost processes or missing error output.

| Task | Red | Green | Refactor |
|---|---|---|---|
| [113/114] deletion during page read | Actual app cleanup showed internal_error; deterministic Page/Watcher/Store regression threw unknown tab from held | Watcher 63/63; deleted-tab send remains refused with zero writes | Only the read path returns an empty list after deletion |
| [115/116] new-session selection races | Two real-Xterm cases failed: an older refresh retained harbour; an unknown launch selected fresh-racing | Focused 3/3; full UI 66/66; lint exit 0 | Select and render only the confirmed returned live tab after the coalesced refresh completes; no new state machine |
| [86/87] recovery explanation | Lead description incorrectly said lead messages: 1 failed; worker dialog kept Resume replies title: 2 failed | Final full browser gate 61/61, full check:all exit 0 | Role-specific title and direction; unchanged snapshot/sequence contract |
| [81/82, 83/84] final UI edges | 2 failed: interrupted deletion offered Resume; null exit status printed null | 4/4 focused browser cases, exit 0 | No re-subscribe on failed-pane refresh |
| [79/80, 81/82, 86/87] UI and lifecycle | Diana 3 browser failures; Root missing tab deletion, latch state and generation failures, all captured | UI 58/58; Node/lifecycle 197/197; extended real deletion 3/3; Tauri latch 1/1, all exit 0 | No unrelated refactor |
| [83/84] startup and failure | Real startup PATH, failed-pane retention, missing preflight and unnecessary live continuation lookup each failed first | 4/4 real startup cases; Diana failure UI 2/2 and full UI 60/60, all exit 0 | Preflight only on a new launch; resolved directory included in worker PATH |
| [81/82] deletion modal race | Diana 1/1 failed, exit 1 | 3/3 related UI cases, exit 0 | Snapshot target; disable cancel and ignore Escape while deleting |
| [TEST-PANE-83] local harness | Real Rust/Node/CLI/PTY test: 1 failed, exit 1 — spawn claude ENOENT despite successful lead detection in user-local bin | See grouped GREEN evidence above | none |
| [TEST-PANE-86] runtime draft state | `cargo test ... production_ipc_consumes_sequence_before_size_refusal`: 1 failed, exit 101 — Null instead of false | See grouped GREEN evidence above | none |
| [TEST-PANE-81] process deletion | Real integration: 1 failed, exit 1 — unknown-op. Corrected fake harness setup to hold the worker alive before RED. | See grouped GREEN evidence above | none |
| [TEST-PANE-81] tab identity | `node --test --test-name-pattern TEST-PANE-81 tests/tabs.test.mjs`: 1 test, 1 failed, exit 1 — beginDelete missing | See grouped GREEN evidence above | none |
| [TEST-PANE-59] Worker PATH | Real Node editor and Rust PTY with Finder PATH: 0/1, exit 1; pane output says spawn claude ENOENT and no worker transcript appears | focused real-process test 1/1, authority contract 1/1, full integration 16/16, all exit 0; alpha.24 starts actual OpenCode/Gefjon and Claude/Zeus workers | Earlier fixture gave both processes the same rich PATH, masking desktop behavior. Full integration also exposed a pre-existing observation race: await lead invalidation, which follows durable delivery suspension, instead of asserting between its two writes |
| [TEST-PANE-55] Pi discovery | node --test tests/install.test.mjs: 50 tests, 11 failed, exit 1; wrong native skill directory/override and migration. Independently, installed Pi loadSkills + formatSkillsForPrompt: no ConsensFlow entry, exit 1 | node --test tests/install.test.mjs: 50/50, exit 0; actual Pi loadSkills and formatSkillsForPrompt discover one enabled ConsensFlow skill, exit 0; fresh native Pi startup lists it and uses cf run @gefjon | Includes an already-green compatibility guard for an explicit override selecting the legacy directory; that case must remain supported |
| [TEST-PANE-57] Session terminology | focused browser test failed on New session, then with empty-state assertion failed on Open a session; both exit 1 | focused 1/1 and full UI 40/40, exit 0 | Existing directory/harness command assertions retained |
| Final macOS release gate, alpha.23 | initial audit lacked check:all, integration and packaged smoke; the completed gates found four real packaged/lifecycle defects | final npm run check:all exit 0: Node 1048 pass / 4 skipped, Rust 68+12, clippy -D warnings, UI 40/40, integration 15/15, packaged smoke 1/1 | final app rebuilt after source freeze; codesign verification and hdiutil verify exit 0; mounted DMG contains the identical 50-file tested bundle; Zeus and Gefjon final reviews report no material blocker |
| [TEST-PANE-43] completed fault matrix | real bridge SIGKILL after observed paste before CR left submitting on disk; body-loss/early-close/marker-only cases originally lacked controls | Diana final full matrix15/15, exit0, 10.123s | raw-mode CR observation plus SIGSTOP barrier; uncertain is asserted before teardown, then real Node/Rust restart and real lead resume wait two watcher intervals with no replay; all missing part numbers reach answers.list |
| Node EOF durable shutdown | real-process bridge fault above; synchronous process.exit preempted the watcher mutation | focused fault1/1 and lifecycle/UI/CLI67/67, exit0 | idempotent bounded drain of watcher and store before EOF/fatal/EPIPE exit; HTTP partial request cannot hold durable flush behind server.close; ordinary read-pipe EPIPE behavior retained |
| [TEST-PANE-53] visual follow-through | initial immediate capture omitted some not-yet-painted content | Gefjon stable-paint probes display both focused worker and lead titles, and full-window Agents iframe content | root inspected images; no additional product CSS change needed for capture timing; UI regression39/39 before the new part-progress test, focused part-progress1/1 |
| Unconfirmed file parts, tasks35/43 | real page API omitted partProgress; browser menu omitted the receipt summary; each focused test failed, exit 1 | API 1/1 and browser 1/1, exit 0 | expose only latest attempt id, total and uncovered part numbers; receipt evidence controls coverage; accepted answers hide obsolete missing-part notices; no answer bodies added to page state |
| Pi timeout withdrawal | actual inbox file still present after ack timeout, 0/1, exit 1 | channels + Pi extension 45/45, exit 0 | remove the unread inbox offer while preserving admitted:null / uncertain; a later real extension consume sends nothing; Zeus reviewed and approved the uncertainty semantics |
| [TEST-PANE-41] GUI shutdown adjudication | characterization tests pin the existing order; waiting for launch closure before stopping the real editor cannot complete | two real editor-present native tests, exit 0; full Rust 68 unit + 12 headless, exit 0 | closing the editor closes admission; then drain admitted launches, reap pane groups and drain transport. Closing a lead stops its own tree; workers survive until app exit so held replies can finish |
| [TEST-PANE-45] packaged smoke | missing boot report, invalid Tauri event, zero output arrivals, and missing system-tool PATH each produced exit 1 | Zeus: real built app smoke 1/1, exit 0 | bundled assets, real PTY rows/input hex, >1MiB ACK credit flow, packaged Pi extension send/ack with no checkout resolution, bundled Node lock refusal, ordinary app exit and no child residue; final candidate must be rebuilt after all edits |
| Phase6 documentation consumers | two engine README assertions required retired foreground/direct-run contracts, 0/2 exit 1 | two current app-owned pane/send-return/unread/artifact assertions, exit 0 | native execution tests retained; source-to-sibling parity checks unchanged |
| Lead tool PATH | BO10 real server test 0/1, exit 1: actual pane.open PATH ended in bin/cf.mjs instead of preserving inherited executable directories | BO10 1/1, exit 0 | src/panes.js passes the environment PATH to leadEnv; bundled cf still comes first |
| [TEST-PANE-53] pane labels | two browser failures, exit 1: raw auto/manual and tab-human/pane-human tokens | two focused tests passed, exit 0 | title labels and provenance tooltip use plain language; session manual veto and lead preference remain tested |
| Output subscription | packaged smoke saw zero page arrivals after repeated state refresh; real Tauri Channel drop ends the callback | Zeus split subscribe_output from list_state; root browser test confirms output after two state refreshes and one subscription | page fixture rejects duplicate subscription and any channel passed with state refresh |
| [TEST-PANE-49] standalone skill | Gefjon: 22 tests, 9 passed and 13 failed, exit 1 | 22/22, exit 0; consumers 144 passed, 3 skipped, exit 0 | eight cf-only eval scenarios; actual stub/offline assertions verified; real model evals unrun and outside check:all |
| [TEST-PANE-43] first integration tranche | real-process suite initially failed against obsolete headless protocol, 3 failed, exit 1 | Diana: 9/9, exit 0; standalone CLI 82/82, page ops 121/121, watcher 45/45 | real cf read every part, unread frontier established via real catchup; deterministic body-loss/bridge-death faults still in progress |
| [TEST-PANE-67] Pi transcript paths | Diana 4 tests, 1 pass / 3 fail, exit 1; root independently reproduced display failures then completion failures after the display-only fix | 115/115 across paths, display, completion and installer, exit 0 | shared native tilde/empty/override resolution, valid default-path decoys |
| [TEST-PANE-75] dedicated result reader | three missing watcher methods RED; real CLI unknown results command RED | watcher 53/53 and reader/existing native integration 9/9, exit 0 | real HTTP scope, 60000-byte result, actual child CLI reads all parts, full native receipt, no Node terminal writes and omitted discussion stays unread |
| [TEST-PANE-77/78] six visible panes | tall viewport 7/10/20 cases all RED, exit 1; all panes incorrectly visible | Diana: layout 18/18, page 48/48, exit 0 | third row below viewport, final rows scroll reachable, two/one columns responsive and compact result indicator cannot widen page |
| [TEST-PANE-69] Claude 2.1.263 | 8 tests: 1 pass / 7 fail on the version gate; admitting the version left final settlement RED | 56/56 completion tests, exit 0 | native turn_duration finalizer, root messageCount and zero pending background/workflow checks; unknown versions, truncated records and unfinished tool loops fail closed |
| [TEST-PANE-71/73] rename and concurrency | rename: Node 14/15 and browser 40/41, exit 1 | Diana: tabs 15/15, page 41/41, ui-panes 136/136, concurrent real PTYs 1/1, all exit 0 | Rust rename bridge and installed-app verification remain Root work |
| [TEST-PANE-61] opaque-input recovery | Initial wiring test RED then GREEN did not prove causality; replacement no-guess regression fails against the heuristic | No-guess native Node/Rust/PTY 1/1, exit 0; human epoch/generation unit and real Tauri sequence checks pass | Zeus/Diana review rejected automatic text matching; human-only confirmation replaces it |
| [IMPL-PANE-63/76] final authority and latest-turn guard | Bridge old/current epoch clear succeeds before removal; Kimi queued latest incorrectly settled (both RED) | Focused GREEN; final whole gates recorded with release | Removed legacy bridge authority; preserved Kimi latest readiness; UI 52/52 |
| [IMPL-PANE-76] review regressions | Pi/Kimi historical finals, manual restart and in-flight duplicate: 0/4, exit 1 | 4/4 exit 0; combined completion/watcher/v263 112/112 exit 0 | Open tools stay excluded; draft-held manual reads remain available |
| [TEST-PANE-65] Pi launch evidence | three focused failures, exit 1: valid launch held, wrong launch admitted, worker evidence missed | 3/3 focused and 48/48 watcher, exit 0 | read extension evidence from the bound target launch, never editor-global hints |
| Phase 7 integration recheck | one transient teardown asserted a child still alive; direct inspection then found it gone | isolated case and final suite 16/16 exit 0 | no sleeps or weakened cleanup assertions were added; production cleanup contract retained |
| [TEST-PANE-53] UI refinement | four browser RED failures, exit 1: visible top roster, ambiguous label, and 36 px vertical offset for one/four panes | four focused browser tests passed, exit 0 | full-window Agents dialog retains the iframe/output ACKs; explicit delivery help; focused navigation inside title bar removes the offset |
| [TEST-PANE-43] native helper parity | real `cargo test --offline --manifest-path app/src-tauri/Cargo.toml --test headless product_bridge_contract`: exit 101, 0 passed / 3 failed; app-issued identity/launch fields rejected, Enter and natural exit events absent | all 12 headless tests passed, exit 0, including the new three and existing nine | `consensflow-bridge` calls shared `commands::run_headless`; production launch/input/claim/draft/exit handlers, launch deduplication, and output ACK behavior are the integration target |
| [TEST-PANE-39] unfinished-answer menu | new Playwright test failed on the old Uncertain/Resend presentation, 1 failed, exit 1 | full page suite 36/36, exit 0 | backend `ready` distinguishes unfinished answers; visible In progress row with send disabled; completed uncertain deliveries still offer explicit resend |
| Page event contract, 2026-09-07 | packaged smoke exposed Tauri rejecting `state.changed`; the focused browser external-worker check then failed against its stale dotted emitter (1/1 failed, exit 1) | page shim now enforces Tauri event-name characters and emits `state-changed`; full Playwright 36/36, exit 0 | Rust keeps the Node bridge name `state.changed` and translates only the Tauri hop; fixtures no longer hide invalid Tauri names |
| Phase 6 engine regression migration | old outside-app CLI engine paths failed as expected after switch-over | real runner subprocess/packet/stream/permission/auth and Pi extension suites 19/19, exit 0 | retained real spawned engine fixtures at the `runAgent` boundary; standalone requester/controller tests cover the app CLI; Pi busy-arrival test observes actual send instead of assuming its handler owns the asynchronous drain, deadline-specific tests retain explicit short expiry |
| [TEST-PANE-47] switch-over, 2026-09-07 | Installer 3 failures, CLI retirement 4 failures, settings 3 failures, cmux lead fallback 1 failure, pre-mint retirement 1 failure; each captured before its production change, all exit 1 | focused installer 3/3, CLI 4/4, settings/full UI 27/27, exit 0 | final root-owned installation/CLI/UI/ownership/threads regression: 126/126, exit 0; logs in release task directory; skill prose is still assigned to Gefjon |
| [IMPL-PANE-48] root portion | — | `installEverywhere`, mode-free targets/heal, retired `use`/`mode`/`mint`, app-only run/attach, no direct CLI thread writes/cmux lookup, settings no mode controls | removed obsolete mode module/tests; retained meaningful reset/off/ownership/PATH tests in install suite; full Phase 6 remains open until one skill prose and eval work land |
| [TEST-PANE-39] picker regression, 2026-09-07 | `cd app && npx playwright test tests/page.spec.mjs --grep 'offers only canonical lead harness'`: 1 test, 1 failed, exit 1; the menu still offers Kimi against the recorded four-lead contract | — | corrected the stale five-lead expectation before removing the option |
| [IMPL-PANE-40] picker regression, 2026-09-07 | — | same focused command: 1 passed, 0 failed, exit 0 | no further refactor; `cd app && npm run test:ui`: 35 passed, 0 failed, exit 0 |
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
| [TEST-PANE-25] | zeus, `ERR_MODULE_NOT_FOUND hosts/lib/readiness.js` (16 tests); after asteria's BLOCK and the settlement-proof contract: 21 RED against the old module (30 tests) | — | — |
| [IMPL-PANE-26] | — | 30/30, then 33/33 after her two test-gap edits; lead re-ran 52/52 with policy; isolated staged-tree check exit 0 | asteria: BLOCK (a bare `settled` boolean authorised delivery; missing evidence permitted ready; no native freshness boundary) → rebuilt to consume `{state, provenance, cursor, boundary, evidence}` → **approve with edits** → edits done |
| [TEST-PANE-27] | zeus, `ERR_MODULE_NOT_FOUND hosts/lib/policy.js` (13 tests; a 36-combination precedence table) | — | — |
| [IMPL-PANE-28] | — | 13/13, then 19/19 with `recordLeadPreference` exercising the `--notify` write clause; lead re-ran | asteria: **approve with edits** (the write clause was unproven) → done |
| [IMPL-PANE-06] (Node half) | — | `node --test tests/bridge.test.mjs`: 17 passed, 0 failed; lead re-ran: 17/17, `tests/ui.test.mjs` 29/29 | biome format + two assignment-in-expression lints fixed; one self-inflicted test sizing (a 64-byte budget could not fit the refusal frame) corrected in the TEST, noted as a test bug not an assertion change |
| [TEST-PANE-15] | `node --test tests/launch.test.mjs`: exit 1, 1 test, 1 failed — `ERR_MODULE_NOT_FOUND src/launch.js` | — | — |
| [IMPL-PANE-16] | — | `node --test tests/launch.test.mjs`: 18 passed, 0 failed (hyperion; zeus: approve, on condition it lands with the store) | tab-scoped tokens, single-use tickets, `childEnv` strips the six control variables |
| [TEST-PANE-11] | brokkr, `node --test tests/store.test.mjs`: exit 1 — `ERR_MODULE_NOT_FOUND src/store.js` | — | — |
| [IMPL-PANE-12] | — | brokkr 11/11; after six asteria rounds (S1–S6 and the lock give-back, zeus rounds 3–6): 40/40; asteria: **approve**, 95/95 across store, tabs, binding, launch | file primitives replaced by the kernel `O_EXLOCK` lock (Decision Log 2026-09-07) |
| [TEST-PANE-13] | brokkr, `node --test tests/tabs.test.mjs`: exit 1 — module missing, and the `leadId` clause demonstrably returning `sess-9` | — | — |
| [IMPL-PANE-14] | — | 14/14 (persisted pane allocator, generation-checked removal after asteria's T1/T2); asteria: **approve** | `CONSENSFLOW_LEAD_ID` first in `LEAD_KEYS` |
| [TEST-PANE-23] | hyperion's replacement suite (gefjon's first version blocked by hyperion against the real stores): `node --test tests/engine/completion.test.mjs`: exit 1, 21 tests, 1 pass, 20 failed | — | — |
| [IMPL-PANE-24] | — | 29/29, then after asteria's four rounds (CM1–CM10, then CM4/CM7 residue): completion 44/44, readiness 33/33; asteria: **approve**, 77/77, real OpenCode session settled across late metadata updates, 56 fixture rows verified against native records | five version-gated adapters, `itemsAfterCursor` and `settledAfter` own cursors and freshness, Pi derived 120 s quiet window, OpenCode cancellation gated (Deviations) |
| [TEST-PANE-29] | gefjon's first suite red on a missing `hosts/lib/deliveries.js`; apollo's round-2 suite against gefjon's module: 82 tests, 32 pass, 50 fail, exit 1; his round-3 suite against the round-2 module: 106 tests, 70 pass, 36 fail (hashes recorded in his report) | — | — |
| [IMPL-PANE-30] | — | 119/119, coverage 99.49 / 99.69 / 100; asteria blocked round 1 (DEL1–DEL8), hyperion blocked rounds 2 and 3, then **approve**, no remaining findings, 22 mutations caught, safe-integer allocator verified on the tree | ids injected and validated (`d-<digits>`), injective part framing with byte count, one plan-time Unicode normalisation, explicit conversation in `seenAfter`, `channelFor` removed, time injected; the catchup clause moved to tasks 19–20 (Deviations) |
| [TEST-PANE-17] | zeus, `node --test tests/ui-panes.test.mjs`: exit 1, 19 tests, 0 pass, 18 cancelled — `server.attachBridge is not a function`; each later behaviour red first (refused launch leaves no pane row, preallocated harness told its session id, unresolved launch is nobody's live pane, the four asteria residuals, the serialisation boundary) | — | — |
| [IMPL-PANE-18] | — | 50/50 ui-panes, 133/133 with store and ui; asteria over four rounds (H1–H9, then four residuals, then four more, then one test): **approve**, 285/285 on her snapshot | `store.admit` as the single admission mutation, one owner of give-the-launch-back, the opId ledger before any mutable-state validation, the seen walk owned by the server over `{id, role, printed}` items |
| [TEST-PANE-39] | hyperion, `npm run test:ui`: exit 1, 20 failed (`#app` absent); every later finding red first through the production invoke handler (1,000 requests reordered as 0002, 0001, 0004, 0000; the oversized paste answering sequence-gap) | — | — |
| [IMPL-PANE-40] | — | Playwright 35/35, cargo test 73 (66 unit + 7 headless incl. B8/C7 at exactly 1,024 unacked and 14,400 drained), clippy -D warnings clean, app-only bundle signed; asteria over five rounds (11 findings, then 3, then 1, then 1): **approve**, no remaining findings | two input paths (human vs emulator replies), synchronous sequence-checked admission with async ticket completion, per-pane writers off the blocking pool, `state.changed` forwarded to the page, headless shutdown order, the page bundling `policy.js` and `layout.js` |
| [TEST-PANE-33] | diana, `node --test tests/channels.test.mjs`: exit 1 — `ERR_MODULE_NOT_FOUND src/channels.js`; each later finding red first (pointer for cf-read: 2 failures; the 3,000 → 30,000 ms revert failing; the required-timeout guard) | — | — |
| [IMPL-PANE-34] | — | channels 31/31 with the pi extension suite; P5 and P6 PASSED live and recorded in findings-01.md; apollo over four rounds (8 findings, then 4, then 2): **approve**, cf-read pointer reaching real pi in 248 ms, a mid-turn arrival admitted at 3,966 ms, six reverts caught | `launchConfiguration` and `enabledChannels` as the producer of a lead's native-channel launch; the extension accepts exactly the envelope or the pointer rebuilt from the record, byte for byte |
| [TEST-PANE-31] | phoebus, `node --test tests/delivery-watch.test.mjs`: exit 1 — `ERR_MODULE_NOT_FOUND src/delivery-watch.js`; then three self-arranged asteria rounds (9, 4, 3 gaps) and hyperion's six, each red first | — | — |
| [IMPL-PANE-32] | — | 39/39, 118/118 with channels, the pi extension and the bridge; hyperion (independent): **approve**, wait grace held at +3,999 ms and released at +4,000, a pi negative ack failed and replanned under a new id, close waited for an admitted operation, zero watcher-emitted `state.changed` | pointer versus envelope by record channel, `admitted:false` as affirmative non-admission, planning deferred while a newer question may still be landing, close drains admitted work |

## Deviations

| Task | Spec Said | Actually Did | Why |
|---|---|---|---|
| review fix (9) idle tracking | RED before GREEN, always | GREEN without a captured RED | the RED build's link step failed with `No space left on device`; hyperion freed ~1.2 GB (cargo artifacts, Homebrew cache) and continued; the test exists and passes — recorded here rather than hidden |
| B8 hidden consumer | a hidden pane keeps acking | left unproven in Phase 1 | the consumer is the page's emulator (Phase 4); asteria's finding 8: crediting a fixture that asserts a local flag would prove nothing |
| [TEST-PANE-29] coverage | `coverage` maps delivery → item ids | `coverage` maps delivery id → the WORKER answer ids it covered; the lead-side receipt item ids stay on the record as `evidenceIds` | asteria DEL5: unread bookkeeping advances over the worker transcript, so it needs the worker's ids; the lead's receipt ids are evidence of delivery, not of what was covered (apollo, deliveries round 2) |
| [IMPL-PANE-24] OpenCode cancellation | a cancelled OpenCode turn is recognised natively | `MessageAbortedError` is `failed:true, cancelled:false`: OpenCode claims no cancellation until a supported-version native fixture establishes the shape | asteria CM8: the real database holds 14 such rows, none from a supported version, and the fixtures cover none; a discriminator without evidence is a guess (hyperion, completion round 3) |
| [IMPL-PANE-30] part marker | parts are opened by `[part k of N]` | the open marker is `[part k of N — <n> bytes]`, `matchPart` parses exactly one body of that length and compares one digest; `cf read` still prints `part.text` verbatim | hyperion's round-2 review: `abc` and `abc\n` framed identically, and the matcher accepted either digest, so a changed body passed receipt; the byte count makes the framing injective (apollo, round 3). Lone surrogates are normalised once at plan time to U+FFFD (`normalized: true`), so such an answer is representable and may go `pty-inline` |
| [IMPL-PANE-30] catchup clause | task 30 carries the standalone `cf catchup` integration (`bin/cf.mjs:379`, turn counts today) | the clause is delivered by the `cf`-side unit, tasks 19–20 | one worker edits `bin/cf.mjs`; the deliveries module stays pure and exposes `seenAfter`, the CLI consumes it |
| [TEST-PANE-17] `seen` | `seen {session, items}` by item id | the request carries the ordered transcript items the client read, as objects `{id, role, printed}`, and the SERVER runs `seenAfter` over them with the deliveries and the store row; the client reads no delivery file | asteria (handlers round 3) and hyperion (cf review) found the two ends incompatible: the client sent ids from its own walk over a local copy of the deliveries file, the server required objects. One owner for the walk, the one holding the deliveries |
| Architecture, pi channel | `sendUserMessage(…, {deliverAs: 'followUp'})` on `agent_settled` | `sendUserMessage(text)` with no `deliverAs`, delivered from an inbox at once when the agent is already idle and on `agent_settled` otherwise | pi's own types apply `deliverAs` only while a turn is streaming; P6 proved an arrival while idle is delivered immediately (apollo's channels review, diana's P6 record) |
| Lead harnesses | every roster harness can lead | kimi is withdrawn from `LEAD_HARNESSES` until a kimi lead can be seeded with a launch nonce | zeus, task 21: kimi's interactive start takes no positional prompt and `-p` is non-interactive, so a kimi lead opens bare and can never bind |
| Phase 3 exit, Pi | Pi settles by a derived 120 s quiet window and delivers automatically on it | the pi extension records observed `agent_settled` evidence in app-owned state tied to launch, session and frontier, invalidated by new work; the adapter consumes it as a native boundary; a derived-only Pi settlement is not eligible for AUTOMATIC delivery (manual reading and `deliver.now` remain) | astraeus, co-lead, Phase 3 exit: the 60 s is a configurable default, not a maximum, and a retry delay does not bound provider execution, compaction or queued continuations; the adapter reported empty queues it never observed |

Implementation evidence 2026-09-09 (isolated checkout): native-reader regression failed with unsupported Codex 0.154.0; readers then passed 54/54. Binding/peer/launch regression failed before removal; focused suite passed 77/77 afterward. No installed files touched. TEST-PANE-157 is next.


### Isolated implementation checkpoint

Completed source slices: TEST/IMPL 155–160, 163–164, 169–170. RED evidence captured for version rejection, missing lead transcript, saved answer after native transcript deletion, lost worker row, and wrong lead width. GREEN: native completion fixtures 57 tests; launch/binding/peer suite 77; result/delivery/tab suite 213; browser UI suite 82; release packaging 15; retired compatibility bridge 1. Rust updater tests pass (see test output in isolated validation log). Full regression still required.

Pending: extension-free Pi automatic delivery (native interactive external inbox not established), native role-skill acceptance and remaining legacy packaging/evaluation cleanup, PM role/window and restricted manual commands, and full release verification. The asynchronous question asks whether Pi automatic delivery remains a release blocker; no answer means the original requirement stays in force. Existing Pi extension remains in this unfinished source candidate until a replacement decision is established; this candidate MUST NOT be installed.

Installed application, global CLI/skills, active sessions and original checkout have not been changed by this implementation pass. The isolated checkout contains the prior dirty source snapshot plus these new changes, not a clean published release.


Role-skill implementation checkpoint (same isolated checkout): lead/PM Markdown
is stored under the app-private roles directory. Lead launch supplies the role
through Claude add-dir, Pi --skill, OpenCode skills.paths, or a Codex developer
instruction reference. Codex config/read probes preserved root/project native
instructions without a model turn; modern profile composition and four-harness
native role-discovery acceptance are still open. Setup, roster refresh, update,
off and uninstall no longer write/delete global skill files; forced cleanup is
also covered by a global-file canary. CLI/UI installation regressions pass 61/61;
private installation/role tests pass 21/21. Failed role preparation suspends the
unopened tab and sends no pane request (focused regression 1/1). Tasks 165/166
remain unchecked until native acceptance and legacy packaging/eval cleanup.

Protection check: SHA256 matches the recorded baseline for installed Info.plist,
installed cf.mjs, installed channels.js and the global Codex ConsensFlow skill.
No restart, installation, global cleanup, ConsensFlow dispatch or commit was run.

Final Node regression checkpoint for this source pass: `node --test
--test-concurrency=2 tests/*.test.mjs tests/engine/*.test.mjs` exited 0:
1178 total, 1173 passed, 5 skipped, 0 failed. Browser layout suite previously
passed 82/82; updater Rust subset passed 7/7. This does not close native
role/Pi/PM acceptance or VERIFY-PANE-171, and no installed-instance test was run.


### Current implementation evidence: Pi preparation and harness diagnostics

| Slice | RED | GREEN / refactor |
|---|---|---|
| TEST-PANE-161, private preparation subset | `node --test tests/pi-install.test.mjs`: missing src/pi-install.js; opening subset then failed on absent piExtension status. | 4/4 pass; formatting then rerun passes. Conditional Pi detection, immutable private files with resolved imports, unchanged global settings, idempotence and errors. |
| TEST-PANE-161, launch wiring subset | `node --test --test-name-pattern='Pi lead loads' tests/ui-panes.test.mjs`: path outside private extension directory. | 1/1 pass: real server's Pi lead launch uses prepared private bundle. Native automatic delivery acceptance remains pending. |
| TEST-PANE-172, diagnostics subset | `node --test tests/harness-admin.test.mjs`: missing src/harness-admin.js. Fixture corrected to create its declared HOME before executing native shim. | 3/3 pass; combined diagnostic/preparation refactor run 7/7. Missing harnesses, version parsing, update checks/cache/refresh, offline, invalid id and no false integration OK. |

No tasks checked complete for these subsets: UI controls, live integration
proof, full diagnostic edge matrix and native delivery validation remain.
Official metadata references: https://registry.npmjs.org/@openai/codex/latest,
https://code.claude.com/docs/en/setup, https://opencode.ai/docs/cli/,
https://pi.dev/news/2026/5/7/pi-has-a-new-home and
https://github.com/MoonshotAI/kimi-cli. Update status is informational; the current
prototype compares published release metadata, not a machine's package-manager
channel, and labels that distinction. Distribution-specific resolution remains
required before task 173 is complete.

| TEST-PANE-167, PM identity and authority subsets | New PM identity, credential, watcher and opening tests each failed before implementation. | Focused identity/launch 40 passed; PM watcher and real-server opening tests passed individually. One companion per parent, distinct native launch, PM-only permissions and no automatic worker delivery. Window and manual communication acceptance remain pending. |


### Phase 19 final implementation evidence — 2026-09-09

- PM manual send/read: one scoped call, exact file text, immutable multipart reads,
  native-file-loss recovery and idempotent send; unauthorized worker/admin calls
  refused. Focused PM regression: 287 passed. Browser suite: 85 passed, including
  the PM sibling/window route, separate grid, original terminal palette and admin.
- PM native output routes by window and generation; a destroyed window cannot
  receive a newer PM's output or close its pane. Parent deletion cascades to its
  own companion. Fresh role names are persisted for legacy tabs. Native command
  tests verify authority and independent output; headless PM routing reproduced
  a missing-output RED and now passes without exposing PM output in the main UI.
- Harness diagnostics use current-generation native receipt evidence, not a green
  launch label. Tests cover obsolete/mismatched receipts and publisher versus
  detected Homebrew/native update sources. Pi preparation preserves profiles and
  copies its imports into an immutable private bundle.
- Global generic skill template and eval dependency retired; eval stages use their
  own ConsensFlow root and the bundled role loader. Generated role files are
  updated with the application/roster, without a separate skill-update workflow.
- Full Node run: 1197 passed, 5 gated skips. A later loaded-machine run had one
  one-second Pi editor-probe timeout; the unchanged focused suite passed 20/20.
  Full Rust: 89 unit plus 16 process tests passed. PTY integration: 25/25. Its
  simulator now explicitly uses the generic PTY channel instead of pretending to
  implement Claude's native inbox, and parses the private skill argument correctly.
- Native receipt artifacts: `/tmp/cf-alpha37-native/{claude-code,codex,pi,opencode}.json`.
  Pi's default Go provider hit a weekly quota; the alternative authenticated
  provider reported an invalid OAuth token. These did not erase the worker result:
  the transport's native receipt was verified. OpenCode's free model completed the
  same test after the default Go model entered a provider retry wait.
- Installed app and bundled CLI report 3.0.0-alpha.37. The built real app terminal
  smoke passed. PM packaged input/output and full updater replacement are the
  remaining final acceptance checks, recorded below when complete.

- Packaged PM acceptance passed after a RED that reported no PM echo. Its own
  webview subscribes, renders a real PTY banner, sends text through `pm_command`,
  and observes the child's exact hex echo. This also exercises real Tauri window
  permissions. No main-window terminal or mocked renderer substitutes for PM.
- Packaged updater acceptance passed: open panes blocked installation; after
  their cleanup the signed update replaced an isolated alpha.36 fixture bundle
  with alpha.37, restarted and reported alpha.37 with zero remaining blockers.
- Native Claude restart acceptance passed: retained worker navigation, exact
  same-session `--resume`, whole result through `cf read`, no worker redispatch.
- Final full Node run with bounded test concurrency passed 1197/1202; the three
  missing optional legacy sibling parity checks remain skipped, and the two
  packaged checks passed separately. Final UI 85/85; CLI wording/private-role
  regression 37/37; Clippy passes with warnings denied.

- Closure of VERIFY-PANE-128/143/171: all applicable gates above passed, final
  bundle installed without backups and `cf --version`/`cf doctor` report alpha.37
  from `/Applications/ConsensFlow.app`. Local review replaces another agent pass
  under the accepted plan's explicit no-ConsensFlow/no-other-subagents instruction;
  no independent reviewer is claimed for this final pass. The final reviewed
  boundaries were role authority, window/generation ownership, durable reads,
  native receipt evidence, private installation and updater admission.
- User histories and native settings are preserved. Old generic global files and
  their five obsolete manifest entries were removed manually. No GitHub release,
  rolling feed publication or commit was performed; prepared release artifacts
  are in `/Users/gabrielvoicu/ConsensFlow-Releases/3.0.0-alpha.37/`.

### Final legacy navigation migration — 2026-09-09

The installed alpha.34 data had already lost worker pane rows while retaining
threads. Startup now reconstructs closed navigation rows only for exact surviving
tab ownership and past/current lead generations, using the central pane allocator.
It never starts a worker or rewrites its native binding. Other tabs in the same
folder, deleted tabs, future generations and malformed ownership are excluded.
Reopening a lead does not mark recovered workers running or starting.

Regression: RED missing worker navigation, then GREEN, including idempotent second
restart and unchanged thread bindings. The real-data isolated migration recovered
all seven Fortuna workers with every threads.json byte unchanged; final installed
startup recovered the same seven closed rows. Full Node 1198 passed / 1203 total
(five previously documented skips), UI 85 passed, rebuilt packaged lead/PM smoke
passed. Final bundle is installed and its 55 files match the built artifact.

## Phase 20 — focused geometry and complete session resume [in-progress]

- [x] TEST-PANE-176: Reproduce focused worker height shrinking after grid selection and viewport resize; reproduce resume opening only the lead.
- [x] IMPL-PANE-177: Keep focused cards stretched and attach every bound worker conversation on session resume, with no task resend. Isolate/report individual failures; leave unbound work and PM separate.
- [x] VERIFY-PANE-178: Run UI and lifecycle regressions, package and validate the corrected app, then install without disrupting active work.

RED 2026-09-10: focused card bottom missed stage bottom by 477.8px; resume opened one pane instead of lead plus saved worker. Logs /tmp/cf-focus-red.log and /tmp/cf-resume-red.log.

GREEN 2026-09-10: focused worker fills stage at 720/1000/1100px viewport heights; session resume reattaches both saved workers despite a removed roster agent, skips an unbound conversation, and sends no new task. Node lifecycle/store/page 227/227, browser UI 86/86. Alpha.38 package verification follows; alpha.37 remains running untouched.

Alpha.38 packaged lead/PM real-terminal smoke passed (1/1); session-concurrency real-process tests 3/3; Biome passed. Signed DMG/archive prepared in ~/ConsensFlow-Releases/3.0.0-alpha.38. Installation is pending restart approval because Fortuna currently has 8 open panes. Source fixes complete; VERIFY-PANE-178 remains open until installation.

## Permanent pane removal — Phase 20 extension [in-progress]

- [x] TEST-PANE-179: RED for missing delete command/UI and durable deletion; verify stale generation and lead deletion refuse before killing anything, Escape sends nothing, and native history survives.
- [x] IMPL-PANE-180: Separate Delete pane from reversible Close pane. Confirm using a captured pane ID/generation. Stop only that process and retain a session-scoped deletion record so legacy recovery and resume never recreate it. Sidebar action works for closed panes too. A failed stop remains retryable; project files/native history remain intact.
- [x] VERIFY-PANE-181: Validate deletion, rebuild the pending alpha.38 package, and install when the active-session restart is approved.

RED logs: /tmp/cf-delete-red.log, /tmp/cf-delete-route-red.log, /tmp/cf-delete-ui-red.log. Focused GREEN confirms durable removal and isolated process kill; full regressions and rebuild follow.

Permanent-delete validation: 229/229 backend tests, 88/88 UI tests, 89/89 Rust unit tests. The action is in each non-lead sidebar row, keeping the terminal titlebar compact. Closed rows can be deleted without attach/resume. Native histories are preserved; deletion records prevent automatic restoration and further admission of that conversation in the same session. Alpha.38 installation still awaits the pending restart decision.

Alpha.38 rebuilt with permanent deletion; packaged lead/PM smoke passed and Clippy passed with warnings denied. Signed release artifacts refreshed. Only approved installation remains for VERIFY-PANE-178/181.

## PM in the main window — Phase 20 correction [in-progress]

User supersedes the earlier separate-window requirement: PM appears above the
lead, at the same sidebar level, and selecting it shows one full-size pane in
the main window. It stays out of the parent session grid. Its native conversation,
role skill and manual-only lead send/read remain independent.

- [x] TEST-PANE-182: RED PM was below the lead and absent from the main pane view; verify full-height output/input, selection and retained buffer across switching.
- [x] IMPL-PANE-183: Use the shared main-window terminal registry, place PM first, hide worker/reply controls in PM view; remove PM-only windows, IPC handlers, renderer files and output routing.
- [x] VERIFY-PANE-184: Validate UI/role boundaries and native packaged main-window PM input/output, rebuild alpha.39 and install when restart is approved.

Focused RED /tmp/cf-pm-inline-red.log; GREEN /tmp/cf-pm-inline-green.log.

Installed alpha.38 verified against its signed release archive: all installed files match. User screenshot confirms the permanent-delete sidebar and corrected focused height. VERIFY-PANE-178/181 closed; alpha.39 carries the newer PM main-window change. Remaining progress 183/184.


## Lead conversation changes — Phase 21 [in-progress]

The reply destination is the same ConsensFlow lead pane and generation. A native
conversation change must refresh its identity using process ownership, never cwd
or newest-history guesses. Pending replies follow the new identity; submitted or
uncertain replies retain their original receipt target and cannot replay blindly.

- [x] TEST-PANE-185: Reproduce Claude /clear retaining the obsolete ID; verify complete single delivery to the current pane conversation and exclusion of another process group.
- [x] IMPL-PANE-186: Expose pane process group, validate native registry/process credentials, refresh lead and pending targets atomically, hold zero-byte missing-inbox races for reconciliation.
- [x] VERIFY-PANE-187: Build/install alpha.40 and verify native delivery after conversation change. Cross-harness conversation-change acceptance remains open.

Evidence: /tmp/cf-clear-red.log failed on old versus new session ID; focused
regression green; /tmp/cf-clear-suite.log 85 passed; /tmp/cf-clear-rust.log 89 passed.
Alpha.39 installed with 58 files compared and bundled cf version verified.

Alpha.40 built; /tmp/cf-clear-suite.log 87 passed, Rust 89 passed, packaged smoke 1 passed. Installation awaits the active-session restart answer. Existing failed alpha.39 replies are preserved and are not silently replayed.


### Live matrix follow-up — alpha.40/41
Claude /clear with real OpenCode worker: native receipt accepted in the new lead
conversation. Claude, Pi, and OpenCode leads received complete Claude/Codex/OpenCode
worker tokens. Pi worker weekly quota and Kimi insufficient balance prevent
completed outputs. Pi lead records inbound receipts but has invalid OAuth for
its response model. Codex testing exposed /var vs /private/var path equivalence:
exact launch metadata existed but cwd string equality excluded it.

- [x] TEST-PANE-188: RED Codex workspace symlink alias with exact launch originator.
- [x] IMPL-PANE-189: Canonicalize workspace path comparison while preserving unique launch metadata binding. Session binding suite 26 passed.
- [x] VERIFY-PANE-190: Complete live matrix report and install alpha.41 containing the path correction.

Final alpha.41 acceptance: [live matrix and limits](acceptance-alpha41.md). Installed and reopened; Claude clear and Codex alias live tests passed. 12 native receipt combinations passed; eight provider-blocked cells remain explicitly unverified.

## Automatic role instructions — Phase 22 [completed]

Gabriel approved on 2026-09-10: the complete assigned role must be present in
the first model request after launching or resuming a lead/PM pane, without
asking the user to invoke a skill. Skill discovery alone is insufficient.
Use the existing app-private role documents: Claude appends the role file and
disables old system-prompt snapshot reuse, Pi appends the role text, OpenCode
adds the file to process-local `instructions`, and Codex appends its contents
to the native effective `developer_instructions`. Keep native defaults and
existing configuration, preserve skill discovery, and give workers no role.

Testing uses the existing Node test runner and real temporary files. The Codex
configuration resolver is a subprocess boundary; no model/network calls belong
in the regression suite. Test both roles across all four harnesses, preservation
and invalid OpenCode instruction configuration, worker isolation, and real pane
launch regressions. Build alpha.42, verify the packaged app, then reinstall and
check installed bytes and retained session state. User authorized the restart.

- [x] [TEST-PANE-191] Add full-role startup matrix and preservation regressions in `tests/role-skills.test.mjs`.
- [x] [IMPL-PANE-192] Add native startup instructions in `src/role-skills.js`; update the fake CLI parser and role documentation. → satisfies [TEST-PANE-191]
- [x] [TEST-PANE-193] Reject invalid OpenCode instruction lists before launch; preserve worker isolation.
- [x] [IMPL-PANE-194] Validate the additional instruction-list boundary. → satisfies [TEST-PANE-193]
- [x] [VERIFY-PANE-195] Run regressions, build alpha.42, verify and reinstall the bundle, preserving saved conversations.

### Phase 22 acceptance

- [x] Both roles' complete documents enter native startup context across all four harnesses; Claude resumes rebuild the prompt.
- [x] Existing configuration and instruction content survives; workers receive no role and invalid instruction lists fail closed.
- [x] Role tests and packaged verification pass; alpha.42 is installed and reopened with saved session identities preserved. Broader suite and live-provider limits are recorded below.

### Phase 22 TDD log

| Task | Red | Green | Refactor |
|---|---|---|---|
| [TEST-PANE-191] | `node --test tests/role-skills.test.mjs`: 14 tests, 10 failed, exit 1; all eight harness/role combinations lack full startup instructions. `/tmp/cf-role-startup-red.log`. | — | — |
| [IMPL-PANE-192] | — | `node --test tests/role-skills.test.mjs`: 14/14 passed, exit 0. | Biome formatting; rerun 14/14, exit 0. |
| [TEST-PANE-193] | `node --test tests/role-skills.test.mjs`: 21 tests, 6 failed, exit 1; invalid instruction lists are accepted or fail with an unhelpful iterator error. `/tmp/cf-role-validation-red.log`. | — | — |
| [IMPL-PANE-194] | — | `node --test tests/role-skills.test.mjs`: 21/21 passed, exit 0. | No refactor needed; broader launch run exposed two outdated strict OpenCode config expectations and one lifecycle timing failure. Update expectations to the approved full-role behavior and investigate lifecycle separately. |
| [VERIFY-PANE-195] | — | Role suite 21/21, exit 0; `npm run smoke` 1/1, exit 0; `node --test --test-concurrency=1 tests/integration/session-concurrency.test.mjs` 3/3, exit 0. Alpha.42 build and codesign verification exit 0. | Final review confirms only assigned roles receive full instructions; existing configuration survives; all 50 bundled CLI source files and all 57 installed bundle files match. |

### Phase 22 verification record — 2026-09-10

- Production change: `src/role-skills.js`; tests cover both roles across all four
  harnesses, exact full body inclusion, Codex user instructions with quotes and
  newlines, OpenCode existing instruction/skill configuration and idempotence,
  malformed instruction lists, and no role for workers. OpenCode pane frame
  expectations now include the approved instruction list; fake Claude consumes
  the new native flags without treating them as a user task.
- Native fresh PM checks used no file reads/skill invocations: Claude Code
  2.1.267 and Codex 0.154.0 returned their PM role and exact
  `cf lead send --message-file <file>` command from the role body. Codex recorded
  zero tool calls. OpenCode 1.18.30 produced no output before a 90-second timeout;
  Pi 0.85.1 returned provider weekly-usage-limit HTTP 429. Those probes used
  harness defaults, not Gefjon, and are superseded by the successful reruns below.
- Broad Node run at default concurrency: 1227 total, 1219 passed, three failed,
  five skipped, exit 1. Both timing-sensitive channel/pipe tests passed on an
  isolated rerun (2/2). With concurrency 2: 1221 passed, one failed, five skipped,
  exit 1 (`/tmp/cf-role-full-node-limited.log`). The remaining lifecycle test
  expects one `pane.list` but observes two; it reproduces unchanged with the
  installed alpha.41 role module (`/tmp/cf-role-lifecycle-baseline.log`).
- Integration suite: 24/25, exit 1 at `task44.test.mjs:427` (failed worker still
  alive immediately after reservation release). That test passes individually,
  and the isolated pre-change launcher suite passed 25/25. This remains a
  timing-sensitive broader verification failure, not a claimed green gate.
  No worker lifecycle implementation or assertion was changed in this task.
- `npx biome check` on touched JS/config files exits 0; two pre-existing style
  infos in `tests/ui-panes.test.mjs` remain. Final `git diff --check` exits 0.
- Built using `npm --prefix app run build -- --bundles app --config
  '{"bundle":{"createUpdaterArtifacts":false}}'` (exit 0). Packaged real PTY
  input/output and lead/PM smoke passed; `codesign --verify --deep --strict`
  passed before and after installation. No updater feed or release published.
- Installed `/Applications/ConsensFlow.app`, version and bundled CLI both
  `3.0.0-alpha.42`, reopened PID 59749. All 57 file hashes matched the tested
  bundle. Two saved tab/native-conversation identities were unchanged; startup
  marks their panes closed for explicit resume. The previous bundle and state
  snapshot are retained at `/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-role-install-neulzssb`.

### Gefjon model correction and native acceptance — 2026-09-10

Gabriel pointed out that Gefjon is available through both OpenCode and Pi.
The live roster selects `opencode/muse-spark-1.3-contributor-free`, `xhigh`.
Pi's saved/default Muse route was `opencode-go/muse-spark-1.3-contributor`;
that subscription route caused the previous weekly-limit error. Pi's installed
catalog also contains the free model under provider `opencode`, but the provider
was not shown by `--list-models` without an authentication value. OpenCode's
official provider uses the literal `public` for its unauthenticated free route.

Reran the fresh PM startup probes using the installed alpha.42 role module:

- OpenCode 1.18.30: explicit `--model opencode/muse-spark-1.3-contributor-free
  --variant xhigh`, `run --auto --format json`, process-local role instructions,
  and all tool permissions denied. Exit 0; returned
  `ConsensFlow PM — cf lead send --message-file <file>`; no tool-use events.
  Log: `/tmp/cf-role-gefjon-native.log`.
- Pi 0.85.1: explicit `--provider opencode --model muse-spark-1.3-contributor-free
  --thinking xhigh --api-key public`, no tools, no extensions, ephemeral session,
  and the installed role loader's appended instructions. Exit 0; returned
  `Role consensflow-pm, send: cf lead send --message-file <file>`.
  Log: `/tmp/cf-role-gefjon-pi-native.log`.

All four harnesses now have a successful native fresh PM role-awareness check.
The automated startup matrix covers both PM and lead roles. No provider settings,
credentials, roster entries, or application code were changed by these reruns;
no rebuild/reinstall is needed. This closes the two Phase 22 default-model probe
limitations, not the separate lifecycle/integration assertions above.

Current code and official native documentation were reviewed before and after
implementation. The requested advisor tool and Context7 were unavailable; no
dependency or framework was added. Native prompt mechanisms were checked against
Claude CLI reference, OpenAI configuration reference, OpenCode rules, and Pi's
installed documentation. Existing unrelated workspace edits were preserved.

Progress tally correction: the prior registry used highest task ID 190 as a
count. There are 184 existing task checkboxes, plus five Phase 22 tasks; the
registry now counts actual task checkboxes (acceptance criteria excluded).


## Native-menu updates — Phase 23 [completed]

Gabriel requested removal of the dedicated app-header Updates button because
macOS already offers Check for Updates, and reported a failing check. Live
inspection on 2026-09-10 confirms both configured GitHub rolling feeds return
HTTP 404; published releases contain DMGs only. This is a distribution setup
gap, not a menu dispatch failure. The Stable selection is a saved preference;
alpha builds still default to Alpha when no preference exists.

Scope: remove the duplicate header button and the updater's dependency on it.
Keep the native menu, update notice, quiet scheduling, channel preference,
download verification and explicit guarded install. Describe an unavailable
feed by channel without claiming success, unpublished status from arbitrary
HTTP failures, or falling back across channels. Other updater errors retain
their original details. Document the missing publication prerequisite.

Verification: production UI Playwright tests invoke the native menu event at
the Tauri boundary; Rust exercises actual updater HTTP failures and retry state.
Run the complete app UI suite and focused updater tests, build alpha.43, verify
codesign and packaged behavior, then reinstall under the user's existing
reinstall authorization. Preserve saved conversations. No remote publication.
Advisor and Context7 are unavailable; inspect the installed updater source.

- [x] [TEST-PANE-196] Regress menu-only entry, missing-feed feedback and safe controls.
- [x] [IMPL-PANE-197] Remove the header button and decouple updater initialization. → satisfies [TEST-PANE-196]
- [x] [TEST-PANE-198] Exercise real updater HTTP errors with channel-specific feedback and retained retryability.
- [x] [IMPL-PANE-199] Explain unavailable feeds and document the publication prerequisite. → satisfies [TEST-PANE-198]
- [x] [VERIFY-PANE-200] Verify, build and reinstall alpha.43; record menu behavior and live feed limitation.

### Phase 23 acceptance

- [x] No dedicated header Updates button; native menu still opens the dialog and checks.
- [x] Missing feeds remain failures with useful channel context; download/install stay unavailable.
- [x] UI/updater checks pass and alpha.43 is installed with saved identities preserved.
- [x] Public feed availability is explicitly reported separately from local verification.

### Phase 23 TDD log

| Task | Red | Green | Refactor |
|---|---|---|---|
| [TEST-PANE-196] | Updater Playwright: 11 passed, 1 failed, exit 1; duplicate header button remains. `/tmp/cf-update-menu-red.log`. | — | — |
| [TEST-PANE-198] | Rust regression cannot compile because `describe_check_error` is absent, exit 101. `/tmp/cf-update-feed-red.log`. Live production feeds separately reproduced HTTP 404. A control run with the old error mapping compiled and failed the expected message assertion (0/1, exit 101), `/tmp/cf-update-feed-control-red.log`. | — | — |
| [IMPL-PANE-197] | — | Updater Playwright 12/12, exit 0. `/tmp/cf-update-menu-green.log`. | Removed the obsolete element lookup and click handler; menu event and banner drive the same dialog. |
| [IMPL-PANE-199] | — | Rust updater 8/8, exit 0; includes actual HTTP 404/503, malformed JSON schema, error state and retry. `/tmp/cf-update-feed-final.log`. | Rustfmt; original-behavior control failed the new assertion; restored implementation passes. Other error details are retained. |
| [VERIFY-PANE-200] | — | App UI 89/89; Rust updater 8/8; Node updater 16/16; packaged smoke 1/1; all exit 0. Alpha.43 build/codesign/install passed. | Scoped Biome and diff checks pass. Installed header verified in the native window; all 57 installed file hashes match the tested bundle. |

### Phase 23 verification record — 2026-09-10

- The native `check-updates` event is exercised through the actual frontend,
  with only the Tauri command/event boundary mocked. All 12 updater scenarios
  pass without a header button, including repeated menu opening and errors;
  the full app UI suite is 89/89 (`/tmp/cf-update-ui-all.log`). Native menu
  construction and its event dispatch are unchanged. The installed header was
  separately inspected through accessibility and a screenshot; its Updates
  button is absent (`/tmp/cf-update-installed.png`). A physical post-install
  menu selection was not exercised while the app remained in the background.
- Rust updater tests: 8/8 (`/tmp/cf-update-feed-final.log`), including real
  404/503 replies, malformed metadata diagnostics, retry state and signed archive
  tampering. The old mapping was restored temporarily as a control and failed
  the new assertion; the final implementation was then restored and passed.
- Node release/compatibility tests: 16/16 (`/tmp/cf-update-node.log`).
  `npm run smoke`: 1/1, real packaged UI, Tauri bridge, Node and PTY
  (`/tmp/cf-update-smoke.log`). Build exit 0 (`/tmp/cf-update-build.log`).
- Installed version and bundled CLI: `3.0.0-alpha.43`; all 57 installed file
  hashes match the tested build; deep strict codesign verification passes.
  Existing state had zero saved tabs before replacement. Its complete JSON
  and the saved Stable channel preference match their backups after restart.
  Previous bundle and snapshots:
  `/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-update-install-wagwljvz`.
  The initial background launch exited before final inspection; relaunch by
  app name was verified as `/Applications/ConsensFlow.app/Contents/MacOS/app`,
  PID 95091, with the expected native window and production UI.
- Live GitHub inspection: repository public; both `update-alpha/latest.json`
  and `update-stable/latest.json` return HTTP 404. Existing releases have DMGs,
  no signed updater archive/feed. The local change explains unavailable feeds;
  it does not supply remote releases or make online discovery operational.
- No GitHub release, commit, GitHub push, or NAS push was performed. Gabriel
  asked about publication and NAS during verification; answered that only the
  local build/reinstall had been done. No publication request was inferred.
- Final review: updater initialization no longer depends on a DOM button;
  missing-feed mapping uses Tauri's typed error, preserves other diagnostics,
  and retains failed-check state. Channel policies, signature validation and
  guarded explicit installation remain intact. No dependency was added.


## Publish alpha.43 — Phase 24 [active]

Gabriel explicitly authorized the GitHub release, updater publication and NAS
push on 2026-09-10 ("do it" following the local-only release report). This
supersedes previous no-commit/no-publication instructions for this release.
Publish the current alpha.43 source and matching installed bundle, including
pending standalone-pane changes since the last source commit. Exclude generated
test-results and local runtime data. No force pushes or unrelated ref changes.

Prepare a DMG and signed updater archive from the exact tested/installed app.
The official signer consumes the existing external key; never print or copy
private key material. Publish immutable v3.0.0-alpha.43 assets before the
rolling update-alpha/latest.json. Stable must not serve an Alpha prerelease.
Verify remote asset hashes, feed metadata and signature plus both Git remote
heads. Record remaining test limitations explicitly. No application-code changes
are planned; reuse existing release checks and run packaged upgrade acceptance.

- [x] [PREP-PANE-201] Prepare and validate exact alpha.43 artifacts and source snapshot.
- [x] [VERIFY-PANE-202] Run release regressions and packaged alpha.42-to-alpha.43 upgrade.
- [ ] [RELEASE-PANE-203] Commit/push matching source to NAS and GitHub, publish version and Alpha feed, verify remotely. ← current

### Phase 24 evidence

Preparation in progress. The complete app UI (89), focused Rust updater (8),
Node updater (16) and packaged startup smoke (1) passed for this same bundle
in Phase 23. Both remote main branches currently end at 3dd52bd; local HEAD
1b7a890 is a descendant. Signing public key matches the app's pinned key.

Preparation verified: exact installed bundle copied to
`~/ConsensFlow-Releases/3.0.0-alpha.43`; archive and mounted DMG match its
contents; codesign passes. Tauri signed the archive using the existing external
key, and minisign-verify independently verified it against the app's pinned
public key. Metadata helper validates latest.json and a SHA256SUMS file records
all four artifacts. No private key material is included in release assets.

Fresh release checks: Rust 90 unit + 16 headless passed; integration 25/25;
packaged updater alpha.42-to-alpha.43 passed with signature checks, active-pane
refusal, install and restart. Full Node: 1221 passed, one existing lifecycle
assertion failed (pane.list count 2 versus 1, tests/lifecycle.test.mjs:345), five
skipped. The published prerelease notes explicitly disclose this limitation.
Logs: `/tmp/cf-release43-{node,rust,integration,upgrade,clippy}.log`.
