import assert from 'node:assert/strict'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, isAbsolute, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { detectHarnesses, harnessPath } from '../src/harnesses.js'
import * as installation from '../src/install.js'
import { addAgent } from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('harness executable discovery', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('finds only the harnesses whose CLI resolves', () => {
    stubCli(t.env, 'claude')
    stubCli(t.env, 'codex')

    const harnesses = detectHarnesses(t.env)
    assert.deepEqual(harnesses.map((a) => a.id).sort(), ['claude', 'codex'])
  })

  it('detection returns executable identities without unused global skill destinations', () => {
    for (const name of ['claude', 'codex', 'opencode', 'pi', 'kimi', 'devin']) stubCli(t.env, name)
    const harnesses = detectHarnesses(t.env)
    assert.equal(harnesses.length, 6)
    for (const harness of harnesses)
      assert.deepEqual(Object.keys(harness).sort(), ['command', 'id'])
  })
})

describe('BO12: the path a pane is launched with is absolute, whatever PATH says', () => {
  it('resolves a relative PATH entry before handing it to the pane host', () => {
    // `pane.open` refuses a relative argv[0] outright
    // (`app/src-tauri/src/commands.rs:1121`), and a PATH carrying a
    // relative entry is ordinary — `PATH=.:...` or a `bin` a launcher
    // exported from wherever it happened to be. Joining that with the
    // command name produces a relative candidate, and the pane never opens.
    const root = mkdtempSync(join(tmpdir(), 'cf-relpath-'))
    const previous = process.cwd()
    try {
      mkdirSync(join(root, 'bin'), { recursive: true })
      const shim = join(root, 'bin', 'claude')
      writeFileSync(shim, '#!/bin/sh\nexit 0\n')
      chmodSync(shim, 0o755)
      process.chdir(root)

      const found = harnessPath('claude', { PATH: 'bin', HOME: root })
      assert.notEqual(found, null, 'it is on PATH, relatively')
      assert.equal(isAbsolute(found), true, `relative argv[0]: ${found}`)
      assert.equal(realpathSync(found), realpathSync(shim))
    } finally {
      process.chdir(previous)
      rmSync(root, { recursive: true, force: true })
    }
  })
})

it('app preparation owns its launcher and integrations, not role documents or global skills', () => {
  const t = tempEnv()
  try {
    for (const name of ['claude', 'codex', 'pi', 'opencode']) stubCli(t.env, name)
    const globals = [
      t.env.CLAUDE_CONFIG_DIR,
      t.env.CODEX_HOME,
      join(t.env.XDG_CONFIG_HOME, 'opencode'),
      join(t.env.HOME, '.pi', 'agent'),
    ].map((root) => join(root, 'skills', 'consensflow', 'SKILL.md'))
    for (const file of globals) {
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(file, 'global canary')
    }
    addAgent({ name: 'zeus', harness: 'claude', model: 'example' }, t.env)
    for (let i = 0; i < 2; i++) installation.prepareApp(t.env)
    assert.ok(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, 'cf')))
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')), false)
    for (const file of globals) assert.equal(readFileSync(file, 'utf8'), 'global canary')
  } finally {
    t.cleanup()
  }
})
