import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { detectAgents } from '../src/agents.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import { retireSkillFromNativeHosts, skillTargets } from '../src/sync.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('agents are detected by their CLI on PATH, dirs from their own env', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
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

describe('a host with its own ConsensFlow integration keeps it', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t.env, 'claude')
  stubCli(t.env, 'codex')
  stubCli(t.env, 'pi')

  it('sees no native integration on a bare machine', () => {
    for (const agent of detectAgents(t.env)) {
      assert.equal(agent.native, false, `${agent.id} should look bare`)
    }
  })

  it('spots the consensflow-cc plugin and the consensflow-pi extension', () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })
    mkdirSync(join(t.env.HOME, '.pi', 'agent', 'git', 'github.com', 'ngvoicu', 'consensflow-pi'), {
      recursive: true,
    })

    const byId = Object.fromEntries(detectAgents(t.env).map((a) => [a.id, a]))
    assert.equal(byId.claude.native, true)
    assert.equal(byId.pi.native, true)
    assert.equal(byId.codex.native, false)
  })

  it('installs the generated skill only where nothing else provides one', () => {
    const report = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'GENERATED\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env) },
    )

    const byAgent = Object.fromEntries(report.map((r) => [r.agent, r.action]))
    assert.equal(byAgent.codex, 'installed')
    assert.equal(byAgent.claude, undefined, 'claude has the plugin')
    assert.equal(byAgent.pi, undefined, 'pi has the extension')
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
  })

  it('retires a copy it installed before the host gained its own', () => {
    // Installed everywhere first (the state an upgrade inherits).
    installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'GENERATED\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env, { all: true }) },
    )
    const claudeSkill = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    assert.ok(existsSync(claudeSkill))

    const report = retireSkillFromNativeHosts(t.env)

    assert.ok(report.some((r) => r.action === 'retired' && r.path === claudeSkill))
    assert.equal(existsSync(claudeSkill), false)
    // Only ours, only there: codex keeps its copy.
    assert.ok(existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('never retires a file the user edited', () => {
    installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'GENERATED\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env, { all: true }) },
    )
    const claudeSkill = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    writeFileSync(claudeSkill, 'MY EDITS\n')

    const report = retireSkillFromNativeHosts(t.env)

    assert.ok(report.some((r) => r.action === 'kept-drifted'))
    assert.equal(readFileSync(claudeSkill, 'utf8'), 'MY EDITS\n')
  })

  it('installs everywhere when the user insists', () => {
    const report = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'GENERATED\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env, { all: true }) },
    )

    assert.equal(report.length, 3)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('still installs the cmux skills on every agent — those never collide', () => {
    const report = installSkill(
      { relPath: 'cmux/SKILL.md', content: 'PANES\n', source: 'cmux@abc' },
      t.env,
      { force: true },
    )
    assert.equal(report.length, 3)
  })
})

describe('install writes owned files with a hash manifest; drift is sacred', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
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
