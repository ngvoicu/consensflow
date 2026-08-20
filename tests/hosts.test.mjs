import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { hostStatus, installHost, uninstallHost } from '../src/hosts.js'
import { tempEnv } from './helpers.mjs'

/** git is shimmed: clone copies a fixture payload shaped like consensflow-cc. */
function stubGit(t, commit = 'cc123456') {
  const fixture = join(t.root, 'cc-fixture')
  mkdirSync(join(fixture, 'lib'), { recursive: true })
  mkdirSync(join(fixture, 'scripts'), { recursive: true })
  mkdirSync(join(fixture, 'skills', 'consensflow'), { recursive: true })
  mkdirSync(join(fixture, 'commands'), { recursive: true })
  writeFileSync(join(fixture, 'bin-cf.mjs'), '// engine\n')
  writeFileSync(join(fixture, 'lib', 'runners.js'), '// runners\n')
  writeFileSync(join(fixture, 'scripts', 'session-start-hook.mjs'), '// hook\n')
  writeFileSync(
    join(fixture, 'skills', 'consensflow', 'SKILL.md'),
    'run: node "${CLAUDE_PLUGIN_ROOT}/bin/cf.mjs" status\n',
  )
  writeFileSync(join(fixture, 'commands', 'cf.md'), 'node "${CLAUDE_PLUGIN_ROOT}/bin/cf.mjs" run\n')
  writeFileSync(
    join(fixture, 'hooks.json'),
    JSON.stringify({
      hooks: {
        SessionStart: [
          {
            hooks: [
              {
                type: 'command',
                command: 'node "${CLAUDE_PLUGIN_ROOT}/scripts/session-start-hook.mjs"',
                timeout: 15,
              },
            ],
          },
        ],
      },
    }),
  )

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

/** A pi that records what it was asked to install. */
function stubPi(t) {
  mkdirSync(t.env.PATH, { recursive: true })
  const log = join(t.root, 'pi-calls.log')
  const pi = join(t.env.PATH, 'pi')
  writeFileSync(pi, `#!/bin/sh\necho "$@" >> "${log}"\nexit 0\n`)
  chmodSync(pi, 0o755)
  return log
}

describe('the manager installs the Claude Code side through user config', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubGit(t)

  const skill = () => join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
  const command = () => join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')
  const settings = () => join(t.env.CLAUDE_CONFIG_DIR, 'settings.json')

  it('reports nothing installed before it runs', () => {
    assert.equal(hostStatus(t.env).find((h) => h.id === 'claude').installed, false)
  })

  it('installs the payload, the skill, the command and the hooks', () => {
    const outcome = installHost('claude', t.env)

    assert.equal(outcome.commit, 'cc123456')
    // The payload lands in our own directory, never inside Claude Code.
    const payload = join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude', 'lib', 'runners.js')
    assert.ok(existsSync(payload), 'payload copied')

    // Plugin-root references are rewritten to where the payload actually is.
    const skillText = readFileSync(skill(), 'utf8')
    assert.ok(!skillText.includes('CLAUDE_PLUGIN_ROOT'))
    assert.ok(skillText.includes(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')))
    assert.ok(existsSync(command()))

    const hooks = JSON.parse(readFileSync(settings(), 'utf8')).hooks
    assert.equal(hooks.SessionStart.length, 1)
    assert.ok(!JSON.stringify(hooks).includes('CLAUDE_PLUGIN_ROOT'))
  })

  it('leaves the rest of settings.json exactly as it found it', () => {
    const before = JSON.parse(readFileSync(settings(), 'utf8'))
    assert.equal(before.model, undefined)

    // A user setting written between installs must survive the next one.
    writeFileSync(
      settings(),
      JSON.stringify({ ...before, model: 'opus', voiceEnabled: true }, null, 2),
    )
    installHost('claude', t.env)

    const after = JSON.parse(readFileSync(settings(), 'utf8'))
    assert.equal(after.model, 'opus')
    assert.equal(after.voiceEnabled, true)
    assert.equal(after.hooks.SessionStart.length, 1, 'hooks are replaced, never duplicated')
  })

  it('says it is installed, and at which commit', () => {
    const claude = hostStatus(t.env).find((h) => h.id === 'claude')
    assert.equal(claude.installed, true)
    assert.equal(claude.commit, 'cc123456')
  })

  it('uninstalls exactly what it wrote, keeping the user settings', () => {
    uninstallHost('claude', t.env)

    assert.equal(existsSync(skill()), false)
    assert.equal(existsSync(command()), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')), false)
    const after = JSON.parse(readFileSync(settings(), 'utf8'))
    assert.equal(after.model, 'opus', 'user settings survive')
    assert.equal(after.hooks?.SessionStart ?? [], 0 === 0 ? (after.hooks?.SessionStart ?? []) : [])
    assert.equal((after.hooks?.SessionStart ?? []).length, 0)
    assert.equal(hostStatus(t.env).find((h) => h.id === 'claude').installed, false)
  })
})

describe('a host installed some other way is reported, not denied', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('spots a Claude Code plugin install it did not perform', () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })

    const claude = hostStatus(t.env).find((h) => h.id === 'claude')
    assert.equal(claude.installed, false, 'not ours')
    assert.equal(claude.present, true)
    assert.match(claude.via, /plugin/i)
  })

  it('spots a pi extension it did not install, by asking pi', () => {
    mkdirSync(t.env.PATH, { recursive: true })
    const pi = join(t.env.PATH, 'pi')
    writeFileSync(pi, '#!/bin/sh\necho "  https://github.com/ngvoicu/consensflow-pi"\nexit 0\n')
    chmodSync(pi, 0o755)

    const status = hostStatus(t.env).find((h) => h.id === 'pi')
    assert.equal(status.present, true)
    assert.match(status.via, /pi/i)
  })
})

describe('the manager drives pi through its own supported CLI', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const log = stubPi(t)

  it('installs the extension with pi install, not by hand', () => {
    const outcome = installHost('pi', t.env)

    assert.equal(outcome.ok, true)
    assert.match(readFileSync(log, 'utf8'), /install .*consensflow-pi/)
  })

  it('removes it with pi remove', () => {
    uninstallHost('pi', t.env)
    assert.match(readFileSync(log, 'utf8'), /remove .*consensflow-pi/)
  })

  it('says plainly when pi is not installed at all', () => {
    const bare = tempEnv()
    try {
      assert.throws(() => installHost('pi', bare.env), /pi/)
    } finally {
      bare.cleanup()
    }
  })
})
