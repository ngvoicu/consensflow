import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  addParticipant,
  configRoot,
  editParticipant,
  importV1,
  listParticipants,
  removeParticipant,
} from '../src/roster.js'
import { assertOutsideRealHome, tempEnv } from './helpers.mjs'

const FIXTURES = join(import.meta.dirname, 'fixtures')

describe('the roster lives under CONSENSFLOW_HOME and round-trips', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('resolves its root from the explicit env, never the real home', () => {
    const root = configRoot(t.env)
    assertOutsideRealHome(root)
    assert.equal(root, t.env.CONSENSFLOW_HOME)
  })

  it('starts empty, adds, edits, removes, and persists to disk', () => {
    assert.deepEqual(listParticipants(t.env), [])

    const added = addParticipant(
      { name: 'zeus', runtime: 'claude', model: 'claude-opus-5', effort: 'max' },
      t.env,
    )
    assert.equal(added.name, 'zeus')
    assert.equal(added.permission, 'workspace-write')

    const edited = editParticipant('zeus', { model: 'claude-fable-5' }, t.env)
    assert.equal(edited.model, 'claude-fable-5')

    // Persistence is a file, not process state.
    const onDisk = JSON.parse(readFileSync(join(configRoot(t.env), 'participants.json'), 'utf8'))
    assert.equal(onDisk.participants[0].model, 'claude-fable-5')

    removeParticipant('zeus', t.env)
    assert.deepEqual(listParticipants(t.env), [])
  })

  it('rejects bad names, unknown runtimes, empty models and duplicate names', () => {
    assert.throws(() => addParticipant({ name: 'Bad Name', runtime: 'claude', model: 'm' }, t.env))
    assert.throws(() => addParticipant({ name: 'ok', runtime: 'not-a-cli', model: 'm' }, t.env))
    assert.throws(() => addParticipant({ name: 'ok', runtime: 'claude', model: '' }, t.env))
    addParticipant({ name: 'once', runtime: 'claude', model: 'm' }, t.env)
    assert.throws(() => addParticipant({ name: 'once', runtime: 'codex', model: 'm' }, t.env))
    removeParticipant('once', t.env)
  })

  it('names the missing participant when editing or removing nothing', () => {
    assert.throws(() => editParticipant('nobody', { model: 'm' }, t.env), /nobody/)
    assert.throws(() => removeParticipant('nobody', t.env), /nobody/)
  })
})

describe('the v1 roster imports with its efforts intact', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('maps kinds, pulls pi thinking and preset efforts, and skips image kinds', () => {
    const outcome = importV1(
      {
        v1Path: join(FIXTURES, 'v1-participants.json'),
        presetsPath: join(FIXTURES, 'v1-presets.js'),
      },
      t.env,
    )

    assert.deepEqual(outcome.imported.map((p) => p.name).sort(), [
      'endymion',
      'hyperion',
      'mani',
      'zeus',
    ])
    assert.deepEqual(outcome.skipped, [{ name: 'pygmalion', reason: 'unsupported kind image' }])

    const byName = Object.fromEntries(listParticipants(t.env).map((p) => [p.name, p]))
    assert.equal(byName.zeus.runtime, 'claude')
    assert.equal(byName.zeus.effort, 'max')
    assert.equal(byName.hyperion.runtime, 'codex')
    assert.equal(byName.hyperion.effort, 'ultra')
    assert.equal(byName.endymion.runtime, 'pi')
    assert.equal(byName.endymion.effort, 'xhigh')
    assert.equal(byName.mani.runtime, 'opencode')
    assert.equal(byName.mani.effort, undefined)
  })

  it('is idempotent: importing again updates rather than duplicating', () => {
    const again = importV1({ v1Path: join(FIXTURES, 'v1-participants.json') }, t.env)
    assert.equal(again.imported.length, 4)
    assert.equal(listParticipants(t.env).length, 4)
  })

  it('reports a missing v1 file as an error, not an empty import', () => {
    assert.throws(() => importV1({ v1Path: join(FIXTURES, 'nope.json') }, t.env), /nope\.json/)
  })
})
