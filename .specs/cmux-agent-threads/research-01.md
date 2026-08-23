# Research 01 — cmux-mode agent threads

Date: 2026-08-23. All findings verified against the running machine and the
current tree at `aa7d77b`, not from memory.

## 1. What exists today

`cf run @name "<task>"` builds a packet, spawns the harness CLI as a
subprocess, streams its JSON events back, and writes a run directory. Every
run is **stateless by construction** — each harness is launched with its
session machinery explicitly disabled.

`hosts/lib/runners.js:28 buildRunnerInvocation()`:

| kind | current args (abridged) | session killed by |
|---|---|---|
| `pi` | `--mode json --no-session --no-extensions --thinking <e> --tools … -p <preamble>` | `--no-session` |
| `claude-code` | `-p <preamble> --output-format stream-json --verbose --no-session-persistence --dangerously-skip-permissions` | `--no-session-persistence` |
| `codex` | `exec --json --ephemeral --skip-git-repo-check --ignore-user-config --ignore-rules --dangerously-bypass-approvals-and-sandbox -C <cwd> -` | `--ephemeral` |
| `opencode` | `run --auto --format json --dir <cwd> --file <packet> <preamble>` | (no session requested) |

Confirmed by `grep -riE "resume|session_id|continue|thread"` over
`hosts/lib/runners.js` and `hosts/lib/workflows.js`: **zero matches**. Nothing
resumes, reuses or references a session anywhere in the engine.

Consequence: a follow-up question reaches an agent that has never heard of the
first one. Continuity today is manufactured entirely by the lead re-sending
context through `--handoff-file`, and the provider re-reads it cold each time
(no prefix reuse, so no prompt-cache hit).

## 2. Every harness already supports sessions

Verified with `--help` on the installed binaries (initial attempts used
`timeout`, which does not exist on macOS — those runs were grepping the string
"command not found" and proved nothing; re-run without it):

| harness | resume flags |
|---|---|
| claude | `-r, --resume [session-id]`, `-c, --continue`, `--fork-session` |
| codex | `codex exec resume <id>` / `--last`, `codex exec fork <id>` |
| pi | `--continue`, `--resume`, `--session <path\|id>`, **`--session-id <id>` — "creating it if missing"** |
| opencode | `-c, --continue`, `-s, --session <id>`, `--fork` |

Two acquisition models, and every harness fits one:

- **We mint the id** — pi only. `--session-id <id>` creates the session if it
  does not exist, so a deterministic id per (workspace, agent) needs no capture
  step at all.
- **The harness mints it, we capture it** — claude, codex, opencode. Confirmed
  present in the streamed JSON by grepping the engine fixtures:

  | fixture | field |
  |---|---|
  | `tests/engine/fixtures/claude-stream-json.sample.jsonl` | `session_id` |
  | `tests/engine/fixtures/codex-exec-json.sample.jsonl` | `thread_id` |
  | `tests/engine/fixtures/opencode-timeout.sample.jsonl` | `sessionID` |
  | `tests/engine/fixtures/pi-mode-json.sample.jsonl` | none — not needed, we mint |

## 3. The seam for "the lead sees it"

`hosts/lib/state.js:279 recordLatestRun(cwd, result)` already persists
`latestRunId`, `latestRunDir`, `latestAgentId`, `latestKind` via `saveCurrent`.
Every run already writes `packet.md`, `stdout.txt`, `stderr.txt`,
`transcript.md` and `result.json` into
`~/.consensflow/workspaces/<key>/runs/<runId>/`.

So the run directory is already the shared state between a pane and the lead.
No daemon is required for the lead to read an answer produced elsewhere — only
a read verb over what is already written.

## 4. Pane ↔ conversation binding

The cmux-mode skill (shipped in `aa7d77b`) already instructs one pane per
agent, titled for the agent, reused across consults. Sessions keyed by
`(workspace, agent)` therefore share the *same key* as the pane title. No new
binding mechanism is needed — the agent name is the join.

## 5. Prior art: bb (`/Applications/bb.app`, v0.39.0, MIT, get-bb/bb)

Read from the live SQLite schema (read-only, structure only) and the public
repo.

- Architecture: host daemon + server, `better-sqlite3`, `node-pty`,
  TypeScript monorepo.
- `threads` is a durable row: `provider_id`, `model_override`,
  `reasoning_level_override`, `status`, `parent_thread_id`, `source_thread_id`,
  `archived_at`, `pinned_at`, `last_read_at`, `latest_attention_at`, plus
  `queued_thread_messages`, `thread_sections`, and FTS tables over content.
- **No named-agent concept at all** — no `providers`/`presets`/`agents` table.
  `provider_id` is a bare string (`codex`, `claude-code`); both live threads
  have empty `model_override` and `reasoning_level_override`. The coarsest
  equivalent is `project_execution_defaults`.
- **How bb gets prompt caching**: it does not implement it. Searching the repo
  for `cache_control` returns only HTTP headers for static assets and
  marketplace icons. The mechanism is structural —
  `plugins/provider-codex/src/bridge/app-server-connection.ts` says the bridge
  *"supervises one app-server child per bb thread, a reusable"* connection over
  JSON-RPC 2.0. One long-lived agent process per thread keeps the harness's own
  session alive, so the provider sees the same prefix and its cache applies.
- bb ships `plugins/provider-acp` — ACP is **one provider among several**, not
  the shell. This matches the conclusion of the 2026-08-12 ConsensFlow research
  ("ACP should be an optional runtime/session/event adapter").

**Read across to this spec**: the provider's prompt cache keys on the
conversation prefix, not on our process. Resuming a session id therefore buys
the cache benefit without supervising long-lived children — which is the part
that forces a daemon. We pay a per-run re-init cost that bb does not; we avoid
a supervisor, crash recovery and a schema.

## 6. Risks and open trade-offs

1. **Unbounded growth.** Each follow-up re-sends the whole conversation.
   Cached, not free. Needs a deliberate way to start fresh and probably an
   automatic cutoff.
2. **State escapes the one root.** Session files live in each harness's own
   store (`~/.claude/…`, `~/.codex/sessions/…`), which ConsensFlow does not own
   and `cf off` / `cf reset` will not remove. This bends the documented
   "One root, one meaning" invariant in `CLAUDE.md`.
3. **Concurrency.** Two runs against one session id at once can corrupt it.
   Needs a lock or a refusal.
4. **"Leaves no trace" ends.** `--ephemeral` / `--no-session` is why consults
   currently leave nothing behind on the machine.
5. **Session loss is silent.** If a harness prunes or a user deletes its
   session store, a stored id becomes stale. Resume must degrade to a fresh
   session rather than failing the run.
6. **Effort/model drift.** A resumed session was created with a model and
   effort. If the roster row changes (e.g. `cf agent sync` moves a preset to a
   newer model), resuming into the old session is a silent mismatch.

## 7. Test infrastructure (existing, to be reused)

- Runner: `node --test` via `npm test`; `npm run check` = biome + tests.
  284 tests at `aa7d77b`.
- Isolation: `tests/helpers.mjs` `tempEnv()` builds throwaway `HOME`,
  `CONSENSFLOW_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`,
  `PATH`, `CONSENSFLOW_BIN_DIR`.
- Harness CLIs are **stub shell scripts on a fake PATH**; git is a PATH shim
  copying a fixture tree. No live agent CLIs, no network — an invariant in
  `CLAUDE.md` and enforced throughout.
- Engine fixtures under `tests/engine/fixtures/*.jsonl` are recorded real
  outputs, already containing the session-id fields this spec needs.
- Modules never read `process.env`; the environment is an explicit argument
  everywhere. Any new state helper must follow this.

## 8. Open questions for the interview

Carried into `interview-01.md`.
