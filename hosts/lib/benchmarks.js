import { chmodSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

// Source fields and units: https://artificialanalysis.ai/data-api/docs (2026-09-10).
export const METRICS = [
  [
    'intelligence',
    'Intelligence',
    'artificial_analysis_intelligence_index',
    'points',
    'Broad reasoning and task capability. Compare within the same AA index version.',
  ],
  [
    'coding',
    'Coding',
    'artificial_analysis_coding_index',
    'points',
    'AA Coding Index: a composite of coding evaluations, not a repository success rate.',
  ],
  [
    'agentic',
    'Agentic',
    'artificial_analysis_agentic_index',
    'points',
    'AA Agentic Index: completing multi-step work with tools in AA’s setup. This does not test ConsensFlow coordination.',
  ],
  [
    'terminal',
    'Terminal coding',
    'terminalbench_v4_0',
    '%',
    'Terminal-Bench v4.0: coding and terminal tasks using AA’s evaluation harness.',
  ],
  [
    'hallucinations',
    'Hallucinations',
    'aa_omniscience_non_hallucination_rate',
    '%',
    'AA-Omniscience: incorrect answers divided by incorrect, partial and unanswered outcomes. Correct answers are excluded from this denominator. Lower is better; this is not a code error rate.',
  ],
  [
    'accuracy',
    'Knowledge accuracy',
    'aa_omniscience_accuracy',
    '%',
    'AA-Omniscience: correct answers as a share of all questions. Read alongside hallucinations.',
  ],
  [
    'reliability',
    'Knowledge reliability',
    'aa_omniscience_index',
    'points',
    'AA-Omniscience Index: rewards correct answers, penalizes incorrect answers, and treats abstentions neutrally. Range −100 to 100.',
  ],
  [
    'instructions',
    'Instruction following',
    'ifbench',
    '%',
    'IFBench: compliance with verifiable output constraints.',
  ],
  [
    'context',
    'Long-context reasoning',
    'aa_lcr',
    '%',
    'AA-LCR: reasoning across long documents, not maximum context capacity.',
  ],
  [
    'professional',
    'Professional work',
    'gdpval_aa_elo',
    'Elo',
    'GDPval-AA v2: quality of professional deliverables. Elo points, not percent success.',
  ],
  [
    'scicode',
    'Scientific coding',
    'scicode',
    '%',
    'SciCode: scientific Python problems, not general repository repair.',
  ],
  [
    'hle',
    'Reasoning and knowledge',
    'hle',
    '%',
    'Humanity’s Last Exam: difficult academic knowledge and reasoning questions.',
  ],
  [
    'vision',
    'Visual reasoning',
    'mmmu_pro',
    '%',
    'MMMU-Pro: understanding images, not generating images or designing interfaces.',
  ],
  ['physics', 'Physics reasoning', 'critpt', '%', 'CritPt: specialist research physics tasks.'],
].map(([id, label, field, unit, description]) => ({
  id,
  label,
  field,
  unit,
  description,
  direction: id === 'hallucinations' ? 'asc' : 'desc',
}))

const CACHE = 'artificial-analysis-cache.json'
const DAY = 86_400_000
const BASE = 'https://artificialanalysis.ai/api/v2/language/models'

export function readBenchmarkCache(root) {
  try {
    const data = JSON.parse(readFileSync(join(root, CACHE), 'utf8'))
    if (
      data.schemaVersion === 1 &&
      data.models &&
      typeof data.models === 'object' &&
      !Array.isArray(data.models)
    )
      return data
  } catch {
    /* No usable cache yet. Browsing works without scores. */
  }
  return { schemaVersion: 1, models: {} }
}

function scoresFor(evaluations) {
  const scores = {}
  for (const metric of METRICS) {
    const value = evaluations?.[metric.field]
    if (typeof value !== 'number' || !Number.isFinite(value)) continue
    if (metric.unit === '%' && (value < 0 || value > 1)) continue
    if (
      metric.unit === 'points' &&
      (value < (metric.id === 'reliability' ? -100 : 0) || value > 100)
    )
      continue
    scores[metric.id] =
      metric.unit === '%' ? 100 * (metric.id === 'hallucinations' ? 1 - value : value) : value
  }
  return scores
}

export class ArtificialAnalysis {
  #pending
  #cache
  constructor(root, { fetchImpl = fetch, now = Date.now } = {}) {
    this.root = root
    this.fetch = fetchImpl
    this.now = now
  }
  refresh() {
    this.#pending ??= this.#refresh().finally(() => {
      this.#pending = undefined
    })
    return this.#pending
  }
  async #refresh() {
    const cache = this.#cache ?? readBenchmarkCache(this.root)
    const now = this.now()
    if (cache.retryAt > now || (cache.fetchedAt && now - Date.parse(cache.fetchedAt) < DAY))
      return cache
    let key
    try {
      key = readFileSync(join(this.root, 'artificial-analysis-key'), 'utf8').trim()
    } catch {
      /* Optional integration. */
    }
    if (!key) return { ...cache, status: 'unconfigured' }
    let next
    let retryAt = now + 3_600_000
    try {
      const signal = AbortSignal.timeout(10_000)
      let endpoint = cache.endpoint === 'free' && cache.tier === 'free' ? 'free' : 'pro'
      const models = {}
      let indexVersion, tier, total
      for (let page = 1; ; page++) {
        const request = () =>
          this.fetch(`${BASE}${endpoint === 'free' ? '/free' : ''}?page=${page}`, {
            headers: { 'x-api-key': key, accept: 'application/json' },
            redirect: 'error',
            signal,
          })
        let response = await request()
        if (page === 1 && response.status === 403 && endpoint === 'pro') {
          endpoint = 'free'
          response = await request()
        }
        if (!response.ok) {
          if (response.status === 429) {
            const seconds = Number(response.headers.get('Retry-After'))
            const reset = Number(response.headers.get('X-RateLimit-Reset')) * 1000
            retryAt = Math.max(
              retryAt,
              now + (Number.isFinite(seconds) ? seconds * 1000 : 0),
              Number.isFinite(reset) ? reset : 0,
            )
          }
          throw new Error('AA request failed')
        }
        const body = await response.json()
        const p = body.pagination
        if (
          !Array.isArray(body.data) ||
          !p ||
          p.page !== page ||
          !Number.isInteger(p.total_pages) ||
          p.total_pages < page ||
          p.total_pages > 10 ||
          p.has_more !== page < p.total_pages ||
          !Number.isFinite(body.intelligence_index_version) ||
          !['free', 'pro', 'commercial'].includes(body.tier)
        )
          throw new Error('Invalid AA page')
        if (page === 1) {
          indexVersion = body.intelligence_index_version
          tier = body.tier
          total = p.total_pages
        }
        if (
          indexVersion !== body.intelligence_index_version ||
          tier !== body.tier ||
          total !== p.total_pages
        )
          throw new Error('AA changed during pagination')
        for (const model of body.data) {
          if (
            typeof model.slug !== 'string' ||
            !/^[A-Za-z0-9][A-Za-z0-9.-]*$/.test(model.slug) ||
            typeof model.name !== 'string' ||
            model.name.length > 500 ||
            models[model.slug]
          )
            throw new Error('Invalid AA model')
          models[model.slug] = { name: model.name, scores: scoresFor(model.evaluations) }
        }
        if (!p.has_more) break
      }
      next = {
        schemaVersion: 1,
        status: 'ready',
        endpoint,
        tier,
        indexVersion,
        fetchedAt: new Date(now).toISOString(),
        models,
      }
    } catch {
      next = { ...cache, status: cache.fetchedAt ? 'stale' : 'unavailable', retryAt }
    }
    this.#cache = next
    try {
      mkdirSync(this.root, { recursive: true })
      const path = join(this.root, CACHE)
      const temp = `${path}.${process.pid}.tmp`
      writeFileSync(temp, `${JSON.stringify(next)}\n`, { mode: 0o600 })
      chmodSync(temp, 0o600)
      renameSync(temp, path)
    } catch {
      /* Scores remain usable for this response if local persistence fails. */
    }
    return next
  }
}

// Explicit model identities reviewed against authenticated AA V2 records.
// No rolling DeepSeek alias or fallback to a different tested effort.
// A null effort denotes a reviewed model-level AA record with no stated level.
const MODELS = {
  'gpt-6-astra': ['gpt-6-astra', 'max'],
  'gpt-5.6-sol': ['gpt-5-6-sol', 'max'],
  'gpt-5.6-terra': ['gpt-5-6-terra', 'max'],
  'gpt-5.6-luna': ['gpt-5-6-luna', 'max'],
  'claude-fable-5.1': ['claude-fable-5-1', 'max'],
  'claude-opus-5': ['claude-opus-5', 'max'],
  'claude-sonnet-5': ['claude-sonnet-5', 'max'],
  'gemini-3.8-flash': ['gemini-3-8-flash', 'high'],
  'grok-4.6': ['grok-4-6', 'high'],
  'qwen3.8-27b': ['qwen3-8-27b', 'xhigh'],
  'qwen3.8-max': ['qwen3-8-max', null],
  'glm-5.3-flash': ['glm-5-3-flash', null],
  'minimax-m3': ['minimax-m3', null],
  'nemotron-3-ultra-550b-a55b:free': ['nvidia-nemotron-3-ultra-550b-a55b', null],
  'deepseek-v4-pro-0813': ['deepseek-v4-pro', 'max'],
  'deepseek-v4-flash-0731': ['deepseek-v4-flash', 'max'],
  'glm-5.3': ['glm-5-3', 'max'],
  'kimi-k3': ['kimi-k3', 'max'],
  'muse-spark-1.3': ['muse-spark-1-3', 'max'],
}

export function withBenchmarks(agent, profile, cache) {
  const { benchmarks: _old, ...base } = profile
  const mapping = MODELS[base.modelKey]
  const effort =
    (agent.harness ?? agent.kind) === 'pi' ? (agent.thinking ?? agent.effort) : agent.effort
  if (!mapping) return base
  const [slugBase, defaultEffort] = mapping
  const modelLevel = defaultEffort === null
  if (
    (!modelLevel || effort != null) &&
    !['off', 'low', 'medium', 'high', 'xhigh', 'max'].includes(effort)
  ) return base
  const slug =
    slugBase +
    (modelLevel || effort === defaultEffort ? '' : effort === 'off' ? '-non-reasoning' : `-${effort}`)
  const match = cache?.models?.[slug]
  const namedEffort =
    effort === 'off' ? /\(Non-reasoning\)/i : new RegExp(`\\b${effort}(?: Effort)?[),]`, 'i')
  if (!match || !Object.keys(match.scores ?? {}).length) return base
  if (
    modelLevel
      ? /\b(?:off|minimal|low|medium|high|xhigh|max|ultra)(?: Effort)?[),]/i.test(match.name)
      : !namedEffort.test(match.name)
  ) return base
  return {
    ...base,
    benchmarks: {
      source: 'Artificial Analysis',
      modelKey: base.modelKey,
      effort: effort ?? 'default',
      ...(modelLevel ? { reasoningMatch: 'unspecified' } : {}),
      testedModel: match.name,
      slug,
      url: `https://artificialanalysis.ai/models/${slug}`,
      fetchedAt: cache.fetchedAt,
      indexVersion: cache.indexVersion,
      scores: match.scores,
    },
  }
}
