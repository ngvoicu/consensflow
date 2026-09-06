# Findings — the probes, as they are run

Each row: the harness and its version, the exact command, what was observed.
Nothing here is inferred from a document.

## P1 — a multi-line paste, then a separate `\r`

Method for every row below (the REAL PTY write path, `consensflow-bridge`,
not cmux): the headless binary
`app/src-tauri/target/debug/consensflow-bridge` (rebuilt with `cargo build`
before the run) over piped stdio; `pane.open` with `cwd` = this repository,
`size` 24×80, `backlogBytes` 4 MiB, `env` inherited by the child;
`pane.write_paste` with `epoch` 0 (no human write op exists in the headless
binary, so the epoch stays 0); output read from `pane.output` events, acked
per event. The exact write shape the bridge produced, verified byte-exact
through the real PTY against a raw-mode recorder child
(`stty raw -echo; cat > file` captured 613 bytes for a 600-byte body):
ONE write of `1b 5b 32 30 30 7e` + body + `1b 5b 32 30 31 7e`, then a
SECOND write of a lone `0d` 10 ms later (`ENTER_DELAY_MS = 10`). Each body
is three paragraphs, multi-line, with a marker line
`probe-p1-<harness>-<6 chars>`. "Appeared" times run from the
`pane.write_paste` response (which returns after the `\r` was written) to
the first store read containing the marker; the store poll ran every
400 ms, so times carry that resolution. The store is the harness's own,
read with `harnessTurns` (`hosts/lib/harness-transcript.js`); "intact"
means the stored user turn carries every byte of the body with all 5
newlines preserved (a trailing space at the very end of a body is trimmed
by the reader's `trim()` — noted where it appeared, never a mid-body loss).

| Harness | Date | Observation |
|---|---|---|
| codex (astraeus, `gpt-6-astra`, MAX; cmux note — kept as evidence, NOT a pass of this path) | 2026-09-06 | A long single-paragraph body (~1,400 characters) sent with `cmux send --surface surface:54 '<body>'$'\n'` — the body and the newline in ONE send — landed in the input box and was **not submitted**; `cf catchup --unread` showed nothing new for over a minute and the user saw the text sitting in the box. A second send of a lone newline, `cmux send --surface surface:54 $'\n'`, submitted it: `cf catchup --unread` then showed the question as a new user turn. Short bodies (the day's earlier one-line follow-ups) had submitted with the newline in the same send. |
| codex, `codex-cli 0.153.4` — argv `["/opt/homebrew/bin/codex"]`, no flags | 2026-09-06 | Marker `probe-p1-codex-abccvc`. No trust prompt (this repo is already `trust_level = "trusted"` in `~/.codex/config.toml`); TUI first output 38 ms after open, quiet idle after 8.1 s. 600 B: **intact**, byte-exact, 5/5 newlines, appeared 967 ms after the `\r` (first turn — its rollout is found by a directory scan, so first-turn time carries discovery cost). 4 000 B: **intact**, byte-exact, 413 ms. 20 000 B: **submitted intact**, 416 ms, stored 19 999 bytes — the body's single trailing space is gone (reader trim), every other byte and all 5 newlines preserved. All three in session `01a0773d-6bdb-76e0-a09e-37b330d87d60`. Kitty at startup: `ESC[>5u` + `ESC[?u` query (P2). Inline budget: ≥ 20 000 B — the largest size tried submitted intact. |
| pi, `0.85.1` — argv `["/opt/homebrew/bin/pi"]`, no flags | 2026-09-06 | Marker `probe-p1-pi-z5xftg`. No prompt; first output 521 ms, quiet idle after 6.1 s. 600 B: **intact** (599 stored — trailing space trimmed; 5/5 newlines), appeared 3 249 ms after the `\r`: the FIRST turn of a fresh pi session reached its store up to ~3 s after submit — every following turn was sub-second, so this is first-turn latency, not size latency. 4 000 B: **intact**, byte-exact, 402 ms. 20 000 B: **intact**, byte-exact, 405 ms. Session `01a0773e-6dcb-7033-a5dc-f36b5ac1e228`. Kitty at startup: `ESC[>7u` + `ESC[?u` (P2). Inline budget: ≥ 20 000 B. |
| opencode, `1.18.29` — argv `["/Users/gabrielvoicu/.opencode/bin/opencode"]`, no flags | 2026-09-06 | Marker `probe-p1-opencode-4goyvp`. No prompt; first output 1 920 ms, quiet idle after 8.1 s. 600 B: **intact**, byte-exact, 426 ms. 4 000 B: **intact** (3 999 stored — trailing space trimmed, 5/5 newlines), 411 ms. 20 000 B: **intact** (19 999 stored — trailing space trimmed, 5/5 newlines), 407 ms. Session `ses_f88c0c7cdffeANJRVLwiBceADi`, read from `opencode.db` through `node:sqlite` (the file store is frozen). Kitty at startup: **no `CSI > … u`** — only the query `ESC[?u` (P2). Inline budget: ≥ 20 000 B. |
| kimi, `0.41.0` — argv `["/Users/gabrielvoicu/.kimi-code/bin/kimi"]`, no flags | 2026-09-06 | Marker `probe-p1-kimi-ilk467`. No prompt; first output 1 014 ms, quiet idle after 7.5 s. 600 B, 4 000 B and 20 000 B: **all three byte-exact**, 5/5 newlines each, appeared 433 / 406 / 412 ms after the `\r` — no trim, no loss at any size. Session `session_e4517114-9dc0-4a60-8546-8a2f2c794b0b`. Kitty at startup: `ESC[>7u` + `ESC[?u` (P2). Inline budget: ≥ 20 000 B. |
| claude | 2026-09-06 | **not run** — claude is rate-limited until 22:20 Europe/Athens today; its probes wait for that reset |

Reading (2026-09-06, the live bridge path): the separate-`\r` write shape
submits at every size tried for all four harnesses — the swallow recorded
below for a cmux paste whose Enter rode INSIDE the send does not appear
when the `\r` is its own write: 20 000 bytes pasted as one bracketed
paste submitted in ~0.4 s and stored whole. First-turn store latency is
the one wrinkle: pi took up to ~3 s to write its first turn, codex ~1 s
(includes discovery); opencode and kimi were sub-half-second from the
first turn. A store poller needs to outlive that.

Reading (the earlier cmux note): the swallow described in research-01 §5.3
(claude-code#43169, #30239) is real for codex too, and it is
size-dependent — the Enter that rides inside a large paste is consumed
with the paste. The delivery write shape in SPEC.md (bracketed paste, then
a separate `\r` as a second write) is the right one; the current cmux-era
skill's follow-up recipe (words and newline in one send) is wrong for long
follow-ups and should say so until Phase 6 retires it.

Still to run for P1: claude (after 22:20 Europe/Athens, 2026-09-06).

## P2 — kitty keyboard protocol

Run through the same path: the bytes below come from the bridge's
`pane.output` events captured from `pane.open` to the first idle of each
TUI. "Did a raw `\r` still submit" is proven by the P1 rows — the only
Enter ever sent through the pane was the bridge's separate `\r`, and the
turns appeared in the stores; "did newlines survive" is the 5/5 newline
count of every stored turn at every size, none collapsed.

| Harness | Date | Observation |
|---|---|---|
| codex, `codex-cli 0.153.4` | 2026-09-06 | Emits a kitty keyboard protocol set request at startup: `ESC[>5u` (bytes `1b 5b 3e 35 75`) at byte 15 of the startup output, between `ESC[>4;0m` (modifyOtherKeys off) and `ESC[?1004h` (focus reporting), followed by the kitty query `ESC[?u` at byte 48. A raw `\r` still submitted (all three P1 pastes). Newlines inside the pasted body survived as newlines in the stored turns — no collapsing. |
| pi, `0.85.1` | 2026-09-06 | `ESC[>7u` at byte 202, immediately after `ESC[?2004h` (bracketed paste enable); kitty query `ESC[?u` at byte 207. Raw `\r` submitted; newlines survived, no collapsing. |
| opencode, `1.18.29` | 2026-09-06 | **No `CSI > … u` set request in the captured startup bytes.** opencode emits the kitty QUERY `ESC[?u` at byte 104, right after `ESC[?2026$p` (synchronized-output DECRQM), and nothing matching `1b 5b 3e … 75` before the first idle. Raw `\r` submitted; newlines survived, no collapsing. |
| kimi, `0.41.0` | 2026-09-06 | `ESC[>7u` at byte 161, immediately after `ESC[?2004h`; kitty query `ESC[?u` at byte 166. Raw `\r` submitted; newlines survived, no collapsing. |
| claude | 2026-09-06 | **not run** — session limit resets 22:20 Europe/Athens today |

## P3 — `http://localhost` iframe inside a `tauri://` page

_not run_

## P4 — `portable-pty` 0.9.0 on Windows

_not run_

## P5 — opencode's TUI server accepts a message into the running session

_not run_

## P6 — a pi extension delivers a follow-up when the agent is idle

_not run_
