import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { applyMode, currentMode, MODES, modeReport } from '../src/mode.js'
import { addParticipant, removeParticipant } from '../src/roster.js'
import { refreshInstalledSkill } from '../src/sync.js'
import { tempEnv } from './helpers.mjs'

function stubCli(t, name, script = '#!/bin/sh\nexit 0\n') {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, script)
  chmodSync(path, 0o755)
}

function bundle(t) {
  const root = join(t.root, 'bundled')
  mkdirSync(join(root, 'lib'), { recursive: true })
  mkdirSync(join(root, 'claude', 'skills', 'consensflow'), { recursive: true })
  mkdirSync(join(root, 'claude', 'commands'), { recursive: true })
  mkdirSync(join(root, 'pi'), { recursive: true })
  writeFileSync(join(root, 'lib', 'runners.js'), '// engine\n')
  writeFileSync(join(root, 'claude', 'skills', 'consensflow', 'SKILL.md'), 'cc skill\n')
  writeFileSync(join(root, 'claude', 'commands', 'cf.md'), 'cc command\n')
  writeFileSync(join(root, 'claude', 'hooks.json'), JSON.stringify({ hooks: {} }))
  writeFileSync(join(root, 'pi', 'index.ts'), '// pi\n')
  return root
}

describe('the machine runs exactly one ConsensFlow path', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  for (const cli of ['claude', 'codex', 'opencode', 'pi']) stubCli(t, cli)
  const bundled = bundle(t)
  addParticipant({ name: 'zeus', runtime: 'claude', model: 'claude-opus-5' }, t.env)

  const generated = (dir) => join(dir, 'skills', 'consensflow', 'SKILL.md')

  it('offers exactly three modes and starts in none of them', () => {
    assert.deepEqual([...MODES].sort(), ['claude', 'pi', 'standalone'])
    assert.equal(currentMode(t.env), null)
  })

  it('standalone puts the generated skill on every agent', () => {
    applyMode('standalone', t.env, { bundled })

    assert.equal(currentMode(t.env), 'standalone')
    for (const dir of [t.env.CLAUDE_CONFIG_DIR, t.env.CODEX_HOME]) {
      assert.ok(existsSync(generated(dir)), `${dir} has the generated skill`)
    }
  })

  it('switching to claude takes the generated skill away from everyone', () => {
    applyMode('claude', t.env, { bundled })

    assert.equal(currentMode(t.env), 'claude')
    // The Claude Code payload is in place …
    assert.ok(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude', 'lib', 'runners.js')))
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')))
    // … and nothing else has a ConsensFlow path any more.
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)
    assert.equal(existsSync(generated(join(t.env.XDG_CONFIG_HOME, 'opencode'))), false)
  })

  it('says out loud which agents lose access in this mode', () => {
    const report = modeReport('claude', t.env)

    assert.match(report.join('\n'), /codex/)
    assert.match(report.join('\n'), /opencode/)
    assert.match(report.join('\n'), /no ConsensFlow|nothing/i)
  })

  it('switching to pi removes the Claude Code wiring it installed', () => {
    applyMode('pi', t.env, { bundled })

    assert.equal(currentMode(t.env), 'pi')
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')), false)
    assert.ok(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi', 'index.ts')))
  })

  it('going back to standalone removes the pi payload again', () => {
    applyMode('standalone', t.env, { bundled })

    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi')), false)
    assert.ok(existsSync(generated(t.env.CODEX_HOME)))
  })

  it('adding a participant in a host mode does not smuggle the skill back', () => {
    applyMode('claude', t.env, { bundled })
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)

    // The mutation path must obey the mode, or the invariant only holds
    // until the next roster edit.
    addParticipant({ name: 'apollo', runtime: 'codex', model: 'gpt-5.6-terra' }, t.env)
    refreshInstalledSkill(t.env)

    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false, 'codex stays out in claude mode')
    removeParticipant('apollo', t.env)
    applyMode('standalone', t.env, { bundled })
  })

  it('refuses a mode it does not have, naming the ones it does', () => {
    assert.throws(() => applyMode('emacs', t.env, { bundled }), /claude, pi, standalone/)
  })
})
