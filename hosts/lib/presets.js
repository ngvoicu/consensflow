import { slugify, stripMention } from "./utils.js";


// Same catalog as consensflow-pi, image preset included: here pygmalion rides the Codex CLI's
// ChatGPT login (lib/codex-auth.js) to the same gpt-image-2 backend pi reaches via its
// openai-codex login.
// --- Effort ceilings (audited 2026-08-27) --------------------------------
// Every preset names the HIGHEST level its model actually takes, and no preset names a level the
// model does not have. Both facts come from the harnesses' own catalogs, which each publish the
// per-model list: pi's `thinkingLevelMap` (~/.pi/agent/models-store.json, non-null entries) and
// models.dev's `reasoning_options` (opencode's models.json). They were compared across all 287
// models both carry: 281 agree. That agreement is why one name can mean one thing on both
// harnesses — a pi preset and its opencode twin sit at the same level by rule, asserted in
// tests/catalog.test.mjs.
//
// The audit found five presets naming a level their model has never had — `max` on Qwen3.8 27B and
// on Nemotron 3 Ultra, `xhigh` on Kimi K3, and an effort at all on MiniMax M3 and Laguna S 2.1.
// None of them errored: pi maps an unknown level to null and sends nothing, and opencode validates
// nothing at all (a deliberately bogus `--variant` was probed and ran). So the run quietly used the
// model's default while the label promised MAX — the failure mode this comment exists to prevent.
// Three models take no effort parameter at all (MiniMax M3, Laguna S 2.1 free, and gpt-image-2):
// their presets name no level, because a level nothing honours is worse than a blank one.
//
// The one disagreement that touches this catalog is DeepSeek V4 (all variants): pi says
// {high, xhigh}, models.dev says {low, high, max}. Probing could not settle it — pi returns
// reasoning tokens at both of its levels, and opencode reports zero reasoning on this model at
// every variant, including the `high` both catalogs confirm. So zephyros/hades/freya/odin sit at
// `high`, the only level both sources agree exists. Raise it when one of them is proven right.
//
// The GPT 5.6 trio through OpenCode (sunna/jord/bil) is deliberately NOT at its ceiling: it holds
// the xhigh tier that the same three models occupy on codex and pi, so the trio means the same
// thing on every harness. A tier ladder is a choice; a level the model lacks is a bug.
//
// --- Fable 5.1 (2026-09-01) ----------------------------------------------
// The Fable rows moved to Claude Fable 5.1 on the two harnesses that carry it. Sources, all read
// that day: models.dev lists `reasoning_options` low..max for BOTH `anthropic/claude-fable-5-1`
// and `openrouter/anthropic/claude-fable-5.1`; OpenRouter's /api/v1/models carries the dotted id;
// the Claude Code binary (2.1.257) carries the dashed one. Both ids were then LIVE-PROBED on the
// CLI that will run them — `claude -p --model claude-fable-5-1 --effort low` and `opencode run
// --model openrouter/anthropic/claude-fable-5.1` both answered — because a catalog listing proves
// the id and only a run proves the harness. Same price as Fable 5 ($10/$50 per MTok), so the
// ladder did not move: claude-code keeps max/xhigh/high/medium and OpenCode keeps
// xhigh/high/medium. pi is the exception and the comment above the pi rows says why.
//
// One finding this audit did NOT act on, recorded so the next one starts from it: pi-ai's
// thinkingLevelMap for `claude-fable-5` is {off, xhigh, max} today — no `high`, no `medium` — so
// linus (high) and erato (medium) already run at the model's default while their labels promise a
// tier. That is the exact failure this record exists to prevent, and fixing it means choosing a
// new shape for those two rows (raise, blank, or retire), not editing a string.
export const AGENT_PRESETS = [
  // --- Claude Fable 5.1 — Anthropic's most capable model (priced above Opus).
  // Muse names on claude-code; bard/storyteller names on the other engines.
  {
    preset: "calliope",
    id: "calliope",
    name: "Calliope",
    label: "Claude Code Fable 5.1 MAX",
    description: "Chief muse: Claude Fable 5.1 at max effort — the deepest Claude-powered collaborator in the catalog. Turns can run many minutes.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "max",
  },
  {
    preset: "clio",
    id: "clio",
    name: "Clio",
    label: "Claude Code Fable 5.1 XHIGH",
    description: "Muse of history: Claude Fable 5.1 at xhigh effort, the recommended tier for coding, planning, and agentic work.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "xhigh",
  },
  {
    preset: "euterpe",
    id: "euterpe",
    name: "Euterpe",
    label: "Claude Code Fable 5.1 HIGH",
    description: "Muse of music: Claude Fable 5.1 at high effort — strong reasoning without the xhigh wait.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "high",
  },
  {
    preset: "thalia",
    id: "thalia",
    name: "Thalia",
    label: "Claude Code Fable 5.1 MEDIUM",
    description: "Muse of comedy: Claude Fable 5.1 at medium effort for quicker takes from the top model.",
    kind: "claude-code",
    model: "claude-fable-5-1",
    effort: "medium",
  },

  // --- GPT 5.6 celestial trio (Codex) --------------------------------------
  // OpenAI's 2026 family: Sol (flagship), Terra (balanced), Luna (fast/affordable).
  // Codex's 5.6 effort ladder extends past xhigh with "max" and "ultra" (ultra =
  // max reasoning + automatic task delegation; Sol/Terra only). All combos verified live.
  {
    preset: "hyperion",
    id: "hyperion",
    name: "Hyperion",
    label: "Codex GPT 5.6 Sol ULTRA",
    description: "Titan of heavenly light: GPT 5.6 Sol — the flagship variant — at ultra effort (maximum reasoning with automatic task delegation), the deepest Codex agent in the catalog. Turns can run many minutes.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "ultra",
  },
  {
    preset: "phoebus",
    id: "phoebus",
    name: "Phoebus",
    label: "Codex GPT 5.6 Sol XHIGH",
    description: "The radiant sun: GPT 5.6 Sol at xhigh effort — flagship depth without the ultra wait.",
    kind: "codex",
    model: "gpt-5.6-sol",
    effort: "xhigh",
  },
  {
    preset: "gaia",
    id: "gaia",
    name: "Gaia",
    label: "Codex GPT 5.6 Terra XHIGH",
    description: "Primordial earth: GPT 5.6 Terra — the mid-size variant — at xhigh effort for strong everyday coding and planning.",
    kind: "codex",
    model: "gpt-5.6-terra",
    effort: "xhigh",
  },
  {
    preset: "diana",
    id: "diana",
    name: "Diana",
    label: "Codex GPT 5.6 Luna XHIGH",
    description: "Roman moon goddess: GPT 5.6 Luna — the compact, fast variant — at xhigh effort for quick, sharp takes.",
    kind: "codex",
    model: "gpt-5.6-luna",
    effort: "xhigh",
  },

  // --- GPT 5.6 on the other engines that reach it --------------------------
  // Pi rides the same ChatGPT (Codex) login the codex trio uses — no OpenRouter
  // credits; OpenCode reaches the same three variants through OpenRouter, whose
  // catalog lists openai/gpt-5.6-{sol,terra,luna}. Greek names on pi, Norse on
  // opencode, matching the rest of the catalog.
  {
    preset: "aether",
    id: "aether",
    name: "Aether",
    label: "Pi GPT 5.6 Sol XHIGH",
    description: "Primordial upper air and light: GPT 5.6 Sol — the flagship variant — on Pi, riding your ChatGPT (Codex) login at xhigh thinking.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-sol",
    thinking: "xhigh",
  },
  {
    preset: "rhea",
    id: "rhea",
    name: "Rhea",
    label: "Pi GPT 5.6 Terra XHIGH",
    description: "Titaness of the earth: GPT 5.6 Terra — the balanced variant — on Pi via your ChatGPT (Codex) login at xhigh thinking.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-terra",
    thinking: "xhigh",
  },
  {
    preset: "phoebe",
    id: "phoebe",
    name: "Phoebe",
    label: "Pi GPT 5.6 Luna XHIGH",
    description: "Titaness of the moon: GPT 5.6 Luna — the compact, fast variant — on Pi via your ChatGPT (Codex) login at xhigh thinking.",
    kind: "pi",
    model: "openai-codex/gpt-5.6-luna",
    thinking: "xhigh",
  },
  {
    preset: "sunna",
    id: "sunna",
    name: "Sunna",
    label: "OpenCode GPT 5.6 Sol XHIGH",
    description: "Norse sun goddess: GPT 5.6 Sol — the flagship variant — through OpenCode on OpenRouter at xhigh effort.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-sol",
    effort: "xhigh",
  },
  {
    preset: "jord",
    id: "jord",
    name: "Jord",
    label: "OpenCode GPT 5.6 Terra XHIGH",
    description: "Norse earth goddess, mother of Thor: GPT 5.6 Terra — the balanced variant — through OpenCode on OpenRouter at xhigh effort.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-terra",
    effort: "xhigh",
  },
  {
    preset: "bil",
    id: "bil",
    name: "Bil",
    label: "OpenCode GPT 5.6 Luna XHIGH",
    description: "The child who follows Mani across the night sky: GPT 5.6 Luna — the compact, fast variant — through OpenCode on OpenRouter at xhigh effort.",
    kind: "opencode",
    model: "openrouter/openai/gpt-5.6-luna",
    effort: "xhigh",
  },

  // --- House team: strong default agents per engine --------------------
  {
    preset: "zeus",
    id: "zeus",
    name: "Zeus",
    label: "Claude Code Opus 5 MAX",
    description: "Deepest Opus-tier Claude Code agent for high-stakes architecture, implementation plans, and final checks; half the price of Fable 5.1, which stays the catalog ceiling (@calliope).",
    kind: "claude-code",
    model: "claude-opus-5",
    effort: "max",
  },
  {
    preset: "apollo",
    id: "apollo",
    name: "Apollo",
    label: "Claude Code Opus 5 XHIGH",
    description: "Deep but slightly cheaper/faster Claude Code agent for spec critique, design alternatives, and implementation plans.",
    kind: "claude-code",
    model: "claude-opus-5",
    effort: "xhigh",
  },
  {
    preset: "artemis",
    id: "artemis",
    name: "Artemis",
    label: "Claude Code Opus 5 MEDIUM",
    description: "Apollo's twin: Opus 5 at medium effort for quicker, cheaper Claude Code takes.",
    kind: "claude-code",
    model: "claude-opus-5",
    effort: "medium",
  },

  // --- Frontier models on the other engines that run them ------------------
  // OpenCode reaches Fable 5.1 through OpenRouter, whose id spells the version with a DOT
  // (anthropic/claude-fable-5.1) where Anthropic's own API spells it with a dash
  // (claude-fable-5-1) — one model, two spellings, and the wrong one is a 404.
  // pi stays on Fable 5: pi-ai 0.84.4 (the newest release on 2026-09-01) carries no 5.1 entry for
  // the anthropic provider, and its fallback for an unknown id copies the provider's DEFAULT model
  // (claude-opus-4-8) and swaps the name — so the label would promise a Fable 5.1 tier while the
  // thinking level was mapped through another model's table. Move orpheus/linus/erato the day
  // pi-ai lists the model. The trio keeps xhigh as its tier so calliope stays the catalog ceiling.
  {
    preset: "orpheus",
    id: "orpheus",
    name: "Orpheus",
    label: "Pi Fable 5 XHIGH (Anthropic)",
    description: "The legendary bard: Pi-backed Claude Fable 5 with xhigh thinking; needs Anthropic auth in pi.",
    kind: "pi",
    model: "anthropic/claude-fable-5",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "linus",
    id: "linus",
    name: "Linus",
    label: "Pi Fable 5 HIGH (Anthropic)",
    description: "Orpheus's bard brother: Pi-backed Claude Fable 5 with high thinking; needs Anthropic auth in pi.",
    kind: "pi",
    model: "anthropic/claude-fable-5",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "erato",
    id: "erato",
    name: "Erato",
    label: "Pi Fable 5 MEDIUM (Anthropic)",
    description: "Muse of lyric poetry: Pi-backed Claude Fable 5 with medium thinking; needs Anthropic auth in pi.",
    kind: "pi",
    model: "anthropic/claude-fable-5",
    thinking: "medium",
    skillsPolicy: "default",
  },
  {
    preset: "saga",
    id: "saga",
    name: "Saga",
    label: "OpenCode Fable 5.1 XHIGH",
    description: "Norse goddess of storytelling: OpenCode-backed Claude Fable 5.1 at xhigh variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "xhigh",
  },
  {
    preset: "gunnlod",
    id: "gunnlod",
    name: "Gunnlod",
    label: "OpenCode Fable 5.1 HIGH",
    description: "Guardian of the mead of poetry: OpenCode-backed Claude Fable 5.1 at high variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "high",
  },
  {
    preset: "kvasir",
    id: "kvasir",
    name: "Kvasir",
    label: "OpenCode Fable 5.1 MEDIUM",
    description: "Source of the mead of poetry: OpenCode-backed Claude Fable 5.1 at medium variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/anthropic/claude-fable-5.1",
    effort: "medium",
  },
  // Opus 5 on pi (anthropic provider). pi's model layer gained a "max" thinking level in
  // @earendil-works/pi-ai 0.82 (verified in its registry: claude-opus-5 maps xhigh AND max),
  // but the pi releases shipping today still bundle an older pi-ai that caps at xhigh — so
  // these stay at xhigh/medium, which is valid on both.
  {
    preset: "kronos",
    id: "kronos",
    name: "Kronos",
    label: "Pi Opus 5 XHIGH (Anthropic)",
    description: "Pi-backed Claude Opus 5 with xhigh thinking; needs Anthropic auth in pi.",
    kind: "pi",
    model: "anthropic/claude-opus-5",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "atlas",
    id: "atlas",
    name: "Atlas",
    label: "Pi Opus 5 MEDIUM (Anthropic)",
    description: "Pi-backed Claude Opus 5 with medium thinking; needs Anthropic auth in pi.",
    kind: "pi",
    model: "anthropic/claude-opus-5",
    thinking: "medium",
    skillsPolicy: "default",
  },
  // Opus 5 on OpenCode (via OpenRouter). Unlike the 4.8 generation there is no dotted id:
  // it is plainly anthropic/claude-opus-5. Kept at the xhigh/medium tiers the 4.8 pair used.
  {
    preset: "baldr",
    id: "baldr",
    name: "Baldr",
    label: "OpenCode Opus 5 XHIGH",
    description: "OpenCode-backed Claude Opus 5 at xhigh variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5",
    effort: "xhigh",
  },
  {
    preset: "vali",
    id: "vali",
    name: "Vali",
    label: "OpenCode Opus 5 MEDIUM",
    description: "OpenCode-backed Claude Opus 5 at medium variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/anthropic/claude-opus-5",
    effort: "medium",
  },
  // GPT 5.5 on OpenCode (via OpenRouter).

  // --- Fast/cheap tier: quick gut-checks ----------------------------------
  {
    preset: "hermod",
    id: "hermod",
    name: "Hermod",
    label: "Claude Code Sonnet 5 MAX",
    description: "Fast, cheap Claude Code agent (Haiku) for quick gut-checks.",
    kind: "claude-code",
    model: "claude-sonnet-5",
    effort: "max",
  },
  {
    preset: "nike",
    id: "nike",
    name: "Nike",
    label: "Pi Gemini 3.7 Flash HIGH (fast)",
    description: "Swift, cheap Pi-backed Gemini 3.7 Flash at high thinking — its ceiling — for quick second opinions.",
    kind: "pi",
    model: "openrouter/google/gemini-3.7-flash",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "freya",
    id: "freya",
    name: "Freya",
    label: "OpenCode DeepSeek V4 Flash HIGH (fast)",
    description: "Cheap, fast OpenCode-backed DeepSeek V4 Flash at high variant (via OpenRouter) — the one level both harness catalogs agree this model has.",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4-flash-0731",
    effort: "high",
  },
  {
    preset: "zephyros",
    id: "zephyros",
    name: "Zephyros",
    label: "Pi DeepSeek V4 Flash HIGH (fast)",
    description: "Swift west wind: Pi-backed DeepSeek V4 Flash at high thinking (via OpenRouter) — `low` was not a level this model has.",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4-flash-0731",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "sif",
    id: "sif",
    name: "Sif",
    label: "OpenCode Gemini 3.7 Flash HIGH (fast)",
    description: "Swift, cheap OpenCode-backed Gemini 3.7 Flash at high variant — its ceiling (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/google/gemini-3.7-flash",
    effort: "high",
  },

  // --- pi model zoo (Greek names) — popular OpenRouter models via Pi -------
  {
    preset: "hades",
    id: "hades",
    name: "Hades",
    label: "Pi DeepSeek V4 Pro",
    description: "Pi-backed DeepSeek V4 Pro agent (via OpenRouter).",
    kind: "pi",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "helios",
    id: "helios",
    name: "Helios",
    label: "Pi Gemini 3.1 Pro",
    description: "Pi-backed Google Gemini 3.1 Pro agent (via OpenRouter).",
    kind: "pi",
    model: "openrouter/google/gemini-3.1-pro-preview",
    thinking: "high",
    skillsPolicy: "default",
  },
  {
    preset: "ares",
    id: "ares",
    name: "Ares",
    label: "Pi Grok 4.6 XHIGH",
    description: "Pi-backed xAI Grok 4.6 agent at xhigh thinking — its ceiling (via OpenRouter).",
    kind: "pi",
    model: "openrouter/x-ai/grok-4.6",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "hephaestus",
    id: "hephaestus",
    name: "Hephaestus",
    label: "Pi Qwen3.8 Max XHIGH",
    description: "Pi-backed Qwen3.8 Max agent at xhigh thinking — its ceiling (via OpenRouter).",
    kind: "pi",
    model: "openrouter/qwen/qwen3.8-max",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "athena",
    id: "athena",
    name: "Athena",
    label: "Pi Qwen3.8 27B XHIGH",
    description: "Pi-backed Qwen3.8 27B at xhigh thinking — its ceiling; `max` is not a level this model has (via OpenRouter). The dense sibling of Hephaestus's Max.",
    kind: "pi",
    model: "openrouter/qwen/qwen3.8-27b",
    thinking: "xhigh",
    skillsPolicy: "default",
  },
  {
    preset: "metis",
    id: "metis",
    name: "Metis",
    label: "Pi MiniMax M3",
    description: "Pi-backed MiniMax M3 agent (via OpenRouter); goddess of cunning strategy for 'minimax'. M3 reasons, but no harness can steer how hard: it takes no effort parameter, so this preset names no level.",
    kind: "pi",
    model: "openrouter/minimax/minimax-m3",
    skillsPolicy: "default",
  },
  {
    preset: "prometheus",
    id: "prometheus",
    name: "Prometheus",
    label: "Pi GLM 5.3 MAX",
    description: "Pi-backed Zhipu GLM 5.3 at max thinking — its ceiling (via OpenRouter); the Titan who brought knowledge to mortals.",
    kind: "pi",
    model: "openrouter/z-ai/glm-5.3",
    thinking: "max",
    skillsPolicy: "default",
  },
  {
    preset: "endymion",
    id: "endymion",
    name: "Endymion",
    label: "Pi Kimi K3 MAX",
    description: "Beloved of the moon goddess: Pi-backed Kimi K3 — Moonshot's 1M-context flagship reasoner — at max thinking, its ceiling. K3 takes low/high/max and never had `xhigh`; the kimi-k3 entry in ~/.pi/agent/models.json is still worth having for sane token limits, but the level no longer depends on it (probed 2026-08-27: max returns reasoning tokens).",
    kind: "pi",
    model: "openrouter/moonshotai/kimi-k3",
    thinking: "max",
    skillsPolicy: "default",
  },

  // --- Three OpenRouter models added 2026-08-24, each verified present in
  // `pi --list-models` and `opencode models` before it was written down. Two
  // ride OpenRouter's free tier; the third was stealth/ox-alpha, whose testing
  // period ended 2026-08-27 — the endpoint now 404s and names its own model:
  // ZAI's GLM 5.3 Flash. nyx and nott follow it there rather than keep a name
  // that answers nothing. That id is NEWER than either harness's catalog:
  // neither `pi --list-models` (refreshed) nor `opencode models` carries
  // z-ai/glm-5.3-flash yet, so it was verified another way on 2026-08-27 —
  // present in OpenRouter's own /api/v1/models with reasoning support, and
  // live one-shot probes on both CLIs answered through it as a custom model
  // id. pi says so out loud ("Using custom model id") and still forwards the
  // thinking level: at max the run reports reasoning tokens, at off it reports
  // none. A `~/.pi/agent/models.json` entry (the endymion pattern) is what
  // buys sane token limits until models.dev catches up. All three report
  // thinking support, so all three sit at the ceiling.
  {
    preset: "nyx",
    id: "nyx",
    name: "Nyx",
    label: "Pi GLM 5.3 Flash MAX",
    description: "Primordial goddess of night: ZAI's GLM 5.3 Flash on Pi at max thinking (via OpenRouter) — 1.3M context and reasoning; the stealth model that ran here as ox-alpha, now under its own name.",
    kind: "pi",
    model: "openrouter/z-ai/glm-5.3-flash",
    thinking: "max",
  },
  {
    preset: "oceanus",
    id: "oceanus",
    name: "Oceanus",
    label: "Pi Nemotron 3 Ultra 550B FREE HIGH",
    description: "Titan of the world-encircling river: NVIDIA's 550B-parameter Nemotron 3 Ultra on Pi at high thinking — its ceiling, `max` is not a level it has — on OpenRouter's free tier (1M context there).",
    kind: "pi",
    model: "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
    thinking: "high",
  },
  {
    preset: "triton",
    id: "triton",
    name: "Triton",
    label: "Pi Laguna S 2.1 FREE",
    description: "Herald of the deep, for a model named after a lagoon: Poolside's Laguna S 2.1 on Pi, on OpenRouter's free tier. It reasons, but takes no effort parameter, so this preset names no level.",
    kind: "pi",
    model: "openrouter/poolside/laguna-s-2.1:free",
  },

  // --- opencode model zoo (Norse names) — same models via OpenCode --------
  //
  // Same model AND same effort as the pi twin. A name here is a model plus how
  // hard it thinks, so a pair that agreed on the model and not on the level
  // (ares/thor, hades/odin, hephaestus/tyr, zephyros/freya) was two different
  // agents wearing one description — and it showed: an entry with no effort
  // draws a bare harness tag in the roster UI, which reads as a gap because it
  // was one. Filled in 2026-08-27 against OpenRouter's own
  // supported_parameters: each of those four models lists reasoning_effort, so
  // opencode's `--variant` reaches something. opencode validates nothing here
  // (a bogus variant was probed and ran), which is exactly why the catalog
  // must. Two entries below still carry no effort on purpose — each says why.
  {
    preset: "odin",
    id: "odin",
    name: "Odin",
    label: "OpenCode DeepSeek V4 Pro HIGH",
    description: "OpenCode-backed DeepSeek V4 Pro agent at high variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/deepseek/deepseek-v4-pro-0813",
    effort: "high",
  },
  {
    preset: "heimdall",
    id: "heimdall",
    name: "Heimdall",
    label: "OpenCode Gemini 3.1 Pro",
    description: "OpenCode-backed Google Gemini 3.1 Pro agent at high variant (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/google/gemini-3.1-pro-preview",
    effort: "high",
  },
  {
    preset: "thor",
    id: "thor",
    name: "Thor",
    label: "OpenCode Grok 4.6 XHIGH",
    description: "OpenCode-backed xAI Grok 4.6 agent at xhigh variant — its ceiling (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/x-ai/grok-4.6",
    effort: "xhigh",
  },
  {
    preset: "tyr",
    id: "tyr",
    name: "Tyr",
    label: "OpenCode Qwen3.8 Max XHIGH",
    description: "OpenCode-backed Qwen3.8 Max agent at xhigh variant — its ceiling (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/qwen/qwen3.8-max",
    effort: "xhigh",
  },
  {
    preset: "bragi",
    id: "bragi",
    name: "Bragi",
    label: "OpenCode Qwen3.8 27B XHIGH",
    description: "OpenCode-backed Qwen3.8 27B at xhigh effort — its ceiling; `max` is not a level this model has (via OpenRouter). The dense sibling of Tyr's Max.",
    kind: "opencode",
    model: "openrouter/qwen/qwen3.8-27b",
    effort: "xhigh",
  },
  // No effort, deliberately — one of the three models in this catalog that take none.
  // See "Effort ceilings" at the top of the file.
  {
    preset: "mimir",
    id: "mimir",
    name: "Mimir",
    label: "OpenCode MiniMax M3",
    description: "OpenCode-backed MiniMax M3 agent (via OpenRouter); god of wisdom for 'minimax'.",
    kind: "opencode",
    model: "openrouter/minimax/minimax-m3",
  },
  {
    preset: "mani",
    id: "mani",
    name: "Mani",
    label: "OpenCode Kimi K3 MAX",
    description: "Norse moon god: OpenCode-backed Kimi K3 — Moonshot's 1M-context flagship reasoner — at max effort, its ceiling (via OpenRouter).",
    kind: "opencode",
    model: "openrouter/moonshotai/kimi-k3",
    effort: "max",
  },
  {
    preset: "nott",
    id: "nott",
    name: "Nott",
    label: "OpenCode GLM 5.3 Flash MAX",
    description: "Norse night personified, sister in spirit to Pi's Nyx: ZAI's GLM 5.3 Flash through OpenCode at max effort (via OpenRouter) — the model that was stealth/ox-alpha until it was named.",
    kind: "opencode",
    model: "openrouter/z-ai/glm-5.3-flash",
    effort: "max",
  },
  {
    preset: "ymir",
    id: "ymir",
    name: "Ymir",
    label: "OpenCode Nemotron 3 Ultra 550B FREE HIGH",
    description: "The primordial giant the world was built from: NVIDIA's 550B-parameter Nemotron 3 Ultra through OpenCode at high effort — its ceiling, `max` is not a level it has — on OpenRouter's free tier.",
    kind: "opencode",
    model: "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
    effort: "high",
  },
  {
    preset: "aegir",
    id: "aegir",
    name: "Aegir",
    label: "OpenCode Laguna S 2.1 FREE",
    description: "Norse giant of the sea, for a model named after a lagoon: Poolside's Laguna S 2.1 through OpenCode, on OpenRouter's free tier. It reasons, but takes no effort parameter, so this preset names no level.",
    kind: "opencode",
    model: "openrouter/poolside/laguna-s-2.1:free",
  },

  // --- Kimi Code (Finnish names, so a kimi agent is recognisable as one at a
  // glance — Greek, Norse and muse names are all spoken for). Added 2026-08-24.
  //
  // K2.6 is left out: K3 supersedes it outright, and this list carries the
  // newest of each family — the rule that keeps glm-5.2 and qwen3.7 out too.
  // The K2.7 *code* models stay, because K3 is a general flagship and has no
  // code-specialist counterpart: they are the newest of a different family,
  // not an older version of this one.
  //
  // This bills MOONSHOT DIRECTLY through Kimi Code's own key, unlike endymion
  // and mani, which reach the same K3 through OpenRouter: same model, different
  // account and different rate limits. No effort field, because Kimi Code has
  // no effort flag — `default_effort` lives in that config file (max for K3).
  {
    preset: "ilmarinen",
    id: "ilmarinen",
    name: "Ilmarinen",
    label: "Kimi K3",
    description: "The eternal smith who forged the sky: Kimi K3 on Kimi Code — 1M context, billed to Moonshot directly rather than through OpenRouter. Effort comes from your kimi config (max by default).",
    kind: "kimi",
    model: "moonshot-ai/kimi-k3",
  },
  {
    preset: "seppo",
    id: "seppo",
    name: "Seppo",
    label: "Kimi K2.7 Code",
    description: "The smith at his anvil: Kimi K2.7 Code on Kimi Code — the code-specialist Ilmarinen's general K3 does not replace. 262K context, billed to Moonshot directly.",
    kind: "kimi",
    model: "moonshot-ai/kimi-k2.7-code",
  },
  {
    preset: "ahti",
    id: "ahti",
    name: "Ahti",
    label: "Kimi K2.7 Code Highspeed",
    description: "God of swift water: the same code-specialist tuned for speed — for a quick read or a second pair of eyes, where Seppo is for the careful pass.",
    kind: "kimi",
    model: "moonshot-ai/kimi-k2.7-code-highspeed",
  },
  // --- Image generation (Codex backend → gpt-image-2) ---------------------
  {
    preset: "pygmalion",
    id: "pygmalion",
    name: "Pygmalion",
    label: "Image — gpt-image-2 (via Codex login)",
    description: "Generates images with gpt-image-2 through your existing Codex / openai-codex login. Accepts optional reference images (`--image <path>`, repeatable) to edit/condition on. The model field is only the trigger model; the image backend is always gpt-image-2.",
    kind: "image",
    // The image path calls IMAGE_BACKEND (gpt-image-2) directly; this field is
    // what the catalog displays, so it names the model that actually runs.
    model: "gpt-image-2",
  },
];

export function getPreset(ref) {
  const id = slugify(stripMention(ref));
  return AGENT_PRESETS.find((preset) => preset.preset === id || preset.id === id || slugify(preset.name) === id) ?? null;
}

export function listPresetIds() {
  return AGENT_PRESETS.map((preset) => preset.preset);
}

export function agentFromPreset(ref, overrides = {}) {
  const preset = getPreset(ref);
  if (!preset) return null;
  const nameOverride = stringOverride(overrides.name);
  const idOverride = stringOverride(overrides.id);
  const name = nameOverride ?? preset.name;
  // Keep the preset's canonical id; only derive a new id when the caller renames (--name) or sets
  // an explicit id.
  const id = slugify(idOverride ?? nameOverride ?? preset.id);
  const agent = {
    ...preset,
    // The label, not the catalog card's paragraph: a roster row's description is the
    // one-liner the skill table prints, and sync now keeps it current — a row created
    // with the paragraph would drift the moment it was written.
    description: preset.label ?? preset.description,
    ...allowedOverrides(overrides),
    preset: preset.preset,
    id,
    name,
    kind: preset.kind,
    model: preset.model,
    effort: preset.effort,
    thinking: preset.thinking,
    skillsPolicy: preset.skillsPolicy,
  };
  delete agent.label;
  return agent;
}

// --- Catalog drift -------------------------------------------------------
// A roster entry snapshots its preset's engine fields, so a ConsensFlow update that ships a new
// catalog (Opus 4.8 → Opus 5, say) does not reach agents that were already added. These
// helpers re-resolve that: the fields below are decided entirely by the preset — agentFromPreset
// lets only --name/--id/--cwd/--description through and there is no `agents edit` — so replacing
// them with the catalog's current values is lossless.
// `description` joined the list on 2026-08-27, the maintainer's call, after a live update: nyx moved
// the retired stealth/ox-alpha to z-ai/glm-5.3-flash and the roster — and with it the skill table
// every lead reads — went on saying "Pi Ox Alpha MAX" beside the new model. It was called cosmetic
// while it was only a roster field; it is not, now that the generated skill prints it as the line
// that says WHO an agent is. A label naming a model the agent no longer runs is a wrong answer to
// the only question the table exists to answer.
// It was kept out for two reasons, and both were weighed before it went in. The one that expired:
// two hosts sharing ONE roster worded some descriptions differently (pygmalion's login wording), so
// syncing would never converge — each host re-flagging the other's text forever. The host payloads
// went on 2026-08-23 and nothing but the manager writes a description now. The one that stands:
// `add <preset> --description …` is a real override, and this rewrites it on the next catalog move
// without asking. The escape hatch is provenance, not wording — an agent added with an explicit
// --model or --effort carries no `preset` and is never synced at all.
// Agents with no `preset`, or whose preset has since left the catalog, are left alone.
export const PRESET_OWNED_FIELDS = ["kind", "model", "effort", "thinking", "skillsPolicy", "description"];

// The roster's `description` is the preset's one-line LABEL ("Pi GLM 5.3 Flash MAX") — what an add
// writes and what the generated skill prints beside the agent's name. The preset's own
// `description` is the catalog card's paragraph and belongs to the UI, not to a roster row.
function presetOwnedValue(field, preset) {
  if (field === "description") return preset.label ?? preset.description;
  return presetFieldValue(field, preset);
}

// normalizeAgent() fills these in on save, so compare against the same defaults or every
// non-pi agent reports a phantom skillsPolicy change.
const PRESET_FIELD_DEFAULTS = { skillsPolicy: "default" };

function presetFieldValue(field, source) {
  const value = source?.[field];
  if (value === undefined || value === null || value === "") return PRESET_FIELD_DEFAULTS[field];
  return value;
}

export function presetForAgent(agent) {
  return agent?.preset ? getPreset(agent.preset) : null;
}

// True when the entry names a preset the catalog no longer carries (e.g. the GPT 5.5 presets
// retired in 1.7.0). Those stay pinned to what they were created with — sync never touches them.
export function isOrphanedPreset(agent) {
  return Boolean(agent?.preset) && !getPreset(agent.preset);
}

export function presetDrift(agent) {
  const preset = presetForAgent(agent);
  if (!preset) return [];
  const changes = [];
  for (const field of PRESET_OWNED_FIELDS) {
    const from = presetFieldValue(field, agent);
    const to = presetOwnedValue(field, preset);
    if (from !== to) changes.push({ field, from, to });
  }
  return changes;
}

export function syncAgentWithPreset(agent) {
  const changes = presetDrift(agent);
  if (changes.length === 0) return { agent, changes };
  const synced = { ...agent };
  for (const { field, to } of changes) {
    if (to === undefined) delete synced[field];
    else synced[field] = to;
  }
  return { agent: synced, changes };
}

export function driftedAgents(agents) {
  return (agents ?? []).filter((agent) => presetDrift(agent).length > 0);
}

function stringOverride(value) {
  if (value === undefined || value === null || value === true) return undefined;
  const trimmed = String(value).trim();
  return trimmed || undefined;
}

function allowedOverrides(overrides) {
  const result = {};
  for (const key of ["cwd", "description"]) {
    if (overrides[key] !== undefined) result[key] = overrides[key];
  }
  return result;
}

export function formatPresetLine(preset) {
  const effort = preset.effort ? ` effort=${preset.effort}` : preset.thinking ? ` thinking=${preset.thinking}` : "";
  const skills = preset.kind === "pi" ? ` skills=${preset.skillsPolicy ?? "default"}` : "";
  return `- ${preset.preset} → @${preset.id} (${preset.name}): ${preset.label} [${preset.kind} model=${preset.model}${effort}${skills}]`;
}

export function formatPresets() {
  return ["# ConsensFlow agent presets", "", ...AGENT_PRESETS.map(formatPresetLine), "", "Add one with `/consensflow:agents add <preset>`, or `/consensflow:agents add all`."].join("\n");
}
