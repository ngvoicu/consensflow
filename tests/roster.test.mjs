import assert from 'node:assert/strict'
import { cpSync, existsSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  addAgent,
  agentDrift,
  configRoot,
  editAgent,
  legacyConfigRoot,
  listAgents,
  migrateStateRoot,
  removeAgent,
  rosterPath,
  syncAgents,
} from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

const FIXTURES = join(import.meta.dirname, 'fixtures')

function seedSharedRoster(t) {
  mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
  cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))
}

describe('the saved roster preserves the v1 execution schema', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('uses agents.json inside the explicitly configured private home', () => {
    const path = rosterPath(t.env)
    assert.equal(path, join(t.root, 'consensflow', 'agents.json'))
  })

  it('reads v1 rows as agents: kind→harness, thinking/effort→effort', () => {
    seedSharedRoster(t)
    const byName = Object.fromEntries(listAgents(t.env).map((p) => [p.name, p]))

    assert.equal(byName.zeus.harness, 'claude')
    assert.equal(byName.zeus.effort, 'max')
    assert.equal(byName.endymion.harness, 'pi')
    assert.equal(byName.endymion.effort, 'xhigh')
    assert.equal(byName.mani.harness, 'opencode')
  })

  it('lists an image agent as a harness it runs, not as an oddity', () => {
    const pygmalion = listAgents(t.env).find((p) => p.name === 'pygmalion')
    assert.equal(pygmalion.harness, 'image')
    assert.equal(pygmalion.unsupported, undefined, 'cf run spawns it like any other')
  })
})

describe('writes are v1-faithful: cc and pi keep working on the same file', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  seedSharedRoster(t)

  it('edit updates mapped fields in place and preserves everything else', () => {
    editAgent('zeus', { model: 'claude-fable-5-1', effort: 'xhigh' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const zeus = raw.agents.find((p) => p.id === 'zeus')
    // v1 keys the runner reads:
    assert.equal(zeus.model, 'claude-fable-5-1')
    assert.equal(zeus.effort, 'xhigh')
    assert.equal(zeus.kind, 'claude-code')
    // v1 keys v3 does not understand must survive untouched:
    assert.equal(zeus.skillsPolicy, 'default')
    assert.equal(zeus.preset, 'zeus')
    assert.equal(zeus.name, 'Zeus')
    assert.equal(raw.schemaVersion, 1)
  })

  it('a pi agent edit lands in `thinking`, the key the pi runner reads', () => {
    editAgent('endymion', { effort: 'high' }, t.env)
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const endymion = raw.agents.find((p) => p.id === 'endymion')
    assert.equal(endymion.thinking, 'high')
    assert.equal(endymion.effort, undefined)
  })

  it('add writes a complete v1-shaped row', () => {
    addAgent({ name: 'freya', harness: 'codex', model: 'gpt-5.6-terra', effort: 'xhigh' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const freya = raw.agents.find((p) => p.id === 'freya')
    assert.equal(freya.kind, 'codex')
    assert.equal(freya.name, 'Freya')
    assert.equal(freya.effort, 'xhigh')
    assert.ok(freya.createdAt)
  })

  it('remove deletes exactly that row, any kind included', () => {
    removeAgent('freya', t.env)
    removeAgent('pygmalion', t.env)
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.equal(
      raw.agents.some((p) => p.id === 'freya'),
      false,
    )
    assert.equal(
      raw.agents.some((p) => p.id === 'pygmalion'),
      false,
    )
  })

  it('refuses an effort edit on an image agent, which has none, plainly', () => {
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    raw.agents.push({ id: 'img', name: 'Img', kind: 'image', model: 'gpt-5.5' })
    writeFileSync(rosterPath(t.env), JSON.stringify(raw, null, 2))

    assert.throws(() => editAgent('img', { effort: 'high' }, t.env), /no effort level/)
    editAgent('img', { description: 'still editable' }, t.env)
    removeAgent('img', t.env)
  })
})

describe('an absent shared roster is simply empty, and add creates it', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('starts empty and creates the v1 file shape on first add', () => {
    assert.deepEqual(listAgents(t.env), [])
    addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.equal(raw.schemaVersion, 1)
    assert.equal(raw.agents[0].id, 'zeus')
  })

  it('validates adds: bad names, unknown harnesss, empty models, duplicates', () => {
    assert.throws(() => addAgent({ name: 'Bad Name', harness: 'claude', model: 'm' }, t.env))
    assert.throws(() => addAgent({ name: 'ok', harness: 'not-a-cli', model: 'm' }, t.env))
    assert.throws(() => addAgent({ name: 'ok', harness: 'claude', model: '' }, t.env))
    assert.throws(() => addAgent({ name: 'zeus', harness: 'codex', model: 'm' }, t.env))
  })

  it('names the missing agent on edit and remove', () => {
    assert.throws(() => editAgent('nobody', { model: 'm' }, t.env), /nobody/)
    assert.throws(() => removeAgent('nobody', t.env), /nobody/)
  })
})

it('Kimi effort changes are explicit, validated and persisted without altering inherited saved settings', () => {
  const t = tempEnv()
  try {
    addAgent(
      { name: 'ilmarinen', harness: 'kimi', model: 'moonshot-ai/kimi-k3', preset: 'ilmarinen' },
      t.env,
    )
    assert.equal(listAgents(t.env)[0].effort, undefined)
    assert.ok(agentDrift(t.env)[0].changes.some((c) => c.field === 'effort' && c.to === 'max'))
    syncAgents(t.env, { name: 'ilmarinen' })
    assert.equal(listAgents(t.env)[0].effort, 'max')
    editAgent('ilmarinen', { effort: 'low' }, t.env)
    assert.equal(listAgents(t.env)[0].effort, 'low')
    const before = readFileSync(rosterPath(t.env), 'utf8')
    for (const effort of ['medium', 'xhigh', 'ultra', 'off', 'on', 0, false]) {
      assert.throws(() => editAgent('ilmarinen', { effort }, t.env), /low.*high.*max/)
      assert.throws(
        () =>
          addAgent(
            { name: 'invalid', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort },
            t.env,
          ),
        /low.*high.*max/,
      )
      assert.equal(readFileSync(rosterPath(t.env), 'utf8'), before)
    }
    editAgent('ilmarinen', { effort: '' }, t.env)
    assert.equal(listAgents(t.env)[0].effort, undefined, 'blank restores native settings')
  } finally {
    t.cleanup()
  }
})

describe('a catalog agent can be told its model moved', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  /** Rewrites a row in place, standing in for a catalog that has moved on. */
  function pin(name, model, env) {
    const path = rosterPath(env)
    const document = JSON.parse(readFileSync(path, 'utf8'))
    const row = document.agents.find((r) => r.id === name)
    row.model = model
    row.description = 'my own words'
    writeFileSync(path, `${JSON.stringify(document, null, 2)}\n`)
  }

  it('records which catalog entry it came from, and only then', () => {
    addAgent(
      { name: 'diana', harness: 'codex', model: 'gpt-5.6-luna', effort: 'xhigh', preset: 'diana' },
      t.env,
    )
    addAgent({ name: 'mine', harness: 'codex', model: 'gpt-5.6-sol' }, t.env)

    const byName = Object.fromEntries(listAgents(t.env).map((p) => [p.name, p]))
    assert.equal(byName.diana.preset, 'diana', 'a catalog add carries its provenance')
    assert.equal(byName.mine.preset, undefined, 'a hand-made agent is nobody else’s to move')
  })

  it('reports what the catalog would change, and nothing for pinned rows', () => {
    pin('diana', 'gpt-5.5', t.env)
    pin('mine', 'gpt-5.4', t.env)

    const drift = agentDrift(t.env)
    assert.equal(drift.length, 1, 'only the catalog-backed row drifts')
    assert.equal(drift[0].name, 'diana')
    assert.deepEqual(drift[0].changes, [
      { field: 'model', from: 'gpt-5.5', to: 'gpt-5.6-luna' },
      { field: 'description', from: 'my own words', to: 'Codex GPT 5.6 Luna XHIGH' },
    ])
  })

  it('a dry run says what would happen and writes nothing', () => {
    const applied = syncAgents(t.env, { dryRun: true })
    assert.equal(applied.length, 1)
    assert.equal(
      listAgents(t.env).find((p) => p.name === 'diana').model,
      'gpt-5.5',
      'the roster is untouched',
    )
  })

  it('syncs every field the preset owns, the label included', () => {
    const applied = syncAgents(t.env, {})
    assert.equal(applied.length, 1)

    const byName = Object.fromEntries(listAgents(t.env).map((p) => [p.name, p]))
    assert.equal(byName.diana.model, 'gpt-5.6-luna', 'the model caught up')
    assert.equal(
      byName.diana.description,
      'Codex GPT 5.6 Luna XHIGH',
      'the label follows the catalog too: a name for a model it no longer runs is what the skill table would print',
    )
    assert.equal(byName.mine.model, 'gpt-5.4', 'a pinned agent stays pinned')
    assert.equal(agentDrift(t.env).length, 0, 'nothing left to do')
  })

  it('a name the catalog has dropped stays where it is', () => {
    addAgent({ name: 'ghost', harness: 'codex', model: 'gpt-5.4', preset: 'no-such-preset' }, t.env)
    assert.equal(agentDrift(t.env).length, 0)
    assert.equal(syncAgents(t.env, {}).length, 0)
    assert.equal(listAgents(t.env).find((p) => p.name === 'ghost').model, 'gpt-5.4')
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
    assert.ok(listed.length > 0, 'the old file is read, not ignored')
    assert.ok(listed.some((a) => a.name === 'zeus'))

    // The first write moves the roster to its new name, rows intact.
    addAgent({ name: 'newcomer', harness: 'codex', model: 'gpt-5.6-luna' }, t.env)
    const written = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.ok(Array.isArray(written.agents), 'written under the agents key')
    assert.equal(written.participants, undefined, 'the old key does not survive the write')
    assert.equal(written.agents.length, listed.length + 1, 'nothing was dropped on the way')
    assert.ok(written.agents.some((row) => row.id === 'zeus'))
  })
})

describe('CONSENSFLOW_HOME means one root, to both halves', () => {
  it('puts the roster where the payload looks for it', async () => {
    // The manager read this variable as its state root and the payload read it
    // as the roster root, so setting it split the machine in two: `cf agent
    // list` showed your agents while the session hook said "none configured".
    const t = tempEnv()
    try {
      addAgent({ name: 'diana', harness: 'codex', model: 'gpt-5.6-luna' }, t.env)

      const { agentsPath } = await import('../hosts/lib/state.js')
      const payloadEnv = process.env.CONSENSFLOW_HOME
      process.env.CONSENSFLOW_HOME = t.env.CONSENSFLOW_HOME
      try {
        assert.equal(
          rosterPath(t.env),
          agentsPath(t.root),
          'both halves resolve the roster to the same file',
        )
      } finally {
        if (payloadEnv === undefined) delete process.env.CONSENSFLOW_HOME
        else process.env.CONSENSFLOW_HOME = payloadEnv
      }
    } finally {
      t.cleanup()
    }
  })

  it('puts everything in one directory when it is not set', () => {
    // One answer to "where is ConsensFlow on this machine", and one place for
    // an uninstall to sweep.
    const bare = { HOME: '/home/someone' }
    assert.equal(rosterPath(bare), '/home/someone/.consensflow/agents.json')
    assert.equal(configRoot(bare), '/home/someone/.consensflow')
  })

  it("imports an older machine's state without writing outside the private home", () => {
    const t = tempEnv()
    try {
      // A machine from before the merge: state under XDG, roster beside it.
      const legacy = legacyConfigRoot(t.env)
      mkdirSync(legacy, { recursive: true })
      writeFileSync(join(legacy, 'mode.json'), JSON.stringify({ mode: 'claude' }))
      writeFileSync(join(legacy, 'hosts.json'), JSON.stringify({ hosts: {} }))

      const moved = migrateStateRoot(t.env)
      assert.ok(moved, 'it reports what it did')
      assert.ok(existsSync(join(configRoot(t.env), 'mode.json')), 'the mode came along')
      assert.ok(existsSync(join(configRoot(t.env), 'hosts.json')))
      assert.equal(existsSync(legacy), true, 'the old root is never changed')
      assert.equal(readFileSync(join(legacy, 'hosts.json'), 'utf8'), JSON.stringify({ hosts: {} }))

      // Running again is a no-op, not a second copy.
      assert.equal(migrateStateRoot(t.env), null)
    } finally {
      t.cleanup()
    }
  })

  it('never overwrites a machine that already has the new root', () => {
    const t = tempEnv()
    try {
      mkdirSync(configRoot(t.env), { recursive: true })
      writeFileSync(join(configRoot(t.env), 'mode.json'), JSON.stringify({ mode: 'cmux' }))
      const legacy = legacyConfigRoot(t.env)
      mkdirSync(legacy, { recursive: true })
      writeFileSync(join(legacy, 'mode.json'), JSON.stringify({ mode: 'claude' }))

      assert.equal(migrateStateRoot(t.env), null, 'nothing is merged behind the user')
      const kept = JSON.parse(readFileSync(join(configRoot(t.env), 'mode.json'), 'utf8'))
      assert.equal(kept.mode, 'cmux', 'the live state wins')
    } finally {
      t.cleanup()
    }
  })
})

it('Pi Claude provider updates are explicit and leave custom rows pinned', () => {
  const t = tempEnv()
  try {
    addAgent(
      {
        name: 'erato',
        harness: 'pi',
        model: 'anthropic/claude-fable-5',
        effort: 'medium',
        preset: 'erato',
      },
      t.env,
    )
    addAgent(
      {
        name: 'my-claude',
        harness: 'pi',
        model: 'anthropic/claude-opus-5',
        effort: 'medium',
        description: 'personal',
      },
      t.env,
    )
    const before = readFileSync(rosterPath(t.env), 'utf8')
    assert.equal(listAgents(t.env)[0].model, 'anthropic/claude-fable-5')
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), before)
    assert.ok(
      agentDrift(t.env)
        .find((a) => a.name === 'erato')
        .changes.some((c) => c.to === 'openrouter/anthropic/claude-fable-5.1'),
    )
    syncAgents(t.env, { name: 'erato' })
    const rows = JSON.parse(readFileSync(rosterPath(t.env), 'utf8')).agents
    assert.equal(rows[0].model, 'openrouter/anthropic/claude-fable-5.1')
    assert.equal(rows[0].thinking, 'medium')
    assert.equal(rows[0].effort, undefined)
    assert.equal(rows[1].model, 'anthropic/claude-opus-5')
    assert.equal(rows[1].description, 'personal')
  } finally {
    t.cleanup()
  }
})

it('stores the full UI profile on add, edit and explicit sync', async () => {
  const { agentProfile } = await import('../src/catalog.js')
  const t = tempEnv()
  const raw = () => JSON.parse(readFileSync(rosterPath(t.env), 'utf8')).agents[0]
  try {
    addAgent(
      {
        name: 'custom',
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: 'medium',
        preset: 'maia',
        description: 'Keep my notes',
      },
      t.env,
    )
    assert.deepEqual(raw().profile, agentProfile(listAgents(t.env)[0]))
    assert.deepEqual(listAgents(t.env)[0].profile, raw().profile)
    editAgent('custom', { effort: 'low' }, t.env)
    assert.deepEqual(raw().profile.categories, ['coding'])
    assert.equal(raw().description, 'Keep my notes')
    syncAgents(t.env, { name: 'custom' })
    assert.deepEqual(raw().profile.categories, ['coding', 'reviewer'])
  } finally {
    t.cleanup()
  }
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
