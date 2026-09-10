import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { makeStage } from '../evals/harness.mjs'
import { addAgent, rosterPath } from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

test('behavioral evaluation copies its roster into a private app root and preserves native profiles', () => {
  const t = tempEnv()
  addAgent({ name: 'zeus', harness: 'codex', model: 'example' }, t.env)
  const before = readFileSync(rosterPath(t.env), 'utf8')
  const stage = makeStage({}, t.env)
  try {
    assert.notEqual(stage.env.CONSENSFLOW_HOME, t.env.CONSENSFLOW_HOME)
    assert.equal(stage.env.HOME, t.env.HOME)
    assert.equal(readFileSync(rosterPath(stage.env), 'utf8'), before)
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), before)
  } finally {
    stage.cleanup()
    t.cleanup()
  }
})
