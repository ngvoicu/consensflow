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
| claude, `2.1.263 (Claude Code)` — argv `["/Users/gabrielvoicu/.local/bin/claude"]`, no flags (the real binary; `which claude` is a cmux shim) | 2026-09-06 | Marker `probe-p1-claude-kv3ve4`. No trust prompt; first output 886 ms after open, quiet idle after 8.6 s; a non-blocking banner "⚠ 1 MCP server needs authentication · run /mcp" (later "⚠ 4 MCP servers need authentication"). The login is **expired** — every submitted turn got the assistant reply `⏺ Login expired · Please run /login` (nothing about the usage limit was printed; the blocker was auth, and `/login` needs a browser OAuth the probe cannot run). The user turn is stored regardless: 600 B **byte-exact** (600/600, 5/5 newlines), 4 000 B **byte-exact**, 20 000 B **byte-exact** (20 000/20 000, 5/5 newlines) — the stored `user` record carries the whole multi-line body as one plain string. Times to appear are from the store record's own timestamp against the `\r` write: ~48 ms (600 B), ~19 ms (4 kB), ~39 ms (20 kB) — measured tighter than the 400 ms poll resolution of the rows above. Odd: the TUI **display** collapses a multi-line paste into a `[Pasted text #1 +5 lines] · paste again to expand` chip while the **store** keeps the full text with newlines — the screen is not the evidence, the store is. All three turns in session `62fe5b8f-19a1-463e-bfec-cdd4051e2e71`. Kitty at startup: no set request — claude sends the kitty CLEAR form `ESC[<u` and modifyOtherKeys off (P2). Inline budget: ≥ 20 000 B — the largest size tried submitted intact, byte-exact at every size. |

Reading (2026-09-06, the live bridge path): the separate-`\r` write shape
submits at every size tried for all five harnesses — the swallow recorded
below for a cmux paste whose Enter rode INSIDE the send does not appear
when the `\r` is its own write: 20 000 bytes pasted as one bracketed
paste submitted in ~0.4 s and stored whole. First-turn store latency is
the one wrinkle: pi took up to ~3 s to write its first turn, codex ~1 s
(includes discovery); opencode, kimi and claude were fast from the first
turn (claude's store record is timestamped within ~50 ms of the `\r`).
A store poller needs to outlive that.

Reading (the earlier cmux note): the swallow described in research-01 §5.3
(claude-code#43169, #30239) is real for codex too, and it is
size-dependent — the Enter that rides inside a large paste is consumed
with the paste. The delivery write shape in SPEC.md (bracketed paste, then
a separate `\r` as a second write) is the right one; the current cmux-era
skill's follow-up recipe (words and newline in one send) is wrong for long
follow-ups and should say so until Phase 6 retires it.

Nothing left to run for P1: all five harnesses have rows through the real
bridge path. (The claude row was run with its login expired — the user
turn is stored at submit regardless, which is what P1 measures.)

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
| claude, `2.1.263 (Claude Code)` | 2026-09-06 | **No `CSI > … u` set request at startup.** The `u`-final sequence claude emits is `ESC[<u` (bytes `1b 5b 3c 75`) — the kitty protocol's clear-all-flags form, i.e. it explicitly DISENGAGES kitty — alongside `ESC[>4m` (modifyOtherKeys off; `CSI > 4 m` is not a kitty form). Startup also enables bracketed paste (`ESC[?2004h`) and focus reporting (`ESC[?1004h`), queries cursor style (`ESC[>0q`), and sends `ESC c`. A raw `\r` still submitted — all three pastes were submitted by the lone `\r` (the turns are in the store; only the model reply failed, on the expired login). Newlines inside the pasted body survived as newlines in the stored turns — 5/5 at every size, no collapsing, despite the on-screen paste chip. |

## P3 — `http://localhost` iframe inside a `tauri://` page

**PASSED 2026-09-07 on macOS.** Built the local frontend and signed app with
`cd app && npm run bundle:ui && npm run prepare-sidecar && npx tauri build
--bundles app`. The shipped configuration under test was
`build.frontendDist = "../ui"`, `app.withGlobalTauri = true`,
`app.security.csp = null`, and `bundle.macOS.exceptionDomain = "localhost"`
in `app/src-tauri/tauri.conf.json`. The built `Info.plist` contained exactly:

```text
NSAppTransportSecurity.NSExceptionDomains.localhost = {
  NSExceptionAllowsInsecureHTTPLoads = true;
  NSIncludesSubdomains = true;
}
```

An installed production ConsensFlow was already running, so the observed
bundle used a probe-only merge config changing only `productName` to
`ConsensFlow P3`, `identifier` to `dev.ngvoicu.consensflow.p3`, and bundle
target to `app`; it did not change the frontend, origin, CSP, ATS, Rust, or
JavaScript under test. CuaDriver launched that signed bundle without raising
it. The app-owned Node child listened on `127.0.0.1:50252`; Rust normalised
the handle to `http://localhost:50252/`, and the page added the UI token to
the iframe URL. WebKit recorded the iframe as `isMainFrame=0`, then
`httpStatusCode=200`, `didFinishDocumentLoadForFrame`, and
`didFinishLoadForFrame` for the same frame id (`21474836481`) at
`2026-09-07 01:22:50`. Therefore cleartext loopback HTTP loads inside the
packaged `tauri://` page under the existing ATS exception. No workaround is
required or shipped.

The first probe build also exposed a separate packaging error: with both
`app` and `consensflow-bridge` binaries and no Cargo `default-run`, Tauri
picked the headless helper as `CFBundleExecutable` and opened no window.
`default-run = "app"` now pins the GUI executable; the passing bundle's
`CFBundleExecutable` was `app`.

## P4 — `portable-pty` 0.9.0 on Windows

_not run_

## P5 — opencode's TUI server accepts a message into the running session

**PASSED 2026-09-07 on macOS.** Installed OpenCode is `1.18.29`, binary
`/Users/gabrielvoicu/.opencode/bin/opencode`; `~/.local/share/opencode` has
the database, auth and storage but no source checkout. The embedded Bun bundle
was inspected with `strings`, and `opencode --help`/`opencode serve --help`:
the TUI accepts `--port`/`--hostname`; the server uses HTTP Basic Auth, with
username defaulting to `opencode` and password from
`OPENCODE_SERVER_PASSWORD`. The live route below establishes the endpoint and
admission shape rather than relying on those embedded strings.

Exact TUI launch command (the command ran attached to a PTY; all data/config
homes were throwaway and the password was probe-only):

```text
env HOME=/tmp/consensflow-p5.Y4VGOp/home XDG_CONFIG_HOME=/tmp/consensflow-p5.Y4VGOp/config XDG_DATA_HOME=/tmp/consensflow-p5.Y4VGOp/data XDG_STATE_HOME=/tmp/consensflow-p5.Y4VGOp/state XDG_CACHE_HOME=/tmp/consensflow-p5.Y4VGOp/cache OPENCODE_SERVER_PASSWORD=p5-secret OPENCODE_SERVER_USERNAME=probe /Users/gabrielvoicu/.opencode/bin/opencode --pure /Users/gabrielvoicu/Projects/ngvoicu/consensflow --port 41893 --hostname 127.0.0.1
```

Exact HTTP commands and observed responses:

```text
curl --silent --show-error --include --max-time 5 http://127.0.0.1:41893/global/health
HTTP/1.1 401 Unauthorized
www-authenticate: Basic realm="Secure Area"
Content-Length: 0

curl --silent --show-error --include --max-time 5 -u probe:p5-secret http://127.0.0.1:41893/global/health
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 36
{"healthy":true,"version":"1.18.29"}

curl --silent --show-error --include --max-time 5 -u probe:p5-secret -H 'content-type: application/json' -X POST http://127.0.0.1:41893/session -d '{}'
HTTP/1.1 200 OK
observed JSON fields: id="ses_f86bc3273ffegbmdikZWvzPIT7"; title="New session - 2026-09-07T00:28:01.804Z"; version="1.18.29"; directory="/Users/gabrielvoicu/Projects/ngvoicu/consensflow"

curl --silent --show-error --include --max-time 10 -u probe:p5-secret -H 'content-type: application/json' -X POST http://127.0.0.1:41893/session/ses_f86bc3273ffegbmdikZWvzPIT7/prompt_async -d '{"parts":[{"type":"text","text":"probe-p5-admission-2026-09-07"}]}'
HTTP/1.1 204 No Content
Content-Length: 0

curl --silent --show-error --include --max-time 5 -u probe:p5-secret http://127.0.0.1:41893/session/ses_f86bc3273ffegbmdikZWvzPIT7/message
HTTP/1.1 200 OK
observed JSON: one message with role="user" and one text part with text="probe-p5-admission-2026-09-07"; the response also contained the assistant error record after that user message
```

The `204` is the server's admission response, and the immediate message read
contains the exact probe text as a stored `role:"user"` turn. The TUI's
provider then showed its own missing-key error, but that did not change the
admission or stored user turn. **Pass: `opencode-server` is enabled.**

## P6 — a pi extension delivers a follow-up when the agent is idle

**PASSED 2026-09-07 on macOS.** Installed Pi is `0.85.1`, binary
`/opt/homebrew/bin/pi`. The installed docs and sources were read:
`docs/extensions.md` documents `--extension/-e` and `pi.on(...)`;
`examples/extensions/send-user-message.ts` uses `ctx.isIdle()` and
`pi.sendUserMessage()`; `dist/core/extensions/types.d.ts` defines
`session_start`, `agent_settled`, `session_shutdown` and `ctx.isIdle()`; and
`dist/core/agent-session.d.ts` defines `sendUserMessage` as an actual user
message that always triggers a turn. Pi's `agent-session.js` emits
`agent_settled` only after the run, so the probe also tested the inbox watcher
path while already idle.

The temporary probe extension watched an inbox directory on `session_start`,
checked `ctx.isIdle()` on each filesystem event, read one JSON record, called
`pi.sendUserMessage(record.text)`, wrote `<id>.json` in the ack directory, and
removed the consumed inbox record. It also logged the event order. A local
OpenAI-compatible provider on `127.0.0.1:43218` returned `p6-ok`, so no remote
network or real provider quota was used.

Exact provider command:

```text
node --input-type=module -e 'import { createServer } from "node:http"; import { appendFile } from "node:fs/promises"; const server=createServer(async (req,res)=>{ let body=""; for await (const chunk of req) body+=chunk; await appendFile("/tmp/consensflow-p6.Y4VGOp/run/provider2.jsonl", JSON.stringify({method:req.method,url:req.url,body})+"\n"); res.writeHead(200,{"content-type":"text/event-stream"}); res.write("data: "+JSON.stringify({id:"p6-response",object:"chat.completion.chunk",created:1,model:"p6-model",choices:[{index:0,delta:{role:"assistant",content:"p6-ok"},finish_reason:null}]})+"\n\n"); res.write("data: "+JSON.stringify({id:"p6-response",object:"chat.completion.chunk",created:1,model:"p6-model",choices:[{index:0,delta:{},finish_reason:"stop"}]})+"\n\n"); res.end("data: [DONE]\n\n"); }); server.listen(43218,"127.0.0.1",()=>console.log("p6-provider listening 127.0.0.1:43218"));'
```

Exact Pi command (isolated config/session homes, extension explicitly loaded):

```text
env HOME=/tmp/consensflow-p6.Y4VGOp/home3 PI_CODING_AGENT_DIR=/tmp/consensflow-p6.Y4VGOp/agent3 PI_CODING_AGENT_SESSION_DIR=/tmp/consensflow-p6.Y4VGOp/sessions3 PI_TELEMETRY=0 PI_SKIP_VERSION_CHECK=1 CF_P6_INBOX=/tmp/consensflow-p6.Y4VGOp/run3/inbox CF_P6_ACK=/tmp/consensflow-p6.Y4VGOp/run3/ack CF_P6_LOG=/tmp/consensflow-p6.Y4VGOp/run3/events.jsonl CF_P6_PROVIDER_URL=http://127.0.0.1:43218/v1 /opt/homebrew/bin/pi --no-extensions --extension /tmp/consensflow-p6.Y4VGOp/probe-extension.mjs --approve --provider p6-local --model p6-model --session-dir /tmp/consensflow-p6.Y4VGOp/sessions3 --session-id p6-probe-3
```

Inbox arrival command:

```text
node --input-type=module -e 'import { writeFile } from "node:fs/promises"; await writeFile("/tmp/consensflow-p6.Y4VGOp/run3/inbox/d-p6-3.json", JSON.stringify({id:"d-p6-3",text:"probe-p6-idle-inbox-2026-09-07"})+"\n")'
```

Observed extension log, in order:

```text
{"event":"session_start","idle":true}
{"event":"arrival","idle":true,"delivery":{"id":"d-p6-3","text":"probe-p6-idle-inbox-2026-09-07"}}
{"event":"ack","idleAfter":false,"deliveryId":"d-p6-3"}
{"event":"agent_settled","idle":true}
```

Ack file `/tmp/consensflow-p6.Y4VGOp/run3/ack/d-p6-3.json` contained:
`{"id":"d-p6-3","admitted":true,"mode":"tui"}`. The session file
`/tmp/consensflow-p6.Y4VGOp/sessions3/2026-09-07T00-33-40-446Z_p6-probe-3.jsonl`
contained the exact user turn with `role:"user"` and the assistant response
`p6-ok` with `stopReason:"stop"`. Arrival and ack preceded the first
`agent_settled`, proving that an inbox record written while Pi was already
idle was delivered immediately rather than stranded. **Pass:
`pi-extension` is enabled.**

## Completion markers per harness (Phase 3, TEST-PANE-23 / IMPL-PANE-24)

Re-verified 2026-09-06–07 against the exact native stores named below. The
adapter supports only the recorded protocol/schema versions in this table;
any other or missing version returns
`{unknown:true, reason:"unsupported version …"}`.

Every successful result exposes a native total-order `cursor`, and every
item carries `at` plus `seq`. Model-visible tool results are lossless
`role:"tool"` items, so a later receipt reader can search strictly after a
delivery cursor. Settlement is reported independently:

```
settlement = {
  state: "settled" | "in-flight" | "unknown",
  provenance: "native" | "derived" | "unknown",
  cursor,
  boundary,
  evidence: { complete, openTools: [], queuedTurns: [], hooksInFlight: [] }
}
```

A derived result is settled only when `complete` is true, all three evidence
arrays are empty, and `boundary` names the observed post-turn record.
Provider failure is `failed` with its message, never cancellation. A native
terminal failure may therefore be settled while `evidence.complete` is
false; readiness still has no completed answer to deliver.

| Harness | Supported store version | Completed-answer evidence | Settlement boundary record | Provenance | Verified real store |
|---|---|---|---|---|---|
| Codex | `session_meta.cli_version = 0.153.4` | Only `event_msg.item_completed.item.type = AgentMessage` with `phase:final_answer`. A successful boundary must also carry an exact, non-null `task_complete.last_agent_message` equal to that item. | `event_msg task_complete`, after its error/last-message fields are checked and all tool/sub-agent activity is closed. The error fixture's boundary is ordinal 2056, but settlement cannot be observed until the sub-agent completion appended at ordinal 2057. | Native | `/Users/gabrielvoicu/.codex/sessions/2026/09/06/rollout-2026-09-06T07-14-10-01a074ec-7aff-74b0-8cf6-aa00d8e451cb.jsonl`, especially ordinals 2017–2018 and 2045–2057; successful comparison at ordinals 2481–2485. |
| Claude Code | `2.1.241`, `2.1.247`, `2.1.250` | Assistant fragments are grouped by native `message.id`; physical `stop_reason:end_turn` records are fragment markers, not completion. The grouped item becomes complete only at the non-continuing post-turn hook. | Completion: `system subtype:stop_hook_summary` with `preventedContinuation:false`, after queue enqueue/dequeue/remove, queued user turns, `server_tool_use`/advisor results, ordinary tools, and hooks on the current frontier are clear. Failure: `assistant isApiErrorMessage:true`. Cancellation: user `[Request interrupted by user]`. | Derived completion; native failure/cancellation | Fragments, advisor, interrupt recovery, and 2.1.247→2.1.250 upgrade: `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/15fba934-d727-4777-8791-123675a63649.jsonl`, lines 91, 93, 107, 117–121, 128, 238, 246, and 690–692. Queue/hook ordering and interrupt: `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/1b09fb15-feb1-4595-9f47-5eb9ff768191.jsonl`, lines 2219–2232 and 3261. Provider failure: `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/33383216-87a0-4e6d-a273-07c4b229cdb1.jsonl`, line 1040. |
| Pi | Session protocol `3` (Pi `0.85.1`) | Terminal assistant `stopReason:stop`. A user message opens the turn; `toolResult` only closes its exact `toolCallId` and never closes the turn. | Completion: terminal assistant `message` with `stopReason:stop` and no open call IDs; Pi has no separate stored settlement event. Failure: terminal assistant `message` with `stopReason:error`. | Derived completion; native failure | Completion: `/Users/gabrielvoicu/.pi/agent/sessions/--Users-gabrielvoicu-Projects-ngvoicu-consensflow--/2026-08-24T18-00-00-703Z_hazy-ridge.jsonl`, lines 4–8 (no-open-tool gap) and 114 (terminal stop). Provider failure: `/Users/gabrielvoicu/.pi/agent/sessions/--Users-gabrielvoicu-Projects-ngvoicu-consensflow--/2026-08-26T10-14-49-150Z_triton-jade-fern.jsonl`, line 7. |
| Kimi | Wire protocol `1.5` | The assistant item is keyed by native step UUID; `step.end finishReason:end_turn` marks that step complete. Tool calls/results close through the call UUID or `toolCallId`, even though results carry no `turnId`. | `turn.ended {reason:completed}` after every call belonging to the turn is closed; `turn.ended {reason:failed}` is native terminal failure, never cancellation. | Native | `/Users/gabrielvoicu/.kimi-code/sessions/wd_consensflow-site_ed8b7a271238/session_11c123b3-dd33-4f21-8862-beabdc50cd18/agents/main/wire.jsonl`, especially lines 315–318, 747–759, and 861–868. |
| OpenCode | Session schema/CLI `1.18.27`, `1.18.29` | Native message `data.time.completed` is required; `data.finish:stop` is complete, while `length` and provider errors are terminal but incomplete. A `step-finish` part alone is insufficient. | The assistant message's native `time.completed` together with its terminal `finish` or `error`, read with messages and parts under one SQLite read transaction. | Native | `/Users/gabrielvoicu/.local/share/opencode/opencode.db`: message `msg_0773f385a001oy2xD1d5J3DNge`, event seq 12/13/15 (92 ms stop-to-completed window); `msg_0779166e7001sYP03lgChIzdvh` (`finish:length`); `msg_066fa2779001GkIYu5W7ugNXUR` (`APIError`). |

Cancellation and failure evidence:

- Codex cancellation is native `event_msg turn_aborted`, verified in
  `rollout-2026-09-06T21-23-51-01a077f6-6663-7bc2-81cd-e287ccaabdbd.jsonl`
  ordinal 764.
- Claude cancellation is the native user record
  `[Request interrupted by user]`, verified at line 3261 of the queue source
  above.
- Claude provider failure is native `assistant isApiErrorMessage:true`,
  verified at line 1040 of
  `/Users/gabrielvoicu/.claude/projects/-Users-gabrielvoicu-Projects-ngvoicu-consensflow/33383216-87a0-4e6d-a273-07c4b229cdb1.jsonl`;
  it is `failed`, not `cancelled`.
- Pi has no captured user-cancellation marker. The real
  `triton-jade-fern` `stopReason:error` record is provider 429 and is
  `failed`, not `cancelled`.
- Kimi has no captured user-cancellation reason; none is synthesised or
  inferred. Its native `turn.ended {reason:failed}` provider 429 at line 318
  of the table's protocol-1.5 source is `failed`, not `cancelled`.
- OpenCode `APIError` and `UnknownError` are provider failures.
  `MessageAbortedError` remains the native cancellation discriminator if it
  occurs in a supported schema, but no supported-version cancellation row is
  claimed by these fixtures.

Unsupported/identity notes:

- Codex preserves both different native user IDs even when their text is
  identical; response/event mirrors sharing one assistant ID are one item.
- Claude assistant identity is `message.id`, never the physical record UUID.
- Real Kimi protocol-1.5 `turn.prompt` has neither UUID nor `promptId`;
  its stable fallback is the native timestamp. Assistant IDs are native step
  UUIDs and tool-result IDs are native call `parentUuid` values.
- Kimi protocol 1.4 is explicitly unsupported. The real
  `/Users/gabrielvoicu/.kimi-code/sessions/wd_btb_3cabe80dc1f7/session_159aa36f-e114-4bef-a9d2-144efdb84c10/agents/main/wire.jsonl`
  ends at line 1590 with `step.end/end_turn` and no `turn.ended`; it now
  returns unknown instead of busy forever.
- Readiness invalidates a fork/replacement even though its historical items
  remain readable. Malformed interior JSONL fails closed; only an unterminated
  malformed final append is tolerated.

### Round 3 corrections (2026-09-07, asteria CM1–CM10, fixed by hyperion)

The table above is round 2. Round 3 changed these semantics; where the two
disagree, this list wins.

- Cursors are opaque. `itemsAfterCursor(kind, items, cursor)` is the exported
  cursor API; comparison stays inside the adapter dispatch, and OpenCode uses
  the native event sequence, not `time`, for snapshot, item and settlement
  cursors (two real tool results in `ses_f87e22f72ffewC2qJ2dAyyfPe1` share
  one timestamp while their completion events are sequence 47 and 48).
- Pi: `stopReason:stop` is only a completion candidate. Settlement is
  `session.quiet_window`, 120 seconds after the last append with no open
  tools, always `derived`. Pi emits `agent_settled` only in memory
  (`@earendil-works/pi-coding-agent/dist/core/agent-session.js:347`, after
  retry, compaction and queued continuations at `:772`); its maximum
  provider backoff is 60 s (`settings-manager.js:610`). The real
  `triton-jade-fern` session shows two 429 records then success with no
  user turn between.
- Claude: cancellation is recognised only by the exact native record shape
  (the exact marker content plus `interruptedMessageId`), never by a
  substring; `popAll` queue records are honoured, so the `1b09…` session
  reports an empty queue after its final non-continuing stop hook at line
  8320. Fragments are deduplicated by native record identity, never by text.
- Kimi: `prompt.accepted` is outstanding admission and invalidates readiness
  the moment it appears, reconciled with the `turn.prompt` that follows
  (real wire, lines 870–871). Fragments deduplicated by native identity.
- OpenCode: cancellation is deliberately unsupported. `MessageAbortedError`
  is `failed:true, cancelled:false` until a supported-version native
  cancellation fixture establishes the shape (the real database holds 14
  such rows, none from a supported version).
- Malformed tails: an incomplete JSON prefix at the end of a file is
  tolerated as an append in progress; invalid syntax (`definitely-not-json`,
  `{"type":!}`) returns unknown.
- No `process.env` default anywhere in the module; `env` is an explicit
  parameter the entry point passes.
- Tests: identity asserted unchanged on every positive fixture, completion
  results exercised through readiness, a competing SQLite writer between two
  reads of one snapshot, the fork fixture populated. 38/38 completion,
  33/33 readiness, 25 fixtures.

| Harness | Completion evidence | Settlement boundary | Provenance |
|---|---|---|---|
| Codex | `AgentMessage phase:final_answer`, matching `task_complete.last_agent_message` exactly | `task_complete`; `turn_aborted` for verified cancellation | Native |
| Claude | Fragments grouped by native `message.id`; `end_turn` alone is insufficient | `system.stop_hook_summary` with `preventedContinuation:false`, empty queue/tools/hooks | Derived for successful turns; native API-error and exact interrupt records settle their terminal states |
| Pi | `stopReason:stop` is only a candidate | `session.quiet_window`, 120 s after the last append with no open tools | Always derived |
| Kimi | `step.end finishReason:end_turn`; admitted prompts and tools must be clear | `turn.ended`; `prompt.accepted` invalidates readiness immediately | Native |
| OpenCode | `message.data.time.completed` plus `finish` or terminal error | `message.updated` event carrying `time.completed` (native event sequence 15 in `completion-window.json`) | Native |

## P7 — a crash-released exclusive lock from Node on macOS (probed 2026-09-07, lead)

`fs.openSync(path, O_RDWR | O_CREAT | 0x20 /* O_EXLOCK */ | O_NONBLOCK, 0o600)`
on macOS 25.5 with Node 26.8.1: a second open with the same flags in the
SAME process fails with `EAGAIN`; a child process opening the same file
fails with `EAGAIN` while the parent holds the fd; closing the fd lets the
next open succeed. Node passes the raw flag through to `open(2)`, whose
BSD `O_EXLOCK` takes a `flock`-style exclusive lock released with the
descriptor — so a crashed owner releases it. This is the store's ownership
primitive from round 5 on; Linux has no `O_EXLOCK` (later spec).
