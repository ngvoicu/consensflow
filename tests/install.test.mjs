import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { detectHarnesses } from '../src/harnesses.js'
import { installSkill, skillsStatus, skillsSummary, uninstallSkills } from '../src/install.js'
import { addAgent } from '../src/roster.js'
import {
  healOnOpen,
  refreshInstalledSkill,
  retireSkillFromNativeHosts,
  skillGaps,
  skillTargets,
  staleSkills,
} from '../src/sync.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

function stubCli(env, name) {
  mkdirSync(env.PATH, { recursive: true })
  const path = join(env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('harnesses are detected by their CLI on PATH, dirs from their own env', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
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

  it('places opencode under XDG config and pi under the home', () => {
    stubCli(t.env, 'opencode')
    stubCli(t.env, 'pi')
    const byId = Object.fromEntries(detectHarnesses(t.env).map((a) => [a.id, a]))
    assert.equal(byId.opencode.skillsDir, join(t.env.XDG_CONFIG_HOME, 'opencode', 'skills'))
    assert.equal(byId.pi.skillsDir, join(t.env.HOME, '.pi', 'harness', 'skills'))
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
    for (const harness of detectHarnesses(t.env)) {
      assert.equal(harness.native, false, `${harness.id} should look bare`)
    }
  })

  it('spots the consensflow-cc plugin and the consensflow-pi extension', () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })
    mkdirSync(
      join(t.env.HOME, '.pi', 'harness', 'git', 'github.com', 'ngvoicu', 'consensflow-pi'),
      {
        recursive: true,
      },
    )

    const byId = Object.fromEntries(detectHarnesses(t.env).map((a) => [a.id, a]))
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

    const byHarness = Object.fromEntries(report.map((r) => [r.harness, r.action]))
    assert.equal(byHarness.codex, 'installed')
    assert.equal(byHarness.claude, undefined, 'claude has the plugin')
    assert.equal(byHarness.pi, undefined, 'pi has the extension')
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

  it('still installs the cmux skills on every harness — those never collide', () => {
    const report = installSkill(
      { relPath: 'cmux/SKILL.md', content: 'PANES\n', source: 'cmux@abc' },
      t.env,
      { force: true },
    )
    assert.equal(report.length, 3)
  })
})

describe('install writes owned files with a hash manifest; ours to replace, a stranger file is not', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t.env, 'claude')
  stubCli(t.env, 'codex')

  it('installs into every detected harness and records ownership', () => {
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

  it('replaces a copy the user edited, and calls it replaced', () => {
    // The skill is generated from the roster, not a document the user keeps:
    // an edited copy makes every agent reading it answer for a roster that has
    // moved. So it is rewritten — and reported as `replaced`, because losing an
    // edit in silence is the only part of this that would be wrong.
    const target = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    writeFileSync(target, 'USER EDITED\n')

    const report = installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'SKILL V3\n', source: 'consensflow' },
      t.env,
    )

    assert.equal(report.find((r) => r.path === target).action, 'replaced')
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

describe('a harness in scope with no skill of ours is named, not left silent', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('reports the harness whose install was refused', () => {
    stubCli(t.env, 'claude')
    stubCli(t.env, 'codex')
    chooseCmuxMode(t)
    addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)

    // Someone else's file at the path we would write: installSkill refuses it
    // and records nothing — correct, but it means codex silently consults
    // nothing while the install report scrolls past looking healthy.
    const theirs = join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')
    mkdirSync(dirname(theirs), { recursive: true })
    writeFileSync(theirs, 'a skill someone else put here\n')

    installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'ours\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env) },
    )

    assert.deepEqual(skillGaps(t.env), ['codex'], 'the refused harness is named')
  })

  it('names nobody once every harness in scope carries it', () => {
    installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'ours\n', source: 'consensflow' },
      t.env,
      { targets: skillTargets(t.env), force: true },
    )

    assert.deepEqual(skillGaps(t.env), [])
  })

  it('names a file that carries an older version of the skill', () => {
    // What an app upgrade does: the template moves, the installed files do not.
    // Every other count still calls them healthy — they are ours and unedited —
    // so this is the only thing that can say the lead is reading last version's
    // prose (live 2026-08-27: five harnesses, 738 characters behind, all `ok`).
    const paths = skillTargets(t.env).map((h) => join(h.skillsDir, 'consensflow', 'SKILL.md'))
    installSkill(
      {
        relPath: 'consensflow/SKILL.md',
        content: 'an older ConsensFlow wrote this\n',
        source: 'consensflow',
      },
      t.env,
      { targets: skillTargets(t.env), force: true },
    )

    assert.deepEqual(staleSkills(t.env).sort(), paths.sort(), 'every installed copy is behind')

    // Regenerated: nothing is behind any more.
    refreshInstalledSkill(t.env)
    assert.deepEqual(staleSkills(t.env), [])

    // And an edit of the user's is not staleness: drift is theirs, and stays.
    writeFileSync(paths[0], 'my own notes\n')
    assert.deepEqual(
      staleSkills(t.env),
      [],
      'a file the user edited is not out of date, it is theirs',
    )
  })

  it('names nobody when there is no roster to generate from', () => {
    const fresh = tempEnv()
    try {
      stubCli(fresh.env, 'claude')
      chooseCmuxMode(fresh)
      assert.deepEqual(skillGaps(fresh.env), [], 'no agents, nothing owed')
    } finally {
      fresh.cleanup()
    }
  })
})

describe('an install that changes nothing says so, and writes nothing', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const skill = {
    relPath: 'consensflow/SKILL.md',
    content: 'the same bytes\n',
    source: 'consensflow',
  }

  it('installs the first time', () => {
    stubCli(t.env, 'claude')
    const report = installSkill(skill, t.env, { targets: detectHarnesses(t.env) })

    assert.deepEqual(
      report.map((r) => r.action),
      ['installed'],
    )
  })

  it('reports unchanged, not updated, when the content is identical', () => {
    const report = installSkill(skill, t.env, { targets: detectHarnesses(t.env) })

    assert.deepEqual(
      report.map((r) => r.action),
      ['unchanged'],
    )
  })

  it('does not rewrite the file it left alone', () => {
    const path = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    const before = statSync(path).mtimeMs

    installSkill(skill, t.env, { targets: detectHarnesses(t.env) })

    assert.equal(statSync(path).mtimeMs, before, 'an identical install touches nothing')
  })

  it('still reports an update when the content really moved', () => {
    const report = installSkill({ ...skill, content: 'different bytes now\n' }, t.env, {
      targets: detectHarnesses(t.env),
    })

    assert.deepEqual(
      report.map((r) => r.action),
      ['updated'],
    )
    assert.equal(
      readFileSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'), 'utf8'),
      'different bytes now\n',
    )
  })
})

describe('the count answers the question a reader actually asks', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('separates skills from files, and ours from cmux', () => {
    stubCli(t.env, 'claude')
    stubCli(t.env, 'codex')
    installSkill(
      { relPath: 'consensflow/SKILL.md', content: 'ours\n', source: 'consensflow' },
      t.env,
      { targets: detectHarnesses(t.env) },
    )
    // A skill is a directory: this one is three files, like cmux-browser is
    // eleven. Counting files alone is what made 312 read as 312 skills.
    for (const rel of ['cmux-core/SKILL.md', 'cmux-core/reference.md', 'cmux-core/agents/x.yaml']) {
      installSkill({ relPath: rel, content: rel, source: 'cmux@abc123' }, t.env, {
        targets: detectHarnesses(t.env),
      })
    }

    const s = skillsSummary(t.env)

    assert.equal(s.files, 8, '4 files per harness, two harnesses')
    assert.equal(s.ours, 2, 'one generated skill each')
    assert.equal(s.cmux, 6)
    assert.equal(s.cmuxCommit, 'abc123')
    assert.equal(s.harnesses, 2)
    assert.equal(s.perHarness, 2, 'consensflow + cmux-core, not four')
  })

  it('reports zeroes on a machine with nothing installed', () => {
    const fresh = tempEnv()
    try {
      const s = skillsSummary(fresh.env)
      assert.equal(s.files, 0)
      assert.equal(s.skills, 0)
      assert.equal(s.cmuxCommit, null)
      assert.equal(s.perHarness, 0, 'no division by zero')
    } finally {
      fresh.cleanup()
    }
  })
})

describe('opening the app puts right what its own buttons would', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const launcher = () => join(t.env.CONSENSFLOW_BIN_DIR, 'consensflow')

  it('touches nothing before a mode is chosen', () => {
    stubCli(t.env, 'claude')

    const done = healOnOpen(t.env)

    assert.equal(done.mode, null)
    assert.equal(done.skills, 0)
    assert.equal(existsSync(launcher()), false, 'a machine that picked no path stays untouched')
  })

  it('installs the command the skill teaches, and says it did', () => {
    chooseCmuxMode(t)
    addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)

    const done = healOnOpen(t.env)

    assert.equal(done.command, 'claimed')
    assert.ok(existsSync(launcher()), 'the skill teaches `cf run`; now something answers it')
  })

  it('brings a skill left behind by an app upgrade up to date', () => {
    // The upgrade shape: the template moved, the installed file did not.
    installSkill(
      {
        relPath: 'consensflow/SKILL.md',
        content: 'an older ConsensFlow wrote this\n',
        source: 'consensflow',
      },
      t.env,
      { targets: skillTargets(t.env), force: true },
    )
    assert.equal(staleSkills(t.env).length, 1, 'behind, before the app was opened')

    const done = healOnOpen(t.env)

    assert.equal(done.skills, 1)
    assert.deepEqual(staleSkills(t.env), [], 'and current after')
  })

  it('rewrites a skill file that was edited — the roster owns what it says', () => {
    const path = join(skillTargets(t.env)[0].skillsDir, 'consensflow', 'SKILL.md')
    writeFileSync(path, 'my own notes, over the generated skill\n')

    const done = healOnOpen(t.env)

    assert.equal(done.skills, 1, 'an edited copy is one of the things opening the app puts right')
    assert.notEqual(readFileSync(path, 'utf8'), 'my own notes, over the generated skill\n')
  })

  it('claims the command even when it runs another ConsensFlow', () => {
    // A machine is meant to hold one install. The app you just opened is the
    // one that should answer `cf run`, so it takes the command rather than
    // reporting that something else holds it.
    const other = join(t.root, 'Other.app', 'node')
    mkdirSync(dirname(other), { recursive: true })
    writeFileSync(other, '')
    writeFileSync(
      launcher(),
      `#!/bin/sh\n# Installed by ConsensFlow.\nexec "${other}" "${join(t.root, 'Other.app', 'cf.mjs')}" "$@"\n`,
    )

    const done = healOnOpen(t.env)

    assert.equal(done.command, 'claimed')
    assert.ok(readFileSync(launcher(), 'utf8').includes(process.execPath), 'points here now')
  })
})
