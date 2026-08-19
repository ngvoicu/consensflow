import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { detectAgents } from '../src/agents.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import { tempEnv } from './helpers.mjs'

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('agents are detected by their CLI on PATH, dirs from their own env', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('finds only the agents whose CLI resolves', () => {
    stubCli(t.env, 'claude')
    stubCli(t.env, 'codex')

    const agents = detectAgents(t.env)
    assert.deepEqual(agents.map((a) => a.id).sort(), ['claude', 'codex'])
  })

  it('honours CLAUDE_CONFIG_DIR and CODEX_HOME for the skills dirs', () => {
    const byId = Object.fromEntries(detectAgents(t.env).map((a) => [a.id, a]))
    assert.equal(byId.claude.skillsDir, join(t.env.CLAUDE_CONFIG_DIR, 'skills'))
    assert.equal(byId.codex.skillsDir, join(t.env.CODEX_HOME, 'skills'))
  })

  it('places opencode under XDG config and pi under the home', () => {
    stubCli(t.env, 'opencode')
    stubCli(t.env, 'pi')
    const byId = Object.fromEntries(detectAgents(t.env).map((a) => [a.id, a]))
    assert.equal(byId.opencode.skillsDir, join(t.env.XDG_CONFIG_HOME, 'opencode', 'skills'))
    assert.equal(byId.pi.skillsDir, join(t.env.HOME, '.pi', 'agent', 'skills'))
  })
})

describe('install writes owned files with a hash manifest; drift is sacred', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t.env, 'claude')
  stubCli(t.env, 'codex')

  it('installs into every detected agent and records ownership', () => {
    const report = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'SKILL V1\n', source: 'consensflow' },
      t.env,
    )

    assert.equal(report.filter((r) => r.action === 'installed').length, 2)
    const target = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    assert.equal(readFileSync(target, 'utf8'), 'SKILL V1\n')

    const status = skillsStatus(t.env)
    assert.equal(status.length, 2)
    for (const row of status) assert.equal(row.state, 'ok')
  })

  it('rewrites its own unchanged files on update without drama', () => {
    const report = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'SKILL V2\n', source: 'consensflow' },
      t.env,
    )
    assert.equal(report.filter((r) => r.action === 'updated').length, 2)
  })

  it('refuses to clobber a file the user edited, unless forced', () => {
    const target = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    writeFileSync(target, 'USER EDITED\n')

    const refused = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'SKILL V3\n', source: 'consensflow' },
      t.env,
    )
    assert.equal(refused.find((r) => r.path === target).action, 'refused-drifted')
    assert.equal(readFileSync(target, 'utf8'), 'USER EDITED\n')

    const forced = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'SKILL V3\n', source: 'consensflow' },
      t.env,
      { force: true },
    )
    assert.equal(forced.find((r) => r.path === target).action, 'updated')
    assert.equal(readFileSync(target, 'utf8'), 'SKILL V3\n')
  })

  it('never adopts a pre-existing unowned file silently', () => {
    const foreign = join(t.env.CODEX_HOME, 'skills', 'other', 'SKILL.md')
    mkdirSync(join(t.env.CODEX_HOME, 'skills', 'other'), { recursive: true })
    writeFileSync(foreign, 'SOMEONE ELSE\n')

    const report = installSkill(
      { relPath: 'other/SKILL.md', content: 'MINE\n', source: 'consensflow' },
      t.env,
    )
    assert.equal(report.find((r) => r.path === foreign).action, 'refused-unowned')
    assert.equal(readFileSync(foreign, 'utf8'), 'SOMEONE ELSE\n')
  })

  it('uninstall removes exactly what the manifest owns and nothing else', () => {
    const foreign = join(t.env.CODEX_HOME, 'skills', 'other', 'SKILL.md')
    const report = uninstallSkills(t.env)

    assert.ok(report.filter((r) => r.action === 'removed').length >= 2)
    assert.ok(existsSync(foreign), 'foreign file must survive')
    assert.equal(skillsStatus(t.env).length, 0)
  })

  it('uninstall leaves no empty directory shells behind, even for nested skills', () => {
    installSkill({ relPath: 'nested/SKILL.md', content: 'top\n', source: 'consensflow' }, t.env, {
      force: true,
    })
    installSkill(
      { relPath: 'nested/references/deep.md', content: 'deep\n', source: 'consensflow' },
      t.env,
      { force: true },
    )

    uninstallSkills(t.env, { force: true })

    // The skill's own directory must vanish with its files; the skills root stays.
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'nested')), false)
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills')), true)
  })
})
