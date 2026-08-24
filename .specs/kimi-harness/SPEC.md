---
id: kimi-harness
title: Kimi Code as a fifth harness
status: completed
created: 2026-08-24
updated: 2026-08-24
priority: high
tags: [harness, kimi, runners, catalog]
---

# Kimi Code as a fifth harness

## Overview

ConsensFlow knows four harnesses. Kimi Code is installed on this machine, reads
the same Agent Skills format, and meets all five requirements (see
`research-01.md`). It joins as `kimi`: a harness that can consult, be consulted,
hold conversations and open its own window in a cmux pane.

It takes the **codex shape** — no minted session id, so turn one streams and
the pane becomes `kimi -S <id>` after. Two things are unlike every other
harness and are deliberate, not gaps: no per-run effort (kimi's lives in the
user's `config.toml`) and no billing guard (kimi authenticates from that same
file, so there is no env var to strip).

## Acceptance Criteria

- [x] `cf doctor` detects kimi when its CLI is present, and names its skills
      directory (`$KIMI_CODE_HOME/skills`, else `~/.kimi-code/skills`)
- [x] The generated skill installs into kimi in cmux mode and is taken back
      outside it, exactly like the other four
- [x] `cf run @<kimi agent> "<task>"` streams, captures the session id from
      `session.resume_hint`, and records the conversation
- [x] A follow-up resumes it (`-p … -S <id>`) and the agent remembers
- [x] In a cmux terminal the pane becomes `kimi -S <id>` after the streamed
      first turn — the codex path, since kimi cannot open cold
- [x] `cf catchup` reads kimi's own `wire.jsonl`: user turns from `turn.prompt`,
      assistant turns from `content.part` joined per `turnId`, thinking parts
      skipped, read-only, empty rather than throwing
- [x] kimi agents run with full permissions — implied by `-p`, asserted as
      such so the missing flag never reads as an omission
- [x] A kimi agent carries a model and no effort; setting one is ignored with
      a reason, not silently dropped
- [x] Catalog presets for kimi's own models, named so their billing path
      (Moonshot direct, not OpenRouter) is visible
- [x] `npm run check` green with no live kimi and no network

## Tasks

Phase 1 — the harness exists
- [x] T1 `src/harnesses.js`: kimi entry + `~/.kimi-code/bin` in its locations
- [x] T2 `src/roster.js`: `kimi` in HARNESSES, kind↔harness mapping, effort rule

Phase 2 — running it
- [x] T3 `runners.js`: `buildRunnerInvocation` for kimi (one-shot, resume,
      packet as `-p`), `extractSessionId` from `session.resume_hint`,
      `interactiveResume` = `kimi -S <id>`, `interactiveStart` = null
- [x] T4 transcript events: map kimi's stream shape into the one vocabulary

Phase 3 — reading it back
- [x] T5 `harness-transcript.js`: `findDir` + the `wire.jsonl` reader

Phase 4 — offering it
- [x] T6 `presets.js`: kimi presets; `catalog.js` effort list (empty)
- [x] T7 skill prose + UI: five harnesses, not four

Phase 5 — ship
- [x] T8 docs pair + README; `npm run check`; rebuild; reinstall; push
