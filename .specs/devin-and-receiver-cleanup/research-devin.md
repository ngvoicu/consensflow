# Research Notes — Devin roles and receiver integration
## Date: 2026-09-12
## Researcher: Spec Mint TDD research subagent

## Project Architecture

ConsensFlow is a Node ESM CLI/service inside a Tauri macOS application. Stock harnesses run in PTYs. `src/launch.js` issues separate coordinator, controller, and native receiver capabilities. `hosts/lib/inbox.js` owns immutable results and per-part claim/receipt state; `hosts/lib/receiver.js` is the common native fetch loop. Native integrations supply the selected session, readiness, and actual insertion. `src/delivery-watch.js` indexes worker history and verifies native receipts; it no longer submits answers itself.

This investigation is bounded to adding Devin. It does not authorize a different UI or a weaker delivery promise. No installed ConsensFlow files, real sessions, credentials, or settings were changed.

## Tech Stack & Dependencies

- Root manifest: ConsensFlow `3.0.0-alpha.61`, Node `>=20`, ESM, `ws 8.21.3` (also resolved in lockfile), Biome `2.5.8`.
- App manifest: Tauri CLI `^2.11.4`, xterm `^6.0.0`, Playwright `^1.63.0`, esbuild `^0.25.10`.
- Installed executable: `/Users/gabrielvoicu/.local/bin/devin`, Mach-O arm64, **`devin 3000.6.14 (18033302)`**.
- No Devin SDK or dependency is required to speak JSON-RPC over its supported stdio ACP interface.

## Relevant Code Analysis

### Files Examined

Targeted reads covered root/app manifests and lockfiles; `src/launch.js`, `src/inbox.js`, `src/claude-install.js`, `src/channels.js`, `src/delivery-watch.js`; `hosts/lib/receiver.js`, `hosts/lib/runners.js`, `hosts/lib/state.js`, `hosts/lib/completion.js`, `hosts/claude-receiver.mjs`; and existing inbox, receiver, Claude receiver, Codex session and OpenCode installation tests. This is an integration-seam review, not a second exhaustive repository audit.

### Key Patterns Found

- The receiver is the component that knows the actual selected native conversation. Registering the most recent database session would violate existing ownership rules.
- Native API acceptance is not sufficient for a received result. The service compares a complete, uniquely framed claim against actual native history.
- Receiver capability is private to one coordinator launch/pane/generation. Workers must not inherit it.
- Private immutable integration installation already exists; Devin should use the same installation convention and supplied `--config` file, without a project `.devin` directory or global edits.
- Source completion and receiver receipt are different responsibilities. A successful ACP request does not by itself justify marking all observed assistant chunks complete.

### Data Models / Schemas

**Verified from a private database created by the installed native binary**, not inferred from a third-party library:

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  working_directory TEXT NOT NULL,
  backend_type TEXT NOT NULL,
  model TEXT NOT NULL,
  agent_mode TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_activity_at INTEGER NOT NULL,
  title TEXT,
  main_chain_id INTEGER,
  shell_last_seen_index INTEGER DEFAULT 0,
  cogs_json TEXT,
  workspace_dirs TEXT,
  hidden INTEGER NOT NULL DEFAULT 0,
  metadata TEXT
);
CREATE TABLE message_nodes (
  row_id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  node_id INTEGER NOT NULL,
  parent_node_id INTEGER,
  chat_message TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  metadata TEXT,
  FOREIGN KEY (session_id) REFERENCES sessions(id),
  UNIQUE(session_id, node_id)
);
CREATE TABLE tool_call_state (
  session_id TEXT NOT NULL,
  tool_call_id TEXT NOT NULL,
  tool_call_json TEXT,
  tool_call_update_json TEXT,
  PRIMARY KEY (session_id, tool_call_id)
);
```

Other tables: `prompt_history`, `rendered_commits`, `app_state`, migration history and SQLite sequence. No generic persisted session `busy` or `turn_completed` field was observed.

The active history is the ancestor chain from `sessions.main_chain_id` through `message_nodes.parent_node_id`. A `session/load` in the probe rebuilt nodes using new numeric IDs, retaining the same `chat_message.message_id` and `message_nodes.metadata.extensions["compact/prior_node_ids"]`. **Do not parse all rows as current history, and do not use numeric node IDs as immutable answer identity.**

A verified native user message has this shape:

```json
{
  "message_id": "<uuid>",
  "role": "user",
  "content": "LOCAL_RESEARCH_MARKER_ONLY",
  "metadata": {
    "num_tokens": null,
    "is_user_input": true,
    "request_id": null,
    "metrics": null,
    "finish_reason": null,
    "extensions": {
      "agent-ext/title-source-text": "LOCAL_RESEARCH_MARKER_ONLY",
      "chisel/acp-content-blocks": [{"type":"text","text":"LOCAL_RESEARCH_MARKER_ONLY"}]
    },
    "created_at": "<ISO timestamp>",
    "telemetry": {"source":"user","operation":"unknown"}
  }
}
```

Verified SessionStart/UserPromptSubmit `additionalContext` persists as a **system** message: `message_id`, `role:"system"`, `content:<complete marker string>`, metadata with `finish_reason:null`, `created_at`, and `telemetry:{source:"system",operation:"unknown"}`. There is no hook-event discriminator in that persisted message. The existing receipt parser must not assume it appears as a user/tool entry. Any mapping into receipt evidence needs the exact private claim body, session and ID; arbitrary system messages are not receipts.

**Not verified:** genuine assistant response serialization, successful completion boundary, tool-result persistence, interrupted assistant fragments or native compaction after a model response. An unauthenticated probe prompt returned an authentication error and produced no assistant response. The binary contains serialization field names `tool_calls`, `tool_call_id`, `thinking`, `reasoning_details` and metadata `finish_reason`; these strings are not evidence of a complete assistant answer or the meaning of a finish value.

### API Routes / Endpoints

Native RPCs actually exercised, with `protocolVersion:1` and no authentication:

- `initialize {protocolVersion:1,clientCapabilities:{},clientInfo:{name,version}}` succeeded.
- `session/new {cwd:<absolute private workspace>,mcpServers:[]}` succeeded. Session IDs are short native names, e.g. `relic-bonsai`, not UUIDs.
- `session/prompt {sessionId,prompt:[{type:"text",text:<local marker>}]}` was accepted by the protocol and **rejected for missing authentication before any model call**. A user-history entry still persisted. Thus native history receipt means context admission, not successful model processing.
- `session/load {sessionId,cwd:<same absolute workspace>,mcpServers:[]}` succeeded and replayed history.
- **`session/resume` returned `-32601 Method not found`**. Generic protocol names in binary strings are not a capability. Use `session/load` for this version.

Advertised native capabilities: `loadSession:true`, `promptCapabilities:{image:true,audio:false,embeddedContext:true}`, session list/delete/additionalDirectories, MCP HTTP/SSE false. Modes: `accept-edits` (Code), `ask` (Ask), `plan` (Plan), `bypass` (Bypass Permissions). No `sessionCapabilities.resume` or `close` was advertised. Unauthenticated model options were empty, so no model catalog or effort levels were validated.

A replayed user chunk was:

```json
{
  "method": "session/update",
  "params": {
    "sessionId": "relic-bonsai",
    "update": {
      "sessionUpdate": "user_message_chunk",
      "content": {"type":"text","text":"LOCAL_RESEARCH_MARKER_ONLY"},
      "_meta": {
        "cognition.ai/clientMessageId": "<same native message UUID>",
        "cognition.ai/messageSubIndex": 0,
        "cognition.ai/timestamp": "<ISO timestamp>"
      }
    }
  }
}
```

### Test Coverage

There is no Devin-specific production adapter or acceptance proof yet. Existing receiver tests already cover lease changes between claim and insertion, complete multipart bodies, source/receiver ownership and uncertain outcomes. Devin needs those same tests, plus native message-chain reconstruction and replay deduplication.

## Internet Research

### Best Practices

Use the protocol’s advertised capabilities, not a method list from generic serialization code. ACP clients receive streamed chunks and a final `session/prompt` response. The official protocol’s normal terminal result is `stopReason:"end_turn"`; cancellation and other stop reasons must remain distinct. A client that owns the active session can queue inbox work until its current prompt settles. [ACP prompt lifecycle](https://agentclientprotocol.com/protocol/v1/prompt-turn)

Loading is the supported history-replay operation when `loadSession` is advertised; resume and close require their own advertised capabilities. This matches the installed Devin probe, which supports load and refuses resume. [ACP session setup](https://agentclientprotocol.com/protocol/session-setup)

### Library Documentation

Devin officially supports `devin acp` as a JSON-RPC stdio server for ACP-aware editors. Its own documentation warns this is not an interactive terminal command. Existing CLI credentials can be used; authentication should remain Devin’s responsibility. Native flags include explicit `--resume <id>`, `--prompt-file`, private `--config`, `--export` after turns, and `--model`. [Devin command reference](https://docs.devin.ai/cli/reference/commands)

Documented hooks include SessionStart/End, UserPromptSubmit, Stop, PostCompaction and tool hooks. They carry `session_id` and per-turn `prompt_id`. Hook `additionalContext` supports startup/prompt context. **No FileChanged hook or independent idle callback is documented.** [Hook overview](https://docs.devin.ai/cli/extensibility/hooks/overview), [Hook lifecycle](https://docs.devin.ai/cli/extensibility/hooks/lifecycle-hooks)

Private user configuration can hold hooks and disable config imports from other tools. Devin may automatically migrate old inline `mcpServers` config into a separate file; a ConsensFlow-specific configuration should not copy that obsolete field and trigger writes into an unintended directory. [Configuration reference](https://docs.devin.ai/cli/reference/configuration/config-file)

### Security Considerations

Native acceptance probes used a private home/config/data/cache/state/tmp directory plus a macOS sandbox denying non-loopback network and writes outside the probe root (except `/dev`). No credentials were copied. Background feature-fetch attempts were denied; no account or model calls occurred. Config, logs, SQLite and hook observations stayed beneath `~/.consensflow/tmp/devin-research`.

`ACP_BACKEND` is **model-backend/credential-policy selection**, not a stock-TUI ACP transport override. In this build, setting `ACP_BACKEND=openai` aborts with “Unknown backend type: openai”, even though the accompanying error recommends that exact value for OpenRouter. Do not build tests or a product API route around that misleading suggestion.

### Community Patterns

Devin’s official Zed integration uses the editor’s ACP interface; it explicitly says rendering differs from the standalone CLI and some richer interactions exist only in the CLI. This demonstrates a supported integration route, not stock-TUI parity. [Devin in Zed](https://docs.devin.ai/cli/acp/zed)

## Library Comparisons

No new package choice is necessary. The material choice is who owns the interactive session.

| Approach | Exact selected session | Idle result arriving after turn | Native CLI UI | Assessment |
|---|---|---|---|---|
| Stock TUI plus private hooks | Hook session identity at supported events | No documented wake callback; next user turn can collect | Yes | Insufficient for the existing automatic idle-delivery promise |
| ConsensFlow ACP host | Host creates/loads/selects exact native session | Host can fetch while idle and issue session/prompt | No, needs a ConsensFlow chat surface | Strongest supported protocol route, contingent on approved UI choice |
| Separate ACP process loading the TUI’s session | A different process can load history | Does not prove active TUI receives/owns injected turn | Superficially | Reject: recreates stale background-session delivery |
| PTY text injection / Outposts bridge | Draft/selection fence not established | Possible raw input only | Yes | Reject: reintroduces old correctness problem |

**Recommendation:** If an ACP chat pane is acceptable, use stock `devin acp` with the common receiver and explicit host session ownership. If the exact standalone TUI is mandatory, do not promise automatic idle receipt on the currently verified hooks; that is a missing native capability, not an installation task. A long blocking Stop hook is not equivalent to a documented idle receiver and needs explicit cancellation/lifecycle proof before consideration.

## UI/UX Research

The ACP route requires normal chat rendering, user prompt entry, live assistant/tool updates, approval prompts, interruption, model/mode choices and native history replay. It must not be presented as the same terminal pane while silently removing these functions. User choice is a prerequisite for replacing the standalone terminal interaction. PM advisor categories do not themselves select Ask/Plan mode: those modes are native behavior controls with their own write limitations, and PM must retain specification-writing authority.

## Test Infrastructure Analysis

### Current Setup

Node’s built-in test runner (`npm test`), private filesystem fixtures, local HTTP/WebSocket boundary fakes, Playwright browser tests and Rust/bridge integration tests. The root/source manifest contains no configured coverage gate, mutation tool or Testcontainers dependency. No `.github/workflows` directory was present during this bounded scan. Keep the current testing stack; a new testing library is unnecessary.

### Test Quality Assessment

Existing tests are behavioral and target important races: native selection changing after claim/begin, complete framing, immutable installation, no global settings changes, and ownership. Synthetic adapter tests do not substitute for native successful assistant completion/history proof.

### Recommended Testing Stack

| Tool | Recommendation | Rationale |
|---|---|---|
| Node test runner | Keep | Already exercises receiver and adapter contracts |
| Fake stdio ACP subprocess | Add fixture for Devin adapter | Tests request IDs, replay, cancellation, prompt serialization and errors without paid calls |
| Private native Devin process | Keep as separate acceptance gate | Confirms hooks/schema/protocol actually supported by installed binary |
| Native SQLite | Read-only fixture parser | Must traverse main chain and deduplicate stable message IDs |
| Playwright | Keep if ACP UI chosen | Covers visible results, model/mode and permission interactions |

## Risk Assessment

- **Critical capability gap:** hooks alone do not establish a reliable idle wake mechanism in stock TUI.
- **History identity:** load changes numeric nodes; reading all rows duplicates old branches and context.
- **Receipt semantics:** failed unauthenticated prompts still persist user/context messages; receipt is admission, not a successful response.
- **Completion:** no genuine native assistant completion has been verified, so a production source parser must fail closed until that proof exists.
- **Credentials/billing:** account-dependent model discovery was deliberately not queried; no guaranteed model or effort catalog can be asserted from this probe.
- **Storage:** `CHISEL_SESSION_DB` is an undocumented override but empirically created/used the private specified database. Native default storage strings suggest `XDG_DATA_HOME/devin/cli/sessions.db`; that exact default file was not created or verified here. Use the explicitly supplied path for ConsensFlow rather than guessing the default.
- **Compatibility:** pin/document tested native version and detect missing capabilities. Strings from generic Rust protocol structs are not runtime guarantees.

## Open Questions

1. May Devin use a ConsensFlow-owned ACP chat pane, or must it retain the stock TUI? Main agent is handling this user decision.
2. How can successful native model output be simulated in this Devin build? The OpenAI override is rejected; no paid/account trial is authorized for this research. Unit fixtures remain possible, but full successful native completion is unverified.

## Local Evidence

- Probe script: `/Users/gabrielvoicu/.consensflow/tmp/devin-research/probe-acp.py`
- Hook script: `/Users/gabrielvoicu/.consensflow/tmp/devin-research/hook-probe.py`
- Latest native wire log: `/Users/gabrielvoicu/.consensflow/tmp/devin-research/acp-native/wire.json`
- Native probe database: `/Users/gabrielvoicu/.consensflow/tmp/devin-research/acp-native/sessions.db`
- Hook inputs: `/Users/gabrielvoicu/.consensflow/tmp/devin-research/acp-native/hooks.jsonl`
- All owned probe processes were terminated normally. No real session content was inspected.

## Research Completeness Checklist

- [x] Relevant manifest/dependency and module seams inspected
- [x] Existing receiver and native integration tests inspected
- [x] More than three focused web searches; primary documentation used for conclusions
- [x] Native version/help/ACP handshake/new/load/hooks/private persistence verified
- [x] Protocol and hook approaches compared; risks documented
- [x] No installed application or real session changes
- [ ] Genuine native assistant completion, interruption and multipart result efficacy
- [ ] Stock TUI idle wake callback or supported ACP transport override
- [ ] Account-specific model/effort catalog
