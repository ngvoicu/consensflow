import { PARTICIPANT_PRESETS } from '../hosts/lib/presets.js'

/**
 * Ready-made participants, per tool.
 *
 * Typing a model identifier and guessing an effort level is the friction
 * between installing this and using it, so every runtime ships a curated
 * list: pick a name, get a working participant. `cf participant add zeus`
 * needs no flags; the UI offers the same lists as one-click adds.
 *
 * **One list, derived.** These are the payload's own presets — the very
 * records that run a participant — reshaped into the manager's vocabulary
 * (kind→runtime, thinking/effort→effort). Until 2026-08-21 the manager kept
 * a second, hand-written list of 22 names: the merge that brought the
 * payloads into this repo ended the duplicated engine but not the
 * duplicated catalog, and the two drifted until five names meant different
 * models on the two sides — `nike` was GPT-5.6-luna to the app and Gemini
 * 3.7 Flash to the runtime. A name must mean one model, so the runtime's
 * list won: it is the superset, and it is what actually launches the run.
 *
 * Verified live 2026-08-21: all 16 OpenRouter ids exist in
 * openrouter.ai/api/v1/models; claude-fable-5, claude-opus-5 and
 * claude-haiku-4-5 each answered a real `claude -p`; pi lists every
 * `openai-codex/gpt-5.6-*` id. The five `anthropic/claude-*` presets on pi
 * (orpheus, linus, erato, kronos, atlas) are correct but need pi's
 * anthropic provider configured — `pi auth check --provider anthropic`.
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
}

/**
 * The payload speaks in kinds, the manager in runtimes. `image` has no
 * runtime here on purpose: an image participant is generated through the
 * Codex backend rather than launched as a CLI, so the roster cannot create
 * one and offering it as a quick-add would hand the user a dead button.
 */
const KIND_TO_RUNTIME = { 'claude-code': 'claude', codex: 'codex', pi: 'pi', opencode: 'opencode' }

function entryFor(preset) {
  const effort = preset.effort ?? preset.thinking
  return {
    name: preset.preset,
    model: preset.model,
    ...(effort ? { effort } : {}),
    // `label` is the one-line headline ("Claude Code Fable 5 MAX"); the
    // preset's own prose is kept alongside for the card that wants it.
    description: preset.label ?? preset.description,
    detail: preset.description,
    // Provenance: what a roster row records so a later catalog change can be
    // offered as an update instead of silently diverging.
    preset: preset.preset,
  }
}

export const CATALOG = PARTICIPANT_PRESETS.reduce((catalog, preset) => {
  const runtime = KIND_TO_RUNTIME[preset.kind]
  if (runtime === undefined) return catalog
  if (catalog[runtime] === undefined) catalog[runtime] = []
  catalog[runtime].push(entryFor(preset))
  return catalog
}, {})

export function catalogEntry(name) {
  for (const [runtime, entries] of Object.entries(CATALOG)) {
    const entry = entries.find((e) => e.name === name)
    if (entry !== undefined) return { ...entry, runtime }
  }
  return undefined
}
