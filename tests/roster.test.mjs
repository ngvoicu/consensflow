import assert from 'node:assert/strict'
import { cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  addAgent,
  agentDrift,
  editAgent,
  listAgents,
  removeAgent,
  rosterPath,
  syncAgents,
} from '../src/roster.js'
import { assertOutsideRealHome, tempEnv } from './helpers.mjs'

const FIXTURES = join(import.meta.dirname, 'fixtures')

function seedSharedRoster(t) {
  mkdirSync(join(t.env.HOME, '.consensflow'), { recursive: true })
  cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))
}

describe('the roster IS the shared v1 file that cc and pi read', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('lives at ~/.consensflow/agents.json under the given HOME', () => {
    const path = rosterPath(t.env)
    assertOutsideRealHome(path)
    assert.equal(path, join(t.env.HOME, '.consensflow', 'agents.json'))
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
    editAgent('zeus', { model: 'claude-fable-5', effort: 'xhigh' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const zeus = raw.agents.find((p) => p.id === 'zeus')
    // v1 keys the runner reads:
    assert.equal(zeus.model, 'claude-fable-5')
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
    assert.deepEqual(drift[0].changes, [{ field: 'model', from: 'gpt-5.5', to: 'gpt-5.6-luna' }])
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

  it('syncs the fields the preset owns, and never the words you wrote', () => {
    const applied = syncAgents(t.env, {})
    assert.equal(applied.length, 1)

    const byName = Object.fromEntries(listAgents(t.env).map((p) => [p.name, p]))
    assert.equal(byName.diana.model, 'gpt-5.6-luna', 'the model caught up')
    assert.equal(byName.diana.description, 'my own words', 'the description is yours')
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
    const legacy = join(t.env.HOME, '.consensflow', 'participants.json')
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
