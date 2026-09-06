# Findings — the probes, as they are run

Each row: the harness and its version, the exact command, what was observed.
Nothing here is inferred from a document.

## P1 — a multi-line paste, then a separate `\r`

| Harness | Date | Observation |
|---|---|---|
| codex (astraeus, `gpt-6-astra`, MAX; codex CLI version to record) | 2026-09-06 | A long single-paragraph body (~1,400 characters) sent with `cmux send --surface surface:54 '<body>'$'\n'` — the body and the newline in ONE send — landed in the input box and was **not submitted**; `cf catchup --unread` showed nothing new for over a minute and the user saw the text sitting in the box. A second send of a lone newline, `cmux send --surface surface:54 $'\n'`, submitted it: `cf catchup --unread` then showed the question as a new user turn. Short bodies (the day's earlier one-line follow-ups) had submitted with the newline in the same send. |

Reading: the swallow described in research-01 §5.3 (claude-code#43169, #30239) is real for codex too, and it is size-dependent — the Enter that rides inside a large paste is consumed with the paste. The delivery write shape in SPEC.md (bracketed paste, then a separate `\r` as a second write) is the right one; the current cmux-era skill's follow-up recipe (words and newline in one send) is wrong for long follow-ups and should say so until Phase 6 retires it.

Still to run for P1: the largest body that submits intact per harness (decides `pty-inline` vs `pty-file`), and claude, pi, opencode, kimi.

## P2 — kitty keyboard protocol

_not run_

## P3 — `http://localhost` iframe inside a `tauri://` page

_not run_

## P4 — `portable-pty` 0.9.0 on Windows

_not run_

## P5 — opencode's TUI server accepts a message into the running session

_not run_

## P6 — a pi extension delivers a follow-up when the agent is idle

_not run_
