import assert from 'node:assert/strict'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, isAbsolute, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { detectHarnesses, harnessPath } from '../src/harnesses.js'
import * as installation from '../src/install.js'
import { skillsStatus, skillsSummary, uninstallSkills } from '../src/install.js'
import { addAgent } from '../src/roster.js'
import { healOnOpen, refreshInstalledSkill, skillGaps, staleSkills } from '../src/sync.js'
import { tempEnv } from './helpers.mjs'

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('harnesses are detected by their CLI on PATH, dirs from their own env', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('finds only the harnesses whose CLI resolves', () => {
    stubCli(t.env, 'claude')
    stubCli(t.env, 'codex')

    const harnesses = detectHarnesses(t.env)
    assert.deepEqual(harnesses.map((a) => a.id).sort(), ['claude', 'codex'])
  })

  it('honours CLAUDE_CONFIG_DIR and CODEX_HOME for the skills dirs', () => {
    const byId = Object.fromEntries(detectHarnesses(t.env).map((a) => [a.id, a]))
    assert.equal(byId.claude.skillsDir, join(t.env.CLAUDE_CONFIG_DIR, 'skills'))
    assert.equal(byId.codex.skillsDir, join(t.env.CODEX_HOME, 'skills'))
  })

  it('places opencode under XDG config and pi in its native agent directory', () => {
    stubCli(t.env, 'opencode')
    stubCli(t.env, 'pi')
    const byId = Object.fromEntries(detectHarnesses(t.env).map((a) => [a.id, a]))
    assert.equal(byId.opencode.skillsDir, join(t.env.XDG_CONFIG_HOME, 'opencode', 'skills'))
    assert.equal(byId.pi.skillsDir, join(t.env.HOME, '.pi', 'agent', 'skills'))
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

describe('private app skill lifecycle', () => {
  for (const force of [false, true])
    it(`preserves owned and unowned global skills with force=${force}`, () => {
      const t = tempEnv()
      try {
        for (const name of ['claude', 'codex', 'pi', 'opencode']) stubCli(t.env, name)
        const globals = detectHarnesses(t.env).map((harness) =>
          join(harness.skillsDir, 'consensflow', 'SKILL.md'),
        )
        for (const file of globals) {
          mkdirSync(dirname(file), { recursive: true })
          writeFileSync(file, 'global canary')
        }
        addAgent({ name: 'zeus', harness: 'claude', model: 'example' }, t.env)
        installation.installEverywhere(t.env, { force })
        refreshInstalledSkill(t.env)
        healOnOpen(t.env)
        const rows = skillsStatus(t.env)
        assert.equal(rows.length, 1)
        assert.ok(rows[0].path.startsWith(t.env.CONSENSFLOW_HOME))
        assert.match(readFileSync(rows[0].path, 'utf8'), /name: consensflow-lead/)
        assert.equal(skillGaps(t.env).length, 0)
        assert.equal(staleSkills(t.env).length, 0)
        uninstallSkills(t.env, { force })
        for (const file of globals) assert.equal(readFileSync(file, 'utf8'), 'global canary')
      } finally {
        t.cleanup()
      }
    })

  it('generates a useful lead skill before any workers are configured', () => {
    const t = tempEnv()
    try {
      healOnOpen(t.env)
      assert.ok(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, 'cf')))
      const rows = skillsStatus(t.env)
      assert.equal(rows.length, 1)
      assert.match(readFileSync(rows[0].path, 'utf8'), /cf agent list/)
      assert.equal(skillsSummary(t.env).skills, 1)
    } finally {
      t.cleanup()
    }
  })

  it('unchanged private generation preserves the file and reports unchanged', () => {
    const t = tempEnv()
    try {
      const first = refreshInstalledSkill(t.env)[0]
      const before = statSync(first.path).mtimeMs
      const next = refreshInstalledSkill(t.env)[0]
      assert.equal(next.action, 'unchanged')
      assert.equal(statSync(first.path).mtimeMs, before)
    } finally {
      t.cleanup()
    }
  })

  it('reset removes app data and its launcher but leaves global skill cleanup manual', () => {
    const t = tempEnv()
    try {
      const global = join(t.env.HOME, '.claude', 'skills', 'consensflow', 'SKILL.md')
      mkdirSync(dirname(global), { recursive: true })
      writeFileSync(global, 'keep for manual cleanup')
      healOnOpen(t.env)
      installation.resetEverything(t.env, { force: true })
      assert.equal(existsSync(t.env.CONSENSFLOW_HOME), false)
      assert.equal(existsSync(join(t.env.CONSENSFLOW_BIN_DIR, 'cf')), false)
      assert.equal(readFileSync(global, 'utf8'), 'keep for manual cleanup')
    } finally {
      t.cleanup()
    }
  })
})
