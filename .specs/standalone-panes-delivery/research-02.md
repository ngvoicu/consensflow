# Research 02 — what the round-1 answers commit us to (2026-09-06)

> **Authority:** SPEC.md revision 3 and the round-2 answers govern. Sections
> marked SUPERSEDED or AMENDED below record the first design; where any other
> line here disagrees with SPEC.md, SPEC.md wins. Known stale points after
> revision 3: §5 says the app's variables are "stripped by `childEnv` for
> every agent it spawns" — true for workers' harness children, but a LEAD
> keeps its credentials (launch roles, SPEC.md); §6's "Node is the only
> reader/writer" became one app-wide queue with an instance lock; §7's
> phase numbers refer to revision 1 (P1/P2 now gate Phase 2, P3 Phase 4,
> P4 is deferred with Windows, P5/P6 gate only their channels).

Round 1 answers, verbatim in `interview-01.md`. This note records what each
one forces, so the spec's tasks can be read against a reason.

## 1 "Full answer" → a bounded body, a gate, and a fallback — SUPERSEDED 2026-09-06 (round 2)

> Round 2: Gabriel chose **no cap** ("the lead absolutely must read
> everything"), astraeus's finding 7 stood, and the gate below was found
> insufficient (finding 1). Revision 2 of SPEC.md replaces this section:
> the whole answer is delivered inline when the lead harness admits the
> paste, else as a file the lead is told to read in full; readiness comes
> from the lead's own transcript plus silence plus no human draft; every
> delivery is a record with a receipt. The text below is kept as the
> record of the first design.

The user chose the full answer over the one-line notice research-01 §5.4
recommended. The two risks that recommendation rested on do not go away, so
the design absorbs them:

- **Bound.** One cap for everything that reaches a lead: `ANSWER_LIMIT`
  (8000, `bin/cf.mjs:1538`). A body over the cap is cut at the cap and ends
  with a pointer line — `[…N more characters — cf catchup <name>]` — the
  same shape `boundedAnswer` already prints.
- **Gate.** A body is written into the lead's PTY only when that PTY has
  been silent for `idleMs` (default 3000) — a lead mid-task streams a spinner
  and tool output, an idle lead prints nothing. Silence is measured in Rust
  from the bytes the PTY produced; nothing reads the screen. Deliveries queue
  per pane while the lead is busy; a queue is drained in order.
- **Never retry blind.** A write either returns (the bytes reached the PTY)
  or fails (the pane is gone). There is no "did it submit" check that pastes
  again — that is how six copies landed in one kimi pane (CLAUDE.md, before
  its deletion, 2026-08-27 entry; `bin/cf.mjs:563-577`).
- **Fallback.** The body is multi-line, and research-01 §5.3 found the
  TUI-side hazard: CSI-u encoded carriage returns inside paste brackets are
  dropped by Claude Code's paste tokenizer. So the probe list below carries
  "multi-line bracketed paste + separate `\r` submits" **per lead harness**,
  and a harness that fails it delivers a one-line notice instead of the body.
  Notice, never silence.

## 2 "cmux disappears" → one path, migrated, in one late phase

`MODES` becomes `['claude', 'pi', 'standalone']` and `ALIASES` becomes
`{ cmux: 'standalone' }` (`src/mode.js:37-40`, reversed). A `mode.json`
holding `cmux` reads as `standalone` — nobody re-chooses a mode because a
word moved. Rows holding a cmux `surface` id are never matched by the app's
pane table, so `liveWindowElsewhere`'s replacement fails open on them exactly
as it fails open today (`bin/cf.mjs:503-508`). The three cmux seams
(research-01 §2.2), the skill's cmux section (`src/skill.js:198-378`), the
three cmux-only tests, and every `cmux` string in `evals/` go in ONE phase,
after the app can open a pane and deliver — a rename before that teaches
commands that do not exist yet (research-01 risk 17).

## 3 "auto" → default auto, downgraded per harness by the probe — AMENDED (round 2)

> Round 2: `auto` means every completed worker reply, including replies to
> the human. There is no notice downgrade; a harness whose probe fails at
> every paste size gets the `pty-file` channel or a native channel, and a
> delivery nothing can admit stays `pending`, visible.

`notify` defaults to `auto` for a new conversation. The downgrade in §1 is
keyed on the **lead's** harness — that is the PTY being written — recorded
in one table (`DELIVERY` in `hosts/lib/delivery.js`), filled by
`findings-01.md`. Until a harness is probed it is `notice`, which is the
auto behaviour with a one-line body, so nothing is silently off.

## 4 "whatever is cleaner" → the roster editor stays in an iframe

Cleaner is the one that keeps "one implementation of the editor"
(`app/src-tauri/src/lib.rs:7-15`): the page on the Tauri origin frames
`http://localhost:<port>/?token=…` for the claude/pi views and for the roster
panel inside standalone. The frame needs no CORS: it is a navigation, not a
`fetch`. It needs the probe "an `http://localhost` iframe loads inside a
`tauri://` page under the existing ATS exception" (`tauri.conf.json:43`).

## 5 "every pane is in ConsensFlow" → the app is the workspace

There is no lead outside the app. A **tab** is a directory plus one lead
pane; a lead is started from the page (directory picker, harness picker),
and every consult it opens lands in the same tab. A worker's delivery target
is its tab's lead pane — recorded on the row as `requester`, the pane id the
consult was requested from — so "Send to lead" always has a target and a
human-started worker is not a special case: the human starts it from a tab,
and the tab has a lead.

Consequence for `cf`: outside the app there is no standalone consult.
`cf run` in standalone mode reads `CONSENSFLOW_APP` and
`CONSENSFLOW_APP_TOKEN` from its environment — set by the app on every pane
it opens, stripped by `childEnv` for every agent it spawns
(`hosts/lib/runners.js:249-256`) — and refuses without them, naming the
app. No token file on disk: a file is readable by an agent's shell, an
environment variable it was never given is not.

## 6 Who talks to whom (the decision research-01 left implicit)

```
page (tauri://)  ── Tauri IPC (invoke, Channel) ──►  Rust
Rust             ── JSON-lines on the existing stdin/stdout pipe ──►  Node (cf ui)
cf (in a pane)   ── HTTP + token, named operations only ──►  Node
```

- **Node** is the only process that reads or writes `threads.json` and the
  harness stores (one writer: research-01 risk 10 closed), decides delivery,
  and serves `cf`.
- **Rust** owns PTYs, byte timing (idle detection, the paste-then-`\r`
  write, `write_when_idle`), and the page's data feed.
- **The page** never fetches Node's `/api` directly (no CORS to declare);
  what it needs from Node arrives through Rust.

The pipe's first stdout line stays the handle line, which
`tests/ui.test.mjs:17-50` pins; the JSON-lines stream begins after it.

## 7 The probes — gates, not a phase

Each recorded in `findings-01.md` with the exact command, the harness
version and the observed result. None can be settled from a document.

| # | Probe | Gates |
|---|---|---|
| P1 | A multi-line body in bracketed paste, then a separate `\r`, submits — per harness: claude, codex, pi, opencode, kimi | Phase 4 (delivery), and the `DELIVERY` table's `body` vs `notice` per lead harness |
| P2 | The TUI enables the kitty keyboard protocol; a raw `\r` still submits when it has | Phase 4 |
| P3 | An `http://localhost:<port>` iframe loads inside a `tauri://` page under the existing ATS exception | Phase 3 (page) |
| P4 | `portable-pty` 0.9.0 reads a clean byte stream on Windows (wezterm#6783) | Phase 5 (packaging); pin 0.8.1 or patch ConPTY flags if it fails |

| P5 | opencode's TUI server accepts a message into the running session (astraeus finding 1) | Phase 3 |
| P6 | a pi extension delivers `sendMessage` as a follow-up when the agent is idle (astraeus finding 1) | Phase 3 |

Claude probes cannot run before the session limit resets (22:20
Europe/Athens, 2026-09-06). Windows is native only (Gabriel, 2026-09-06);
codex runs natively there in PowerShell with the Windows sandbox, per
OpenAI's docs — research-01 §7.3's "WSL-only" was wrong.
