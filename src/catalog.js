/**
 * Ready-made participants, per tool.
 *
 * Typing a model identifier and guessing an effort level is the friction
 * between installing this and using it, so every runtime ships a curated
 * list: pick a name, get a working participant. `cf participant add zeus`
 * needs no flags; the UI offers the same lists as one-click adds.
 *
 * Everything here was verified live on 2026-08-20 against the CLIs and the
 * OpenRouter catalog — model ids exist, effort levels are the ones each CLI
 * documents, and each family carries its newest release (glm-5.3 over 5.2,
 * qwen3.8-max over 3.7-max). Custom participants remain free-form: the
 * catalog is a convenience, never a constraint.
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

export const CATALOG = {
  claude: [
    {
      name: 'zeus',
      model: 'claude-opus-5',
      effort: 'max',
      description: 'Claude Opus 5 at full depth — high-stakes architecture and final checks',
    },
    {
      name: 'calliope',
      model: 'claude-fable-5',
      effort: 'max',
      description: 'Claude Fable 5 at full depth — the deepest reviewer on the roster',
    },
    {
      name: 'apollo',
      model: 'claude-opus-5',
      effort: 'xhigh',
      description: 'Claude Opus 5, a step faster — design alternatives and spec critique',
    },
    {
      name: 'artemis',
      model: 'claude-sonnet-5',
      effort: 'medium',
      description: 'Claude Sonnet 5 — quick, capable second opinions',
    },
    {
      name: 'hermes',
      model: 'claude-haiku-4-5-20251001',
      effort: 'low',
      description: 'Claude Haiku 4.5 — the fastest sanity check available',
    },
  ],
  codex: [
    {
      name: 'hyperion',
      model: 'gpt-5.6-sol',
      effort: 'ultra',
      description: 'GPT 5.6 Sol at ultra — maximum reasoning; turns can run many minutes',
    },
    {
      name: 'gaia',
      model: 'gpt-5.6-terra',
      effort: 'xhigh',
      description: 'GPT 5.6 Terra — the balanced Codex participant',
    },
    {
      name: 'diana',
      model: 'gpt-5.6-luna',
      effort: 'xhigh',
      description: 'GPT 5.6 Luna — compact and fast, still deep',
    },
    {
      name: 'helios',
      model: 'gpt-5.6-sol',
      effort: 'high',
      description: 'GPT 5.6 Sol at high — flagship reasoning without the ultra wait',
    },
    {
      name: 'nike',
      model: 'gpt-5.6-luna',
      effort: 'minimal',
      description: 'GPT 5.6 Luna at minimal — near-instant checks',
    },
  ],
  pi: [
    {
      name: 'endymion',
      model: 'openrouter/moonshotai/kimi-k3',
      effort: 'xhigh',
      description: 'Kimi K3 with a 1M-token window — for questions that span a whole repo',
    },
    {
      name: 'prometheus',
      model: 'openrouter/z-ai/glm-5.3',
      effort: 'high',
      description: 'GLM 5.3 via OpenRouter — a genuinely different lineage to check against',
    },
    {
      name: 'hephaestus',
      model: 'openrouter/qwen/qwen3.8-max',
      effort: 'high',
      description: 'Qwen 3.8 Max via OpenRouter — strong, independent implementation opinions',
    },
    {
      name: 'aether',
      model: 'openai-codex/gpt-5.6-sol',
      effort: 'xhigh',
      description: 'GPT 5.6 Sol through pi — Codex reasoning in pi’s harness',
    },
    {
      name: 'rhea',
      model: 'openai-codex/gpt-5.6-terra',
      effort: 'xhigh',
      description: 'GPT 5.6 Terra through pi — the balanced variant',
    },
    {
      name: 'phoebe',
      model: 'openai-codex/gpt-5.6-luna',
      effort: 'xhigh',
      description: 'GPT 5.6 Luna through pi — the fast variant',
    },
  ],
  opencode: [
    {
      name: 'mani',
      model: 'openrouter/moonshotai/kimi-k3',
      description: 'Kimi K3 through opencode — 1M context, no effort control',
    },
    {
      name: 'sunna',
      model: 'openrouter/openai/gpt-5.6-sol',
      effort: 'xhigh',
      description: 'GPT 5.6 Sol through opencode',
    },
    {
      name: 'jord',
      model: 'openrouter/openai/gpt-5.6-terra',
      effort: 'xhigh',
      description: 'GPT 5.6 Terra through opencode',
    },
    {
      name: 'bil',
      model: 'openrouter/openai/gpt-5.6-luna',
      effort: 'xhigh',
      description: 'GPT 5.6 Luna through opencode',
    },
    {
      name: 'saga',
      model: 'openrouter/z-ai/glm-5.3',
      description: 'GLM 5.3 through opencode',
    },
    {
      name: 'kvasir',
      model: 'openrouter/qwen/qwen3.8-max',
      description: 'Qwen 3.8 Max through opencode',
    },
  ],
}

/** The catalog entry with this name, whatever tool it belongs to. */
export function catalogEntry(name) {
  for (const [runtime, entries] of Object.entries(CATALOG)) {
    const entry = entries.find((e) => e.name === name)
    if (entry !== undefined) return { ...entry, runtime }
  }
  return undefined
}
