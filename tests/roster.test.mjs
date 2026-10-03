import assert from 'node:assert/strict'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { AGENT_PRESETS } from '../hosts/lib/presets.js'
import {
  addAgent,
  agentRow,
  editAgent,
  listAgents,
  normalizeRoster,
  preferences,
  removeAgent,
  rosterPath,
  setPreferences,
} from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

const FIXTURES = join(import.meta.dirname, 'fixtures')
const raw = (env) => JSON.parse(readFileSync(rosterPath(env), 'utf8'))
const byName = (env) => Object.fromEntries(listAgents(env).map((p) => [p.name, p]))

function seedSharedRoster(t) {
  mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
  cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))
}

describe('the roster is the catalog plus what is the human’s own', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('uses agents.json inside the explicitly configured private home', () => {
    assert.equal(rosterPath(t.env), join(t.root, 'consensflow', 'agents.json'))
  })

  it('lists every catalog agent with no file at all, as the catalog has it', () => {
    const agents = listAgents(t.env)
    assert.equal(agents.length, AGENT_PRESETS.length)
    const gefjon = agents.find((p) => p.name === 'gefjon')
    assert.deepEqual(
      [gefjon.harness, gefjon.model, gefjon.effort, gefjon.preset, gefjon.custom, gefjon.edited],
      [
        'opencode',
        'opencode/muse-spark-1.3-contributor-free',
        'xhigh',
        'gefjon',
        undefined,
        undefined,
      ],
    )
    assert.equal(gefjon.description, 'OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH')
    assert.ok(gefjon.profile.workTier)
    assert.equal(existsSync(rosterPath(t.env)), false, 'listing writes nothing')
    const row = agentRow('gefjon', t.env)
    assert.deepEqual([row.kind, row.model, row.effort], ['opencode', gefjon.model, 'xhigh'])
    assert.equal(agentRow('@gefjon', t.env).id, 'gefjon')
  })

  it('reads v1 rows as agents: kind→harness, thinking/effort→effort', () => {
    seedSharedRoster(t)
    const agents = byName(t.env)
    assert.equal(agents.zeus.harness, 'claude')
    assert.equal(agents.zeus.effort, 'max')
    assert.equal(agents.endymion.harness, 'pi')
    // The fixture's copy says xhigh; a catalog agent reads as the catalog has it.
    assert.equal(agents.endymion.effort, 'max')
    assert.equal(agents.mani.harness, 'opencode')
  })

  it('lists an image agent as a harness it runs, not as an oddity', () => {
    const pygmalion = byName(t.env).pygmalion
    assert.equal(pygmalion.harness, 'image')
    assert.equal(pygmalion.unsupported, undefined, 'cf run spawns it like any other')
  })
})

it('reports current tiers from legacy rows without writing during discovery', () => {
  const t = tempEnv()
  try {
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    const original = JSON.stringify({
      agents: [
        {
          id: 'renamed',
          kind: 'claude-code',
          model: 'claude-fable-5-1',
          effort: 'max',
          skillsPolicy: 'default',
          profile: { categories: ['coding', 'chief', 'pm'] },
        },
      ],
    })
    writeFileSync(rosterPath(t.env), original)
    const agent = byName(t.env).renamed
    assert.equal(agent.profile.workTier, 'critical')
    assert.equal(agent.custom, true)
    assert.equal(Object.hasOwn(agent.profile, 'categories'), false, 'stale pills are dropped')
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), original)
  } finally {
    t.cleanup()
  }
})

describe('a catalog agent stays as the catalog has it', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('is neither edited nor removed, and its name cannot be defined again', () => {
    assert.throws(() => editAgent('gefjon', { effort: 'low' }, t.env), /catalog agent and stays/)
    assert.throws(() => editAgent('gefjon', { description: 'x' }, t.env), /catalog agent and stays/)
    assert.throws(() => removeAgent('gefjon', t.env), /not yours to remove/)
    assert.throws(
      () => addAgent({ name: 'gefjon', harness: 'codex', model: 'm' }, t.env),
      /catalog agent: pick another name/,
    )
    assert.equal(existsSync(rosterPath(t.env)), false, 'nothing was written')
    assert.deepEqual(
      [byName(t.env).gefjon.effort, byName(t.env).gefjon.custom],
      ['xhigh', undefined],
    )
  })

  it('names an unknown agent on edit and remove', () => {
    assert.throws(() => editAgent('nobody', { model: 'm' }, t.env), /nobody/)
    assert.throws(() => removeAgent('nobody', t.env), /nobody/)
  })
})

describe('agents defined by hand are stored in full, v1-shaped', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('starts with the catalog only and creates the v1 file shape on first add', () => {
    assert.equal(
      listAgents(t.env).every((p) => p.custom === undefined),
      true,
    )
    addAgent({ name: 'mine', harness: 'claude', model: 'claude-opus-5' }, t.env)
    const file = raw(t.env)
    assert.equal(file.schemaVersion, 1)
    assert.equal(file.agents[0].id, 'mine')
    assert.equal(file.agents[0].name, 'Mine')
    assert.equal(file.agents[0].kind, 'claude-code')
    assert.ok(file.agents[0].createdAt)
    assert.equal(Object.hasOwn(file.agents[0], 'profile'), false, 'no display data in the file')
    const mine = byName(t.env).mine
    assert.deepEqual([mine.custom, mine.preset, mine.harness], [true, undefined, 'claude'])
    assert.equal(listAgents(t.env).length, AGENT_PRESETS.length + 1)
  })

  it('validates adds: bad names, unknown harnesses, empty models, duplicates', () => {
    assert.throws(() => addAgent({ name: 'Bad Name', harness: 'claude', model: 'm' }, t.env))
    assert.throws(() => addAgent({ name: 'ok', harness: 'not-a-cli', model: 'm' }, t.env))
    assert.throws(() => addAgent({ name: 'ok', harness: 'claude', model: '' }, t.env))
    assert.throws(() => addAgent({ name: 'mine', harness: 'codex', model: 'm' }, t.env))
    // A pi agent edit lands in `thinking`, the key the pi runner reads.
    addAgent({ name: 'my-pi', harness: 'pi', model: 'openrouter/moonshotai/kimi-k3' }, t.env)
    editAgent('my-pi', { effort: 'high' }, t.env)
    const pi = raw(t.env).agents.find((p) => p.id === 'my-pi')
    assert.deepEqual(
      [pi.thinking, pi.effort, byName(t.env)['my-pi'].effort],
      ['high', undefined, 'high'],
    )
    // An image agent has no effort to edit, plainly.
    addAgent({ name: 'my-image', harness: 'image', model: 'codex-image' }, t.env)
    assert.throws(() => editAgent('my-image', { effort: 'high' }, t.env), /no effort level/)
    editAgent('my-image', { description: 'still editable' }, t.env)
  })

  it('edits and removes a custom agent in place', () => {
    addAgent({ name: 'freya-2', harness: 'codex', model: 'gpt-5.6-terra', effort: 'xhigh' }, t.env)
    // An edit that would leave it without a model is refused, and saves nothing.
    const before = readFileSync(rosterPath(t.env), 'utf8')
    for (const model of ['', 42])
      assert.throws(() => editAgent('freya-2', { model }, t.env), /an agent needs a model/)
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), before)
    editAgent('freya-2', { model: 'gpt-6-astra', effort: 'low' }, t.env)
    const stored = raw(t.env).agents.find((p) => p.id === 'freya-2')
    assert.deepEqual([stored.model, stored.effort, stored.kind], ['gpt-6-astra', 'low', 'codex'])
    removeAgent('freya-2', t.env)
    assert.equal(
      raw(t.env).agents.some((p) => p.id === 'freya-2'),
      false,
    )
    assert.equal(byName(t.env)['freya-2'], undefined)
  })

  it('defines no agent on Kimi, and reads one an older build saved as a harness it does not run', () => {
    assert.throws(
      () => addAgent({ name: 'my-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3' }, t.env),
      /unknown harness "kimi"/,
    )
    const file = raw(t.env)
    file.agents.push({
      id: 'old-kimi',
      name: 'Old-kimi',
      kind: 'kimi',
      model: 'moonshot-ai/kimi-k3',
    })
    writeFileSync(rosterPath(t.env), JSON.stringify(file))
    assert.equal(byName(t.env)['old-kimi'].unsupported, true)
    removeAgent('old-kimi', t.env)
  })

  it('a custom row that took a catalog name on another harness hides that entry', () => {
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    const file = raw(t.env)
    file.agents.push({
      id: 'zeus',
      name: 'Zeus',
      kind: 'opencode',
      model: 'opencode/muse-spark-1.3',
    })
    writeFileSync(rosterPath(t.env), JSON.stringify(file, null, 2))
    const zeus = byName(t.env).zeus
    assert.deepEqual([zeus.harness, zeus.custom, zeus.preset], ['opencode', true, undefined])
    assert.equal(listAgents(t.env).filter((p) => p.name === 'zeus').length, 1)
    removeAgent('zeus', t.env)
    assert.equal(byName(t.env).zeus.harness, 'claude', 'the catalog entry is back')
  })
})

describe('what older builds wrote is read the same, and folded at start', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('a copy of a catalog entry, edited or not, reads as the catalog has it, without writing', () => {
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    const original = JSON.stringify({
      schemaVersion: 1,
      agents: [
        {
          id: 'gefjon',
          name: 'Gefjon',
          kind: 'opencode',
          model: 'opencode/muse-spark-1.3-contributor-free',
          effort: 'xhigh',
          preset: 'gefjon',
          profile: { workTier: 'light' },
        },
        {
          id: 'apollo',
          name: 'Apollo',
          kind: 'claude-code',
          model: 'claude-opus-5',
          effort: 'low',
          preset: 'apollo',
        },
        {
          id: 'mine',
          name: 'Mine',
          kind: 'codex',
          model: 'gpt-6-astra',
          effort: 'low',
          skillsPolicy: 'default',
        },
      ],
    })
    writeFileSync(rosterPath(t.env), original)
    const agents = byName(t.env)
    assert.deepEqual(
      [agents.gefjon.custom, agents.apollo.effort, agents.mine.custom],
      [undefined, 'xhigh', true],
    )
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), original, 'a read writes nothing')
  })

  it('normalizing keeps only the human’s own agents, drops stored display data, and is idempotent', () => {
    assert.equal(normalizeRoster(t.env), true)
    assert.deepEqual(
      raw(t.env).agents.map((row) => [row.id, Object.keys(row).sort()]),
      [['mine', ['effort', 'id', 'kind', 'model', 'name']]],
    )
    assert.equal(normalizeRoster(t.env), false)
  })

  it('normalizing a home with no roster writes nothing', () => {
    const fresh = tempEnv()
    try {
      assert.equal(normalizeRoster(fresh.env), false)
      assert.equal(existsSync(rosterPath(fresh.env)), false)
    } finally {
      fresh.cleanup()
    }
  })
})

describe('a roster written before the rename keeps working', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('reads participants.json and its participants key, then writes agents.json', () => {
    // Exactly what a machine set up before 2026-08-21 has on disk.
    const legacy = join(dirname(rosterPath(t.env)), 'participants.json')
    mkdirSync(dirname(legacy), { recursive: true })
    cpSync(join(FIXTURES, 'v1-participants.json'), legacy)

    const listed = listAgents(t.env)
    assert.ok(
      listed.some((a) => a.name === 'zeus'),
      'the old file is read, not ignored',
    )

    // The first write moves the roster to its new name, rows intact.
    addAgent({ name: 'newcomer', harness: 'codex', model: 'gpt-5.6-luna' }, t.env)
    const written = raw(t.env)
    assert.ok(Array.isArray(written.agents), 'written under the agents key')
    assert.equal(written.participants, undefined, 'the old key does not survive the write')
    assert.ok(written.agents.some((row) => row.id === 'newcomer'))
    assert.equal(
      listAgents(t.env).some((a) => a.name === 'newcomer'),
      true,
    )
  })
})

describe('an agents file that cannot be read', () => {
  it('is said, never read as an empty roster, and never saved over', () => {
    const t = tempEnv()
    try {
      addAgent({ name: 'mybuilder', harness: 'claude', model: 'claude-opus-5-5' }, t.env)
      const file = rosterPath(t.env)
      // A hand edit leaves a trailing comma.
      const broken = readFileSync(file, 'utf8').replace(/\n\s*\]/, ',\n  ]')
      writeFileSync(file, broken)
      const said = /agents file .*agents\.json is not valid JSON: fix it or move it away/
      for (const read of [
        () => listAgents(t.env),
        () => agentRow('mybuilder', t.env),
        () => preferences(t.env),
        () => normalizeRoster(t.env),
      ])
        assert.throws(read, said)
      for (const write of [
        () => setPreferences({ ownHarnessOnly: true }, t.env),
        () => addAgent({ name: 'other', harness: 'codex', model: 'gpt-6-astra' }, t.env),
        () => editAgent('mybuilder', { model: 'claude-sonnet-5-5' }, t.env),
        () => removeAgent('mybuilder', t.env),
      ])
        assert.throws(write, said)
      assert.equal(readFileSync(file, 'utf8'), broken, 'the file is as the human left it')
      writeFileSync(file, 'null')
      assert.throws(() => listAgents(t.env), /is not an agents file/)
    } finally {
      t.cleanup()
    }
  })

  it('is written whole or not at all, beside itself and then in its place', () => {
    const t = tempEnv()
    try {
      addAgent({ name: 'mybuilder', harness: 'claude', model: 'claude-opus-5-5' }, t.env)
      const file = rosterPath(t.env)
      const before = statSync(file).ino
      setPreferences({ ownHarnessOnly: true }, t.env)
      assert.notEqual(statSync(file).ino, before, 'a new file took its place')
      assert.deepEqual(readdirSync(dirname(file)), ['agents.json'], 'nothing is left beside it')
      assert.equal(preferences(t.env).ownHarnessOnly, true)
    } finally {
      t.cleanup()
    }
  })
})

describe('the roster keeps what the human chose about it', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('hides Claude and OpenAI models on Pi and OpenCode when they are kept to their own harnesses', () => {
    {
      assert.deepEqual(preferences(t.env), { ownHarnessOnly: false })
      assert.ok(!listAgents(t.env).some((p) => p.hidden), 'nothing hidden by default')
      assert.deepEqual(setPreferences({ ownHarnessOnly: true }, t.env), { ownHarnessOnly: true })
      const hidden = (name) => listAgents(t.env).find((p) => p.name === name).hidden === true
      assert.deepEqual(
        ['kronos', 'baldr', 'phoebe', 'bil', 'aurora', 'apollo', 'diana', 'ares', 'gefjon'].map(
          hidden,
        ),
        [true, true, true, true, true, false, false, false, false],
        'Opus, Luna and Sol through Pi or OpenCode; never on Claude Code or Codex, never Grok or Muse',
      )
      assert.deepEqual(raw(t.env).preferences, { ownHarnessOnly: true }, 'kept in the file')
      normalizeRoster(t.env)
      assert.deepEqual(preferences(t.env), { ownHarnessOnly: true }, 'a fold at start keeps it')
      assert.throws(() => setPreferences({ ownHarnessOnly: 'yes' }, t.env), /is on or off/)
      assert.throws(() => setPreferences({ colour: true }, t.env), /no preference named colour/)
      setPreferences({ ownHarnessOnly: false }, t.env)
      assert.ok(!listAgents(t.env).some((p) => p.hidden))
    }
  })
})
