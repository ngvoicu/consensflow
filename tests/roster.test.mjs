import assert from 'node:assert/strict'
import { cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  removeParticipant,
  rosterPath,
} from '../src/roster.js'
import { assertOutsideRealHome, tempEnv } from './helpers.mjs'

const FIXTURES = join(import.meta.dirname, 'fixtures')

function seedSharedRoster(t) {
  mkdirSync(join(t.env.HOME, '.consensflow'), { recursive: true })
  cpSync(join(FIXTURES, 'v1-participants.json'), rosterPath(t.env))
}

describe('the roster IS the shared v1 file that cc and pi read', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('lives at ~/.consensflow/participants.json under the given HOME', () => {
    const path = rosterPath(t.env)
    assertOutsideRealHome(path)
    assert.equal(path, join(t.env.HOME, '.consensflow', 'participants.json'))
  })

  it('reads v1 rows as participants: kind→runtime, thinking/effort→effort', () => {
    seedSharedRoster(t)
    const byName = Object.fromEntries(listParticipants(t.env).map((p) => [p.name, p]))

    assert.equal(byName.zeus.runtime, 'claude')
    assert.equal(byName.zeus.effort, 'max')
    assert.equal(byName.zeus.permission, 'workspace-write')
    assert.equal(byName.endymion.runtime, 'pi')
    assert.equal(byName.endymion.effort, 'xhigh')
    assert.equal(byName.mani.runtime, 'opencode')
  })

  it('lists an unsupported kind rather than hiding it, marked as such', () => {
    const pygmalion = listParticipants(t.env).find((p) => p.name === 'pygmalion')
    assert.equal(pygmalion.runtime, 'image')
    assert.equal(pygmalion.unsupported, true)
  })
})

describe('writes are v1-faithful: cc and pi keep working on the same file', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  seedSharedRoster(t)

  it('edit updates mapped fields in place and preserves everything else', () => {
    editParticipant('zeus', { model: 'claude-fable-5', effort: 'xhigh' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const zeus = raw.participants.find((p) => p.id === 'zeus')
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

  it('a pi participant edit lands in `thinking`, the key the pi runner reads', () => {
    editParticipant('endymion', { effort: 'high' }, t.env)
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const endymion = raw.participants.find((p) => p.id === 'endymion')
    assert.equal(endymion.thinking, 'high')
    assert.equal(endymion.effort, undefined)
  })

  it('add writes a complete v1-shaped row', () => {
    addParticipant(
      { name: 'freya', runtime: 'codex', model: 'gpt-5.6-terra', effort: 'xhigh' },
      t.env,
    )

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    const freya = raw.participants.find((p) => p.id === 'freya')
    assert.equal(freya.kind, 'codex')
    assert.equal(freya.name, 'Freya')
    assert.equal(freya.toolsPolicy, 'workspace-write')
    assert.equal(freya.effort, 'xhigh')
    assert.ok(freya.createdAt)
  })

  it('remove deletes exactly that row, any kind included', () => {
    removeParticipant('freya', t.env)
    removeParticipant('pygmalion', t.env)
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.equal(
      raw.participants.some((p) => p.id === 'freya'),
      false,
    )
    assert.equal(
      raw.participants.some((p) => p.id === 'pygmalion'),
      false,
    )
  })

  it('refuses effort or permission edits on an unsupported kind, plainly', () => {
    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    raw.participants.push({ id: 'img', name: 'Img', kind: 'image', model: 'gpt-5.5' })
    writeFileSync(rosterPath(t.env), JSON.stringify(raw, null, 2))

    assert.throws(() => editParticipant('img', { effort: 'high' }, t.env), /image/)
    editParticipant('img', { description: 'still editable' }, t.env)
    removeParticipant('img', t.env)
  })
})

describe('an absent shared roster is simply empty, and add creates it', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('starts empty and creates the v1 file shape on first add', () => {
    assert.deepEqual(listParticipants(t.env), [])
    addParticipant({ name: 'zeus', runtime: 'claude', model: 'claude-opus-5' }, t.env)

    const raw = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.equal(raw.schemaVersion, 1)
    assert.equal(raw.participants[0].id, 'zeus')
  })

  it('validates adds: bad names, unknown runtimes, empty models, duplicates', () => {
    assert.throws(() => addParticipant({ name: 'Bad Name', runtime: 'claude', model: 'm' }, t.env))
    assert.throws(() => addParticipant({ name: 'ok', runtime: 'not-a-cli', model: 'm' }, t.env))
    assert.throws(() => addParticipant({ name: 'ok', runtime: 'claude', model: '' }, t.env))
    assert.throws(() => addParticipant({ name: 'zeus', runtime: 'codex', model: 'm' }, t.env))
  })

  it('names the missing participant on edit and remove', () => {
    assert.throws(() => editParticipant('nobody', { model: 'm' }, t.env), /nobody/)
    assert.throws(() => removeParticipant('nobody', t.env), /nobody/)
  })
})
