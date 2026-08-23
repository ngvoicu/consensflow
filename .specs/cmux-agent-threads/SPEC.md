---
id: cmux-agent-threads
title: Agent Threads — named, resumable conversations
status: completed
created: 2026-08-23
updated: 2026-08-23
priority: high
tags: [threads, sessions, cmux, runners]
---

# Agent Threads — named, resumable conversations

## Overview

Today every `cf run` is a fresh process: each harness is launched with its
session machinery explicitly disabled (`--no-session`, `--no-session-persistence`,
`--ephemeral`), so a follow-up reaches an agent that has never heard of the
first question. Continuity is faked by the lead re-sending `--handoff-file`,
which the provider reads cold every time — no conversation, no prompt-cache hit.

This spec makes a consult resumable by **remembering the harness's own session
id** and passing it back on the next run. The harness owns the session; we only
remember which one. Sessions carry short human names (`bubble-sky`), so one
agent can hold several conversations at once and each is addressable — "ask
ares in bubble-sky about xyz". In cmux mode each session gets its own pane.

No daemon, no database, no supervised children. 16 tasks across 5 phases.

## Acceptance Criteria

- [x] A second `cf run @ares "…"` in the same workspace continues the first
      conversation — the harness receives the session id the first run created
- [x] `--new` starts a fresh conversation and reports its new name
- [x] `--session <name>` addresses one specific conversation
- [x] Two named sessions for the same agent run without interfering
- [x] A stale or pruned session id degrades to a fresh session; the run still
      exits 0 and says a new conversation was started
- [x] `cf sessions` lists name, agent, run count and last-used per workspace
- [x] `cf last <name>` and `cf last @agent` print the last answer
- [x] Threads default ON in cmux mode and OFF in claude/pi mode; `--thread` and
      `--no-thread` override in any mode
- [x] Session names are two hyphenated words, unique per workspace, and never
      equal to a roster agent name
- [x] ConsensFlow never writes to or deletes a harness's own session store
- [x] `cf off` leaves sessions intact; `cf reset` removes them with the root
- [x] The cmux-mode skill tells the lead one pane per **session**, titled with
      the session name
- [x] `npm run check` exits 0 with no live agent CLI and no network in tests

## Architecture

```
cf run @ares "question"  --new | --session <name> | (default: current)
        │
        ▼
 hosts/lib/threads.js ── reads/writes ──►  <config>/workspaces/<key>/threads.json
        │                                   { "bubble-sky": {
        │  resolve (agent, name) →            agent: "ares", kind: "pi",
        │  harness session id                 sessionId: "…", runs: 3,
        ▼                                     createdAt, lastRunAt, lastRunId } }
 buildRunnerInvocation(agent, packet, cwd, session)
        │
        ├─ pi        --session-id <id>          (we mint the id)
        ├─ claude    --resume <id>              (captured from session_id)
        ├─ codex     exec resume <id>           (captured from thread_id)
        └─ opencode  --session <id>             (captured from sessionID)
        │
        ▼
   spawn ──► JSON stream ──► capture session id if new ──► store
        │
        ▼
   run dir: packet.md · transcript.md · result.json   ← cf last reads this
```

The run directory remains the only shared state between an agent pane and the
lead. That is what replaces a daemon.

## Testing Architecture

### Test Framework & Tools

| Tool | Choice | Purpose |
|---|---|---|
| Test runner | `node --test` (built in) | All tests |
| Lint/format | biome | `npm run check` gate |
| Harness CLIs | stub shell scripts on a fake PATH | No live agent CLI, ever |
| Fixtures | recorded `*.jsonl` under `tests/engine/fixtures/` | Real stream shapes |

### Isolation Strategy

| Layer | Approach |
|---|---|
| Thread store | Real files under a throwaway `CONSENSFLOW_HOME` (`tempEnv()`) |
| Invocation building | Pure function assertions — no spawn |
| Session-id capture | Replay recorded fixture lines; no process |
| End-to-end run | Stub CLI on a fake PATH that echoes a chosen session id |
| Network | None. No test may reach the network |

Modules never read `process.env`; the environment is an explicit argument
everywhere, which is what keeps `tempEnv()` airtight. New helpers follow it.

### Coverage Targets

| Metric | Target |
|---|---|
| New modules (`threads.js`, name generator) | every exported function exercised |
| Each harness resume path | one test per harness, all four |

### Test Commands

| Command | Purpose |
|---|---|
| `npm test` | Run all tests |
| `npm run check` | biome + tests — the gate |

## Library Choices

| Need | Choice | Rationale |
|---|---|---|
| Session persistence | the harness's own store | It already exists and is the only thing the provider's cache keys on |
| Our state | one JSON file per workspace | Package is zero-dependency; a DB would re-adopt what v3 deleted |
| Name generation | built-in `crypto` + an inline word list | No dependency for two random words |

## Phase 1: Session store and naming [completed]

- [x] [TEST-THR-01] `tests/threads.test.mjs` — the store round-trips: unknown
      workspace reads `{}`; `saveThread`/`loadThreads` persist name → `{agent,
      kind, sessionId, runs, createdAt, lastRunAt, lastRunId}`; writes are
      atomic (tmp+rename, mirroring `state.js`); a corrupt file reads as `{}`
      rather than throwing. `tempEnv()` for isolation.
- [x] [IMPL-THR-02] `hosts/lib/threads.js` — `threadsPath(cwd)`,
      `loadThreads(cwd)`, `saveThread(cwd, name, record)`,
      `removeThread(cwd, name)`. Stored at
      `<config>/workspaces/<key>/threads.json`, beside `runs/`.
      -> satisfies [TEST-THR-01]
- [x] [TEST-THR-03] `tests/threads.test.mjs` — `newSessionName(existing,
      agentNames)` returns `word-word`, never collides with an existing session
      name in that workspace, never equals a roster agent name, and is drawn
      from concrete vocabulary (asserted: not in the mythology list used by
      presets). 200 draws produce no collision and no invalid shape.
- [x] [IMPL-THR-04] `hosts/lib/threads.js` — inline two-word vocabulary
      (weather/colour/nature/texture), `crypto.randomInt` selection, retry on
      collision, throw after a bounded number of attempts.
      -> satisfies [TEST-THR-03]

## Phase 2: Resuming each harness [completed]

- [x] [TEST-THR-05] `tests/engine/runner-session.test.mjs` — with no session,
      `buildRunnerInvocation` produces today's one-shot flags exactly
      (`--no-session`, `--no-session-persistence`, `--ephemeral`). With a
      session it produces, per kind: pi `--session-id <id>` and NO
      `--no-session`; claude `--resume <id>` and NO `--no-session-persistence`;
      codex `exec resume <id>` and NO `--ephemeral`; opencode `--session <id>`.
      Billing guards (`dropEnv`) and permission flags are unchanged in both
      shapes.
- [x] [IMPL-THR-06] `hosts/lib/runners.js` — `buildRunnerInvocation(agent,
      packetPath, cwd, session)` takes an optional `{sessionId}` and branches
      per kind. -> satisfies [TEST-THR-05]
- [x] [TEST-THR-07] `tests/engine/runner-session.test.mjs` —
      `extractSessionId(kind, parsedLine)` returns the id from each recorded
      fixture: claude `session_id`, codex `thread_id`, opencode `sessionID`;
      pi returns null (we mint). Unknown/garbage lines return null without
      throwing.
- [x] [IMPL-THR-08] `hosts/lib/runners.js` — `extractSessionId`, called from
      the existing `onStdoutLine` path so capture costs no extra parse.
      -> satisfies [TEST-THR-07]

## Phase 3: Threading a run [completed]

- [x] [TEST-THR-09] `tests/cli.test.mjs` — with a stub CLI that echoes a
      session id: first `cf run @zeus "q"` creates a session, prints its name,
      and stores the captured id; a second run passes that id to the child
      (asserted from the stub's recorded argv); `--new` creates a second
      session with a different name; `--session <name>` targets it; `--thread`
      and `--no-thread` override the mode default.
- [x] [IMPL-THR-10] `bin/cf.mjs` + `hosts/lib/workflows.js` — resolve the
      session before the run (default current / `--new` / `--session`), pass it
      into `runAgent`, capture and persist the id after, increment `runs`.
      -> satisfies [TEST-THR-09]
- [x] [TEST-THR-11] `tests/cli.test.mjs` — a stored session the harness no
      longer knows: the stub exits non-zero on resume, and the run retries once
      as a fresh session, exits 0, says a new conversation was started, and the
      stale record is replaced.
- [x] [IMPL-THR-12] `hosts/lib/workflows.js` — resume-failure detection and
      one fresh-session retry. -> satisfies [TEST-THR-11]

## Phase 4: Reading a conversation from the lead's pane [completed]

- [x] [TEST-THR-13] `tests/cli.test.mjs` — `cf sessions` lists name, agent,
      runs and last-used for the workspace and prints a clear line when there
      are none; `cf last <name>` prints the last answer plus the transcript
      path; `cf last @agent` resolves that agent's current session; `--json`
      emits machine-readable output; an unknown name exits non-zero naming the
      sessions that do exist.
- [x] [IMPL-THR-14] `bin/cf.mjs` — `sessionsVerb()` and `lastVerb()` reading
      `threads.json` and the recorded run dir; help text for both.
      -> satisfies [TEST-THR-13]

## Phase 5: Mode default and the skill [completed]

- [x] [TEST-THR-15] `tests/skill.test.mjs` + `tests/mode.test.mjs` — cmux mode
      defaults threads on and host modes off; the cmux skill says one pane per
      **session** titled with the session name, teaches `--new` and
      `--session`, and still states that a pane is a window and not a memory
      for the un-threaded case; claude/pi skills mention neither panes nor
      sessions.
- [x] [IMPL-THR-16] `src/skill.js` + `src/mode.js` — mode-aware prose replacing
      the pane-per-agent wording, and the per-mode threading default.
      -> satisfies [TEST-THR-15]

---

## Resume Context

> Complete. All 16 tasks green, all acceptance criteria met.
> Verified end to end outside the suite: `cf run @hyperion "…"` printed
> `conversation: amber-glade`, the second run passed `exec resume thread-real`
> to the child, `cf sessions` showed `amber-glade @hyperion 2 runs`, and
> `cf last @hyperion` returned the answer.
> Final gate: `npm run check` exit 0, 325 tests, 322 pass, 0 fail.

## Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-08-23 | Resume the harness's own session rather than build a session layer | The provider's prompt cache keys on the conversation prefix, not on our process. bb gets caching by supervising one child per thread; resuming an id gets the same benefit without a daemon (research-01 §5) |
| 2026-08-23 | Sessions carry two-word names, unique per workspace | User's idea, and it dissolves the concurrency problem: several conversations per agent, each addressable, no lock and no refusal |
| 2026-08-23 | Vocabulary deliberately not mythology | Agent names are mythological; a session name must never be mistakable for an agent name |
| 2026-08-23 | `--new` is the only reset | User's call. No age or run-count cutoff. Cost of an unbounded thread is visible to the user, who can start a new one |
| 2026-08-23 | Never clean a harness's session store | Same rule that stopped us writing Claude Code's `settings.json`: other tools' state is not ours. Documented as a deliberate dent in "one root, one meaning" |
| 2026-08-23 | Resume even when the roster row has drifted | User's call. Consequence: an answer may come from the model the session was created with, not the one `cf agent list` shows |
| 2026-08-23 | Capability in every mode, defaulted on only in cmux | Keeps one engine and one test surface; the mode decides a default, not a mechanism |
| 2026-08-23 | Panes follow sessions, not agents | Follows from named sessions; reverses the pane-per-agent prose shipped in `aa7d77b` |

## TDD Log

| Task | Red | Green | Refactor |
|---|---|---|---|
| [TEST-THR-01] | `node --test tests/threads.test.mjs`: 1 test, 1 failed — `ERR_MODULE_NOT_FOUND: Cannot find module 'hosts/lib/threads.js'` | — | — |
| [IMPL-THR-02] | — | `node --test tests/threads.test.mjs`: 11 passed, 0 failed | Exported `writeJsonAtomic` from `state.js` and dropped the duplicate tmp+rename in `threads.js` — one atomic writer for the engine. Re-ran green: 11/11 |
| [TEST-THR-03] | `node --test tests/threads.test.mjs`: `does not provide an export named 'allSessionNames'` | — | — |
| [IMPL-THR-04] | — | 16 passed, 0 failed | none — generator is 30 lines |
| [TEST-THR-05] | `node --test tests/engine/runner-session.test.mjs`: `does not provide an export named 'extractSessionId'` | — | — |
| [IMPL-THR-06] | — | 11 passed, 0 failed | Restored pi's original argv order so the one-shot shape stays byte-identical — two existing tests in `claude-core` pin it |
| [TEST-THR-07] | (same red as THR-05 — one file) | — | — |
| [IMPL-THR-08] | — | `npm run check`: 311 tests, 308 pass, 0 fail | none |
| [TEST-THR-09] | `node --test tests/cli.test.mjs`: 44 tests, 6 failed — `unknown option '--thread'` | — | — |
| [IMPL-THR-10] | — | 44 passed, 0 failed | Fixed two flaws in my own tests, not the code: the argv log leaked between cases, and the stub emitted a constant id so "the id changed" could never be true |
| [TEST-THR-11] | (same red as THR-09) | — | — |
| [IMPL-THR-12] | — | 44 passed, 0 failed | none |
| [TEST-THR-13] | `node --test tests/cli.test.mjs`: 50 tests, 6 failed — `unknown command "sessions"` | — | — |
| [IMPL-THR-14] | — | `npm run check`: 323 tests, 320 pass, 0 fail | Spread order in `--json`: `...result` was overwriting `agent` with the whole agent row |
| [TEST-THR-15] | `node --test tests/skill.test.mjs`: 17 tests, 2 failed — no `one pane per conversation` | — | — |
| [IMPL-THR-16] | — | `npm run check`: 325 tests, 322 pass, 0 fail | Escaped a backtick that closed the PAGE template literal (third time that trap has bitten this session) |

## Deviations

| Task | Spec Said | Actually Did | Why |
|---|---|---|---|
| [TEST-THR-01] | Isolate with `tempEnv()` | Local `withTempHome()` setting `process.env.CONSENSFLOW_HOME` | The spec's Testing Architecture note ("modules never read `process.env`") holds for `src/`, not for `hosts/lib`: `state.js:22 configHome()` reads `process.env.CONSENSFLOW_HOME` directly, so `tempEnv()`'s env object does not isolate it. Mirrors `withTempDir()` in `tests/engine/claude-e2e.test.mjs`. Later engine-side tasks (THR-05/07) must use the same shape |
| [IMPL-THR-02] | `threads.js` owns its atomic write | Exported `writeJsonAtomic` from `state.js` and reused it | Refactor step removed a duplicated tmp+rename. One atomic writer for the engine |
| [IMPL-THR-16] | Only add cmux prose | Also deleted two tests written earlier the same day | They asserted `one pane per agent` and `a pane is a window, not a memory` — both deliberately replaced by this spec. Superseded, not softened; the new assertions are stricter |
| [IMPL-THR-10] | `--thread` opt-in flag | Also treat `--session`/`--new` as implying it | Naming a conversation is already a request to thread; requiring `--thread` alongside would be ceremony |
