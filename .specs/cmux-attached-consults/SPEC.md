---
id: cmux-attached-consults
title: Attached consults — the pane IS the agent's window
status: completed
created: 2026-08-24
updated: 2026-08-24
priority: high
tags: [cmux, attach, runners, skill]
---

# Attached consults — the pane IS the agent's window

## Overview

In cmux mode a consult currently streams and exits; attaching is an opt-in
flag. That was half of what was agreed. This spec makes every consult in a
terminal BE the harness's own window from its first turn: `cf run @athena
"task"` in a pane opens pi's interface seeded with the task, on the named
conversation, and stays there. The lead reads with `cf catchup`; follow-ups
are typed (by the user) or sent as plain text into the pane (by the lead).

A pipe cannot host a TUI, so `cf run` from a program — the lead's tool call,
a test, `--json` — streams exactly as today. Host modes are untouched.

## Acceptance Criteria

- [x] In cmux mode, in a terminal, `cf run @name "task"` replaces itself with
      the harness's own window: claude on a uuid we mint, pi on the
      conversation's name, opencode discovered from its store, codex after
      streaming its first answer (`codex resume <id>`)
- [x] The task (as the packet on turn 1, bare on later turns) is the seeded
      first message of the window
- [x] The conversation row records the session id and lead before or as soon
      as the window can produce one, so the next `cf run` resumes it
- [x] Every interactive spawn strips `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` —
      the billing guard holds on the attached path (including `cf attach`)
- [x] Non-terminal `cf run` (pipes, `--json`) streams as before; host modes
      unchanged; `--attach` flag removed
- [x] `cf catchup <name> --wait` blocks until the agent's next answer lands in
      the harness's store, then prints it
- [x] `cf last` on a conversation with no runs of ours points at catchup
      instead of failing on a missing run directory
- [x] The cmux skill teaches the new shape: pane → window, follow-ups as text
      into the pane, `cf catchup --wait` as the read; never scrape the screen
- [x] `npm run check` green with no live CLIs and no network; terminal-ness is
      injectable (`CONSENSFLOW_TTY`) so tests cover the attached path

## Tasks

Phase 1 — engine
- [x] T1 `interactiveStart(agent, sessionId, seed)` in runners.js: fresh-window
      argv per kind (claude `--session-id`, pi `--session-id`, opencode
      `--prompt`; codex → null = stream first). `interactiveResume` gains a
      seed argument. Both return the same `dropEnv` the one-shot carries.
- [x] T2 `discoverOpencodeSession(cwd, since, env)` beside the transcript
      reader: newest `storage/session/*/*.json` with `directory === cwd`,
      `time.created >= since`. Read-only, empty on anything odd.
- [x] T3 `handOver` (cf attach + run) spawns with the guard env applied.

Phase 2 — cf run attaches
- [x] T4 terminal rule: cmux + threading + terminal + not `--json` → window;
      terminal-ness from `CONSENSFLOW_TTY` when set, `process.stdout.isTTY`
      otherwise. Save the row (id, lead) before the window for claude/pi;
      after discovery for opencode; after the streamed turn for codex.
- [x] T5 retire `--attach`; non-terminal path unchanged and still recorded.

Phase 3 — reading
- [x] T6 `cf catchup --wait`: poll `harnessTurns` until an assistant turn
      newer than the starting count arrives; print the new turns.
- [x] T7 `cf last` degrades to a catchup pointer when a row has no runs.

Phase 4 — prose
- [x] T8 skill (cmux): the pane recipe now opens a window; follow-ups are
      plain text sent into it; the read is `cf catchup <name> --wait`; codex
      panes stream turn 1 first. Host-mode skill untouched.
- [x] T9 README + CLAUDE.md/AGENTS.md pair + UI command help.

Phase 5 — ship
- [x] T10 `npm run check` green → rebuild app → reinstall → verify bundle →
      `cf skills update` → push both remotes.
