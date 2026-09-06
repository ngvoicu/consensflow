# Research — ConsensFlow owns the panes, and delivers results to the lead

Spec ID: `standalone-panes-delivery` · researched 2026-09-05 · against
`3.0.0-alpha.21` (`package.json:3`), commit `03ec499`.

Every codebase claim carries a `file:line`. Every external claim carries a URL,
fetched 2026-09-05. Where a claim could not be settled without running a live
agent CLI — which this research was told not to do — it is marked
**PROBE IN PHASE 1** rather than asserted.

> **Line numbers are as of 2026-09-05 and the working tree was dirty.**
> `git status` during this research showed uncommitted edits to `src/skill.js`
> (384 → 392 lines), `tests/skill.test.mjs` (391 → 398), `evals/scenarios.mjs`,
> `evals/harness.mjs`, `evals/run.mjs` and `CLAUDE.md`/`AGENTS.md`, plus the
> deletion of several `.specs/` files. `src/skill.js` moved by five lines
> *during* this session — its "a follow-up is the question itself" paragraph was
> at 302 in the first read and 307 in the last. Citations below are re-anchored
> to the working tree, not to `03ec499`. Re-check any `src/skill.js`,
> `tests/skill.test.mjs` or `evals/` line before relying on it.

**The three findings that should change the plan:**

1. **The recommended emulator is four months old and its open issues are
   exactly TUI issues.** wterm's first release was 2026-04-30 and 0.5.0 landed
   2026-09-04, one day before this was written. Its built-in core ignores
   DECSTBM, ignores mouse tracking, and clamps resize at 256×256 — see §4.3.
   xterm.js should ship; wterm belongs behind the same interface.
2. **Writing into the lead's PTY mid-turn is a documented way to derail it.**
   Claude Code's queue flushes at the next tool boundary, not at end of turn,
   and injected text "often gets injected mid-task, derailing ongoing work".
   That is an argument for delivering a one-line *notice*, not the answer —
   see §5.
3. **The window loads an external origin, so Tauri IPC is not available to
   it.** `WebviewUrl::External` at `app/src-tauri/src/lib.rs:137` puts the page
   on the Node server's origin. This forks the architecture three ways and is
   the decision the spec turns on — see §4.1.

---

## 1 Project map

### 1.1 What ships

| Path | Owns | Lines |
|---|---|---|
| `bin/cf.mjs` | every verb: `run`, `catchup`, `attach`, `chat`, `sessions`, `mint`, `last`, `ui`, `doctor`, `agent …`, `skills …`, `use`, `mode`, `off`, `reset` | 1994 |
| `src/ui.js` | the loopback roster editor — one HTTP server, one inline page, one bearer token | 1022 |
| `src/skill.js` | SKILL.md generation; the cmux prose is `src/skill.js:198-378` | 392 |
| `src/mode.js` | the one-path-per-machine invariant, `MODES`, `scopeTargets`, `applyMode`, `turnOff`, `resetEverything` | 355 |
| `src/roster.js` · `src/catalog.js` · `src/harnesses.js` · `src/install.js` · `src/manifest.js` · `src/sync.js` · `src/terminal.js` · `src/host-payloads.js` | roster, presets, harness detection, hash-manifest install, launcher | 340/91/161/168/43/217/186/133 |
| `hosts/lib/runners.js` | `buildRunnerInvocation`, `runAgent`, `interactiveStart`, `interactiveResume`, `childEnv` | 610 |
| `hosts/lib/harness-transcript.js` | read-only readers for all five harness stores, plus three session discoverers | 640 |
| `hosts/lib/threads.js` | the conversation row: names, `leadId` | 172 |
| `hosts/lib/state.js` | one config root, workspace keys, `writeJsonAtomic` | 297 |
| `hosts/lib/packets.js` | `createPacket` (one-shot) and `createWindowSeed` (a window's first message) | 117 |
| `app/` | Tauri v2 window around `cf ui` — 186 lines of Rust, one static HTML error page | — |

### 1.2 Two hard rules the new work inherits

**Zero runtime dependencies.** `package.json:28-30` lists one devDependency
(`@biomejs/biome`) and no `dependencies` key at all. `files`
(`package.json:14-20`) ships `bin`, `src`, `hosts`, `skill`. Anything vendored
into `src/` ships inside a zero-dependency npm package — so a terminal emulator
vendored there would put a WASM VT engine in the CLI. The emulator belongs under
`app/`, which has its own `package.json` (`app/package.json`) and is not
published.

**Modules never read `process.env`.** The environment is an explicit argument
everywhere; `tests/helpers.mjs:10-26` builds throwaway homes on that basis. Two
sanctioned exceptions predate the rule: `hosts/lib/state.js:21-23`
(`configHome` reads `CONSENSFLOW_HOME`) and `hosts/lib/codex-auth.js`. New code
takes `env` as an argument.

### 1.3 The registry

`.specs/registry.md` lists four completed specs; this one needs a fifth row.
The house research format is `.specs/kimi-harness/research-01.md`: a
requirements table first, then the probes that produced it, verbatim.

---

## 2 The consult path today, in sequence

A lead in a cmux pane consults `@nyx`:

1. **The lead reads the skill.** `src/skill.js:218-247` gives five commands:
   `cf mint @<name>` → `cmux new-pane` → `cmux send` the consult line →
   `cmux rename-tab` → `cf sessions` to confirm the launch was real.
2. **`cf run` refuses to run in a pipe.** `bin/cf.mjs:818-831`: cmux mode, not
   `--json`, not a terminal ⇒ refused, with the pane recipe in the refusal.
   `isTerminal()` (`bin/cf.mjs:424-427`) is `process.stdout.isTTY`, overridable
   by `CONSENSFLOW_TTY` for tests.
3. **An agent may not spawn agents.** `bin/cf.mjs:716-720` bails on
   `CONSENSFLOW_CHILD === '1'`; the same guard is in `chatVerb`
   (`bin/cf.mjs:1082`) and `attachVerb` (`bin/cf.mjs:1177`).
4. **The conversation is resolved.** `resolveConversation`
   (`bin/cf.mjs:257-312`) is the single rule: `--session` names one, `--new`
   mints one, otherwise the lead's own most-recent row for that agent.
   Ownership comes from `leadId(env)` (`hosts/lib/threads.js:166-172`), reading
   `CLAUDE_CODE_SESSION_ID`, then `CMUX_SURFACE_ID`, `ITERM_SESSION_ID`,
   `TERM_SESSION_ID` (`hosts/lib/threads.js:148`). `null` matches no row.
5. **The window opens.** `wantsWindow` (`bin/cf.mjs:861`) = threading ∧ cmux ∧
   not `--json` ∧ a terminal. `openWindow` (`bin/cf.mjs:554-660`) then:
   - a known `sessionId` ⇒ `liveWindowElsewhere` check, then `interactiveResume`;
   - claude / pi ⇒ mint an id (`randomUUID()` / the conversation's own name) and
     `interactiveStart`;
   - opencode / codex ⇒ `interactiveStart` cold, with a discovery loop running
     *alongside* the window (`bin/cf.mjs:614-655`);
   - kimi ⇒ `false` at `bin/cf.mjs:578`; the caller streams turn one and the
     pane becomes its window after.
6. **The row is written before the window opens.** `saveWindowRow`
   (`bin/cf.mjs:466-491`) records `agent`, `kind`, `lead`, `surface` (from
   `env.CMUX_SURFACE_ID`, `bin/cf.mjs:478-479`), `sessionId`, `createdAt`,
   `lastRunAt`. It spreads `...record`, so `seen` and `startedAt` survive.
7. **The terminal is handed over.** `handOver` (`bin/cf.mjs:1235-1244`) spawns
   with `stdio: 'inherit'` and `childEnv(process.env, invocation)`.
8. **The lead reads back.** `cf catchup <name> --unread`
   (`bin/cf.mjs:1254-1414`) reads the harness's own store via `harnessTurns`
   (`hosts/lib/harness-transcript.js:22-42`) — never the pane's screen.

### 2.1 The invariants any replacement inherits

| Invariant | Where | Cost of breaking it |
|---|---|---|
| One window per conversation | `bin/cf.mjs:509-534`, refusal text `bin/cf.mjs:536-546` | two processes writing one session store, two half-conversations |
| A conversation belongs to the lead that started it | `hosts/lib/threads.js:150-172`, `bin/cf.mjs:300-310` | a new session inherits a stranger's conversation (live, 2026-08-24) |
| An agent never gets pane control | `hosts/lib/runners.js:249-256` — `childEnv` deletes every `CMUX_SOCKET*` key and `CMUX_CLAUDE_HOOK_CMUX_BIN` | a child could type into any pane, the lead's included |
| An agent never spawns agents | `bin/cf.mjs:716`, `1082`, `1177` | recursion |
| The screen is never read | `src/skill.js:265-272` | ANSI codes, wrapping, no completion signal |
| The harness's store is read-only | `hosts/lib/harness-transcript.js:15-21` | corrupting somebody else's live database |
| Named operations only over HTTP | `src/ui.js:166-170` | an endpoint that runs a supplied command is a remote shell |
| Reading the terminal multiplexer fails OPEN | `bin/cf.mjs:503-508` | a check that can invent a mistake is worse than none |

### 2.2 The exact cmux seams to replace

Three, and only three, places know cmux exists:

| Seam | Line | Replacement |
|---|---|---|
| `CMUX_SURFACE_ID` as a lead identity | `hosts/lib/threads.js:148` | add `CONSENSFLOW_PANE_ID` to `LEAD_KEYS`, ranked below `CLAUDE_CODE_SESSION_ID` for the same reason the comment at `hosts/lib/threads.js:140-147` gives |
| `CMUX_SURFACE_ID` recorded as `surface` | `bin/cf.mjs:478-479` | the app's pane id, same field |
| `cmux tree` shelled out for liveness | `bin/cf.mjs:509-534` | a `GET` to the app's pane API, keeping the fail-open contract at `bin/cf.mjs:503-508` verbatim |

Plus one that must be *extended*, not replaced: `childEnv`
(`hosts/lib/runners.js:249-256`) strips `CMUX_SOCKET*` so an agent cannot type
into the lead's pane. **If the app hands `cf` a token by environment variable,
that variable must be stripped in exactly the same place** — otherwise an agent
in a pane holds the key to the lead's pane, which is the same hole with a new
name.

### 2.3 `threads.json` already has two writers; the app is a third

`hosts/lib/threads.js:23-25` puts it at
`<CONSENSFLOW_HOME>/workspaces/<key>/threads.json`; every write goes through
`writeJsonAtomic` (`hosts/lib/state.js:92-97`) — tmp file plus `rename`.

`rename` is atomic but there is **no compare-and-swap**: `saveThread`
(`hosts/lib/threads.js:42-46`) is read-modify-write, so two writers racing lose
one update. The mitigation already exists and should be copied, not reinvented:
`recordTurn` (`bin/cf.mjs:405-408`) re-reads the row instead of spreading a
stale snapshot, and its comment says why — a rebuild from a literal wiped the
`seen` marks.

### 2.4 What "a turn finished" means today

There is no event anywhere. `--wait` (`bin/cf.mjs:1291-1345`) polls:

- `pending(list)` = the list is empty, or its last turn is `role: 'user'`
  (`bin/cf.mjs:1300`).
- Not pending ⇒ a grace window (`CONSENSFLOW_WAIT_GRACE_MS`, default 4000 ms,
  `bin/cf.mjs:1304`) watches for a newer *user* turn, because a fast lead can
  start `--wait` before its own question has reached the store.
- Then a 2-second poll to a 15-minute deadline (`bin/cf.mjs:1315-1319`).
- The thread row is re-read every round (`bin/cf.mjs:1295-1297`) because an
  opencode session id can land after the window opened.

**The delivery watcher is this loop made continuous.** Not a new detector: the
two races it survives were paid for live, twice.

`startedAt` is *not* the signal. `markRunning` (`bin/cf.mjs:441-464`) sets it,
`recordTurn` clears it, and `saveWindowRow` never sets it at all — which is why
a window conversation shows no "working since" in `cf sessions`
(`bin/cf.mjs:1474`).

### 2.5 The `seen` marks, and why delivery must move them

`readMark` / `markRead` (`bin/cf.mjs:379-391`) keep a per-lead turn count under
`seen`. The rule at `bin/cf.mjs:1355-1362`: the mark advances only when what was
printed starts at or before it. A lead that cannot be named gets no mark.

**Delivery must move this mark**, or the lead's next `--unread` re-shows what
was just pushed into its own pane. The app knows the lead pane's environment, so
it can compute `leadId` and write through the same shape. This is a
one-line-of-reasoning bug that will otherwise be found in production.

---

## 3 The app today

186 lines of Rust and one static HTML page. It is a window, not an editor.

| Fact | Where |
|---|---|
| Runs the bundled Node against the bundled CLI, consulting nothing on the machine | `app/src-tauri/src/lib.rs:40-65` |
| Sidecar at `resources/binaries/node`, else beside the executable | `app/src-tauri/src/lib.rs:45-55` |
| Spawns `node cf.mjs ui --json --no-open` | `app/src-tauri/src/lib.rs:84-95` |
| Reads one JSON handle line for `{url, token}` | `app/src-tauri/src/lib.rs:109-123` |
| Rewrites `127.0.0.1` → `localhost` for macOS ATS | `lib.rs:119-122`; exception domain at `tauri.conf.json:43` |
| Builds the window AT the address, never navigates after | `lib.rs:130-145`; `WebviewUrl::External` at `:137` |
| A Finder-launched `.app` has almost no PATH, so it asks the login shell | `lib.rs:67-78` — `$SHELL -lc 'printf %s "$PATH"'` |
| The editor dies with the app | `lib.rs:177-185` (`RunEvent::Exit`), and from the child's side the stdin pipe at `lib.rs:91` + `src/ui.js:386-395` (`isPipe(0)` accepts FIFO **or** socket, because Node's `'pipe'` is a socketpair on macOS) |
| Only `core:default` capabilities | `app/src-tauri/capabilities/default.json:8-10` |
| Adhoc signing plus the two JIT entitlements V8 needs under hardened runtime | `tauri.conf.json:44-45`, `entitlements.plist:12-15` |
| CSP is null | `tauri.conf.json:22-24` |
| `frontendDist: "../ui"` is a 37-line error page | `tauri.conf.json:7`, `app/ui/index.html` |
| Tauri 2.11.3, tauri-build 2.6.3, CLI ^2.11.4; deps are only `serde`, `serde_json`, `log`, `tauri-plugin-log` | `Cargo.toml:18-25`, `app/package.json:17` |
| Bundle targets `app` + `dmg` only | `tauri.conf.json:28-31` |
| **No Rust tests exist**, and no test harness under `app/src-tauri` | — |
| `healOnOpen` runs **once per launch**, before the page is served — it claims the `cf` launcher and brings the installed skill up to this version | `src/ui.js:374`, `src/sync.js:192` |

One note on `healOnOpen` for a long-lived pane app: it fires once, in
`serveUi`, and its result is stashed in a module-level `opened`
(`src/ui.js:158-162`) that `systemState` reports. Opening a pane must **not**
re-trigger it. "Opening the app IS the deliberate act" is a statement about
launching the app, not about every interaction inside it; re-healing per pane
would rewrite a user's edited skill file mid-session, which is the one write
this project insists on saying out loud.

### 3.1 The build scripts and what they assume

`app/scripts/prepare-sidecar.mjs` downloads a pinned official Node (`v26.7.0`,
`:27`) and copies `bin/ src/ hosts/ skill/` into `src-tauri/resources/cli`
(`:66-80`). `TRIPLES` (`:29-34`) covers darwin arm64/x64 and linux x64/arm64 —
**no Windows entry** — and shells out to `curl`, `tar`, `chmod` (`:58`, `:61`,
`:87`).

`app/scripts/sync-cli.mjs` mirrors the staged CLI with `rsync -a --delete`
(`:65-66`) into the built bundle and then into whatever copy `cf` on PATH runs,
found via `terminalRuntime(process.env)?.entry` (`:76-81`). Both the path
(`:42-53`) and the tools are macOS-shaped.

---

## 4 Pane ownership — options compared

### 4.1 The fork nobody has picked yet: which origin owns the page

The window loads an **external** origin (`app/src-tauri/src/lib.rs:137`). Tauri
exposes its IPC — `invoke`, `ipc::Channel`, events — to the app's own origin.
The old v1 escape hatch `dangerousRemoteDomainIpcAccess` **does not exist in
Tauri v2**; it was replaced by a `remote` field on a capability
([v2.tauri.app/reference/config](https://v2.tauri.app/reference/config/)):

```json
"remote": {
  "urls": ["https://*.tauri.app"]
}
```

Tauri warns about it directly: *"On Linux and Android, Tauri is unable to
distinguish between requests from an embedded `<iframe>` and the window
itself"*
([v2.tauri.app/security/capabilities](https://v2.tauri.app/security/capabilities/)).
And external-URL IPC has a live class of bugs: the embedded webview omits the
`Origin` header on IPC requests, so Tauri's scope checker refuses to dispatch
([tauri#15190](https://github.com/tauri-apps/tauri/issues/15190),
[tauri#8476](https://github.com/tauri-apps/tauri/issues/8476),
[tauri#5088](https://github.com/tauri-apps/tauri/issues/5088)).

So there are four shapes, not one:

| # | Shape | IPC works? | Cost |
|---|---|---|---|
| **A** | PTY in Rust; pane UI served from `frontendDist` (Tauri origin); **roster editor kept as-is in an `<iframe>` on the Node origin** | yes, natively | the pane API needs a home outside HTTP — see §4.1.1 |
| B | PTY in Node (`node-pty`); page stays wholly on the Node origin | not needed | a native addon; see §4.2 |
| C | PTY in Rust behind its own loopback HTTP/WS listener; page stays on the Node origin | not needed | two servers, two tokens, two lifetimes |
| D | Keep the external page and grant it IPC via a capability `remote.urls` | fragile | the Origin-header bugs above, plus the Linux iframe warning |

**Recommendation: A.** It is the only one where streaming bytes to the webview
is a first-class, supported path (`tauri::ipc::Channel`, which takes
`InvokeResponseBody` and so carries binary efficiently —
[docs.rs/tauri Channel](https://docs.rs/tauri/latest/tauri/ipc/struct.Channel.html)),
and the only one that adds no second token. Crucially it *preserves* the rule
the app was built on — "there is one implementation of the editor, not two"
(`app/src-tauri/src/lib.rs:7-15`) — because the roster editor keeps being the
Node server's own page, merely displayed in a frame instead of the whole window.
No CORS is needed: an iframe is a navigation, not a `fetch`, and `src/ui.js`
sends no CORS headers today (`src/ui.js:236-239` writes only `content-type`).

**PROBE IN PHASE 1:** that an `http://localhost:<port>` iframe loads inside a
`tauri://` page under the existing `exceptionDomain` (`tauri.conf.json:43`).
ATS is configured for the domain, not the frame type, so this should hold — but
"should" is what §3's ATS note already cost once.

#### 4.1.1 Where the pane API lives under A — the cost the table used to hide

The brief's pane API (`cf` asks the app to open a pane, write into one, list
them, ask whether one is alive) has to be reachable **from a `cf` process in a
terminal**, which is not the webview and cannot use Tauri IPC at all. Under A
the only HTTP server is Node's, and the PTYs are in Rust. So something has to
bridge them, and the obvious-but-wrong answer is a second loopback listener in
Rust — which is exactly the "two servers, two tokens" cost charged to option C.

**There is already a private channel between those two processes, and it is
carrying nothing.** Rust holds the editor's `stdin` as a pipe
(`app/src-tauri/src/lib.rs:91`) purely as a liveness signal, and Node reads it
for the same reason and nothing else (`src/ui.js:390-395`). Node writes exactly
one line to `stdout` — the handle line — and Rust reads exactly that one line
(`app/src-tauri/src/lib.rs:109-123`).

So: make it a JSON-lines protocol. Node's HTTP endpoint receives the named
operation from `cf` with the token it already checks (`src/ui.js:241-246`),
forwards it as one line up `stdout`, and reads the reply back down `stdin`.

- No second port, no second token, no CORS.
- The pane API stays *named operations only* (`src/ui.js:166-170`), because it
  is the same server enforcing the same rule.
- It costs the handle-line reader becoming a loop, and `cf ui --json`'s stdout
  becoming a stream rather than one line — a contract `tests/ui.test.mjs:17-50`
  already pins and would need extending, not rewriting.

Alternative, if that coupling is unwanted: Rust owns a second listener and the
app hands `cf` its address and token by environment variable — in which case
that variable **must** join the strip list in `childEnv`
(`hosts/lib/runners.js:249-256`), or an agent in a pane holds the key to the
lead's pane. That is risk 8, and it is the whole reason `CMUX_SOCKET*` is
stripped today.

#### 4.1.2 What does not have to change

`isTerminal()` is `process.stdout.isTTY` (`bin/cf.mjs:424-427`), and **a child
spawned into a PTY passes it**. So `wantsWindow` (`bin/cf.mjs:861`), the whole
of `openWindow` (`bin/cf.mjs:554-660`), `handOver` (`bin/cf.mjs:1235-1244`) and
the piped-run refusal (`bin/cf.mjs:818-831`) keep working unchanged inside an
app-owned pane. The app opens a pane running `cf run @name "<task>" --new
--session <name>`; from `cf`'s point of view nothing is different from a cmux
pane except which environment variable names the pane. **The window path is not
being rewritten** — only the three seams in §2.2 are.

### 4.2 PTY: Rust `portable-pty` vs Node `node-pty`

| | `portable-pty` 0.9.0 | `node-pty` 1.1.0 |
|---|---|---|
| License / source | MIT, wezterm ([crates.io](https://crates.io/crates/portable-pty)) | MIT ([npm](https://www.npmjs.com/package/node-pty)) |
| Maturity | 14M downloads; 9.7M on 0.9.0 alone | powers VS Code |
| Build | pure Rust, in the existing `cargo` build | **native addon**: `binding.gyp` + `nan` |
| API | `native_pty_system()`, `openpty(PtySize{rows,cols,pixel_width,pixel_height})`, `CommandBuilder::new/arg/cwd/env`, `slave.spawn_command`, `master.try_clone_reader`, `master.take_writer`, `resize` | `spawn`, `onData`, `write`, `resize`, `kill` |
| Windows | ConPTY, with caveats — see below | ConPTY, with a dedicated build variant |

**`node-pty` is the wrong choice here for three concrete reasons**, all of them
this project's own constraints:

1. It is a native addon built against a specific Node ABI, and the app ships a
   *pinned* Node it downloads itself (`prepare-sidecar.mjs:27`, currently
   v26.7.0). Every Node bump becomes a rebuild-or-break.
2. macOS hardened runtime is already on (`tauri.conf.json:44-45`), and it
   validates loaded libraries. An unsigned `.node` dylib would need
   `com.apple.security.cs.disable-library-validation` added to
   `entitlements.plist` — widening exactly the entitlement set the file's own
   comment (`entitlements.plist:5-11`) is careful to keep minimal.
3. It would put a compiler in the build of a project whose whole identity is
   "zero-dependency, no build step" (§1.2).

**`portable-pty` has one open Windows regression worth knowing before the
Windows phase:** [wezterm#6783](https://github.com/wezterm/wezterm/issues/6783),
*"pty.read is returning garbage in windows starting with version 0.9.0"* —
0.8.1 works on the same test. Open since 2025-03-11, no maintainer response.
More broadly, the upstream crate does not pass the modern ConPTY creation flags
(`PSEUDOCONSOLE_RESIZE_QUIRK`, `PSEUDOCONSOLE_WIN32_INPUT_MODE`,
`PSEUDOCONSOLE_PASSTHROUGH_MODE`) that Windows 10/11 want for correct resize and
key handling. **PROBE IN PHASE 1 (Windows):** read a byte stream back on
Windows before building anything on top of it.

### 4.3 Emulator: xterm.js vs wterm — challenging the recommendation

The brief proposes wterm with xterm.js as fallback. The evidence points the
other way.

| | `@xterm/xterm` 6.0.0 | `@wterm/dom` + `@wterm/ghostty` 0.5.0 |
|---|---|---|
| License | MIT | Apache-2.0 |
| Age | since 2014; ships in VS Code | `@wterm/ghostty`'s first published version is **0.3.0, 2026-04-30**; the project is a little older (the changelog starts at 0.1.0, undated, and issue #55 is 2026-04-28). 0.5.0 landed **2026-09-04** |
| Rendering | canvas / WebGL addon | DOM — native selection, find, accessibility |
| Size | not measured here | ~12 KB built-in core; **~400 KB** for the ghostty core (project's own figure) |
| Addons | `@xterm/addon-fit` 0.11.0, plus `addon-webgl` and `addon-unicode11`, all from the same repo | none needed — fit is `autoResize`, built in |
| API | `write`, `onData`, `resize`, `dispose`, `loadAddon` | `new WTerm(el, {onData, onResize, autoResize})`, `term.write()`, `term.resize(cols, rows)`, `term.destroy()`, plus a `WebSocketTransport` helper |

Sources: [npm @xterm/xterm](https://registry.npmjs.org/@xterm/xterm),
[npm @wterm/dom](https://registry.npmjs.org/@wterm/dom),
[npm @wterm/ghostty](https://registry.npmjs.org/@wterm/ghostty),
[github vercel-labs/wterm](https://github.com/vercel-labs/wterm).

wterm's DOM rendering is genuinely attractive — native text selection in a pane
is worth real money to this product, because the human is meant to read and copy
from worker panes. But its nine open issues are, disproportionately, the ones a
Claude Code or codex TUI will hit:

- **#97** — `@wterm/core` ignores DECSTBM (`CSI Pt;Pb r`); region-limited
  scrolls grow the grid instead of scrolling. Explicitly noted as affecting TUI
  apps.
- **#55** — mouse tracking and focus-event modes silently ignored.
- **#56** — `Terminal.resize` silently clamps to 256×256.
- **#87** — panel borders in btop/htop do not form straight columns.
- **#85**, **#70** — IME composition leaks a raw character; IME window
  mispositioned.

Several vim-related bugs *are* fixed, and every one of those fixes is in the
**ghostty** core (#88, #86, #83, #78) — which is the read to take from the
changelog: the built-in Zig core is not TUI-complete, and the ghostty core is
the only configuration worth considering. That is also the 400 KB one.

**Recommendation:** define the emulator interface first — `write(bytes)`,
`onData(cb)`, `resize(cols, rows)`, `dispose()` is the whole surface, and both
libraries already match it almost exactly — and **ship xterm.js behind it**.
Keep a wterm adapter as a second implementation and re-evaluate in a quarter.
The interface costs about 40 lines; being wrong about a four-month-old VT engine
costs a release where Claude Code's own TUI renders wrong in ConsensFlow's own
window.

One wterm detail worth carrying over regardless: v0.1.9 strips ESC bytes from
pasted content "to prevent escape sequence injection". Whatever the app writes
into a PTY on the human's behalf should do the same — see §9.

### 4.4 Prior art for PTY → webview

- **VS Code** — `node-pty` in the extension host, xterm.js in the renderer.
  The reference implementation for the pairing, and the reason xterm.js is as
  hardened as it is.
- **tauri-terminal** ([github.com/marc2332/tauri-terminal](https://github.com/marc2332/tauri-terminal))
  — the exact shape recommended here: xterm.js plus `portable-pty` in Tauri.
  Read this one first; it is small and it is the same problem.
- **tauri-plugin-pty** ([crates.io](https://crates.io/crates/tauri-plugin-pty))
  — an off-the-shelf plugin doing spawn-and-transport to xterm.js. Worth
  reading before writing the Rust; probably not worth depending on, since the
  pane lifecycle here is ConsensFlow's own (a pane is a *conversation*, not a
  shell).

Wave Terminal, Tabby and Hyper were named in the brief but **not researched** —
no repo paths are cited here, and none should be quoted from this document.

### 4.5 The webview, per platform

**Linux (WebKitGTK).** *"High input latency or low frame rates can appear in
WebGL-heavy views like terminal emulators, editors, maps, and charts"* is a
documented symptom, and on NVIDIA setups WebKitGTK and the driver can disagree
badly enough to produce blank windows
([Linux Graphics Issues](https://v2.tauri.app/develop/debug/linux-graphics/)).
Consequences: prefer the DOM or canvas renderer over the WebGL addon on Linux,
and expect IME bugs ([tauri#11412](https://github.com/tauri-apps/tauri/issues/11412),
[tauri#8264](https://github.com/tauri-apps/tauri/issues/8264)).

**Windows (WebView2).** Three caveats that touch a terminal UI directly:

- **Some default platform shortcuts are disabled and some are not**
  ([wry#569](https://github.com/tauri-apps/wry/issues/569)) — so which of
  Ctrl-C, Ctrl-V, Ctrl-W reaches the emulator versus the webview has to be
  tested, not assumed. A terminal wants nearly all of them.
- **Browser accelerator keys are a WebView2 setting**
  (`AreBrowserAcceleratorKeysEnabled`,
  [CoreWebView2Settings](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2settings)),
  and turning them off is likely required so the emulator sees them first.
- **Some WebView2 calls deadlock in synchronous commands or event handlers** —
  Tauri's own guidance is to create windows from async commands on separate
  threads. Relevant because a pane is created in response to a UI event.

Clipboard access is enabled by default for the rendered page on Windows and
Linux; macOS needs menu-item accelerators declared for shortcuts to fire.

---

## 5 Delivery to the lead

### 5.1 The four mechanisms, and which survive

| Mechanism | Verdict |
|---|---|
| **Write into the lead's PTY** — the app owns both ends | **the only one that works**, with the hazards in §5.2 |
| **Claude Code `Stop` hook** | fires in the **worker's** session and *"hooks cannot inject messages into different running sessions"* ([hooks docs](https://code.claude.com/docs/en/hooks)). Also, ConsensFlow never writes `settings.json` — a standing rule. Useful only as a **trigger** (a user-installed hook could run `cf notify <name>`), never as the channel |
| **`--resume` with a new message** | spawns a second process on a live session — precisely the one-window-per-conversation violation `liveWindowElsewhere` exists to prevent (`bin/cf.mjs:509-534`) |
| **Pull (`cf catchup`)** | stays, unchanged; it is the fallback whenever push is off or fails |

### 5.1.1 How delivery finds the lead's pane

Push needs a target, and nothing records one today. The loop closes with the
identity work that already exists:

1. `cf run` executes **inside the lead's own pane**, so its environment carries
   that pane's id — the same variable §2.2 adds to `LEAD_KEYS`.
2. `saveWindowRow` (`bin/cf.mjs:466-491`) already records `env.CMUX_SURFACE_ID`
   as `surface` (`:478-479`). Add a second field — the pane the consult was
   *requested from*, not the one it opened in.
3. Delivery writes into that pane.

Two consequences worth stating rather than discovering:

- **A lead outside the app has no delivery target.** A lead running in iTerm or
  a bare terminal, with only its workers in the app, can be identified (through
  `ITERM_SESSION_ID`, `hosts/lib/threads.js:148`) but not written to — the app
  owns no PTY for it. That case must degrade to pull (`cf catchup`), announce
  itself when the conversation is created, and never silently look like `auto`
  is working. This is open question 6.
- **The requesting pane can die.** A lead that closes its pane and opens a new
  one leaves a row pointing at a dead PTY. Delivery must fail open and fall back
  to pull, the same contract as `bin/cf.mjs:503-508`.

### 5.2 What happens when you write into a working Claude Code

This is the risk that should shape the feature, and it is well documented.

Claude Code has a queue, but *"messages queued while Claude is working flush at
the next LLM pause, not at true end-of-turn… Queued messages often get injected
mid-task, derailing ongoing work"*
([gist: YoraiLevi](https://gist.github.com/YoraiLevi/f7c454a0e3a1e206124004241940f972)).
The open feature requests agree: [#49373](https://github.com/anthropics/claude-code/issues/49373)
("Queue messages to send at true end-of-turn, not next LLM pause"),
[#63190](https://github.com/anthropics/claude-code/issues/63190),
[#36817](https://github.com/anthropics/claude-code/issues/36817). The CLI's own
changelog confirms queueing exists and is actively worked on (2.1.251, 2.1.247).

The other harnesses differ, and the differences matter for a per-agent policy:
codex uses `Tab` to queue and `Enter` to steer; Copilot has an explicit
`immediate` vs `enqueue` toggle; opencode shows a queue but currently interrupts
(FR #16102) — same source.

**Gap:** that source covers claude, codex, opencode, Copilot, Cline, Gemini and
Aider. **pi and kimi were not researched for mid-turn stdin behaviour**, and no
source was found for either. Both need the phase-1 probe in §5.3, and until then
their delivery mode should default to `manual`.

### 5.3 Does a programmatic write actually submit? Two sources disagree

**This is the single most important thing to probe, and the report must not
paper over it.**

- The gist asserts: *"tmux `send-keys`, AutoHotkey, AppleScript… Claude Code's
  Ink-based TUI distinguishes physical Enter from programmatic `\r`/`\n`"* —
  i.e. it never submits.
- ConsensFlow's own live finding says the opposite for the shape it uses. The
  skill sends the follow-up as plain words into the live pane
  (`src/skill.js:307-314`), and the project record states that *"claude, codex,
  pi and opencode all submit a send"* (`CLAUDE.md:289`), with kimi the sole
  exception because its TUI takes a send as a paste (`src/skill.js:330-332`,
  `bin/cf.mjs:563-577`).

The repo's evidence is primary, specific and recent, so it wins on the narrow
question. The reconciliation is visible in a third source: it is a **race**, not
an impossibility. *"The Enter may be sent before Claude Code has converted the
bracketed paste into its pending input widget, so the Enter is ignored/swallowed
… if you send the text and the Enter in the same send-keys call — or even fire
Enter immediately after — the Enter gets swallowed"*, and the recommended fix is
to *"wait until the pane reflects a pending pasted input before sending Enter"*
([claude-code#43169](https://github.com/anthropics/claude-code/issues/43169),
[claude-code#30239](https://github.com/anthropics/claude-code/issues/30239)).

**So the delivery write should be shaped as:** `ESC[200~` + body + `ESC[201~`,
then — as a **separate write, after a confirmation or a short delay** — a lone
`\r`. Bracketed paste keeps the body atomic against a human typing in the same
pane (§9), and the separated `\r` avoids the swallow. **PROBE IN PHASE 1**, per
harness: this cannot be settled without running the CLIs.

Note the interaction with §4.3: wterm strips ESC from *pasted* content. The
app's delivery write goes to the PTY directly, not through the emulator's paste
path, so that stripping does not apply — but the two must not be confused in the
implementation.

**The probe must also check the keyboard protocol.** Claude Code runs under
kitty-protocol terminals (changelog 2.1.247 fixes Ctrl shortcuts "in
kitty-protocol terminals"), and [#43169](https://github.com/anthropics/claude-code/issues/43169)
is specifically about CSI-u encoding *inside* paste brackets: *"Claude Code's
bracketed paste tokenizer does not decode CSI-u sequences within paste
brackets, so the encoded carriage returns are silently lost."* So the phase-1
probe has three questions, not one:

1. does the TUI enable the kitty protocol (the emulator will see a `CSI > … u`
   request on startup)?
2. does a raw `\r` still submit when it has?
3. does a newline *inside* the pasted body survive, or collapse?

The safe shape that falls out: keep delivered bodies **single-line** — which
the notice recommendation in §5.4 gives for free.

### 5.4 Recommendation: deliver a notice, not the answer

Push a single short line into the lead's pane:

```
@nyx answered in ares-bubble-sky (3 new turns) — cf catchup ares-bubble-sky --unread
```

Four reasons, each grounded:

1. **A long answer pasted into a TUI is a failure this project has already
   paid for.** `boundedAnswer` (`bin/cf.mjs:1538-1543`) exists because a kimi
   run's output measured 493,390 characters and `cf last` pasted every one of
   them into the lead.
2. **A one-line notice survives a mid-turn queue flush.** If it lands mid-task
   (§5.2), it reads as a note, not as a new instruction that derails the work.
   A 4,000-word review landing mid-task does not.
3. **It keeps the read path the one that already works** — `cf catchup
   --unread`, which handles packet unwrapping (`hosts/lib/harness-transcript.js:204-213`),
   injected-block stripping (`:229-233`), and the `seen` marks correctly.
4. **It avoids the app deciding what "the answer" is.** The app would have to
   pick the assistant turns and truncate them; `harnessTurns` gives structured
   turns but not a summary, and inventing one is a second product.

The full text should still be one click away *in the app* — the pane is right
there, and the human reads it on screen. Push is for the lead, which cannot look
at a screen.

### 5.5 The `auto | manual` setting needs two fields, not one

The requirement is: the human sets it from the UI; the lead may state a
preference at `cf run --notify auto|manual`; **the lead can never override a
human-set value.** That is unimplementable with a single field, because
precedence depends on *who* wrote it. Minimum shape on the thread row:

```
notify:      'auto' | 'manual'      // the effective setting
notifySetBy: 'human' | 'lead'       // who last set it
```

`cf run --notify` writes only when `notifySetBy !== 'human'`. The UI always
writes, and sets `notifySetBy: 'human'`. This mirrors the existing rule that a
row's `lead` is never rewritten by a later turn (`bin/cf.mjs:405-409`).

Both writers must use the spread-and-re-read pattern (`bin/cf.mjs:405-408`,
`saveWindowRow` at `:466-491`), or the first `cf run` after a UI change will
wipe the human's choice — the exact bug the `seen` marks already suffered.

The pane title the brief asks for ("conversation name, agent, delivery mode")
comes free from the same row: `name`, `record.agent`, `record.notify`.

### 5.6 "Send to lead" from a right-click

This is the same write as §5.3 with a human-chosen body, and it is the *safer*
of the two paths: a person picked the moment and the text. It should reuse one
function, with the automatic path passing a generated notice and the manual path
passing the selection. One write path means one place where escaping, bracketed
paste and the separated `\r` are correct.

---

## 6 Store watching

### 6.1 `fs.watch` is not reliable enough to be the only signal

Quoting the Node docs ([nodejs.org/api/fs.html#caveats](https://nodejs.org/docs/latest/api/fs.html#caveats)):

- *"On Windows, no filenames or events are provided in all cases."*
- *"On macOS, `fs.watch` does not work reliably on files until they are first
  opened. On Linux, in certain conditions, renaming or deleting files in a
  watched directory does not generate any events."*
- Inodes: *"if the watched file is deleted and recreated, it is assigned a new
  inode. The watcher will emit an event for the deletion but will continue
  watching the old inode."*
- *"`fs.watchFile()` uses stat polling and does not rely on filesystem events.
  `fs.watchFile()` is more reliable but significantly less efficient."*

Two additional facts from this codebase:

- Four of the five stores are appended JSONL — inode-stable, so watching works
  where events arrive at all.
- **opencode is SQLite with a write-ahead log.** `withOpencodeDb`
  (`hosts/lib/harness-transcript.js:271-297`) opens read-only per call and holds
  nothing between reads, precisely because it is *"somebody else's live database
  with a write-ahead log beside it"*. Watching `opencode.db` will miss writes
  that land in `opencode.db-wal`; there is no file-event answer here at all.

### 6.2 Recommendation: quiescence as the hint, the store as the truth

The app owns the worker's PTY, so it has a signal cmux never gave: **bytes
stopped arriving.** That is not reading the screen — it is noticing there is no
screen activity — so it does not touch the "the screen is never read" rule
(`src/skill.js:265-272`).

So:

1. **Hint** — PTY output has been idle for N seconds (start at 2 s, the same
   figure `--wait` already polls at, `bin/cf.mjs:1316`).
2. **Confirm** — call `harnessTurns(kind, sessionId, env)` and apply the exact
   `pending()` test from `bin/cf.mjs:1300`: the conversation is answered when
   its last turn is `assistant`.
3. **Floor** — a slow poll (5–10 s) for every live conversation regardless, so a
   missed hint costs latency, never the delivery.
4. **Grace** — keep `CONSENSFLOW_WAIT_GRACE_MS` semantics (`bin/cf.mjs:1304`):
   a just-sent question that has not reached the store yet must not be mistaken
   for a finished turn.

`fs.watch` may be added as a fourth wake source, but never as the only one, and
never for opencode.

---

## 7 Windows / Linux blockers

### 7.1 Already branching on Windows

| Concern | Where |
|---|---|
| Launcher script — a `.cmd` shim with `%*` on Windows, `#!/bin/sh` + `exec` elsewhere; names become `cf.cmd` | `src/terminal.js:71-88` |
| Launcher location — `%LOCALAPPDATA%\Programs\ConsensFlow\bin` | `src/terminal.js:43-54` |
| Executable-bit check skipped on win32 | `src/harnesses.js:99-135` |
| App-data directories a reset clears, per platform | `src/mode.js:265-290` |
| `home()` falls back to `USERPROFILE` | `hosts/lib/harness-transcript.js:44` |

### 7.2 Blocks

| Blocker | Where | Fix shape |
|---|---|---|
| `spawn('open', [url])` | `src/ui.js:384` | `start` / `xdg-open`; or never call it — the app already passes `--no-open` |
| Login-shell PATH probe via `$SHELL -lc` | `app/src-tauri/src/lib.rs:73-78` | not needed on Windows; a GUI app inherits the user PATH |
| No Windows sidecar triple | `app/scripts/prepare-sidecar.mjs:29-34` | add `win32-x64 → x86_64-pc-windows-msvc`; the Node archive is `.zip`, so `fetchNode` needs a branch |
| `curl` / `tar` / `chmod` shelled out | `prepare-sidecar.mjs:58,61,87` | Node's own `fetch` plus a zip reader |
| `rsync` and a hardcoded macOS bundle path | `app/scripts/sync-cli.mjs:42-53,65-66` | a Node mirror function; per-platform path |
| Bundle targets are `app` + `dmg` | `app/src-tauri/tauri.conf.json:28-31` | add `nsis`/`msi`, `deb`/`appimage` |
| `env -u ANTHROPIC_API_KEY` in generated commands | `src/skill.js:9` | `env -u` is POSIX and cmd.exe has no equivalent. **App-owned panes remove this**: the app spawns the child itself, where `childEnv` strips keys in-process (`hosts/lib/runners.js:249-256`). A real argument for the feature |
| Every test stub is `#!/bin/sh` | `tests/cli.test.mjs:55-59`, `tests/ui.test.mjs:9-14`, `evals/harness.mjs:50` | `.cmd` stubs, or declare the suite POSIX-only |
| `cmux` itself | all of cmux mode | cmux is macOS/Linux. App-owned panes are what makes Windows possible at all |

### 7.3 Harness availability per platform

| Harness | Windows | Source |
|---|---|---|
| claude | native, confirmed (changelog 2.1.239 ships Windows-specific features) | [changelog](https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md) |
| codex | **not native** — the official quickstart installs "for macOS and Linux"; Windows is documented via WSL and a Windows sandbox page | [learn.chatgpt.com/docs/codex/cli](https://learn.chatgpt.com/docs/codex/cli) |
| pi | Windows x64 binary published, **but requires a bash shell** (Git for Windows) | [pi-mono windows.md](https://github.com/badlogic/pi-mono/blob/main/packages/coding-agent/docs/windows.md) |
| opencode | runs on Windows without WSL | [codeagentswarm guide](https://www.codeagentswarm.com/en/guides/opencode-on-windows) |
| kimi | runs on Windows; first-party PowerShell installer | [codeagentswarm guide](https://www.codeagentswarm.com/en/guides/kimi-code-on-windows) |

Third-party guides are weaker sources than first-party docs; treat opencode and
kimi as "probably yes, verify on the machine". codex's is first-party and clear:
**plan for codex to be WSL-only on Windows.**

---

## 8 Test strategy

### 8.1 What exists

`npm test` is `node --test` (`package.json:22`), no network, no live CLI. Agent
CLIs are stub scripts on a fake PATH (`tests/cli.test.mjs:55-59`); every test
gets a throwaway home (`tests/helpers.mjs:10-26`). `npm run eval`
(`evals/run.mjs`) runs a **real lead** against the **real installed skill** with
`cf` and `cmux` replaced by recording stubs, and asserts which commands it chose
— it spends tokens and sits outside `npm test`.

### 8.2 (a) The Rust PTY layer

There are no Rust tests at all today. Add `#[cfg(test)]` tests in
`app/src-tauri`:

- open a PTY, spawn `sh -c 'printf hello'` (`cmd /c echo hello` on Windows),
  read until EOF, assert the bytes.
- spawn `cat`, write bracketed-paste-wrapped text plus a separate `\r`, assert
  the echo — this is the §5.3 shape under test without any agent CLI.
- resize and assert the child sees it (`sh -c 'stty size'`).
- kill and assert the reader ends.

These need no agent CLI and no network, so they match the house rule exactly.

### 8.3 (b) The pane API contract, from Node

Follow `tests/ui.test.mjs:17-50`: start the server, read its handle line, drive
it over HTTP. For the app's pane API the app is not available in `node --test`,
so invert it — write a **stub pane server** in `tests/` that implements the
contract, and test `cf`'s side against it: that `liveWindowElsewhere`'s
replacement fails open on a refused connection, a non-200, and a moved response
shape (all three are required by `bin/cf.mjs:503-508`), and that `cf run` sends
the right named operation. The contract itself is then one document, tested from
both ends.

### 8.4 (c) Delivery rules — the strongest available test

Make them pure functions over a thread row and a turn list:

- `shouldDeliver(row, turns, leadId)` — auto vs manual, lead-set vs human-set
  precedence (§5.5), already-delivered suppression.
- `deliveryNotice(name, row, turns)` — the exact line pushed.
- `nextSeen(row, leadId, turns)` — the mark after delivery (§2.5).

Fixture-driven, in the style of `tests/engine/harness-transcript.test.mjs` (501
lines of exactly this). No PTY, no app, no timing.

### 8.5 (d) The webview

**There is no UI test setup in this repo at all.** The cheapest honest option is
Playwright against the served page, and the `webapp-testing` skill exists at
`/Users/gabrielvoicu/.claude/skills/webapp-testing/` (confirmed present) to
drive it. Constraints: Playwright must go in `app/package.json`, never the root
one (§1.2), and it can only test the *page*, not the Tauri shell — so it covers
the tiling progression, the sidebar, the mode selector and the context menu, and
covers nothing about PTYs. That is still the 80% worth having, and it should be
its own script (`npm run test:ui` in `app/`), outside `npm test`, for the same
reason `npm run eval` is.

### 8.6 (e) The evals will all break, and that is the point

`evals/scenarios.mjs` asserts the *shape* of `cmux send` lines
(`evals/scenarios.mjs:16-23`, `:75-80`, `:209`, `:292`). Every one of those
assertions is about a command that will no longer exist. New scenarios needed,
each mapped to a failure this design can produce:

1. the lead opens a pane through the app and does **not** run the consult in its
   own pane (the existing scenario 1, re-expressed);
2. a follow-up goes into the existing pane as words, not as a shell line (the
   2026-08-31 lesson, re-expressed);
3. **new** — a lead that has had a result delivered does not then re-read it and
   report it twice (the `seen` interaction, §2.5);
4. **new** — a lead told a conversation is on `manual` does not try to switch it
   to `auto` itself (§5.5).

---

## 9 Risks

| # | Risk | Why it is real | Mitigation |
|---|---|---|---|
| 1 | **Delivery lands mid-turn and derails the lead** | Claude Code's queue flushes at the next tool boundary, not end of turn; injected text "often gets injected mid-task, derailing ongoing work" ([gist](https://gist.github.com/YoraiLevi/f7c454a0e3a1e206124004241940f972)) | deliver a one-line notice, never a body (§5.4); default `manual`, not `auto` |
| 2 | **Delivery lands on a permission prompt and answers it** | a prompt is the user's to answer (a standing rule); a pane can sit on codex's or kimi's directory prompt | residual. Mitigate with an idle-and-no-prompt heuristic on the lead's own output; never claim it is solved |
| 3 | **The `\r` is swallowed and the notice sits unsubmitted** | documented race ([#43169](https://github.com/anthropics/claude-code/issues/43169), [#30239](https://github.com/anthropics/claude-code/issues/30239)); ConsensFlow already has the pane-paste scar (`bin/cf.mjs:563-577`) | bracketed paste, then a separate `\r`; verify delivery by re-reading the store, and **never retry blind** — retrying is what pasted six copies into one kimi pane |
| 4 | **Two writers on one PTY interleave** | the human types while the app writes | bracketed paste makes the body atomic to the TUI; sanitize ESC out of any delivered body, as wterm does for pastes (changelog 0.1.9) |
| 5 | **A long answer is pasted into a TUI** | 493,390 characters once reached a lead (`bin/cf.mjs:1538-1543`) | notice-only; the app's own pane is where the human reads the full text |
| 6 | **Two windows on one conversation** | today's guard shells out to `cmux tree` (`bin/cf.mjs:509-534`) and disappears with cmux | replace with a pane-API query keeping the fail-open contract (`bin/cf.mjs:503-508`) *and* the "the pane you are standing in never counts" rule (`bin/cf.mjs:512`) |
| 7 | **The lead's `seen` mark is not moved after a push** | `--unread` then re-shows what was just delivered; the lead reports it twice | move the mark through the same `markRead` shape (`bin/cf.mjs:384-391`) |
| 8 | **An agent gets the app's pane token** | `childEnv` strips `CMUX_SOCKET*` for exactly this reason (`hosts/lib/runners.js:249-256`) | add the app's variable to the same strip list, in the same function, in the same commit that introduces it |
| 9 | **An agent calls the pane API itself** | `CONSENSFLOW_CHILD` guards `run`/`chat`/`attach` (`bin/cf.mjs:716`, `1082`, `1177`) but nothing else yet | guard every new pane verb the same way; the API is named-operations-only (`src/ui.js:166-170`) with no command passthrough, ever |
| 10 | **Two writers race on `threads.json`** | `saveThread` is read-modify-write with no CAS (`hosts/lib/threads.js:42-46`) | re-read before write everywhere (`bin/cf.mjs:405-408`); consider a single writer in the app once it is long-lived |
| 11 | **Reading opencode's SQLite while opencode writes** | live DB with a WAL | already correct: read-only, opened per call, closed after (`hosts/lib/harness-transcript.js:271-297`). Do not "optimize" it into a held handle |
| 12 | **wterm is too young for agent TUIs** | DECSTBM ignored (#97), mouse modes ignored (#55), resize clamped at 256 (#56), all open | ship xterm.js behind an interface (§4.3) |
| 13 | **ConPTY garbage on Windows** | [wezterm#6783](https://github.com/wezterm/wezterm/issues/6783) open against 0.9.0; modern ConPTY flags not passed upstream | probe on Windows before building on it; be ready to pin 0.8.1 or patch the flags |
| 14 | **Hardened runtime plus a native addon** | signing is on (`tauri.conf.json:44-45`) and library validation would need a new entitlement | choose `portable-pty`, not `node-pty` (§4.2) |
| 15 | **A synced bundle is unverifiable** | `sync-cli.mjs:91-97` — mirroring into a release bundle breaks `codesign --verify` | unchanged by this work; only `npm run build` restores a signature |
| 16 | **Polling CPU with many live conversations** | one store read per conversation per tick | quiescence hint plus a slow floor poll (§6.2); bound by the number of open panes, which is bounded by the screen |
| 17 | **The skill and every eval assume cmux** | `src/skill.js:198-378`, `evals/scenarios.mjs` throughout, and the three cmux describes at `tests/skill.test.mjs:109`, `:137`, `:217` | rewrite together, in one change; a half-migrated skill teaches commands that do not exist |

---

## 10 Open questions for the user

Only what research cannot settle.

1. **Is "standalone" a fourth mode, or the renaming of `cmux`?** Today
   `MODES = ['claude', 'pi', 'cmux']` (`src/mode.js:37`) and `ALIASES` maps
   `standalone → cmux` (`src/mode.js:40`) — so the user's word "standalone" is
   currently the *old* name for the cmux path. Three options, and this is a
   product decision: (a) rename `cmux` → `standalone` and drop cmux support;
   (b) keep both, making four modes, and break the one-path-per-machine
   invariant's simplicity; (c) keep three and make `cmux` mean "app panes",
   which would be a lie in the name. Recommendation: (a), with `cmux` becoming
   the alias that `standalone` is today — but the cost is that anyone running
   the cmux path loses it.

2. **Notice or full answer?** §5.4 recommends pushing a one-line notice and
   leaving the body to `cf catchup`. The brief says "delivering the result".
   If the full answer must be pushed, say so — the design changes (truncation
   policy, a summarizer, a much larger blast radius on risk 1).

3. **Does the delivery write submit, per harness?** §5.3 has two sources
   disagreeing and a plausible reconciliation, but it needs five live probes
   (claude, codex, pi, opencode, kimi) that this research was not permitted to
   run. It is the first task of phase 1, and the answer decides whether push is
   viable at all for some harnesses.

4. **Is the roster editor's page acceptable in an iframe?** §4.1's
   recommendation keeps `cf ui` exactly as it is, framed inside the app's own
   page. If the intent is instead to rebuild the roster editor as part of the
   new frontend, the "one implementation of the editor" rule
   (`app/src-tauri/src/lib.rs:7-15`) is being deliberately retired, and that
   should be a decision, not a side effect.

5. **Does cmux support end, or run alongside?** Every seam in §2.2 can support
   both, at the cost of two liveness paths and two lead-id sources forever. A
   date for dropping cmux would let the code carry one.

6. **What happens to a lead that is not in the app?** §5.1.1: a lead running in
   iTerm with only its workers in ConsensFlow's window can be identified but not
   written to. Options: (a) delivery is only offered when the lead is itself in
   an app pane — simple, and makes the app the whole workspace; (b) the lead's
   own pane must be opened in the app before `auto` is available, announced at
   conversation creation; (c) a terminal-side fallback (a file the lead's own
   `cf` polls), which is a daemon in all but name. Recommendation: (a) for v1,
   because it is the only one with no silent-failure mode.

### 10.1 Not researched — do not quote this document for these

Stated so a spec built on this knows where its floor is:

- **Wave Terminal, Tabby, Hyper** — named in the brief, not investigated
  (§4.4). VS Code, tauri-terminal and tauri-plugin-pty are the prior art
  actually read.
- **pi and kimi mid-turn stdin behaviour** — no source found (§5.2).
- **Whether a programmatic write submits in any harness** — reasoned about from
  three sources that partly disagree, settled by none (§5.3).
- **xterm.js bundle size** — the addon versions are fetched; a size figure was
  not, and none is asserted.
- **wterm under a real agent TUI** — the assessment in §4.3 is from its own
  issue tracker and changelog, not from running Claude Code inside it.
- **ConPTY behaviour on a real Windows machine** — one open upstream issue is
  cited (§4.2); nothing was executed.
- **Whether an `http://localhost` iframe loads inside a `tauri://` page** —
  reasoned from the ATS config, not observed (§4.1).
