import assert from 'node:assert/strict'
import {
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
import { fakeExecutable, tempEnv } from './helpers.mjs'

/** A launcher is `cf` on POSIX and `cf.cmd` on Windows. */
const CMD = process.platform === 'win32' ? '.cmd' : ''

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  fakeExecutable(path)
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
    for (const name of ['claude', 'codex', 'opencode', 'pi', 'devin']) stubCli(t.env, name)
    const harnesses = detectHarnesses(t.env)
    assert.equal(harnesses.length, 5)
    for (const harness of harnesses)
      assert.deepEqual(Object.keys(harness).sort(), ['command', 'id'])
  })
})

describe('BO12: the path a pane is launched with is absolute, whatever PATH says', () => {
  it('resolves a relative PATH entry before handing it to the pane host', () => {
    // `pane.open` refuses a relative argv[0] outright
    // (`validate_open_request` in `crates/cf-panes/src/pane_handlers.rs`), and a
    // PATH carrying a relative entry is ordinary — `PATH=.:...` or a `bin` a
    // launcher exported from wherever it happened to be. Joining that with the
    // command name produces a relative candidate, and the pane never opens.
    const root = mkdtempSync(join(tmpdir(), 'cf-relpath-'))
    const previous = process.cwd()
    try {
      mkdirSync(join(root, 'bin'), { recursive: true })
      const shim = fakeExecutable(join(root, 'bin', 'claude'))
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
    addAgent({ name: 'mine', harness: 'claude', model: 'example' }, t.env)
    for (let i = 0; i < 2; i++) installation.prepareApp(t.env)
    assert.ok(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, `cf${CMD}`)))
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'integrations')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')), false)
    for (const file of globals) assert.equal(readFileSync(file, 'utf8'), 'global canary')
  } finally {
    t.cleanup()
  }
})

it('app preparation says why its launcher could not be installed, and prepares the integrations all the same', () => {
  const t = tempEnv()
  try {
    for (const name of ['pi', 'opencode']) stubCli(t.env, name)
    // A file where the launcher's folder goes: nothing can be written into it.
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'bin'), 'not a folder')
    const prepared = installation.prepareApp(t.env)
    assert.equal(prepared.report.length, 1)
    assert.match(prepared.report[0], /^The cf launcher could not be installed: \S/)
    assert.deepEqual(
      [prepared.piExtension.state, prepared.opencodeExtension.state],
      ['installed-unverified', 'installed-unverified'],
    )
    assert.equal(readFileSync(join(t.env.CONSENSFLOW_HOME, 'bin'), 'utf8'), 'not a folder')
  } finally {
    t.cleanup()
  }
})
