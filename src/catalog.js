import { AGENT_PRESETS, agentProfile } from '../hosts/lib/presets.js'

export { agentProfile } from '../hosts/lib/presets.js'

/**
 * Ready-made agents, per tool.
 *
 * Typing a model identifier and guessing an effort level is the friction
 * between installing this and using it, so every harness ships a curated
 * list: every agent in it is in the roster already, exactly as the catalog
 * has it, and moves with it when a release moves an entry (2026-09-23).
 *
 * **One list, derived.** These are the payload's own presets — the very
 * records that run an agent — reshaped into the manager's vocabulary
 * (kind→harness, thinking/effort→effort). Until 2026-08-21 the manager kept
 * a second, hand-written list of 22 names: the merge that brought the
 * payloads into this repo ended the duplicated engine but not the
 * duplicated catalog, and the two drifted until five names meant different
 * models on the two sides — `nike` was GPT-5.6-luna to the app and Gemini
 * 3.7 Flash to the harness. A name must mean one model, so the harness's
 * list won: it is the superset, and it is what actually launches the run.
 *
 * Pi Claude presets use OpenRouter API, as selected by the user.
 */

/** Effort levels each CLI accepts, quoted from its own help output. */
export const EFFORTS = {
  // claude --help: "Effort level for the current session (low, medium, high, xhigh, max)"
  claude: ['low', 'medium', 'high', 'xhigh', 'max'],
  // The API enum is none…max; `ultra` is a codex-CLI level above it (verified live).
  codex: ['minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
  // pi --help: "Set thinking level: off, minimal, low, medium, high, xhigh, max"
  pi: ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'],
  // opencode --help: "provider-specific reasoning effort, e.g., high, max, minimal"
  opencode: ['minimal', 'low', 'medium', 'high', 'xhigh', 'max'],
  // Devin writes the level into its model id (claude-opus-5-5-max): an agent
  // names the family and one of these, and the launch joins them.
  devin: ['low', 'medium', 'high', 'xhigh', 'max'],
}

/**
 * The payload speaks in kinds, the manager in harnesses. `image` has no
 * harness here on purpose: an image agent is generated through the
 * Codex backend rather than launched as a CLI, so the roster cannot create
 * one and offering it as a quick-add would hand the user a dead button.
 */
const KIND_TO_HARNESS = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  devin: 'devin',
  image: 'image',
}

function entryFor(preset) {
  const effort = preset.effort ?? preset.thinking
  return {
    name: preset.preset,
    model: preset.model,
    ...(effort ? { effort } : {}),
    // `label` is the one-line headline ("Claude Code Fable 5.1 MAX"); the
    // preset's own prose is kept alongside for the card that wants it.
    description: preset.label ?? preset.description,
    detail: preset.description,
    profile: agentProfile({ harness: KIND_TO_HARNESS[preset.kind], model: preset.model, effort }),
    // Provenance, as a row an older build saved names its entry: the roster
    // reads such a copy as the catalog's own agent.
    preset: preset.preset,
  }
}

export const CATALOG = AGENT_PRESETS.reduce((catalog, preset) => {
  const harness = KIND_TO_HARNESS[preset.kind]
  if (harness === undefined) return catalog
  if (catalog[harness] === undefined) catalog[harness] = []
  catalog[harness].push(entryFor(preset))
  return catalog
}, {})

export function catalogEntry(name) {
  for (const [harness, entries] of Object.entries(CATALOG)) {
    const entry = entries.find((e) => e.name === name)
    if (entry !== undefined) return { ...entry, harness }
  }
  return undefined
}
