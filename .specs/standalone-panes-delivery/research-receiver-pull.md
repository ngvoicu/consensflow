# Receiver-driven result inbox

Research date: 2026-09-12. Status: proposed design, not implementation or installed acceptance. Research used primary documentation and the current working tree. No native harness was launched, no model was called, and no installed application, running process, live ledger, settings or native history was changed. The working tree contains changes newer than the reported alpha.61 incident.

## Decision after isolated native experiments

**Invest in a common result inbox and receiver-ownership refactor across all four harnesses, reusing the native integrations. Do not replace automatic delivery with a model polling loop, and do not discard the existing receipt ledger.** This is a recommendation for the next implementation, not a migration applied to the product.

The initial research-only scope above is superseded for the experiments in this section: separate stock native TUIs were launched with isolated state and localhost mock providers. No installed/running ConsensFlow, real-session records or real-model accounts were changed or used. The original research and the experimental evidence are distinct.

| Harness | Receiver integration tested | Complete native/context evidence |
| --- | --- | --- |
| Claude Code 2.1.269 | Private SessionStart/Stop hooks with asyncRewake | Six results: first/second/third, busy, clear/resume and draft preservation; separate actual background continuation delivered to the successor while its predecessor remained alive. |
| Codex 0.154.0 | Existing private broker plus deterministic inbox fetch and native `turn/start.toolOutput` | Five results across idle, successive answers, new/resume and busy; zero model-selected fetch/tool calls. |
| Pi 0.85.1 | Native extension using its own current session and displayed custom messages | Five independent results across idle/busy/new/resume; another owner's result remained untouched. |
| OpenCode 1.18.30 | Private TUI integration using its current route and native prompt API | Five independent results across idle/busy/new/resume; home route held results until a concrete session existed; another owner's result remained untouched. |

Each pass above means the full identified body was observed in native history and an actual request to the local mock provider. It does not establish real-model understanding, production installation, statistically greater reliability or a completed migration. No controlled current-production A/B failure-rate benchmark was performed. These are mechanism and lifecycle feasibility tests, informed by the user's reported production failures.

Reports and runnable evidence: [Claude](probe-pull-claude.md), [Codex](probe-pull-codex.md), [Pi and OpenCode](probe-pull-pi-opencode.md).

Two negative results reject a simplistic rewrite. Codex kept an interrupted shell reader alive across `/new`; the naive helper fetched a result under the old thread but it reached neither native tool output nor model context. Pi duplicated a result when a prototype retried before native persistence became observable. Therefore fetching cannot mark an answer received, and absent immediate evidence cannot authorize replay. The existing uncertainty and part-coverage rules remain necessary.

The substantive change is **who owns pending work**: immutable answer IDs belong to the logical Lead/PM group; its current receiver claims them using verified native identity. Availability must not depend on a cached destination session in app state. Pi already polls an inbox, OpenCode already selects from its native route, and Codex already owns selection in its broker. Reuse these components. Claude hooks are the largest mechanism change. This is one common storage/claim/receipt protocol with four native adapters, not four independent delivery architectures.

Visibility is a separate product requirement. Claude collapsed the hook content; Codex's deterministic full-body native tool-output path emitted none of the five bodies in the TUI although all five entered history and model input. Pi/OpenCode emitted their short result bodies, but terminal rendering is still not durable app visibility. Implement source-worker/advisor badges from distinct unconfirmed answer IDs and an owning Lead/PM results history with full content. Every completed reply throughout the worker/advisor conversation has its own status, with no first-N or latest-only tracking limit. Receipt of one answer never hides any other unconfirmed answer. Human viewing must not mark lead receipt.

Before release, the same four-harness gate must cover switch exactly between claim/insertion, concurrent receivers, crash after insertion before ACK, restart/reconciliation, large multipart output and compaction. Additional adapter gates remain: Claude long-idle watcher lifetime and explicit continuation-versus-fork authority; Codex atomic broker admission; OpenCode draft/modal preservation. Preserve accepted and uncertain historical attempts through migration. No blind replay, no native binding by cwd/recency, and no silent claim takeover by a leftover process.

The next implementation should first add these shared failure regressions around the existing ledger and adapters, then add the common inbox projection/receiver claim path and visible result history. Keep the current installed application unchanged until a separately tested build is explicitly selected for live testing.

## Recommendation

Compare the three alternatives below across **all four harnesses before choosing a migration**. The recommended direction is a durable result inbox owned by the PM or lead group, fetched through the receiver's normal tool result or its private native integration. ConsensFlow owns the inbox and its visible history. Native wake mechanisms are an optional latency improvement, not the only way an answer can be discovered. This research does not recommend another one-harness patch or immediate replacement of the current transport.

This improves the failure boundary: a stale sender-side session address cannot make an answer disappear. It does **not** make native identity, idle wakeup, crash recovery or acknowledgements unnecessary. Merely telling the model to poll `cf results` is insufficient: models can stop, omit checks or consume only the first result.

Suggested behavior:

1. Every completed worker/advisor answer becomes an immutable inbox item, independently of transport availability.
2. The running receiver checks at session start/resume, safe turn boundaries and before declaring the delegated work complete.
3. A private watcher/long poll notifies the receiver when new answers exist; it makes no model request when the inbox is empty.
4. A supported native wake starts or queues a turn when idle. When that capability is unavailable, the item remains visibly waiting and is collected at the next supported boundary or explicit fetch.
5. The native conversation receives a clearly attributed result message or exact, paginated reader output. The app always exposes the full answer, including when native rendering hides integration messages.

The same mechanism serves Lead/workers and PM/advisors, with different owner identities and permissions. It must not route an advisor answer into the parent lead.

## Alternatives to compare

| Alternative | What actually moves the answer | Common across four harnesses | Main limit |
| --- | --- | --- | --- |
| A. Current native push | App chooses a native destination and submits an envelope/pointer | Common ledger, different native transports | Correct destination and admission can diverge from the visible receiver; native rendering can hide content |
| B. Receiver calls `cf fetch` / bounded `cf wait` | An ordinary shell/tool call returns stored answer parts to its own calling conversation | All four can execute a local CLI and receive its tool result | The model must invoke the tool and remain in a usable tool-call lifecycle; an idle model with no pending tool cannot spontaneously fetch |
| C. Private receiver integration | Hook/extension/broker fetches from the same inbox at safe boundaries; native wake handles idle state | Shared inbox protocol, harness-specific lifecycle adapters | More integration work; each native version and conversation-switch path needs acceptance evidence |

`cf fetch` and `cf wait` are proposed command names, not existing capabilities. B is the simplest common transport to prototype first. It removes the need to choose a socket destination for the answer body: the native harness returns the ordinary tool output to the conversation that invoked it. It does not prove that the model acted on the output, that the TUI visibly expanded it, or that a subsequent native session inherited it.

For B, return a batch manifest and bounded answer parts immediately if available; otherwise wait up to a fixed deadline and return `no results yet` without acknowledging anything. Completion of answer 1 does not end the contract for outstanding answers 2/3. Long polling should wait on inbox change, not repeatedly invoke a model. Handle cancellation, harness command timeout, backgrounded shell tools and partial output explicitly. A retained CLI process from a retired conversation must not silently claim results for a successor. A server-side owner token authorizes access but is not, by itself, evidence of which native conversation received a tool result.

C should use exactly B's result IDs, pagination and receipt protocol rather than create another transport-specific inbox. Start by checking at native turn boundaries, then add idle wake only where documented and verified. A model instruction saying “always fetch before finishing” is a behavioral rule; a tested native stop/boundary integration can enforce a deterministic check. Do not report the two as equivalent guarantees.

## What already exists

The current ledger already has an `answerId`, multiple delivery attempts/copies, manual reads and receipt-based acceptance. `readResult` is a useful starting point; preserve its immutable bodies and part coverage instead of replacing all delivery storage. A read attempt is deliberately weaker than a verified receipt. Sources: [`delivery-watch.js`](../../src/delivery-watch.js#L436), [`store.js`](../../src/store.js#L1078).

Existing private integrations provide concrete insertion points:

- Codex: a ConsensFlow broker observes main-thread start/resume/fork requests and rechecks the selected thread before forwarding a native queue request. It is already more than a transcript watcher. [`hosts/codex-session.mjs`](../../hosts/codex-session.mjs#L136)
- OpenCode: a TUI integration reads the actual selected route and calls `session.promptAsync`, rechecking identity immediately before submission. [`consensflow-session.mjs`](../../hosts/opencode-extension/consensflow-session.mjs#L24)
- Pi: its extension watches a private inbox, checks native session/editor state, sends messages and observes their entry into the run. It already resembles a receiver-driven transport, but consumes sender-addressed delivery envelopes. [`consensflow-delivery.mjs`](../../hosts/pi-extension/consensflow-delivery.mjs#L360)
- Claude: the channel sends native peer messages and then verifies transcript receipts. The current source now contains explicit continuation handling; that is a working-tree fact, not proof of a corrected installed release. [`claude-peer.js`](../../src/channels/claude-peer.js#L190)

The current pane badge filters by the **destination pane** and keeps only `pending`/`waiting` after selecting the latest delivery copy. Consequently, a source worker has no equivalent complete per-answer view, and uncertain/submitting answers can fall outside this badge. Source: [`panes.js`](../../app/ui/panes.js#L351). This is a concrete UI gap regardless of transport choice.

## Confirmed native capabilities and proposed adapters

### Claude Code

Confirmed: hooks receive `session_id` and `transcript_path`; `SessionStart` covers startup/resume/clear/compact. `UserPromptSubmit` and tool/stop hooks offer context-delivery boundaries. `Stop` feedback can continue a turn, with loop protection. An `asyncRewake` command hook can wake idle Claude by exiting 2; ordinary asynchronous hook output waits for another turn. Async hooks are not deduplicated, and async context/output need not appear in the user's transcript view. These are documented capabilities, not evidence that the exact installed version and continuation path satisfy our requirements. [Official hooks reference](https://code.claude.com/docs/en/hooks)

Proposed: load private hook settings with the session's `--settings` argument, which accepts a file or inline JSON without modifying global/project settings. Preserve the user's existing hook configuration when merging. [Official CLI reference](https://code.claude.com/docs/en/cli-reference)

Use native hook identity for inbox fetches. A bounded, deduplicated `asyncRewake` wait can issue a result-available reminder; synchronous fetch at the next appropriate hook returns the data. Do not keep a predecessor hook authoritative after its conversation retires. Hook inheritance, callback order, renewal after timeout, cancellation during `/clear` and transfer into a `continued-in` background successor require isolated acceptance tests. The inspected documentation does not establish that transfer. Until verified, a continuation must retire old routing and show waiting results rather than silently choosing a background agent.

### Codex

Confirmed: app-server supports explicit `thread/start`, `thread/resume`, `thread/fork`, `thread/read`, `turn/start`, and `turn/steer` with `expectedTurnId`. Current official docs also describe `thread/inject_items` for persisted model-visible history without starting a turn, and `turn/start` with standalone `toolOutput`. Dynamic tools are experimental. None of these method names alone proves the stock TUI displays injected content or that the installed server implements every current method. [Official Codex App Server documentation](https://learn.chatgpt.com/docs/app-server)

Proposed: extend the existing broker, using the TUI-selected **thread ID**, not only the session-tree root ID. Fetch at selected-thread binding and turn boundaries. A registered reader tool can provide model-initiated pull; supported native tool-output/queue submission can wake or feed a turn. Capability-check the installed schema in an isolated probe before choosing these newer methods. Preserve the user's existing automatic-delivery preference until an accepted replacement is available. Never resume a second independent backend just to inject into the first backend's conversation; it does not establish that the visible TUI shares that runtime.

### OpenCode

Confirmed: session APIs expose message history, session status and asynchronous message submission. `prompt_async` returns HTTP 204 rather than a completed answer. Plugins receive session events, including idle/status, and can expose tools. [Official server API](https://opencode.ai/docs/server/), [Official plugins documentation](https://opencode.ai/docs/plugins/)

The current v2 TUI documentation exposes current route/tab and rendering APIs; its names differ from this repository's current `api.route.current` integration. Do not transplant the v2 API into an older harness without version-specific verification. [Official v2 TUI plugin API](https://opencode.ai/v2/docs/build/plugins/cli/)

Proposed: extend the existing private TUI receiver to fetch owner-scoped inbox items, then submit through its native client for the selected session. The server-side idle event alone is not proof of the conversation shown by this TUI. Recheck route/revision after fetching and before admission; read back the exact returned/native message ID and content. HTTP success is admission evidence, not a full receipt or a display acknowledgement. Keep the draft intact; do not use append/clear/submit-prompt to simulate typing.

### Pi

Confirmed: current extension documentation describes shutdown/reload/start on `/new` and `/resume`, with fresh session context; captured old contexts must not be reused. `before_agent_start` can add a persistent message; `context` runs before model calls. `sendMessage` supports attributed custom messages, `display: true`, idle `triggerTurn`, and `steer`/`followUp`/`nextTurn` timing. Plain `appendEntry` does not enter model context. [Official Pi extension source documentation](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/extensions.md)

Proposed: reuse the private extension's watcher and lifecycle cleanup. Fetch using the fresh native session context and persist a custom result message carrying result IDs. Prefer an attributed message over text that appears user-typed. Register its renderer for native visibility, then confirm actual insertion/context evidence separately from the void send call. Verify compatibility with the installed Pi API; the current integration references Pi 0.85.1 types, while fetched upstream documentation can change.

## Identity, acknowledgements and recovery

Proposed inbox identity: `(owner group, worker conversation, worker native session, answerId)`, with a stable body hash. `deliveryId` remains an attempt/receipt identifier; manual reading or retrying the same answer must not create another logical answer. Count every result: successful receipt of answer 1 never hides answers 2 or 3.

Maintain a receiver lease with owner, launch, pane generation, native conversation ID, and monotonically increasing binding revision. A fetch is authorized only for that owner. Any native switch invalidates the previous lease, even when its process remains alive. A continuation can transfer ownership only through explicit predecessor/successor evidence plus trusted launch ownership; cwd, recency, model name and a still-running process are insufficient. `/new`/resume select a new receiver epoch for the same owner group. Historical receipts remain attached to the actual receiving epoch; they are not rewritten to look as though the new conversation received old content.

Record separate evidence:

| Fact | Required evidence | User-facing meaning |
| --- | --- | --- |
| Available | Complete immutable worker answer stored | Waiting for lead/PM |
| Fetched | Receiver obtained identified parts/body hash | Being collected |
| In conversation | Exact result/parts found in that native receiver's history or authoritative native insertion event | Received by lead/PM |
| Included in a model request | Supported pre-request evidence for the full identified body | Optional diagnostic detail; never infer understanding |
| Displayed | App result panel rendered, or native renderer acknowledgement | Display evidence only; not model receipt |

Do not promote `lastRead`, HTTP success, a queue acknowledgement, hook completion, or worker idle state to full receipt. An ACK must identify the receiver epoch and all answer parts. An old-epoch ACK may add historical evidence but cannot authorize new sends or confirm successor receipt. Repeated fetches are idempotent; incomplete coverage stays incomplete.

There is no general atomic transaction spanning the inbox ledger and four native histories. A crash after native insertion but before ACK creates uncertainty. Reconcile by stable message/result ID before retrying. Preserve uncertain attempts; neither drop them nor blindly replay them. Keep model-context state separate from human-view state and from any optional lead summary.

## Visible result history

Add a source-worker/advisor badge counting distinct answers without a complete recipient receipt, including waiting, collecting and uncertain states. Example: `2 results waiting`, even when an earlier answer succeeded. The owning PM/Lead header aggregates the same ledger projection.

A result drawer beside the coordinator should retain all answers, showing source name, answer number/time, receipt state and expandable full content. That gives the user reliable access when Claude hides integration content or receives only a reader pointer. Opening this drawer marks only human viewing, never receipt by the lead. Do not rely on the language model voluntarily echoing every worker answer.

For the reported Claude symptom, the available evidence supports separating receipt and visibility; this research did not inspect the live lead's terminal and cannot establish which native rendering rule hid its initial replies.

## Acceptance before changing the running product

Decision gate: run the same A/B/C mechanism comparison across all four harnesses; do not adopt a one-harness migration or extend the old delivery design while that comparison is unresolved. Result projection/history can be designed independently of transport.

Minimal isolated probes, proposed and **not run in this research**:

1. Create one private probe root under `~/.consensflow`, with distinct native homes/config/state/cache paths, loopback-only local provider fixtures and dummy credentials. Use the exact installed harness binaries, but independent native processes and no connection to the running ConsensFlow app, its controller or its real ledgers. Record binary version and capability schema/help. Preserve the short Unix-socket path requirement already enforced by Codex.
2. For B on each harness, have the local fixture request the normal shell tool running the proposed bounded reader. Add three inbox answers sequentially. Capture native tool-call ID, complete returned parts, native conversation ID and raw request payload sent back to the mock provider. Interrupt/switch after fetch but before ACK; prove no result becomes silently accepted and the successor can recover without altering predecessor evidence. This proves mechanics, not whether a real model reliably chooses to call the reader.
3. For C on Claude, use private `--settings` hooks and test ordinary boundary fetch plus `asyncRewake` while idle. Reproduce `/clear`, `/resume` and explicit `continued-in` ownership; after retirement, the old callback cannot wake/ack for the successor. Existing local Claude fixtures use `--safe-mode`, which disables hooks: that fixture must be adapted inside the probe, not run unchanged. [Official CLI reference](https://code.claude.com/docs/en/cli-reference)
4. For C on Codex, place the test broker around a separate stock TUI/app-server pair. Enumerate supported methods, test normal reader tool return first, then only supported injection/tool-output/queue wake methods. A selected-thread revision change between fetch and submit must prevent insertion. Capture the message in native history **and** the next mock-provider request; separately observe whether the TUI renders it.
5. For C on OpenCode, load the private test extension only in a separate TUI. Test route changes before/after fetch, idle `promptAsync`, and native message readback. For Pi, use a private test extension with custom message rendering and `triggerTurn`, then test its shutdown/reload/start context replacement. Neither probe may edit a global plugin directory or project config.
6. On every harness, leave the inbox empty for a measured interval and assert zero provider requests; add one answer and assert one wake/consumption, then second and third answers each appear with independent receipts and badges. Test a busy receiver and a nonempty typed draft without clearing or submitting that draft.

Only after the isolated mechanical comparison passes should a separately authorized real-model trial measure behavioral compliance for B: immediate fetch, repeated fetch, bounded wait renewal, partial-read completion and “all workers answered” accuracy. Mock models cannot establish that real models follow these instructions. No paid calls are required for the mechanism decision gate.

Broader acceptance cases for the selected design: app resume; concurrent workers; same preset in PM and Lead; partial reads; crash between insertion and ACK; duplicate wake; hidden grid; unavailable native API. Required user-visible output: three answer cards, matching per-source counts, and explicit uncertainty where proof is missing.

Claude additionally needs the incident-shaped `continued-in` successor in another process group while the predecessor remains alive, including complete manual-read receipts in the successor. Verify hook settings and lease ownership survive or are explicitly re-established. Preserve uncertain predecessor submissions.

Publish separate outcomes for complete answer storage, native receipt, model-request inclusion when observable, and visible result rendering. Current app remains untouched until the owner explicitly permits replacing/restarting it; this note does not authorize deployment.
