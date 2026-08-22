# CLAUDE.md

Byte-identical to `AGENTS.md` — keep both in sync (workspace convention).

## What this is

ConsensFlow (the manager, npm `consensflow`): skills-first. A zero-dependency Node ESM npm package
(`cf`/`consensflow` bins, no build step) that manages a roster of named AI
agents and generates/installs **one skill** teaching every harness
(claude, codex, pi, opencode — all read the same Agent Skills `SKILL.md`
format) how to consult them via exact one-shot CLI commands. In `cmux`
mode it also installs and updates cmux's own skills. There is deliberately **no
delegation engine** — no daemon, no SQLite, no panes, no state machine (the
v2 engine that had one was retired and deleted 2026-08-19).

## Source map

| Path | Owns |
|---|---|
| `bin/cf.mjs` | All verbs: setup, agent …, skills …, ui, doctor. Setup never seeds agents; a machine that ran cc/pi already has the shared roster, so setup installs the skill straight from it. Every roster mutation installs-or-regenerates the skill — first add installs it everywhere |
| `src/roster.js` | Roster = the SHARED v1 file `~/.consensflow/agents.json` (cc + pi read/write it too): v1-schema-faithful mapping (kind↔runtime, thinking/effort↔effort, toolsPolicy↔permission), unknown fields preserved, unsupported kinds listed+marked, never dropped. **One root, one meaning**: roster, state and workspaces all live in `~/.consensflow`, or under `CONSENSFLOW_HOME` when it is set. The state used to sit under XDG while the roster sat here, and the variable meant a different directory to each half — a machine set up that way is moved into the one root on first run (`migrateStateRoot`) |
| `src/catalog.js` | A **view over `hosts/lib/presets.js`** — one catalog, not two (they disagreed on five names until 2026-08-21) — plus each CLI's real effort levels. 49 of the 50 presets are offered; the image preset has no runtime to launch it. `cf agent add <name>` resolves through it and records `preset` on the row, which is what makes `cf agent sync` and the UI's Update button possible |
| `src/skill.js` | SKILL.md generation — the prose IS the product. One command for every agent (`cf run @name "<task>"`), so the table says who each agent is rather than what to type |
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

- **The app is self-contained**: `app/scripts/prepare-sidecar.mjs` downloads an
  official Node build (pinned) into `src-tauri/binaries/node-<triple>` and
  copies `bin/ src/ hosts/ skill/` into `src-tauri/resources/cli`. The Rust
  side runs that pair and consults nothing on the machine. Never copy the
  system Node: package-manager builds link machine-local dylibs.
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

- **One mode per machine** (`src/mode.js`): `claude` | `pi` | `cmux` (named after the three products; `standalone` is accepted as the old name for `cmux`),
  recorded in `<config>/mode.json`. `applyMode` installs the chosen path and
  removes the others' — so two ConsensFlow paths can never be live at once.
  `cf skills install` refuses in a host mode rather than quietly breaking the
  invariant. cmux's own skills belong to `cmux` mode alone (`syncCmuxSkills`,
  the single rule the CLI, the UI and `applyMode` all call): a host mode
  installs none, takes back any it finds, and never clones. A failed fetch
  never costs the mode switch. Every entry point states the cost of a host mode (codex and
  opencode get nothing).

- **One spawn verb, three modes.** `cf run @name "<task>"` builds the packet,
  applies the billing guards and streams the run, whichever harness is behind
  the name — the manager calls the same `hosts/lib` engine the payloads do.
  Image agents included: `image` is a harness like the rest, and its run lives
  in `hosts/lib/image-run.js` so no caller carries a second copy.

- **Nobody assigns an agent a persona.** The packet has no "Who you are" and
  never calls a run a coding session; `--brief` is the lead's own words about
  what it wants from THIS spawn, and when there is none, nothing stands in for
  it. Per-spawn rather than stored on the roster, because the same agent is a
  reviewer in one call and a researcher in the next.

- **The handoff is the lead's to give.** In a host mode the integration stashes
  the session and passes it; anywhere else the lead passes `--handoff-file`,
  because the lead IS the harness holding the conversation. No transcript
  discovery, no per-harness adapters — that was the v2 engine.

- **`cf` on PATH is part of cmux mode**, not an optional extra: the generated
  skill tells four harnesses to run it, so `applyMode('cmux')` installs the
  launcher and a host mode (whose skill names its payload CLI by absolute path)
  takes it back. `off` removes ours, by the marker it writes.

- **No harness intercepts a mention.** pi used to swallow `@zeus …` and run the
  agent itself; the lead composes every run now, in all three modes. Claude
  Code's hook still routes @mentions, but by injecting the command for the lead
  to run — which is the behaviour pi moved toward.

- **Nothing the app installs assumes Node on PATH.** The hooks and the
  `/consensflow` command name their runtime absolutely — `${CONSENSFLOW_NODE}`,
  rewritten at install to the runtime doing the installing, which from the app
  is its own bundled Node. The cost is that moving or deleting whatever
  provided it breaks the wiring, so `cf doctor` reports the runtime and says
  MISSING when it is gone.

- **Modules never read `process.env`** — the environment is an explicit
  argument everywhere. That is the whole test-isolation story
  (`tests/helpers.mjs` builds throwaway homes).
- **Drift is sacred.** A manifest-owned file whose hash changed was edited by
  the user: refuse without `--force`, never clobber. A file not in the
  manifest is never touched, installed over, or deleted.
- **A host with its own ConsensFlow keeps it.** `detectAgents` marks `native` (cc plugin cache / pi extension checkout); `skillTargets` excludes those from the generated skill, `retireSkillFromNativeHosts` removes copies installed before the host had one (unedited only), and `--all` overrides. Without this, claude and pi see two same-named skills with the same trigger.
- **Full permissions, no permission concept.** There is still no knob, tier or
  policy to choose — but the default is now everything, not the CLI's own
  default: `--dangerously-skip-permissions` (claude),
  `--dangerously-bypass-approvals-and-sandbox` (codex, replacing the
  workspace-write sandbox), `--auto` (opencode), nothing for pi, whose tools
  are on already. A agent is a helper you hand a task to, so it writes
  anywhere and reaches the network; the approval gate on *keeping* its work is
  what protects the user, not a fence around the run. Asserted in
  `tests/skill.test.mjs` and both engine suites so no refactor re-fences it.
- **The generated commands carry `env -u ANTHROPIC_API_KEY` / `-u
  OPENAI_API_KEY`** — v1's dropEnv as prose; prevents subscription logins
  silently switching to API billing.
- **Every roster mutation regenerates the installed skill** (CLI and UI both
  call `refreshInstalledSkill`); only where the manifest says it is installed.
- **Pane control belongs to cmux's skills**, never ours — the v2 lesson
  (typed-bootstrap verification is a minefield). Our skill says *what* to
  run; theirs say *how* to drive panes. It is also a different product from
  consulting, which is why only `cmux` mode carries it: a Claude Code install
  runs its agents as subprocesses and never touches a pane.
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
