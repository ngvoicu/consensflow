import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { staleClaudeHooks } from '../src/host-payloads.js'
import { tempEnv } from './helpers.mjs'

describe("Claude Code's settings are reported, never written", () => {
  const t = tempEnv()
  after(() => t.cleanup())

  const settings = () => join(t.env.CLAUDE_CONFIG_DIR, 'settings.json')

  function seed(value) {
    mkdirSync(t.env.CLAUDE_CONFIG_DIR, { recursive: true })
    writeFileSync(settings(), `${JSON.stringify(value, null, 2)}\n`)
  }

  it('names the events still holding a hook of ours', () => {
    seed({
      model: 'opus',
      hooks: {
        SessionStart: [{ hooks: [{ type: 'command', command: 'node /x/consensflow/hook.mjs' }] }],
        Stop: [{ hooks: [{ type: 'command', command: 'echo hello' }] }],
      },
    })

    const stale = staleClaudeHooks(t.env)

    assert.deepEqual(stale.events, ['SessionStart'], 'ours is named, theirs is not')
    assert.equal(stale.path, settings())
  })

  it('leaves the file untouched, byte for byte', () => {
    const before = readFileSync(settings(), 'utf8')

    staleClaudeHooks(t.env)

    assert.equal(readFileSync(settings(), 'utf8'), before, 'not ours to write')
  })

  it('reports nothing for settings with no hooks, and never creates the file', () => {
    const t2 = tempEnv()
    try {
      assert.deepEqual(staleClaudeHooks(t2.env).events, [])
      assert.equal(existsSync(join(t2.env.CLAUDE_CONFIG_DIR, 'settings.json')), false)
    } finally {
      t2.cleanup()
    }
  })
})
