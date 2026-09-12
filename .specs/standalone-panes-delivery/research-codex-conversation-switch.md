# Codex conversation switching, retirement, and restart

Research date: 2026-09-11. Native binary: `codex-cli 0.154.0`. Source pin: `openai/codex` tag `rust-v0.154.0`, commit `36eab01061df3cde5f95ec20a526777b430091ba`.

## Finding and proposed direction

There is a concrete native integration path that preserves automatic delivery: run the stock Codex TUI against an app-owned native app-server through a private broker. The broker can correlate the TUI's successful main-conversation start/resume RPCs and serialize ConsensFlow queue admission against them. An isolated native prototype passed fresh startup, native new/clear/resume, both resume pickers, and process restart. This is a medium architectural change to propose for approval, not an implemented product fix or an instruction to disable Codex delivery.

Restarting alone does not retire a saved conversation as a queue target. After killing and restarting the native backend, `thread/loaded/list` returned an empty list, but stock `codex queue --thread OLD` still succeeded. The old queue entry later appeared in OLD's transcript when OLD was resumed. Codex's queue accepts persisted, unarchived roots even when they are not loaded. [Pinned queue admission source](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/app-server/src/request_processors/thread_queue_processor.rs#L251).

The remote TUI and app-server are real stock CLI interfaces. Their protocol is version-specific and the official documentation labels remote WebSocket transport experimental/unsupported for production; that is a compatibility and maintenance cost, not evidence that integration is impossible. No harness binary modification, global configuration mutation, or hook-trust bypass is necessary for the proposed broker. [Official app-server and remote TUI documentation](https://learn.chatgpt.com/docs/app-server).

## Native evidence

All probes used a temporary `CODEX_HOME`, temporary workspace, real PTY, installed native binary, and a local HTTP Responses stub. No real remote model requests, user history edits, global settings edits, or trust changes occurred. The only repository change from this research is this note.

Primary passing run:

- Script: `/tmp/cf-codex-main-restart-probe5.py`.
- Log: `/tmp/cf-codex-main-restart-probe5.log`; process exit **0**.
- Artifacts: `/private/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-codex-switch-w9vnsx8c`.
- Artifacts include `protocol.jsonl`, `screen.bin`, backend logs, and native rollouts.
- A: `01a09094-938f-7fd1-a2d3-315cf92b4559`; B: `01a09094-a559-7db0-bf50-e2309856c3c0`.
- Ten local provider requests; zero remote model requests. Initial idle identity was obtained with **zero** provider requests.

| Native case | Observed result |
|---|---|
| Fresh idle TUI | Exact A returned by its correlated `thread/start` response before any prompt/model request. |
| Automatic queue to fresh idle A | Accepted; complete marker in A's native transcript. No seed turn required. |
| Native `/new` | Exact B returned while still idle. Old A queue rejected before forwarding. |
| TUI restart, `codex resume B`, after B was persisted | Exact B restored; new current-target queue marker appears only in B. |
| Native `/resume` picker | Picker selected A; original TUI connection issued `thread/resume A`; B rejected afterwards. |
| Native `/resume B`, B already loaded | Exact B restored through loaded-thread rejoin. |
| Native `/resume` with nonexistent UUID | Native rejection preserved B; no guessed replacement. |
| Backend and TUI stopped/restarted | New backend's loaded-thread list was empty. |
| Direct stock queue to unloaded old A | Accepted and visible in native `thread/queue/list A`; restarting did not retire A. |
| App-style restart with explicit `codex resume B` | Exact B restored; broker rejected A and admitted B. |
| CLI startup `codex resume` picker | Native chosen B established from actual resume RPC. |
| Native picker cancelled | Main B unchanged; temporary picker connection closure did not invalidate B. |
| Native `/resume CFMainAlpha` and `/resume CFMainBeta` | Native exact-name lookup switched A then B; broker followed RPC results. |
| CLI `codex resume --last` | Native chose A; broker bound A from the response, without implementing recency inference. |
| CLI `codex resume CFMainAlpha` | Exact A restored; B rejected and A admitted. |
| Native `/clear` | New idle C returned; A rejected. |
| Native `/new CFMainNamed` | New idle D returned. |

Five retired-target attempts were refused in this run. None of their markers exists in any native rollout. Three B delivery markers occur only in B; the initial and later A markers occur only in A. The deliberately bypassed old direct queue marker was eventually consumed by A, proving why runtime shutdown cannot substitute for admission retirement.

Earlier bounded evidence remains useful:

- `/tmp/cf-codex-old-queue-probe.log`, exit 0: stock queue writes OLD after `/new`; both old/new rollout FDs remain open.
- `/tmp/cf-codex-main-restart-probe.log`: restarting an **empty** B and requesting `codex resume B` fails with `no rollout found for thread id`, even while the original backend is retained. B had an in-memory identity but no resumable history yet. Restart-on-every-new therefore breaks a valid quiet startup case.
- `/tmp/cf-codex-main-restart-probe3.log`, exit 0: baseline expanded picker/restart matrix.
- Prototype iterations exposed and corrected two test/integration assumptions: picker connections are auxiliary; native `--last` need not choose the app's last displayed ID. The passing run follows native selection rather than making either assumption.

## Main lead conversation versus native subagent focus

Codex distinguishes `primary_thread_id` from the currently viewed/active thread. A successful main new/resume replaces the chat widget and assigns the new primary ID. Native subagent navigation can switch a cached event channel and change `active_thread_id` without changing primary identity or issuing another resume RPC. [Main replacement](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/session_lifecycle.rs#L985), [primary assignment](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/thread_routing.rs#L1392), [cached subagent selection](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/session_lifecycle.rs#L515).

For a ConsensFlow lead, a sensible proposed contract is: native `/new`, `/clear`, and `/resume` change the lead's conversation; merely viewing a native child does not redirect external worker results into that child. This preserves the lead as the receiving agent. It needs explicit product agreement if the existing phrase “current displayed conversation” was intended to mean the child currently on screen. Delivering to the main lead while its child is viewed can produce a background receipt, so a main-root guarantee must not be presented as an always-visible receipt guarantee.

The prototype's version-pinned classifier uses non-ephemeral `thread/start` with `threadSource:user` for fresh main starts and configured `thread/resume` requests carrying `runtimeWorkspaceRoots` for main resumes. Native cached-subagent attachment uses `PreserveExistingThread`, which builds a minimal resume request rather than the configured main-resume request. Native title generation starts separate ephemeral system threads and must be ignored. [TUI request construction](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_server_session.rs#L2010), [minimal rejoin](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_server_session.rs#L2056).

The classifier is supported by the pinned implementation, not a promised permanent semantic marker in the public protocol. Main start/resume paths were exercised natively; built-in subagent focus and `/side`/`/fork` interactions still need native acceptance tests before release. Explicit `/fork` should be treated as a separate main/side classification case, not inferred from the last fork response. The current prototype intentionally does not promote fork responses.

Both `/resume ID-or-name` and the in-session picker converge on `resume_target_session`, which issues the configured native resume. CLI startup resume uses the same server resume mechanism after native target selection. [Direct resume dispatch](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/event_dispatch.rs#L317), [picker convergence](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/session_picker.rs#L70), [startup resume](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/startup.rs#L447).

## Proposed private broker and admission contract

1. Give each Codex pane one launch-owned supervisor/broker and native app-server. The stock TUI remains the displayed interactive application and connects with `--remote`. Prefer a private Unix-socket endpoint for production; the disposable probe used loopback WebSockets. Keep the backend's transport private to the supervisor.
2. Give TUI traffic and ConsensFlow delivery separate authenticated ingress roles. `clientInfo.name` cannot establish identity: native TUI and native `codex queue` both identify as `codex-tui`. An in-session resume picker also opens a temporary connection for reads, then the original TUI connection sends the actual resume. Track the owner of successful main requests within the **pane launch generation**, not the newest connection.
3. Serialize main switch requests and queue admission through one broker state machine. At a main switch request, hold subsequent queue admissions. Correlate response IDs and only publish a new conversation after a successful native response. A definitive unsuccessful switch can retain the old target; transport loss, unknown outcome, or read-only fallback must remain unknown until native evidence resolves it. Disconnection of an auxiliary picker must not retire the main owner.
4. Recheck pane ID, generation, launch identity, current input epoch, target UUID, and selection revision in that same admission operation. Never read current identity in one independently racing component and launch the queue helper later without another comparison. An old target gets an affirmative zero-byte refusal before its message is forwarded.
5. After new B is confirmed, retarget only untouched pending deliveries to B. Preserve original UUID/submission identity for any previous possible admission to A. Receipt checking continues against A without replaying it into B.
6. A successful native `thread/queue/add` response proves queue admission and supplies a queued-submission ID; it does not prove the lead has consumed the message. Preserve the native receipt check. Timeout or disconnect after forwarding is uncertain, not a retry-safe refusal. Retiring A also does not cancel A messages accepted earlier.

The probe uses a lock around switch/queue forwarding and keeps native request IDs correlated. It is evidence for the mechanism, not production crash/race validation: transport-error handling, external-writer fallback, multiple pending switches, input arbitration, backpressure, and exact forwarding/ack fault injection still require tests.

## Process lifecycle and restart choices

Prefer a **per-pane** native backend for the first implementation. It has an explicit owner, exact launch configuration, predictable cleanup, and contains that pane's native root/subagent tree. A proxy to an existing shared Codex daemon could save processes but brings shared configuration, independent clients/writers, and lifecycle ownership beyond the pane. No shared-daemon live acceptance was performed; it should not be the initial shortcut.

A bundled pane supervisor can be the process Rust launches in the PTY. It should spawn the stock TUI with inherited PTY stdio and spawn the native backend with private pipes/socket, without `detached:true`, `setsid`, or daemonization. Both remain in the existing pane process group. Normal supervisor exit must shut down/reap both children and remove its private socket; Rust's existing pane-group kill remains the fallback. This matches the existing [`pty.rs` cleanup contract](../../app/src-tauri/src/pty.rs#L952). The disposable Python probes used separately isolated process groups and explicit cleanup; they do **not** prove the production panePGID integration.

Do not restart on every `/new` or `/resume`. Admission retirement already solves the wrong-target problem while preserving drafts, active turns and native child state. Use restart for app lifecycle/recovery, with the last confirmed **main** UUID. After full restart, wait for a successful native resume before opening automatic admission. For an empty unpersisted thread, exact resume can fail; the product must explicitly preserve/restore its draft and create a replacement empty conversation, or retain the live backend. It must not pretend the old UUID was resumed.

Archiving OLD would make native queue refuse it, but would also mutate the user's session archive and change resume behavior. Deleting history is unnecessary. Neither is proposed as automatic delivery retirement.

## Configuration and login fidelity

The native remote path is not equivalent to appending `--remote` to the existing launch and assuming all settings transfer.

| Setting | Required handling / evidence |
|---|---|
| Codex login | Use the same `CODEX_HOME` and normal auth environment for backend and TUI; do not copy credentials or select a new account. Preserve existing `OPENAI_API_KEY` removal used for subscription login. Actual account acceptance was not exercised by the credential-free probe. |
| Provider/config/profile/cwd | Backend must receive the pane's immutable launch configuration/profile and cwd. Remote TUI deliberately omits `modelProvider`, and only a subset of config is transmitted through RPC. Shared daemon defaults can therefore differ from pane settings. |
| Model and effort | TUI start includes model and its config includes reasoning effort/summary/verbosity. `/new` reloads server defaults while preserving explicit launch overrides; resume can restore saved thread settings or apply intentional launch model overrides. Do not blindly overwrite every resumed thread. |
| Lead/PM developer instructions | Forward existing role configuration to the backend as appropriate and preserve the TUI's `developerInstructions` payload. Verify full instruction content on fresh, `/new`, and resume, including already-loaded roots whose overrides may be ignored. |
| Skills, MCP, plugins | Same config roots/launch environment are necessary because remote requests do not transmit an arbitrary copy of local configuration. Native discovery and startup behavior must be checked per role; not covered by the simple provider probe. |
| Permissions/sandbox | Remote resume restores saved server permissions and rejects some explicit permission overrides. Split backend configuration from TUI-only flags deliberately; do not remove user security policy or substitute permissive defaults to make remote startup work. |

Sources: [remote provider omission](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_server_session.rs#L343), [config allowlist and effort fields](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_server_session.rs#L1817), [remote resume permission behavior](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_server_session.rs#L2106), [new-session defaults](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/new_session.rs#L41), [loaded-thread override behavior](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/app-server/src/request_processors/thread_processor.rs#L4239), [existing login guard](../../hosts/lib/runners.js#L520).

Current ConsensFlow launch wiring matters: Codex role configuration adds `-c developer_instructions=<existing instructions plus role>`; fresh worker startup supplies model, effort, and its existing sandbox/approval flag; interactive resume starts with `resume UUID` and then role reinjection. A broker must preserve these distinctions. Pass the full role developer instructions to the backend explicitly, not only the TUI, and verify their complete content in the first provider request after fresh launch, `/new`, and resume. [Role configuration](../../src/role-skills.js), [native runners](../../hosts/lib/runners.js#L542), [lead launch assembly](../../src/panes.js#L1740).

These are design requirements derived from pinned source, not a claim that production role/config fidelity is already proven. The local probe proved only its isolated local provider/config path.

## Alternatives investigated

`SessionStart` hooks are queued during session construction and run with a turn context; loaded-thread resume can rejoin without constructing another session. `SessionEnd` does not fire immediately on switch. Exact trusted hooks therefore do not establish every idle selected conversation. No hook-trust changes were made. [Session construction](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/core/src/session/session.rs#L1616), [hook execution](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/core/src/hook_runtime.rs#L124), [official hooks](https://learn.chatgpt.com/docs/hooks).

The native session logger enabled with `CODEX_TUI_RECORD_SESSION` / `CODEX_TUI_SESSION_LOG_PATH` can show that an action was requested, but not its successful target identity. Inbound logging records new/clear or an app-event variant, before dispatch; serialized `AppCommand` has no thread ID. It also records user prompt content, so enabling it as permanent identity telemetry adds unnecessary data capture. It does not replace the broker's correlated successful native response and admission gate. [Logger](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/session_log.rs#L148), [event sender](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_event_sender.rs), [outbound command shape](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app_command.rs#L27).

## Size, dependencies, and next decision

This is a medium multi-file integration, not a one-line refresh of the saved UUID. It needs a bundled supervisor/broker, WebSocket/Unix transport and JSON-RPC correlation, process lifecycle wiring, a deliberate config split, a current-session admission channel, watcher retargeting/receipt integration, and native acceptance coverage. The root package currently has no production JavaScript dependencies. A Node implementation would likely add a maintained WebSocket dependency; a Rust implementation would need the corresponding transport support. The Python `websockets` used by probes is not a proposed shipped dependency.

A stdio native app-server behind the broker could avoid a separate backend WebSocket listener, but multiplexing multiple native TUI connections onto one stdio client would need its own correctness work. The tested topology retains one upstream native WebSocket connection per client. Choose that simpler ownership model first if the design is approved, then consider transport simplification.

Proposed next step: approve a bounded Codex native-broker design and settle the main-lead-versus-viewed-child contract, then forge implementation tests for lifecycle/config fidelity and admission races. Keep current automatic delivery behavior while this design is under review; do not silently ship a disablement, a hook-trust change, or the larger redesign as part of research.

## Implementation follow-up: configuration and native focus proof

The user subsequently confirmed implementation authorization; the main agent is implementing the broker. This follow-up is bounded native evidence for that work, with no production-file changes by this research agent.

Two additional native probes completed with **exit 0**, using the same installed `0.154.0` binary, temporary account home/workspace, and only the local Responses provider:

- Config/fork/side: `/tmp/cf-codex-config-focus-probe6.py`, log `/tmp/cf-codex-config-focus-probe6.log`, artifacts `/private/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-codex-switch-v767dm9q`. Fourteen local model requests; zero remote requests.
- Native subagent focus: `/tmp/cf-codex-subagent-focus-probe3.py`, log `/tmp/cf-codex-subagent-focus-probe3.log`, artifacts `/private/var/folders/5f/cy8ywl5d2_z_1g1zcn45g7gr0000gn/T/cf-codex-switch-fcext4ly`. Eleven local model requests, including the native child's calls; zero remote requests. The stub emitted one native `multi_agent_v1.spawn_agent` call, without a model override; its child inherited the local provider.

### Exact configuration result

The first matching provider request on fresh launch, `/new`, already-loaded `/resume`, `/fork`, cold app-server resume, and a stock embedded `codex resume` baseline contained the exact **5,220-character** combined existing developer sentinel and full checked-in ConsensFlow lead role document. This was an actual `role:developer` input item, not merely skill discovery or an abbreviated sentinel match. All six had model `cf-choice` and reasoning effort `high`. The role was passed through `-c developer_instructions=...` to **both** backend and TUI. Native start/resume RPCs carried `developerInstructions:null`, demonstrating why backend role injection is necessary; fork RPCs included the full developer text.

The passing launch split is:

1. Preserve normal `CODEX_HOME`, cwd, provider, profile/config, auth environment, and the app's existing `OPENAI_API_KEY` removal. Give backend and TUI the exact role developer instructions. Model/effort overrides reach the backend as native `-c` settings and the TUI through the existing model/effort arguments.
2. For a fresh worker whose original launch already requested `--dangerously-bypass-approvals-and-sandbox`, give the backend `-c approval_policy="never" -c sandbox_mode="danger-full-access"`. Remove that flag from the remote TUI. The native app-server CLI does not accept the interactive flag.
3. Backend defaults alone are insufficient: the remote TUI initially sends its local home sandbox policy in `thread/start`, overriding the backend default. For that original fresh worker launch intent, the broker's authenticated main `thread/start` mapping must set `approvalPolicy:"never"`, `sandbox:"danger-full-access"`, and `permissions:null`. The probe verified native successful response and persisted turn context, not only the outgoing request. Initial start and `/new` then preserved the worker's existing danger/never behavior; loaded resume and ordinary fork preserved it too.
4. Leave native `thread/resume` permission parameters untouched. App resume launches receive no fresh-worker permission flags. The original worker bypass flag on the remote TUI made `/resume` fail before RPC with `Permission overrides are not supported when resuming a remote task.` Replacing it with TUI `-c` permission settings cannot fix that: the native guard checks session flags and selected-profile permission keys as well as interactive flags. [Native override guard](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/config_persistence.rs#L55).

The launch-permission mapping is deliberately narrower than a general policy rewrite. Native `/new` applies runtime permission overrides. A production guard must stop forcing the initial worker policy once a successful main resume or explicit native permission change establishes another policy. Remote permission selection uses `thread/settings/update`; retain request/response correlation and the native new-session parameters after that transition. This user-change boundary is source-backed and remains a product regression requirement; it was not exercised by the config probe. [Runtime carryover](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/new_session.rs#L94), [native permission update](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/config_persistence.rs#L324).

**Cold-resume nuance:** “restore saved permissions” does not mean restore every historical legacy sandbox value. Native resume restores persisted approval policy and an active named permission-profile ID. The probe's legacy danger-full-access launch had no active named profile. After restart, remote resume used the home `read-only` sandbox, even without backend permission overrides. A stock embedded `codex resume A` on the same isolated history/home produced the same read-only sandbox, `never` approval policy, saved model/high effort, and full role content. Thus this is native baseline behavior to preserve, not a reason to silently force old danger mode. Named-profile restoration is established by source, not claimed as native probe coverage here. [Persisted resume settings](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/app-server/src/request_processors/thread_processor.rs#L4131).

### Fork, side, and native subagent classification

| Native action | Exact observed wire behavior | Lead-target result |
|---|---|---|
| `/fork CFNativeFork` | `thread/fork` includes `threadSource:user` and runtime workspace roots; **omits** `ephemeral` because false is the native default. Successful result has `thread.ephemeral:false`. | Promote correlated fork ID. Queue to former main was refused before forwarding. |
| `/side CF_SIDE_QUESTION` | `thread/fork` explicitly sends `ephemeral:true`, then native `turn/start` addresses the side ID. | Keep parent main. Its automatic queue marker appeared only in the main history. |
| Ctrl+C closes side | Returns to parent; side is unsubscribed. | Main unchanged. |
| First `/subagents` picker focus on real native child | Minimal `thread/resume` has `runtimeWorkspaceRoots:null` and no configured main overrides; user `turn/start` addresses child. | Keep main parent. Child external queue rejected; parent queue accepted and consumed only by parent. |
| Picker returns to main, then focuses cached child again | Neither focus change emits `thread/resume`; actual typed markers have main/child IDs in `turn/start`. | Main target unchanged throughout. Child input markers occur only in child history. |

The fork classifier must therefore accept omitted/false `ephemeral` for a configured user fork, while excluding `true`; strict `ephemeral === false` misses stock `/fork`. Native side configuration explicitly sets `ephemeral=true`; regular fork replaces the main chat widget. [Side configuration](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/side.rs#L598), [regular fork dispatch](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/app/event_dispatch.rs#L351).

The child-focus probe verified all three typed input markers against exact native thread IDs and transcript locations, and the rejected external-child marker was absent from every rollout. Identity did not come from terminal text. These results establish the main-lead contract; they do not establish an always-currently-visible-child delivery contract. One exploratory `/resume CHILD` while CHILD was already displayed issued no configured resume, so an attempted command must never be treated as a successful main switch by itself.

The follow-up does not claim production process-group cleanup, real-account login, arbitrary skills/MCP/plugin configuration, named permission profiles, `/fork` from every possible focused child, or fault-injected queue races. Those remain appropriately scoped implementation/acceptance checks. The actionable native configuration split and main-versus-focus classification above no longer need speculative probes.
