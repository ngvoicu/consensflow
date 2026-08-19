# CLAUDE.md

Byte-identical to `AGENTS.md` — keep both in sync (workspace convention).

## What this is

consensflow-cmux (ConsensFlow v3): skills-first. A zero-dependency Node ESM npm package
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
| `src/skill.js` | SKILL.md generation — the prose IS the product; template live-proven before the generator existed |
| `src/agents.js` | Agent detection (CLI on PATH) + per-agent skills dir (honours `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `XDG_CONFIG_HOME`) |
| `src/manifest.js` + `src/install.js` | Hash-manifest ownership: install/update/status/uninstall; drift is sacred |
| `src/cmux-skills.js` | Shallow-clones manaflow-ai/cmux, installs its `skills/` tree as `cmux@<commit>` |
| `src/ui.js` | Ephemeral loopback roster editor (random bearer token, no daemon) |
| `skill/SKILL.md` | The hand-written v0 the generator's template mirrors |

## Load-bearing rules

- **Modules never read `process.env`** — the environment is an explicit
  argument everywhere. That is the whole test-isolation story
  (`tests/helpers.mjs` builds throwaway homes).
- **Drift is sacred.** A manifest-owned file whose hash changed was edited by
  the user: refuse without `--force`, never clobber. A file not in the
  manifest is never touched, installed over, or deleted.
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
