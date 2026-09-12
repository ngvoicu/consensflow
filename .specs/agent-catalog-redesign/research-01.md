# Research Notes — Agent Catalog Redesign
## Date: 2026-09-10
## Researcher: Spec Mint TDD research subagent

Read-only application research. Only this report was written. No model task was invoked, no home configuration changed, and no credentials were printed. Findings describe the current dirty alpha.45 tree; prior cleanup remains untouched. Parent research owns Artificial Analysis recommendations and image backend selection. **Latest user decision supersedes the earlier native Anthropic proposal: keep OpenRouter for Pi Claude models; this is a paid API route. Do not introduce native Anthropic Pi entries or pretend OpenRouter is a subscription route.**

## Project Architecture

- Native macOS Tauri application wraps a Node ESM loopback service. Agents is the service's `/` page inside the app; Harnesses is a separate `/harnesses` page. Neither is a coding-session tab.
- Canonical definitions are `hosts/lib/presets.js` (87 presets). `src/catalog.js` derives the manager-facing catalog; a second independently maintained model catalog would recreate an explicitly documented historical identity defect.
- Shared persistent roster is `~/.consensflow/agents.json`, schema version 1. Environment arguments point tests at temporary homes. Unknown document and row fields survive updates.
- Worker execution crosses roster → controller → native harness startup → initial task admission. Metadata shown by the catalog must match both one-shot and native app paths.
- No repository `AGENTS.md` or `.github` directory found. User-provided global instructions apply. Existing `.specs/standalone-panes-delivery` remains the prior workflow context.

## Tech Stack & Dependencies

- Node ESM, supported Node >=20; no runtime npm dependencies in root manifest.
- Root lock: Biome 2.5.8.
- App lock: Playwright 1.63.0, Tauri CLI 2.11.4, xterm 6.0.0, addon-fit 0.11.0, esbuild 0.25.12.
- Rust 2021, minimum 1.77.2; Tauri manifest 2.11.3, portable-pty 0.9, updater ~2.11.0.
- Installed Pi resolves to `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent`, version 0.85.1.
- OpenCode metadata checked at `/Users/gabrielvoicu/.cache/opencode/models.json`; provider transport source checked against public tag v1.18.30.

## Relevant Code Analysis
### Files Examined (24 project files, plus installed harness source/docs)

- `package.json`, `package-lock.json` — scripts and resolved root dependency.
- `app/package.json`, `app/package-lock.json` — build/browser test dependencies and scripts.
- `app/src-tauri/Cargo.toml` — native boundary and packaging version.
- `biome.json` — project formatting and lint scope.
- `app/scripts/bundle-ui.mjs` — local bundling, Safari 15 target.
- `hosts/lib/presets.js` — canonical IDs, model/effort definitions, provenance and drift rules.
- `src/catalog.js` — derived harness groups, model/effort vocabulary and catalog lookup.
- `src/roster.js` — schema-preserving read/add/edit/remove/sync behavior.
- `src/ui.js` — authenticated API and inline DOM rendering for both agent lists.
- `hosts/lib/runners.js` — one-shot arguments and interactive launch/resume functions.
- `bin/cf.mjs` — catalog-aware CLI add and native OpenCode task seeding.
- `src/channels.js` — per-harness native channel configuration.
- `src/channels/opencode.js` — HTTP initial-task admission boundary.
- `src/launch.js` — lead/PM capability scopes; category recommendations must not change these.
- `src/tabs.js` — lead/PM creation uses harness choice, not a catalog preset.
- `src/harnesses.js` — executable/home discovery and CLI vocabulary.
- `src/role-skills.js` — role instructions, independent of catalog suitability.
- `tests/catalog.test.mjs` — canonical parity, unique names, provider twins, pinned effort tiers.
- `tests/roster.test.mjs` — saved-field fidelity, custom entries, explicit catalog sync.
- `tests/ui.test.mjs` — real authenticated loopback routes and generated script checks.
- `tests/opencode-launch.test.mjs` — real local HTTP server boundary, startup/resume/cancellation.
- `tests/engine/runner-session.test.mjs` — fresh/resumed invocation arguments and billing guards.
- `app/tests/harnesses.spec.mjs` — real service/browser Agents CRUD and drift feedback.
- `tests/helpers.mjs` — isolated homes and guard against real-user state access.

### Key Patterns Found

1. **One canonical catalog.** `CATALOG` is derived from `AGENT_PRESETS`; add suitability metadata there or derive through one shared exact-model profile map. Do not create a UI-only alternative preset list.
2. **Name is not provenance.** `addAgent` prevents duplicate names, while `preset` tracks catalog origin. `editAgent` keeps `preset` even when the model/effort changes. `agentFromPreset` can produce renamed copies. An edited or renamed preset-backed row still counts as Already added for its original preset.
3. **Classification must describe the current row.** A row with `preset: zeus` may now run Fable, an unknown custom model, or a different effort. Resolve grouping and suitability from its actual harness/model/effort; use provenance only for add-state and explicit sync.
4. **Descriptions have two meanings today.** Preset `label` becomes saved roster `description`; preset prose becomes catalog `detail`. Updating prose alone avoids drifting saved rows. Practical descriptions should not overwrite custom saved descriptions during a read.
5. **No automatic roster migration.** Existing update actions call `syncAgents`; fetching/rendering catalog does not apply changed model definitions. Preserve that behavior for the user-approved Pi Fable move to OpenRouter 5.1 and any scoped Pi Opus route update.
6. **DOM safety.** `el()` assigns textContent. Keep names, descriptions, model strings, and category headings out of HTML interpolation.

### Data Models / Schemas

- Preset: `preset`, `id`, `name`, `label`, `description`, `kind`, `model`, optional `effort`/Pi `thinking`, optional policy fields.
- Manager view: `name`, `harness`, `model`, `effort`, optional `description`, `preset`, `unsupported`; API adds `command`.
- Persistent roster keeps `kind` and Pi `thinking`; do not migrate it to manager field names.
- `PRESET_OWNED_FIELDS` includes kind/model/effort/thinking/skillsPolicy/description. Suitability recommendations should remain derived metadata, outside launch authority and outside automatic syncing of custom fields.
- Recommended catalog state precedence: matching `preset` provenance → Already added (including renamed/customized copies); otherwise canonical name occupied → Name in use; otherwise Add. A legacy row lacking provenance can count as added only under an explicitly defined exact harness/model/effort/name match, never name alone.

### API Routes / Endpoints

- `GET /api/agents` — roster, drift, catalog, harness choices, global effort suggestions.
- `POST /api/agents` — create named row, then refresh role skill text.
- `PATCH /api/agents/:name` — edit mapped fields, preserve provenance/unknown fields.
- `DELETE /api/agents/:name` — remove one row.
- `POST /api/agents/sync` — explicit named/all preset update.
- All stay loopback and token gated. No external model-ranking service should be required to render Agents.

### Verified Compatible Low/Medium Matrix

| Harness | Astra model ID | Fable model ID | Low / medium evidence |
|---|---|---|---|
| Codex | `gpt-6-astra` | No native Fable route in this product | Existing canonical family; parent verifies current Codex model capabilities. |
| Claude Code | No native Astra route in this product | `claude-fable-5-1` | Anthropic effort docs and current canonical medium entry; existing preset audit records low CLI response. |
| Pi | `openai-codex/gpt-6-astra` | `openrouter/anthropic/claude-fable-5.1` | Installed 0.85.1 model store explicitly maps low→low, medium→medium on both routes. Fable is the user-approved paid OpenRouter route. |
| OpenCode | `openrouter/openai/gpt-6-astra` | `openrouter/anthropic/claude-fable-5.1` | Current models.dev cache explicitly lists low/medium/high/xhigh/max; v1.18.30 maps these options to provider variants. App transport fix required below. |

**Pi technical correction, separate from route choice:** The old comment at `hosts/lib/presets.js:45` treats omitted low/medium/high keys as unsupported. Installed 0.85.1 docs say omitted standard keys use provider defaults; explicit `null` means unsupported. Native Fable 5.1 has `thinkingLevelMap: {off:null,xhigh:"xhigh",max:"max"}`. The implementation's `getSupportedThinkingLevels` accepts low/medium; `mapThinkingLevelToEffort` sends their matching names. Native transport capability is not an authorization or subscription-availability claim. The user rejected that provider and chose OpenRouter; the final spec must use OpenRouter.

- Native Fable definition: installed `dist/bundle/chunks/chunk-JVUZSMYM.js`, `anthropic-messages` provider, `claude-fable-5-1`, `forceAdaptiveThinking:true`, `supportsMidConvoEffort:true`.
- Level resolver: installed `dist/bundle/chunks/chunk-IDDQWTHI.js`, `getSupportedThinkingLevels`/`clampThinkingLevel`.
- Transport: installed `dist/bundle/chunks/anthropic-messages-VWZZOSJQ.js`, `mapThinkingLevelToEffort`, `buildParams`, `insertThinkingLevelMessages`. Fable 5.1's per-turn effort is placed in message configuration; its top-level default `output_config.effort: high` is not evidence that selected low/medium was dropped.
- Astra Pi dynamic store explicitly maps low→low and medium→medium under `openai-codex`; `openai-codex-responses-R6VYZTVY.js` carries that into `body.reasoning.effort`.
- These are availability and transport findings, not an account/quota or live task success claim. No paid API calls were made.
- Pi's current OpenRouter store also includes `anthropic/claude-opus-5` with explicit low/medium/high/xhigh/max maps. `kronos` xhigh and `atlas` medium are technically compatible with `openrouter/anthropic/claude-opus-5`; any catalog route update must remain explicit via existing saved-roster sync.
- Neither inspected OpenCode Go catalog contains Fable. OpenCode Zen's public Pi Fable page lists metered token pricing; it does not establish subscription access. No subscription workaround is proposed.

All proposed nine new names are absent from the current 87 presets: `electra`, `maia`, `alcyone`, `merope`, `dagr`, `skirnir`, `terpsichore`, `musaeus`, `suttung`.

### OpenCode Effort Transport Gap

- `hosts/lib/runners.js:149` forwards model and `--variant` for one-shot execution.
- `interactiveStart` OpenCode branch at approximately line 598 forwards model but no effort. The native TUI path seeds via HTTP after launch.
- `bin/cf.mjs:666` calls `seedOpenCodeSession` with initial model but no effort/variant.
- `src/channels/opencode.js:208` accepts model/text, builds `/session/:id/prompt_async` body with text and model, and never includes variant.
- Public v1.18.30 `session/prompt.ts:1424` defines top-level `variant` on `PromptInput`. Fresh worker seed must carry the selected preset effort there. Do not add an unverified TUI CLI flag.
- **Concrete resume path:** after native server readiness, authenticated `GET /session/:sessionID` for the exact bound ID yields `Session.Info`: `agent?: string`, `model?: {id,providerID,variant?}`. Map native model.id to POST model.modelID, preserve providerID and native agent, and pass native model.variant as top-level variant. Use explicit `default` when native variant is absent/default so the server cannot substitute a configured agent's variant. Missing/malformed native model fails before task POST; never substitute the edited roster. The server schema is `packages/opencode/src/session/session.ts:216–244`; storage deliberately uses `variant: info.model.variant ?? "default"`. API accepts string variants; default has no named provider override. Add a default-versus-agent-default boundary test, because existing tests only prove omission and do not prove semantic preservation.
- **Concrete TUI continuation:** installed `opencode --help` has no TUI `--variant`. No new flag or custom default agent is needed. Public v1.18.30 `packages/tui/src/component/prompt/index.tsx:311–330` initializes local model and variant from the last user message when a session first has one; later human submissions send that local variant (around1099). Its `local.tsx` variant state uses `default` as its unset sentinel. Initial HTTP seeding therefore supplies both the worker task effort and the TUI's subsequent choice. Verify this with installed native acceptance; do not edit the user's global model.json.
- Lead/PM creation currently selects a harness only; no catalog preset enters that path. Recommendation-only categories require no lead/PM launch-role change. Audit shared helper callers but do not add direct catalog role launches.

### Test Coverage

- Relevant code already has real Node HTTP tests and real browser/service tests; no new test framework is justified.
- Catalog test currently pins Pi/OpenCode Astra efforts to exactly `[max,xhigh]`; update that contract deliberately to include low/medium.
- Existing medium Fable entries: thalia (Claude), erato (Pi), kvasir (OpenCode). Avoid duplicating them.
- Existing gaps: shared grouping/category behavior, Already added identity/collision cases, errors/double add, customized row classification, and native OpenCode effort delivery/preservation.

## Internet Research
### Best Practices

- [Playwright best practices](https://playwright.dev/docs/best-practices), read 2026-09-10: assert user-observable behavior, isolated state, role/label locators, and web-first assertions. New UI tests should exercise real catalog plus temporary roster, not snapshot implementation strings.
- [W3C grouped form controls](https://www.w3.org/WAI/tutorials/forms/grouping/), read 2026-09-10: label related controls and their groups. Provide distinct accessible names for Your agents and Ready-made toolbars; semantic group headings and real disabled buttons communicate state.

### Library Documentation

- [OpenCode models](https://opencode.ai/docs/models/), read 2026-09-10: provider/model identity and model-specific variants. Its example model list is explicitly not guaranteed current; use model metadata for exact current IDs.
- [OpenCode v1.18.30 transform.ts](https://raw.githubusercontent.com/anomalyco/opencode/v1.18.30/packages/opencode/src/provider/transform.ts), `reasoningVariants` around1590: reads `reasoning_options` effort values. OpenRouter transport uses `reasoning: {effort}`.
- [OpenCode v1.18.30 prompt.ts](https://raw.githubusercontent.com/anomalyco/opencode/v1.18.30/packages/opencode/src/session/prompt.ts), around610 and1424: top-level request variant becomes the stored user model variant; do not assume an omitted variant retains the session's prior value.
- [OpenCode v1.18.30 session schema](https://raw.githubusercontent.com/anomalyco/opencode/v1.18.30/packages/opencode/src/session/session.ts): GET session model uses `id`, providerID and optional variant; prompt model uses modelID instead.
- [OpenCode v1.18.30 TUI prompt](https://raw.githubusercontent.com/anomalyco/opencode/v1.18.30/packages/tui/src/component/prompt/index.tsx) and [local model state](https://raw.githubusercontent.com/anomalyco/opencode/v1.18.30/packages/tui/src/context/local.tsx): session entry restores variant from native last user message; subsequent human submissions include the selected variant. Verified by read-only direct public source fetch.
- [Pi model configuration](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/models.md), read 2026-09-10; installed matching documentation at `docs/models.md:261`: omitted/explicit/null thinking maps have different semantics.
- [Anthropic effort](https://platform.claude.com/docs/en/build-with-claude/effort), read 2026-09-10: effort availability and per-turn support are model-specific; named presets should identify the actual model version.

### Security Considerations

- Scope introduces local display metadata, not new authorization. Preserve token/origin checks and textContent rendering.
- No new external runtime service or npm dependency; no reason to add a credentials table, API key editor, or home configuration mutation for grouping.
- Model identity/provenance cannot be inferred from callsign alone. Never silently overwrite a custom row on Add or on a catalog read.

### Community Patterns

- OpenCode derives variants from model metadata and provider transforms; use its actual schema instead of a universal global effort enum as capability proof.
- Pi keeps model-level capability maps separate from the CLI's global allowed level names. ConsensFlow needs the same conceptual distinction for curated descriptions, without importing a new runtime model registry.

## Library Comparisons

No new library is needed; this is an existing small vanilla DOM page.

| Option | Fit | Pick |
|---|---|---|
| Existing DOM helpers and native selects | Matches current code, no added runtime/bundle dependency | Yes |
| New component framework/data-grid dependency | Requires architecture and bundling changes outside this task | No |
| Runtime third-party benchmark/model feed | Adds network dependency and unstable categorization during use | No; reviewed static metadata instead |

Popularity/download comparisons are not useful here because no technology replacement is proposed.

## UI/UX Research

- Current hierarchy is Your agents cards, Ready-made offers grouped by harness, then Define your own. Preserve the two lists and custom form.
- Apply the same category vocabulary and grouping algorithm to both lists. Canonical model grouping must normalize verified aliases across harness providers while retaining route/harness on cards.
- Distinguish off, no effort support, and unspecified effort. Kimi takes model-configured effort and images have no reasoning effort; neither should be mislabeled Low.
- Lead and PM are recommendations, potentially overlapping with Coding. Unknown custom models need an honest uncategorized/general bucket and remain discoverable.
- Keep actionable empty states separate: no roster rows, no filter matches, catalog already added. Added rows must remain visible under search and grouping.
- Preserve active filter/group settings during load/add/remove/edit refreshes. Disable pending Add immediately and surface server rejection; the current callback ignores non-OK responses.

## Test Infrastructure Analysis

### Current Setup

- Node built-in `node:test` + strict assertions; 57 `.test.mjs` files across tests including engine/integration areas.
- Three Playwright spec files under `app/tests`; `npm --prefix app run test:ui` bundles then runs all three.
- Real temporary JSON filesystem state; real loopback HTTP service; boundary fake executables and local HTTP servers. No database service is introduced by this feature.
- Root `npm test` excludes integration subtree; `npm run check:all` adds Rust tests/clippy, browser tests, integration, and smoke.
- No `.github` CI directory, Docker/testcontainers, mutation tool, or coverage gate found in inspected manifests/scripts/config. This is a bounded finding, not a claim about external CI.
- Existing shared describe fixtures sometimes depend on earlier mutations (`tests/roster.test.mjs`); new cases should use independent temporary data.

### Recommended Testing Stack

Keep Node tests, real temporary roster/service, and Playwright. Mock at executable/HTTP boundaries; add no database, coverage, or component-testing package for this request.

### Concrete Tests for the Spec

1. Every catalog entry resolves a valid display profile; catalog/preset model and effort parity remains exact. Unique preset IDs, including nine new names.
2. Exact low/medium matrix: Astra on Codex/Pi/OpenCode; Fable5.1 on Claude/Pi/OpenCode. Native incompatible pairings absent. Preserve existing medium identities; Pi Fable uses the user-approved OpenRouter provider with paid-API description.
3. Pi roster effort writes to thinking; fresh and resumed runner invocations pass low/medium verbatim. Fable version moves only on explicit sync; custom/no-preset rows stay unchanged.
4. Both lists group by harness/model/effort and filter by identical categories, retain state on CRUD, and show correct counts. Unknown model, missing effort, off, minimal, image, and Kimi cases remain visible in honest groups.
5. Adding a Ready-made row leaves it visible with disabled Already added; deleting the last matching preset copy restores Add. Renamed/customized preset copies remain Already added. Same-name unrelated custom row shows Name in use; exact model tuple with different identity does not accidentally disable unrelated presets.
6. Add pending/duplicate click, server error, and concurrent name collision preserve saved data and report failure; no swallowed API errors.
7. Category/description changes never mutate saved custom fields, generate launch tickets, or change lead/PM scopes. Malicious names/descriptions/model strings render as text.
8. Local OpenCode HTTP boundary receives selected variant on fresh startup alongside exact provider/model. Resume reads the exact native session and preserves its model/variant/agent, even after roster edit. Explicit default does not inherit an agent's nondefault variant; malformed/missing native identity sends zero task POSTs. Cancellation/uncertain admission remains one POST with no replay. Native TUI restores the seeded variant for a later human turn.
9. Controller process test proves `row.effort` reaches native seed, not just that the helper can serialize a manually supplied variant. One-shot/interactive invocation parity verified for all affected harnesses.
10. Browser keyboard labels, grouping headings, disabled state, no-match/reset behavior, practical descriptions visible without hover; installed app visual check after build/reinstall.

## Risk Assessment

- Breaking changes: catalog rename or provider switches damage identity and account expectations. Keep current names, add only nine, use the user's selected OpenRouter route for Pi Claude, clearly label paid API billing, and sync saved rows explicitly.
- Correctness: classifying by stale preset provenance mislabels edited agents; parsing model strings too broadly merges unrelated provider aliases. Use verified exact identities.
- Execution: OpenCode app effort currently drops at initial task admission; metadata additions alone do not satisfy the user's coherent-work requirement.
- Performance: fewer than100 presets after additions; local grouping/filtering is trivial. No pagination/grid dependency or external fetch warranted.
- Migration: display metadata can be derived; no roster schema migration required. User-triggered existing sync applies the approved Pi Fable provider/version update; reading the new catalog must not mutate saved rows.
- Validation limit: source/catalog evidence proves supported transport, not successful account access or model quality. No live model tasks were run during research.

## Open Questions

- Parent owns the accepted decision to keep Pygmalion's Codex login and curates recommendations from Artificial Analysis. No further user clarification needed for this report's scope.
- Implementation must verify the documented OpenCode exact-session/default-variant behavior at the installed native boundary; the concrete schema and TUI path are established above.

## Research Completeness Checklist

- [x] Manifest/lock files and directory/module structure examined
- [x] 15+ relevant application/config files and 5+ tests read
- [x] Preset → roster → UI → native launch dependency chain traced
- [x] More than three primary-source web searches and documentation reads
- [x] Installed Pi/OpenCode model-specific metadata checked without credentials
- [x] Library alternatives considered; no added library recommended
- [x] Security, accessibility, performance, persistence and migration risks considered
- [x] Test runner, isolation, mocking boundaries and browser infrastructure assessed
- [x] Docker/testcontainers, coverage/mutation and repository CI presence checked within relevant configuration
- [x] Concrete TDD cases and exact model/effort transport gap documented
