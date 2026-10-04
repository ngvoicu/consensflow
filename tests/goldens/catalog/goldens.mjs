/**
 * The catalog's and the roster's goldens, as Node computes them: what the
 * Rust catalog (crates/cf-catalog) is held to. Every file is deterministic,
 * so the unit suite holds the committed copies equal to what this computes
 * (tests/catalog-goldens.test.mjs), and `npm run goldens:catalog` writes
 * them again after a change to the presets, the catalog or the roster.
 *
 * - `data/presets.json`: the presets and the model labels, the data the
 *   crate embeds while `hosts/lib/presets.js` stays their one source;
 * - `tests/goldens/catalog.json`: the catalog, each entry by name, the
 *   efforts, the work tiers, the harnesses and each kind's harness;
 * - `tests/goldens/profiles.json`: `agentProfile` over a corpus of agents;
 * - `tests/goldens/roster.json`: each roster operation on a file, at a
 *   fixed time: the file before and after, and what it answered or refused.
 *
 * A value JSON cannot hold, `undefined`, is written `{"$undefined":true}`.
 * Inputs that make the JavaScript throw a TypeError (a row that is no
 * object, a model that is no text) are left out and counted: the Rust
 * refuses them as a file that is no agents file, a difference kept on
 * purpose. The roster's errors name its file `«home»/agents.json`.
 */
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  AGENT_PRESETS,
  agentProfile,
  MODEL_LABELS,
  WORK_TIERS,
} from '../../../hosts/lib/presets.js'
import { CATALOG, catalogEntry, EFFORTS } from '../../../src/catalog.js'
import {
  addAgent,
  agentRow,
  editAgent,
  HARNESSES,
  harnessForKind,
  listAgents,
  normalizeRoster,
  preferences,
  removeAgent,
  setPreferences,
} from '../../../src/roster.js'

const REPO = fileURLToPath(new URL('../../..', import.meta.url))
const KINDS = ['claude-code', 'codex', 'pi', 'opencode', 'devin']
const HARNESS_OF = {
  'claude-code': 'claude',
  codex: 'codex',
  pi: 'pi',
  opencode: 'opencode',
  devin: 'devin',
}

/** `value` with every `undefined` it holds written as the marker. */
function encode(value) {
  if (value === undefined) return { $undefined: true }
  if (Array.isArray(value)) return value.map(encode)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, encode(item)]))
  }
  return value
}

/** A list as one item a line: diffs read item by item. */
const lines = (items) => `[\n${items.map((item) => JSON.stringify(item)).join(',\n')}\n]\n`
const pretty = (value) => `${JSON.stringify(value, null, 2)}\n`

function presetsData() {
  return pretty({ presets: AGENT_PRESETS, modelLabels: MODEL_LABELS })
}

function catalogGolden() {
  const names = [...AGENT_PRESETS.map((preset) => preset.preset), 'nobody', '', '@thoth', 'Thoth']
  return pretty({
    catalog: CATALOG,
    entries: names.map((name) => ({ name, entry: encode(catalogEntry(name)) })),
    efforts: EFFORTS,
    workTiers: WORK_TIERS,
    harnesses: HARNESSES,
    harnessForKind: [...KINDS, 'image', 'claude', ''].map((kind) => ({
      kind,
      harness: harnessForKind(kind),
    })),
  })
}

/** What `agentProfile` answers `agent`; none for an input it throws a TypeError on. */
function profileCase(agent, skipped) {
  try {
    return { agent, profile: encode(agentProfile(agent)) }
  } catch (cause) {
    if (cause instanceof TypeError) {
      skipped.count += 1
      return null
    }
    return { agent, error: cause.message }
  }
}

function profilesGolden(skipped) {
  const agents = AGENT_PRESETS.map((preset) => ({ ...preset }))
  const seen = new Set()
  const pairs = AGENT_PRESETS.map((preset) => [preset.kind, preset.model]).filter(
    ([kind, model]) => {
      const key = `${kind} ${model}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    },
  )
  const levels = (kind) => [...EFFORTS[HARNESS_OF[kind]], undefined, '', 'bogus']
  for (const [kind, model] of pairs) {
    for (const effort of levels(kind)) {
      agents.push({ kind, model, ...(effort === undefined ? {} : { effort }) })
      if (kind === 'pi') {
        agents.push({ kind, model, ...(effort === undefined ? {} : { thinking: effort }) })
        agents.push({
          kind,
          model,
          effort: 'low',
          ...(effort === undefined ? {} : { thinking: effort }),
        })
      }
    }
    // The app's own vocabulary, and a row whose harness and kind disagree.
    agents.push({ harness: HARNESS_OF[kind], model, effort: 'high' })
    agents.push({ harness: 'pi', kind, model, thinking: 'high' })
  }
  const unknown = [
    'openrouter/acme/model-x',
    'opencode/acme-x',
    'openai-codex/gpt-9',
    'anthropic/claude-x',
    'acme-x',
    '',
    undefined,
    'openrouter/anthropic/claude-fable-5.1',
    'claude-opus-5-5',
    'gpt-6-astra',
    'openrouter/moonshotai/kimi-k3',
    'muse-spark-1.3-contributor-free',
  ]
  for (const kind of [...KINDS, 'image', undefined]) {
    for (const model of unknown) {
      agents.push({
        ...(kind === undefined ? {} : { kind }),
        ...(model === undefined ? {} : { model }),
        effort: 'high',
      })
    }
    agents.push({ ...(kind === undefined ? {} : { kind }), model: 'gpt-6-astra', designer: true })
    agents.push({ ...(kind === undefined ? {} : { kind }), model: 'acme-x', designer: 1 })
    agents.push({ ...(kind === undefined ? {} : { kind }), designer: true })
  }
  for (const workTier of [
    undefined,
    null,
    'critical',
    'complex',
    'standard',
    'light',
    'huge',
    7,
    '',
  ]) {
    const tier = workTier === undefined ? {} : { workTier }
    agents.push({ kind: 'codex', model: 'gpt-6-astra', effort: 'max', ...tier })
    agents.push({ kind: 'pi', model: 'acme-x', thinking: 'low', ...tier })
  }
  return lines(agents.map((agent) => profileCase(agent, skipped)).filter(Boolean))
}

// --- the roster ---------------------------------------------------------------

/** A row the human defined, as the file keeps one. */
const row = (id, kind, model, extra = {}) => ({
  id,
  name: id.charAt(0).toUpperCase() + id.slice(1),
  kind,
  createdAt: '2026-09-01T10:00:00.000Z',
  updatedAt: '2026-09-01T10:00:00.000Z',
  model,
  ...extra,
})
const file = (document) => pretty(document)

function documents() {
  const thoth = AGENT_PRESETS.find((preset) => preset.preset === 'thoth')
  const nova = row('nova', 'codex', 'gpt-6-astra', { effort: 'high' })
  const pip = row('pip', 'pi', 'openrouter/acme/model-x', { thinking: 'low' })
  return {
    none: null,
    empty: '',
    invalid: '{ "agents": [1,] }',
    list: '[]',
    null: 'null',
    'empty object': '{}',
    v1: readFileSync(join(REPO, 'tests', 'fixtures', 'v1-agents.json'), 'utf8'),
    custom: file({ schemaVersion: 1, agents: [nova, pip] }),
    'unknown keys': file({
      note: 'kept',
      schemaVersion: 1,
      agents: [{ ...nova, colour: 'green' }, pip],
      2: 'an index key',
      trailing: { deep: [1, { x: null }] },
    }),
    'stale fields': file({
      schemaVersion: 1,
      agents: [{ ...nova, skills: ['a'], skillsPolicy: 'x', profile: { modelKey: 'k' } }],
    }),
    'catalog copies': file({
      schemaVersion: 1,
      agents: [
        {
          id: thoth.id,
          name: thoth.name,
          kind: thoth.kind,
          model: thoth.model,
          preset: thoth.preset,
        },
        { id: thoth.id, name: 'Edited', kind: 'codex', model: 'gpt-x', preset: thoth.preset },
        nova,
      ],
    }),
    'a custom agent hides a catalog name': file({
      schemaVersion: 1,
      agents: [row(thoth.id, 'codex', 'gpt-x', { effort: 'low' }), nova],
    }),
    duplicates: file({ schemaVersion: 1, agents: [nova, { ...nova, model: 'second' }, pip] }),
    'agents that are no list': file({ schemaVersion: 1, agents: 'x', other: 1 }),
    'agents an object': file({ agents: { nova } }),
    'no version': file({ agents: [nova] }),
    'a null version': file({ schemaVersion: null, agents: [nova] }),
    'a later version': file({ schemaVersion: 2, agents: [nova] }),
    preferences: file({
      schemaVersion: 1,
      agents: [nova, row('relay', 'pi', 'openrouter/anthropic/claude-fable-5.1')],
      preferences: { ownHarnessOnly: true, extra: 1 },
    }),
    'an image agent of before': file({
      schemaVersion: 1,
      agents: [row('painter', 'image', 'gpt-image-2', { description: 'Draws' }), nova],
    }),
    'a kind this build does not run': file({
      schemaVersion: 1,
      agents: [row('kim', 'kimi', 'kimi-k3', { effort: 'high' }), nova],
    }),
    'a tier no longer known': file({ schemaVersion: 1, agents: [{ ...nova, workTier: 'huge' }] }),
    // A row that keeps a `custom` key of its own: the roster's mark goes in its place.
    'custom kept': file({
      schemaVersion: 1,
      agents: [
        { id: 'nova', custom: false, kind: 'codex', model: 'gpt-6-astra' },
        { ...pip, custom: 'yes' },
      ],
    }),
    // Numbers as JSON.parse reads them: a whole double, past 2^53, past 1e21.
    numbers: `{"schemaVersion":1.0,"agents":[{"id":"nova","kind":"codex","model":"m","x":9007199254740993,"y":1e21,"z":2.50}],"w":-0.0}\n`,
  }
}

/**
 * Documents of seeded rows, for a sweep of every operation over shapes no
 * hand wrote: ids that are the human's, the catalog's or twice the same,
 * kinds that run and that do not, efforts under either key, tiers, image
 * flags, descriptions of any JSON, stored `custom` marks, provenance that is
 * the entry's or not, stale and unknown fields. Every row is of the shape
 * both implementations read the same (its id, kind and model text when
 * there, its preset, harness, effort and thinking text or null): the
 * shapes the Rust refuses on purpose are its own tests'.
 */
function sweptDocuments(random) {
  const pick = (list) => list[Math.floor(random() * list.length)]
  const maybe = (key, values) => {
    const value = pick(values)
    return value === undefined ? {} : { [key]: value }
  }
  const ids = ['nova', 'pip', 'kim', 'zed', 'thoth', 'zeus', 'pygmalion', 'Nova', '']
  const row = () => ({
    ...maybe('id', [...ids, undefined]),
    ...maybe('name', ['Nova', undefined, 7]),
    ...maybe('kind', [
      'claude-code',
      'codex',
      'pi',
      'opencode',
      'devin',
      'image',
      'kimi',
      '',
      undefined,
    ]),
    ...maybe('designer', [undefined, undefined, true, false, 'yes', 1, null]),
    ...maybe('model', [
      'gpt-6-astra',
      'openrouter/anthropic/claude-fable-5.1',
      'claude-opus-5-5',
      'm',
      '',
      undefined,
    ]),
    ...maybe('effort', [undefined, 'high', 'max', '', null, 'bogus']),
    ...maybe('thinking', [undefined, 'low', '', null]),
    ...maybe('harness', [undefined, undefined, 'codex', 'claude', null]),
    ...maybe('workTier', [undefined, undefined, 'complex', 'light', null, 'huge']),
    ...maybe('description', [undefined, 'Mine', '', 5, null, { a: 1 }, ['x']]),
    ...maybe('custom', [undefined, undefined, true, false, 'yes']),
    ...maybe('preset', [undefined, undefined, 'thoth', 'zeus', 'pygmalion', null, '']),
    ...maybe('skills', [undefined, undefined, ['a']]),
    ...maybe('colour', [undefined, undefined, 'green', { deep: [1] }]),
  })
  return Array.from({ length: 24 }, (_, index) => {
    const agents = Array.from({ length: 1 + Math.floor(random() * 4) }, row)
    const document = {
      ...maybe('note', [undefined, 'kept']),
      ...maybe('schemaVersion', [1, 1, undefined, null, 2]),
      agents,
      ...maybe('preferences', [
        undefined,
        { ownHarnessOnly: true },
        { ownHarnessOnly: 'yes' },
        null,
      ]),
    }
    return [`swept ${index}`, file(document)]
  })
}

/** The roster's operations, each called with the home's environment last. */
const READS = [
  ['listAgents'],
  ['agentRow', 'thoth'],
  ['agentRow', 'nova'],
  ['agentRow', '@nova'],
  ['agentRow', 'nobody'],
  ['agentRow', undefined],
  ['preferences'],
  ['normalizeRoster'],
]
const WRITES = [
  ['setPreferences', {}],
  ['setPreferences', { ownHarnessOnly: true }],
  ['setPreferences', { ownHarnessOnly: 'yes' }],
  ['setPreferences', { other: true, ownHarnessOnly: false }],
  ['setPreferences', null],
  [
    'addAgent',
    {
      name: 'zed',
      harness: 'codex',
      model: 'gpt-6-astra',
      effort: 'high',
      description: 'Mine',
      workTier: 'complex',
    },
  ],
  ['addAgent', { name: 'pi-two', harness: 'pi', model: 'openrouter/acme/model-x', effort: 'low' }],
  ['addAgent', { name: 'painter-two', harness: 'codex', model: 'gpt-image-2', designer: true }],
  ['addAgent', { name: 'Bad Name', harness: 'codex', model: 'm' }],
  ['addAgent', { name: 'thoth', harness: 'codex', model: 'm' }],
  ['addAgent', { name: 'zed', harness: 'kimi', model: 'm' }],
  ['addAgent', { name: 'zed', harness: 'pi', model: 'm', designer: true }],
  ['addAgent', { name: 'zed', harness: 'codex', model: 'm', designer: 'yes' }],
  ['addAgent', { name: 'zed', harness: 'codex', model: '' }],
  ['addAgent', { name: 'zed', harness: 'codex', model: 'm', workTier: 'huge' }],
  ['addAgent', { harness: 'codex', model: 'm' }],
  ['addAgent', { name: 'nova', harness: 'codex', model: 'm' }],
  ['editAgent', 'thoth', { model: 'x' }],
  ['editAgent', 'nobody', { model: '' }],
  ['editAgent', 'nova', { model: 'gpt-x' }],
  ['editAgent', 'nova', { model: '' }],
  ['editAgent', 'nova', { description: 'Changed', workTier: null }],
  ['editAgent', 'nova', { workTier: 'standard' }],
  ['editAgent', 'nova', { workTier: 'huge' }],
  ['editAgent', 'nova', { effort: '' }],
  ['editAgent', 'nova', { effort: null }],
  ['editAgent', 'nova', { effort: 'max' }],
  ['editAgent', 'pip', { effort: 'high' }],
  ['editAgent', 'nova', {}],
  ['editAgent', 'painter', { effort: 'high' }],
  ['editAgent', 'painter', { model: 'gpt-image-3' }],
  ['editAgent', 'kim', { effort: 'high' }],
  ['removeAgent', 'thoth'],
  ['removeAgent', 'nobody'],
  ['removeAgent', 'nova'],
  ['removeAgent', 'pip'],
]
const OPERATIONS = {
  listAgents,
  agentRow,
  preferences,
  setPreferences,
  normalizeRoster,
  addAgent,
  editAgent,
  removeAgent,
}

/** `Date` as the roster reads it: `new Date()` is `now`, any other is itself. */
function fixedDate(now) {
  const RealDate = globalThis.Date
  return class extends RealDate {
    constructor(...args) {
      if (args.length === 0) super(now)
      else super(...args)
    }
    static now() {
      return now
    }
  }
}

/** One operation on a home holding `before`, at `now`: the case, or none for a TypeError. */
function rosterCase(before, call, now, skipped) {
  const home = mkdtempSync(join(tmpdir(), 'cf-catalog-golden-'))
  const path = join(home, 'agents.json')
  if (before !== null) writeFileSync(path, before)
  const [name, ...args] = call
  const RealDate = globalThis.Date
  globalThis.Date = fixedDate(now)
  let outcome
  try {
    outcome = { result: encode(OPERATIONS[name](...args, { CONSENSFLOW_HOME: home })) }
  } catch (cause) {
    if (cause instanceof TypeError) outcome = null
    // The file's path, whatever the platform writes it as, is «home»/agents.json.
    else outcome = { error: cause.message.replaceAll(path, '«home»/agents.json') }
  } finally {
    globalThis.Date = RealDate
  }
  let after = null
  try {
    after = readFileSync(path, 'utf8')
  } catch {}
  const leftovers = readdirSync(home).filter((entry) => entry !== 'agents.json')
  rmSync(home, { recursive: true, force: true })
  if (outcome === null) {
    skipped.count += 1
    return null
  }
  return {
    call: encode(call),
    at: new Date(now).toISOString(),
    ...outcome,
    ...(after === before ? { unchanged: true } : { after }),
    ...(leftovers.length === 0 ? {} : { leftovers }),
  }
}

/** A seeded sequence of numbers in [0, 1): the same sweep every run. */
function seeded(seed) {
  let state = seed >>> 0
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state / 2 ** 32
  }
}

function rosterGolden(skipped) {
  const docs = documents()
  const start = Date.UTC(2026, 9, 4, 12, 0, 0)
  let tick = 0
  const now = () => start + 1000 * tick++
  const cases = []
  for (const [document, before] of Object.entries(docs)) {
    for (const call of [...READS, ...WRITES]) {
      const result = rosterCase(before, call, now(), skipped)
      if (result !== null) cases.push({ document, ...result })
    }
  }
  // A sweep: seeded documents, each with a seeded handful of operations.
  const swept = seeded(20261005)
  const sweptCalls = [...READS.filter(([name]) => name !== 'listAgents'), ...WRITES]
  for (const [document, before] of sweptDocuments(swept)) {
    docs[document] = before
    const calls = [
      ['listAgents'],
      ...Array.from({ length: 10 }, () => sweptCalls[Math.floor(swept() * sweptCalls.length)]),
    ]
    for (const call of calls) {
      const result = rosterCase(before, call, now(), skipped)
      if (result !== null) cases.push({ document, ...result })
    }
  }
  // Sequences: each step starts from the file the one before it left.
  const random = seeded(20261004)
  const pick = (list) => list[Math.floor(random() * list.length)]
  for (const document of ['custom', 'v1', 'preferences', 'none', 'unknown keys']) {
    let before = docs[document]
    for (let step = 0; step < 12; step += 1) {
      const call = pick([...READS, ...WRITES])
      const result = rosterCase(before, call, now(), skipped)
      if (result === null) continue
      cases.push({ sequence: document, step, before, ...result })
      if (result.after !== undefined) before = result.after
    }
  }
  const documentsText = JSON.stringify(docs, null, 2).replaceAll('\n', '\n  ')
  const casesText = cases.map((item) => `    ${JSON.stringify(item)}`).join(',\n')
  return `{\n  "documents": ${documentsText},\n  "cases": [\n${casesText}\n  ]\n}\n`
}

/** Every golden, by its path under crates/cf-catalog; and how many inputs were left out. */
export function catalogGoldens() {
  const skipped = { count: 0 }
  const files = {
    'data/presets.json': presetsData(),
    'tests/goldens/catalog.json': catalogGolden(),
    'tests/goldens/profiles.json': profilesGolden(skipped),
    'tests/goldens/roster.json': rosterGolden(skipped),
  }
  return { files, skipped: skipped.count }
}

/** Writes every golden into `crate`. */
export function writeCatalogGoldens(crate) {
  const { files, skipped } = catalogGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const path = join(crate, ...relative.split('/'))
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, text)
  }
  return { written: Object.keys(files).length, skipped }
}
