import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'
import { ArtificialAnalysis, METRICS, withBenchmarks } from '../hosts/lib/benchmarks.js'
import { agentProfile } from '../src/catalog.js'
import { addAgent, editAgent, listAgents } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv } from './helpers.mjs'

const KEY = 'aa_synthetic_test_credential'
const NOW = Date.parse('2026-09-10T12:00:00Z')
const model = (slug = 'gpt-6-astra', name = 'GPT-6 Astra (max)', evaluations = {}) => ({
  id: slug,
  slug,
  name,
  evaluations: { artificial_analysis_intelligence_index: 51, ...evaluations },
})
const page = (models, current = 1, total = 1, tier = 'pro', version = 4.3) => ({
  tier,
  intelligence_index_version: version,
  pagination: { page: current, page_size: 200, total_pages: total, has_more: current < total },
  data: models,
})
function setup() {
  const t = tempEnv()
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  writeFileSync(join(t.env.CONSENSFLOW_HOME, 'artificial-analysis-key'), KEY, { mode: 0o600 })
  return t
}
const response = (body, status = 200, headers = {}) =>
  new Response(JSON.stringify(body), { status, headers })
const agent = { name: 'test', harness: 'codex', model: 'gpt-6-astra', effort: 'max' }

test('AA uses private headers, Free fallback, complete pagination and a shared daily disk cache', async () => {
  const t = setup()
  const calls = []
  const fetchImpl = async (url, options) => {
    calls.push(url)
    assert.equal(options.headers['x-api-key'], KEY)
    assert.equal(options.redirect, 'error')
    assert.ok(options.signal)
    if (!url.includes('/free')) return response({}, 403)
    const n = Number(new URL(url).searchParams.get('page'))
    return response(
      page(
        [
          model(
            n === 1 ? 'gpt-6-astra' : 'gpt-6-astra-low',
            n === 1 ? 'GPT-6 Astra (max)' : 'GPT-6 Astra (low)',
          ),
        ],
        n,
        2,
        'free',
      ),
    )
  }
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, { fetchImpl, now: () => NOW })
    const snapshots = await Promise.all([aa.refresh(), aa.refresh()])
    assert.deepEqual(snapshots[0], snapshots[1])
    assert.equal(calls.length, 3)
    assert.equal(snapshots[0].tier, 'free')
    assert.equal(Object.keys(snapshots[0].models).length, 2)
    await new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      fetchImpl,
      now: () => NOW + 1000,
    }).refresh()
    assert.equal(calls.length, 3)
    const file = join(t.env.CONSENSFLOW_HOME, 'artificial-analysis-cache.json')
    assert.equal(statSync(file).mode & 0o777, 0o600)
    assert.ok(!readFileSync(file, 'utf8').includes(KEY))
  } finally {
    t.cleanup()
  }
})

test('AA exact effort/snapshot mapping never borrows max scores for ultra, defaults or custom aliases', async () => {
  const t = setup()
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      now: () => NOW,
      fetchImpl: async () =>
        response(
          page([
            model(),
            model('gpt-6-astra-low', 'GPT-6 Astra (low)', {
              artificial_analysis_intelligence_index: 0,
            }),
            model('deepseek-v4-pro', 'DeepSeek V4 Pro 0813 (Reasoning, Max Effort)'),
          ]),
        ),
    })
    const cache = await aa.refresh()
    assert.equal(
      withBenchmarks(agent, agentProfile(agent), cache).benchmarks.scores.intelligence,
      51,
    )
    for (const patch of [
      { effort: 'ultra' },
      { effort: undefined },
      { model: 'custom/gpt-6-astra' },
      { harness: 'image' },
    ]) {
      const row = { ...agent, ...patch }
      assert.equal(withBenchmarks(row, agentProfile(row), cache).benchmarks, undefined)
    }
    const low = {
      ...agent,
      harness: 'pi',
      model: 'openai-codex/gpt-6-astra',
      thinking: 'low',
      effort: undefined,
    }
    assert.equal(withBenchmarks(low, agentProfile(low), cache).benchmarks.scores.intelligence, 0)
    const deep = {
      ...agent,
      harness: 'opencode',
      model: 'openrouter/deepseek/deepseek-v4-pro-0813',
    }
    assert.ok(withBenchmarks(deep, agentProfile(deep), cache).benchmarks)
    const rolling = { ...deep, model: 'opencode/deepseek-v4-pro' }
    assert.equal(withBenchmarks(rolling, agentProfile(rolling), cache).benchmarks, undefined)
    cache.models['gpt-6-astra'].name = 'GPT-6 Astra (high)'
    assert.equal(withBenchmarks(agent, agentProfile(agent), cache).benchmarks, undefined)
  } finally {
    t.cleanup()
  }
})

test('AA accepts published dotted and capitalized slugs without discarding the dataset', async () => {
  const t = setup()
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      now: () => NOW,
      fetchImpl: async () =>
        response(
          page([
            model(),
            model('qwen3-0.6b-instruct', 'Qwen3 0.6B Instruct'),
            model('QwQ-32B-Preview', 'QwQ 32B Preview'),
          ]),
        ),
    })
    const cache = await aa.refresh()
    assert.equal(cache.status, 'ready')
    assert.equal(Object.keys(cache.models).length, 3)
    assert.equal(
      withBenchmarks(agent, agentProfile(agent), cache).benchmarks.scores.intelligence,
      51,
    )
  } finally {
    t.cleanup()
  }
})

test('AA useful metrics retain units, zero, null and inverse hallucination semantics', async () => {
  const t = setup()
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      now: () => NOW,
      fetchImpl: async () =>
        response(
          page([
            model(undefined, undefined, {
              artificial_analysis_coding_index: null,
              terminalbench_v4_0: 0.62,
              aa_omniscience_non_hallucination_rate: 0.25,
              aa_omniscience_accuracy: 0,
              ifbench: 0.8,
              scicode: '0.4',
              hle: -1,
            }),
          ]),
        ),
    })
    const b = withBenchmarks(agent, agentProfile(agent), await aa.refresh()).benchmarks
    assert.equal(b.scores.terminal, 62)
    assert.equal(b.scores.hallucinations, 75)
    assert.equal(b.scores.accuracy, 0)
    assert.equal(b.scores.coding, undefined)
    assert.equal(b.scores.scicode, undefined)
    assert.equal(b.scores.hle, undefined)
    assert.equal(METRICS.find((m) => m.id === 'hallucinations').direction, 'asc')
    assert.equal(METRICS.find((m) => m.id === 'intelligence').unit, 'points')
    assert.equal(b.indexVersion, 4.3)
    assert.equal(b.testedModel, 'GPT-6 Astra (max)')
    assert.equal(b.url, 'https://artificialanalysis.ai/models/gpt-6-astra')
  } finally {
    t.cleanup()
  }
})

test('AA keeps the last complete version on malformed/mixed-version pagination and respects quota reset across restart', async () => {
  const t = setup()
  let now = NOW
  let mode = 'good'
  let calls = 0
  const fetchImpl = async (url) => {
    calls++
    if (mode === 'quota') return response({}, 429, { 'Retry-After': '7200' })
    if (mode === 'mixed')
      return response(
        page(
          [model()],
          Number(new URL(url).searchParams.get('page')),
          2,
          'pro',
          url.endsWith('page=2') ? 5 : 4.3,
        ),
      )
    if (mode === 'bad') return response({ data: [] })
    return response(page([model()]))
  }
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, { fetchImpl, now: () => now })
    await aa.refresh()
    for (const failure of ['mixed', 'bad', 'quota']) {
      now += 86400001
      mode = failure
      const cache = await aa.refresh()
      assert.equal(cache.status, 'stale')
      assert.equal(cache.indexVersion, 4.3)
      assert.equal(cache.models['gpt-6-astra'].scores.intelligence, 51)
    }
    const count = calls
    now += 3600000
    await new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, { fetchImpl, now: () => now }).refresh()
    assert.equal(calls, count)
    now += 3600001
    mode = 'good'
    assert.equal((await aa.refresh()).status, 'ready')
    assert.equal(calls, count + 1)
  } finally {
    t.cleanup()
  }
})

test('AA missing credentials and request failures never prevent agent browsing or expose secrets', async () => {
  const t = tempEnv()
  try {
    let calls = 0
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      fetchImpl: async () => {
        calls++
        throw Error(KEY)
      },
    })
    assert.equal((await aa.refresh()).status, 'unconfigured')
    assert.equal(calls, 0)
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'artificial-analysis-key'), KEY)
    const cache = await aa.refresh()
    assert.equal(cache.status, 'unavailable')
    assert.ok(!JSON.stringify(cache).includes(KEY))
  } finally {
    t.cleanup()
  }
})

test('AA retains a daily in-memory cache when the disk cache cannot be written', async () => {
  const t = setup()
  try {
    mkdirSync(join(t.env.CONSENSFLOW_HOME, 'artificial-analysis-cache.json'))
    let calls = 0
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      now: () => NOW,
      fetchImpl: async () => {
        calls++
        return response(page([model()]))
      },
    })
    await aa.refresh()
    assert.equal((await aa.refresh()).models['gpt-6-astra'].scores.intelligence, 51)
    assert.equal(calls, 1)
  } finally {
    t.cleanup()
  }
})

test('API, saved display profiles and edits use the same cache without credentials in responses', async () => {
  const t = setup()
  let server
  try {
    const aa = new ArtificialAnalysis(t.env.CONSENSFLOW_HOME, {
      fetchImpl: async () => response(page([model()])),
      now: () => Date.now(),
    })
    await aa.refresh()
    addAgent(agent, t.env)
    server = await startUiServer(t.env)
    const result = await fetch(`${server.url}/api/agents`, {
      headers: { authorization: `Bearer ${server.token}` },
    })
    const data = await result.json()
    assert.equal(data.agents[0].profile.benchmarks.scores.intelligence, 51)
    const disk = JSON.parse(readFileSync(join(t.env.CONSENSFLOW_HOME, 'agents.json'), 'utf8'))
    assert.deepEqual(data.agents[0].profile, disk.agents[0].profile)
    assert.ok(data.benchmarks.metrics.some((m) => m.id === 'hallucinations'))
    assert.ok(!JSON.stringify(data).includes(KEY))
    for (const path of ['', '/library']) {
      const html = await (await fetch(`${server.url}${path}?token=${server.token}`)).text()
      assert.ok(!html.includes(KEY))
    }
    editAgent(agent.name, { effort: 'ultra' }, t.env)
    assert.equal(listAgents(t.env)[0].profile.benchmarks, undefined)
  } finally {
    await server?.close()
    t.cleanup()
  }
})

test('AA model-level scores disclose unspecified reasoning without borrowing other efforts or snapshots', () => {
  const fixtures = [
    ['opencode', 'openrouter/qwen/qwen3.8-max', 'xhigh', 'qwen3-8-max', 'Qwen3.8 Max'],
    ['pi', 'openrouter/z-ai/glm-5.3-flash', 'max', 'glm-5-3-flash', 'GLM-5.3-Flash'],
    ['pi', 'openrouter/minimax/minimax-m3', undefined, 'minimax-m3', 'MiniMax-M3'],
    [
      'opencode',
      'openrouter/nvidia/nemotron-3-ultra-550b-a55b:free',
      'high',
      'nvidia-nemotron-3-ultra-550b-a55b',
      'Nemotron 3 Ultra 550B A55B (Reasoning)',
    ],
  ]
  const cache = { fetchedAt: new Date(NOW).toISOString(), indexVersion: 4.3, models: {} }
  for (const [harness, modelId, effort, slug, name] of fixtures) {
    cache.models[slug] = { name, scores: { intelligence: 40, coding: 0 } }
    const row = { harness, model: modelId, effort }
    const score = withBenchmarks(row, agentProfile(row), cache).benchmarks
    assert.equal(score?.reasoningMatch, 'unspecified')
    assert.equal(score.slug, slug)
    assert.equal(score.testedModel, name)
    assert.equal(score.scores.coding, 0)
    const custom = { ...row, model: 'custom/' + modelId }
    assert.equal(withBenchmarks(custom, agentProfile(custom), cache).benchmarks, undefined)
    cache.models[slug].name = name + ' (low)'
    assert.equal(withBenchmarks(row, agentProfile(row), cache).benchmarks, undefined)
  }
  cache.models['muse-spark-1-3-xhigh'] = {
    name: 'Muse Spark 1.3 (xhigh)',
    scores: { intelligence: 45.2 },
  }
  for (const modelId of [
    'openrouter/meta/muse-spark-1.3',
    'opencode-go/muse-spark-1.3-contributor',
    'opencode/muse-spark-1.3-contributor-free',
  ]) {
    const row = { harness: 'opencode', model: modelId, effort: 'xhigh' }
    const score = withBenchmarks(row, agentProfile(row), cache).benchmarks
    assert.equal(score?.scores.intelligence, 45.2)
    assert.equal(score.reasoningMatch, undefined)
    assert.equal(score.effort, 'xhigh')
  }
})
