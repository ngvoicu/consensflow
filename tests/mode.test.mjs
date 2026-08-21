import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { applyMode, currentMode, MODES, modeLabel, modeReport, turnOff } from '../src/mode.js'
import { addAgent, removeAgent } from '../src/roster.js'
import { refreshInstalledSkill } from '../src/sync.js'
import { tempEnv } from './helpers.mjs'

function stubCli(t, name, script = '#!/bin/sh\nexit 0\n') {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, script)
  chmodSync(path, 0o755)
}

/** git that clones a two-file cmux skills tree. */
function stubGit(t, commit = 'cmux1234') {
  const fixture = join(t.root, 'cmux-repo')
  mkdirSync(join(fixture, 'skills', 'cmux-core'), { recursive: true })
  writeFileSync(join(fixture, 'skills', 'cmux-core', 'SKILL.md'), 'pane control\n')
  mkdirSync(t.env.PATH, { recursive: true })
  const git = join(t.env.PATH, 'git')
  writeFileSync(
    git,
    `#!/bin/sh
PATH=/usr/bin:/bin
for last do :; done
if [ "$1" = "clone" ]; then mkdir -p "$last"; cp -R "${fixture}/." "$last/"; exit 0; fi
case "$*" in *rev-parse*) echo "${commit}"; exit 0 ;; esac
exit 1
`,
  )
  chmodSync(git, 0o755)
}

/** How many cmux-sourced files the manifest owns right now. */
function cmuxFiles(t) {
  const manifest = join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')
  if (!existsSync(manifest)) return 0
  const recorded = JSON.parse(readFileSync(manifest, 'utf8')).files ?? {}
  return Object.values(recorded).filter((entry) => entry.source.startsWith('cmux@')).length
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
  addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)

  const generated = (dir) => join(dir, 'skills', 'consensflow', 'SKILL.md')

  it('offers exactly three modes and starts in none of them', () => {
    assert.deepEqual([...MODES].sort(), ['claude', 'cmux', 'pi'])
    assert.equal(currentMode(t.env), null)
  })

  it('labels the cmux mode with the harnesses it covers', () => {
    assert.equal(modeLabel('cmux'), 'cmux (pi, cc, codex, opencode)')
    assert.equal(modeLabel('claude'), 'claude')
  })

  it('still answers to the old name for it', () => {
    applyMode('standalone', t.env, { bundled })
    assert.equal(currentMode(t.env), 'cmux')
  })

  it('cmux mode puts the generated skill on every harness', () => {
    applyMode('cmux', t.env, { bundled })

    assert.equal(currentMode(t.env), 'cmux')
    for (const dir of [t.env.CLAUDE_CONFIG_DIR, t.env.CODEX_HOME]) {
      assert.ok(existsSync(generated(dir)), `${dir} has the generated skill`)
    }
  })

  it('switching to claude takes the generated skill away from everyone', () => {
    applyMode('claude', t.env, { bundled })

    assert.equal(currentMode(t.env), 'claude')
    // The Claude Code payload is in place …
    assert.ok(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'lib', 'runners.js')))
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')))
    // … and nothing else has a ConsensFlow path any more.
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)
    assert.equal(existsSync(generated(join(t.env.XDG_CONFIG_HOME, 'opencode'))), false)
  })

  it('says out loud which harnesses lose access in this mode', () => {
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

  it('going back to cmux mode removes the pi payload again', () => {
    applyMode('cmux', t.env, { bundled })

    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi')), false)
    assert.ok(existsSync(generated(t.env.CODEX_HOME)))
  })

  it('adding an agent in a host mode does not smuggle the skill back', () => {
    applyMode('claude', t.env, { bundled })
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)

    // The mutation path must obey the mode, or the invariant only holds
    // until the next roster edit.
    addAgent({ name: 'apollo', harness: 'codex', model: 'gpt-5.6-terra' }, t.env)
    refreshInstalledSkill(t.env)

    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false, 'codex stays out in claude mode')
    removeAgent('apollo', t.env)
    applyMode('cmux', t.env, { bundled })
  })

  it('installs nothing until a mode is chosen', () => {
    // Adding an agent before picking a path used to install the generated
    // skill into every harness found — so ConsensFlow appeared in Claude Code
    // without anyone choosing Claude Code, and the one-path invariant was
    // broken before the user ever saw the switch.
    const fresh = tempEnv()
    try {
      stubCli(fresh, 'claude')
      stubCli(fresh, 'codex')
      assert.equal(currentMode(fresh.env), null, 'no mode yet')

      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, fresh.env)
      refreshInstalledSkill(fresh.env)

      assert.equal(
        existsSync(join(fresh.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
        false,
        'nothing is installed before a path is chosen',
      )
      assert.equal(
        existsSync(join(fresh.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')),
        false,
      )

      // Choosing the path is what installs it.
      stubGit(fresh)
      applyMode('cmux', fresh.env, { bundled })
      assert.ok(existsSync(join(fresh.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
    } finally {
      fresh.cleanup()
    }
  })

  it('brings the cmux skills with it when cmux mode is chosen', () => {
    stubGit(t)
    applyMode('cmux', t.env, { bundled })

    assert.ok(
      existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')),
      'cmux skills install without being asked for',
    )
  })

  it('keeps cmux pane skills out of a host mode entirely', () => {
    stubGit(t)
    applyMode('cmux', t.env, { bundled })
    // cmux mode is the path named after cmux, so pane control comes with it.
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')))
    assert.ok(existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')))

    // Choosing Claude Code takes them back everywhere: consulting through the
    // host runs a subprocess and never touches a pane, so nobody needs them.
    applyMode('claude', t.env, { bundled })
    assert.equal(cmuxFiles(t), 0, 'a cc-only install should own no cmux files')
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')),
      false,
    )
    assert.equal(existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')), false)
  })

  it('does not reach for cmux at all in a host mode', () => {
    // git that fails loudly: a host mode must never clone, so the switch
    // succeeds without it. (Offline machines choose cc modes too.)
    mkdirSync(t.env.PATH, { recursive: true })
    const git = join(t.env.PATH, 'git')
    writeFileSync(git, '#!/bin/sh\necho "no network" >&2\nexit 1\n')
    chmodSync(git, 0o755)

    const applied = applyMode('claude', t.env, { bundled })
    assert.equal(applied.mode, 'claude')
    assert.equal(
      applied.report.some((line) => line.includes('cmux skills were not fetched')),
      false,
      'a host mode has no cmux skills to miss, so it must not report a failure',
    )
  })

  it('does not fail the switch when the cmux skills cannot be fetched', () => {
    const offline = tempEnv()
    try {
      stubCli(offline, 'codex')
      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, offline.env)

      // No git on PATH at all: the mode still applies, and says what it missed.
      const outcome = applyMode('cmux', offline.env, { bundled })

      assert.equal(currentMode(offline.env), 'cmux')
      assert.ok(existsSync(join(offline.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')))
      assert.match(outcome.report.join(' '), /cmux skills/i)
    } finally {
      offline.cleanup()
    }
  })

  it('turns everything off: no path, no skills, no payloads, no mode', () => {
    stubGit(t)
    applyMode('cmux', t.env, { bundled })
    assert.ok(existsSync(generated(t.env.CODEX_HOME)))

    const outcome = turnOff(t.env)

    assert.equal(currentMode(t.env), null)
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)
    assert.equal(existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')), false)
    assert.ok(outcome.changes.length > 0)

    // "Off" means off: no bookkeeping left claiming state that is gone.
    const config = t.env.CONSENSFLOW_HOME
    const leftovers = existsSync(config) ? readdirSync(config) : []
    assert.deepEqual(leftovers, [], `nothing should remain, found ${leftovers.join(', ')}`)
  })

  it('refuses a mode it does not have, naming the ones it does', () => {
    assert.throws(() => applyMode('emacs', t.env, { bundled }), /claude, pi, cmux/)
  })
})
