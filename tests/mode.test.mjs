import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { applyMode, currentMode, MODES, modeLabel, modeReport, turnOff } from '../src/mode.js'
import { addParticipant, removeParticipant } from '../src/roster.js'
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
    assert.deepEqual([...MODES].sort(), ['claude', 'cmux', 'pi'])
    assert.equal(currentMode(t.env), null)
  })

  it('labels the cmux mode with the agents it covers', () => {
    assert.equal(modeLabel('cmux'), 'cmux (pi, cc, codex, opencode)')
    assert.equal(modeLabel('claude'), 'claude')
  })

  it('still answers to the old name for it', () => {
    applyMode('standalone', t.env, { bundled })
    assert.equal(currentMode(t.env), 'cmux')
  })

  it('cmux mode puts the generated skill on every agent', () => {
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

  it('going back to cmux mode removes the pi payload again', () => {
    applyMode('cmux', t.env, { bundled })

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
    applyMode('cmux', t.env, { bundled })
  })

  it('brings the cmux skills with it, whichever mode is chosen', () => {
    stubGit(t)
    applyMode('cmux', t.env, { bundled })

    assert.ok(
      existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')),
      'cmux skills install without being asked for',
    )
  })

  it('does not fail the switch when the cmux skills cannot be fetched', () => {
    const offline = tempEnv()
    try {
      stubCli(offline, 'codex')
      addParticipant({ name: 'zeus', runtime: 'claude', model: 'claude-opus-5' }, offline.env)

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
  })

  it('refuses a mode it does not have, naming the ones it does', () => {
    assert.throws(() => applyMode('emacs', t.env, { bundled }), /claude, pi, cmux/)
  })
})
