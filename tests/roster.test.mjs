import assert from 'node:assert/strict'
import { cpSync, existsSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { AGENT_PRESETS } from '../hosts/lib/presets.js'
import {
  addAgent,
  agentRow,
  configRoot,
  editAgent,
  legacyConfigRoot,
  listAgents,
  migrateStateRoot,
  normalizeRoster,
  removeAgent,
  resetAgent,
  rosterPath,
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
    assert.equal(agents.endymion.effort, 'xhigh')
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
          profile: { categories: ['coding', 'lead', 'pm'] },
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

describe('a catalog agent carries the human’s overrides, and only those are stored', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('an edit stores what differs from the catalog, and the listing shows it edited', () => {
    editAgent('gefjon', { effort: 'low', description: 'the cheap one' }, t.env)
    const [stored] = raw(t.env).agents
    assert.deepEqual(
      Object.keys(stored).sort(),
      ['createdAt', 'description', 'effort', 'id', 'kind', 'name', 'preset', 'updatedAt'],
      'only the changed fields, with the row’s identity',
    )
    assert.deepEqual(
      [stored.effort, stored.description, stored.model],
      ['low', 'the cheap one', undefined],
    )
    const gefjon = byName(t.env).gefjon
    assert.deepEqual(
      [gefjon.effort, gefjon.description, gefjon.model, gefjon.edited, gefjon.custom],
      ['low', 'the cheap one', 'opencode/muse-spark-1.3-contributor-free', true, undefined],
    )
    assert.equal(agentRow('gefjon', t.env).effort, 'low', 'the launcher runs the override')
    assert.equal(listAgents(t.env).length, AGENT_PRESETS.length, 'still one agent per entry')
  })

  it('a pi agent edit lands in `thinking`, the key the pi runner reads', () => {
    editAgent('endymion', { effort: 'high' }, t.env)
    const endymion = raw(t.env).agents.find((p) => p.id === 'endymion')
    assert.equal(endymion.thinking, 'high')
    assert.equal(endymion.effort, undefined)
    assert.equal(byName(t.env).endymion.effort, 'high')
  })

  it('an edit back to the catalog’s value drops the override, and the row when nothing is left', () => {
    editAgent('gefjon', { effort: 'xhigh' }, t.env)
    assert.deepEqual(raw(t.env).agents.find((p) => p.id === 'gefjon').effort, undefined)
    editAgent(
      'gefjon',
      { description: 'OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH' },
      t.env,
    )
    assert.equal(
      raw(t.env).agents.some((p) => p.id === 'gefjon'),
      false,
    )
    assert.equal(byName(t.env).gefjon.edited, undefined)
  })

  it('a blank effort means the catalog’s own; a tier override stays until cleared', () => {
    editAgent('gefjon', { effort: 'low', workTier: 'complex' }, t.env)
    editAgent('gefjon', { effort: '' }, t.env)
    const gefjon = byName(t.env).gefjon
    assert.deepEqual(
      [gefjon.effort, gefjon.workTier, gefjon.profile.workTier],
      ['xhigh', 'complex', 'complex'],
    )
    editAgent('gefjon', { workTier: null }, t.env)
    assert.equal(byName(t.env).gefjon.workTier, undefined)
    assert.equal(
      raw(t.env).agents.some((p) => p.id === 'gefjon'),
      false,
    )
  })

  it('reset returns a catalog agent to the catalog; remove is refused for it', () => {
    editAgent('endymion', { model: 'openrouter/somebody/else' }, t.env)
    assert.equal(byName(t.env).endymion.edited, true)
    const reset = resetAgent('endymion', t.env)
    assert.deepEqual([reset.edited, reset.model !== 'openrouter/somebody/else'], [undefined, true])
    assert.equal(
      raw(t.env).agents.some((p) => p.id === 'endymion'),
      false,
    )
    resetAgent('endymion', t.env)
    assert.throws(() => removeAgent('endymion', t.env), /in the catalog: reset it/)
  })

  it('validates the effort the harness accepts, and refuses one on an image agent', () => {
    for (const effort of ['medium', 'ultra', 'on', 0, false]) {
      assert.throws(() => editAgent('ilmarinen', { effort }, t.env), /low.*high.*max/)
    }
    editAgent('ilmarinen', { effort: 'low' }, t.env)
    assert.equal(byName(t.env).ilmarinen.effort, 'low')
    editAgent('ilmarinen', { effort: '' }, t.env)
    assert.equal(byName(t.env).ilmarinen.effort, 'max', 'blank: the catalog’s own')
    assert.throws(() => editAgent('pygmalion', { effort: 'high' }, t.env), /no effort level/)
    editAgent('pygmalion', { description: 'still editable' }, t.env)
    assert.equal(byName(t.env).pygmalion.edited, true)
  })

  it('a catalog name cannot be added, and an unknown name is named on edit, reset and remove', () => {
    assert.throws(
      () => addAgent({ name: 'gefjon', harness: 'codex', model: 'm' }, t.env),
      /in the catalog already/,
    )
    assert.throws(() => editAgent('nobody', { model: 'm' }, t.env), /nobody/)
    assert.throws(() => resetAgent('nobody', t.env), /nobody/)
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
  })

  it('edits and removes a custom agent in place, and refuses to reset it', () => {
    addAgent({ name: 'freya-2', harness: 'codex', model: 'gpt-5.6-terra', effort: 'xhigh' }, t.env)
    editAgent('freya-2', { model: 'gpt-6-astra', effort: 'low' }, t.env)
    const stored = raw(t.env).agents.find((p) => p.id === 'freya-2')
    assert.deepEqual([stored.model, stored.effort, stored.kind], ['gpt-6-astra', 'low', 'codex'])
    assert.throws(() => resetAgent('freya-2', t.env), /your own agent/)
    removeAgent('freya-2', t.env)
    assert.equal(
      raw(t.env).agents.some((p) => p.id === 'freya-2'),
      false,
    )
    assert.equal(byName(t.env)['freya-2'], undefined)
  })

  it('a Kimi agent takes only the efforts Kimi accepts, on add and on edit', () => {
    for (const effort of ['medium', 'xhigh', 'ultra', 'off', 'on', 0, false]) {
      assert.throws(
        () =>
          addAgent(
            { name: 'invalid', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort },
            t.env,
          ),
        /low.*high.*max/,
      )
    }
    addAgent(
      { name: 'my-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'low' },
      t.env,
    )
    assert.throws(() => editAgent('my-kimi', { effort: 'medium' }, t.env), /low.*high.*max/)
    editAgent('my-kimi', { effort: '' }, t.env)
    assert.equal(byName(t.env)['my-kimi'].effort, undefined, 'blank restores native settings')
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

  it('a full copy of a catalog entry reads as overrides of what differs, without writing', () => {
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
          description: 'OpenCode Zen Muse Spark 1.3 Contributor FREE XHIGH',
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
          skillsPolicy: 'default',
        },
      ],
    })
    writeFileSync(rosterPath(t.env), original)
    const agents = byName(t.env)
    assert.equal(agents.gefjon.edited, undefined, 'equal to the catalog: nothing overridden')
    assert.deepEqual([agents.apollo.effort, agents.apollo.edited], ['low', true])
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), original, 'a read writes nothing')
  })

  it('normalizing keeps only the overrides, drops stored display data, and is idempotent', () => {
    assert.equal(normalizeRoster(t.env), true)
    const file = raw(t.env)
    assert.deepEqual(
      file.agents.map((row) => [row.id, Object.keys(row).sort()]),
      [['apollo', ['effort', 'id', 'kind', 'name', 'preset']]],
    )
    assert.equal(normalizeRoster(t.env), false)
    assert.deepEqual([byName(t.env).apollo.effort, byName(t.env).gefjon.edited], ['low', undefined])
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

it('legacy import never copies a link that could redirect a future write outside the home', () => {
  const t = tempEnv()
  try {
    const legacy = legacyConfigRoot(t.env)
    mkdirSync(legacy, { recursive: true })
    const outside = join(t.root, 'outside.json')
    writeFileSync(outside, 'preserve')
    symlinkSync(outside, join(legacy, 'hosts.json'))
    writeFileSync(join(legacy, 'mode.json'), JSON.stringify({ mode: 'claude' }))
    migrateStateRoot(t.env)
    assert.equal(existsSync(join(configRoot(t.env), 'hosts.json')), false)
    assert.equal(readFileSync(outside, 'utf8'), 'preserve')
    assert.equal(readFileSync(join(legacy, 'hosts.json'), 'utf8'), 'preserve')
  } finally {
    t.cleanup()
  }
})
