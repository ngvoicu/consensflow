# CLAUDE.md

Byte-identical to `AGENTS.md` — keep both in sync (workspace convention).

## What this is

ConsensFlow (the manager, npm `consensflow`): skills-first. A zero-dependency Node ESM npm package
(`cf`/`consensflow` bins, no build step) that manages a roster of named AI
participants and generates/installs **one skill** teaching every coding agent
(claude, codex, pi, opencode — all read the same Agent Skills `SKILL.md`
format) how to consult them via exact one-shot CLI commands. It also
installs and updates cmux's own skills. There is deliberately **no
delegation engine** — no daemon, no SQLite, no panes, no state machine (the
v2 engine that had one was retired and deleted 2026-08-19).

## Source map

| Path | Owns |
|---|---|
| `bin/cf.mjs` | All verbs: setup, participant …, skills …, ui, doctor. Setup never seeds participants; a machine that ran cc/pi already has the shared roster, so setup installs the skill straight from it. Every roster mutation installs-or-regenerates the skill — first add installs it everywhere |
| `src/roster.js` | Roster = the SHARED v1 file `~/.consensflow/participants.json` (cc + pi read/write it too): v1-schema-faithful mapping (kind↔runtime, thinking/effort↔effort, toolsPolicy↔permission), unknown fields preserved, unsupported kinds listed+marked, never dropped. `configRoot` (manifest only): `CONSENSFLOW_HOME` → XDG → `~/.config/consensflow` |
| `src/catalog.js` | Ready-made participants per runtime + each CLI's real effort levels (live-verified 2026-08-20); `cf participant add <name>` resolves through it |
| `src/skill.js` | SKILL.md generation — the prose IS the product; template live-proven before the generator existed |
| `src/agents.js` | Agent detection (CLI on PATH) + per-agent skills dir (honours `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`) |
| `src/manifest.js` + `src/install.js` | Hash-manifest ownership: install/update/status/uninstall; drift is sacred |
| `src/cmux-skills.js` | Shallow-clones manaflow-ai/cmux, installs its `skills/` tree as `cmux@<commit>` |
| `src/hosts.js` | Installs/removes the host integrations: **claude** via documented user config (payload in `<config>/hosts/claude`, skill + `/consensflow` command + settings.json hook merge, all recorded in `hosts.json`), **pi** via its supported `pi install/remove` CLI. Never writes Claude Code's plugin registry — that is versioned internal state |
| `src/mode.js` | The one-path-per-machine invariant: modes, switching, and the plain-words report of what each mode costs |
| `src/ui.js` | Ephemeral loopback roster editor (random bearer token, no daemon): roster CRUD, catalog quick-adds, the mode switcher (`POST /api/mode`), and the skills panel (`GET /api/system`, `POST /api/skills/install`, `POST /api/skills/uninstall` — uninstall needs `confirm:true`). Named operations only: never an endpoint that executes a supplied command |
| `skill/SKILL.md` | The hand-written v0 the generator's template mirrors |

## The desktop app

`app/` (Tauri v2, Rust) is a window around `cf ui`, never a second editor.
Load-bearing facts, each learned by running the built bundle:

- macOS ATS blocks cleartext http in WKWebView → `bundle.macOS.exceptionDomain
  = "localhost"` and the URL must say `localhost`, not `127.0.0.1`.
- Build the window AT the address (`WebviewUrl::External`); navigating after
  the fact fails silently.
- A .app launched from Finder has a minimal PATH — `locate_cli()` tries PATH,
  known install locations, then the login shell.
- The editor is tied to the app's lifetime by a pipe on its stdin
  (`isPipe` accepts a socketpair, which is what Node's `'pipe'` is on macOS);
  `cf ui` run by hand keeps a terminal/dev-null stdin and is unaffected.

## The merged layout

`hosts/lib` is THE engine — one copy, shared by both host payloads
(`hosts/claude`, `hosts/pi`). It was two hand-synced copies in two repos
until 2026-08-20; the parity test that guarded them is gone because the
duplication is. Host deltas that existed then are resolved: the
`CONSENSFLOW_CHILD` marker is set for every host, the packet's handoff
wording is host-neutral, and cc's session helpers live in the shared
`state.js` (pi simply does not call them).

Payload files reference `${CONSENSFLOW_HOST_ROOT}`; `src/hosts.js` rewrites
that to wherever it installed the payload.

## Load-bearing rules

- **One mode per machine** (`src/mode.js`): `claude` | `pi` | `standalone`,
  recorded in `<config>/mode.json`. `applyMode` installs the chosen path and
  removes the others' — so two ConsensFlow paths can never be live at once.
  `cf skills install` refuses in a host mode rather than quietly breaking the
  invariant. Every entry point states the cost of a host mode (codex and
  opencode get nothing).

- **Modules never read `process.env`** — the environment is an explicit
  argument everywhere. That is the whole test-isolation story
  (`tests/helpers.mjs` builds throwaway homes).
- **Drift is sacred.** A manifest-owned file whose hash changed was edited by
  the user: refuse without `--force`, never clobber. A file not in the
  manifest is never touched, installed over, or deleted.
- **A host with its own ConsensFlow keeps it.** `detectAgents` marks `native` (cc plugin cache / pi extension checkout); `skillTargets` excludes those from the generated skill, `retireSkillFromNativeHosts` removes copies installed before the host had one (unedited only), and `--all` overrides. Without this, claude and pi see two same-named skills with the same trigger.
- **No permission concept anywhere.** Participants run with their CLI's own defaults; the generator never emits `--dangerously-*`. Removed 2026-08-20 across all three ConsensFlow projects.
- **The generated commands carry `env -u ANTHROPIC_API_KEY` / `-u
  OPENAI_API_KEY`** — v1's dropEnv as prose; prevents subscription logins
  silently switching to API billing. `--dangerously-*` flags only on
  participants stored as `full-auto`.
- **Every roster mutation regenerates the installed skill** (CLI and UI both
  call `refreshInstalledSkill`); only where the manifest says it is installed.
- **Pane control belongs to cmux's skills**, never ours — the v2 lesson
  (typed-bootstrap verification is a minefield). Our skill says *what* to
  run; theirs say *how* to drive panes.
- **Tests spawn no live agent CLIs and no network** — agent CLIs are stub
  scripts on a fake PATH; git is a PATH shim copying a fixture tree.

## Commands

```sh
npm test          # node --test (43 tests)
npm run check     # biome + tests
```

## Spec

`.specs/consensflow-v3-skills-first/` — the forge record, including the
skill-first inversion (v0 prose live-proven on claude and codex before any
factory code) and the live-verified one-shot command forms per engine.
