import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { after, describe, it } from 'node:test'
import { hostStatus, installHost, uninstallHost } from '../src/hosts.js'

const VERSION = createRequire(import.meta.url)('../package.json').version

import { tempEnv } from './helpers.mjs'

/** A fixture payload shaped like the bundled hosts/ tree. */
function fixturePayload(t) {
  const fixture = join(t.root, 'bundled')
  mkdirSync(join(fixture, 'lib'), { recursive: true })
  mkdirSync(join(fixture, 'claude', 'bin'), { recursive: true })
  mkdirSync(join(fixture, 'pi'), { recursive: true })
  mkdirSync(join(fixture, 'claude', 'scripts'), { recursive: true })
  mkdirSync(join(fixture, 'claude', 'skills', 'consensflow'), { recursive: true })
  mkdirSync(join(fixture, 'claude', 'commands'), { recursive: true })
  writeFileSync(join(fixture, 'claude', 'scripts', 'session-start-hook.mjs'), '// hook\n')
  writeFileSync(join(fixture, 'claude', 'bin', 'cf.mjs'), '// host cli\n')
  writeFileSync(join(fixture, 'lib', 'runners.js'), '// runners\n')
  writeFileSync(join(fixture, 'pi', 'index.ts'), '// pi extension\n')
  writeFileSync(
    join(fixture, 'claude', 'skills', 'consensflow', 'SKILL.md'),
    // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
    'run: node "${CONSENSFLOW_HOST_ROOT}/bin/cf.mjs" status\n',
  )
  writeFileSync(
    join(fixture, 'claude', 'commands', 'cf.md'),
    // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
    'node "${CONSENSFLOW_HOST_ROOT}/bin/cf.mjs" run\n',
  )
  writeFileSync(
    join(fixture, 'claude', 'hooks.json'),
    JSON.stringify({
      hooks: {
        SessionStart: [
          {
            hooks: [
              {
                type: 'command',
                // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
                command: 'node "${CONSENSFLOW_HOST_ROOT}/scripts/session-start-hook.mjs"',
                timeout: 15,
              },
            ],
          },
        ],
      },
    }),
  )

  return fixture
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
  const bundled = fixturePayload(t)

  const skill = () => join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
  const command = () => join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')
  const settings = () => join(t.env.CLAUDE_CONFIG_DIR, 'settings.json')

  it('reports nothing installed before it runs', () => {
    assert.equal(hostStatus(t.env).find((h) => h.id === 'claude').installed, false)
  })

  it('installs the payload, the skill and the command — and nothing else', () => {
    const outcome = installHost('claude', t.env, { bundled })

    assert.equal(outcome.version, VERSION)
    // The payload lands in our own directory, never inside Claude Code —
    // with the engine beside it, which is where its imports look.
    const payload = join(t.env.CONSENSFLOW_HOME, 'hosts', 'lib', 'runners.js')
    assert.ok(existsSync(payload), 'payload copied')

    // Plugin-root references are rewritten to where the payload actually is.
    const skillText = readFileSync(skill(), 'utf8')
    assert.ok(!skillText.includes('CLAUDE_PLUGIN_ROOT'))
    assert.ok(skillText.includes(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')))
    assert.ok(!skillText.includes('CONSENSFLOW_HOST_ROOT'))
    assert.ok(existsSync(command()))

    // And it writes nothing into Claude Code's settings: there are no hooks
    // any more, so an install is three files and no configuration change.
    assert.equal(existsSync(settings()), false, 'settings.json is not even created')
  })

  it('leaves settings.json byte-identical, and takes back hooks it once wrote', () => {
    // A machine upgrading from a version that installed hooks: ours go, the
    // user's stay, and everything else is untouched.
    const mine = {
      type: 'command',
      command: `node "${join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude', 'scripts', 'session-start-hook.mjs')}"`,
    }
    const theirs = { type: 'command', command: 'echo hello' }
    writeFileSync(
      settings(),
      JSON.stringify(
        {
          model: 'opus',
          voiceEnabled: true,
          hooks: { SessionStart: [{ hooks: [mine] }, { hooks: [theirs] }] },
        },
        null,
        2,
      ),
    )

    installHost('claude', t.env, { bundled, force: true })

    const after = JSON.parse(readFileSync(settings(), 'utf8'))
    assert.equal(after.model, 'opus', 'user settings survive')
    assert.equal(after.voiceEnabled, true)
    assert.deepEqual(
      after.hooks.SessionStart.flatMap((entry) => entry.hooks),
      [theirs],
      'ours is gone, theirs is kept',
    )
  })

  it('says it is installed, and at which version', () => {
    const claude = hostStatus(t.env).find((h) => h.id === 'claude')
    assert.equal(claude.installed, true)
    assert.equal(claude.version, VERSION)
  })

  it('uninstalls exactly what it wrote, keeping the user settings', () => {
    uninstallHost('claude', t.env, { bundled })

    assert.equal(existsSync(skill()), false)
    assert.equal(existsSync(command()), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')), false)
    const after = JSON.parse(readFileSync(settings(), 'utf8'))
    assert.equal(after.model, 'opus', 'user settings survive')
    assert.deepEqual(
      (after.hooks?.SessionStart ?? []).flatMap((entry) => entry.hooks ?? []),
      [{ type: 'command', command: 'echo hello' }],
      'the foreign hook is still there; ConsensFlow left nothing of its own',
    )
    assert.equal(hostStatus(t.env).find((h) => h.id === 'claude').installed, false)
  })
})

describe('it refuses to install alongside an integration already there', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const bundled = fixturePayload(t)

  it('stops when the Claude Code plugin is already installed, and says how to fix it', () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })

    assert.throws(
      () => installHost('claude', t.env, { bundled }),
      /already installed .*plugin|--force/,
    )
    // Nothing was written: two installs would run their hooks twice.
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
  })

  it('installs anyway when told to', () => {
    const outcome = installHost('claude', t.env, { bundled, force: true })
    assert.equal(outcome.host, 'claude')
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
  const bundled = fixturePayload(t)

  it('hands pi the bundled payload and lets pi install it', () => {
    const outcome = installHost('pi', t.env, { bundled })

    assert.equal(outcome.ok, true)
    // The payload is ours to place; adding it to pi's settings is pi's job.
    const payload = join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi')
    assert.ok(existsSync(join(payload, 'index.ts')))
    // `../lib` from index.ts: a sibling of the payload, not a child of it.
    assert.ok(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'lib', 'runners.js')))
    assert.match(readFileSync(log, 'utf8'), new RegExp(`install ${payload}`))
  })

  it('removes it with pi remove, and takes the payload with it', () => {
    uninstallHost('pi', t.env)
    assert.match(readFileSync(log, 'utf8'), /remove /)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi')), false)
  })

  it('says plainly when pi is not installed at all', () => {
    const bare = tempEnv()
    try {
      assert.throws(() => installHost('pi', bare.env, { bundled }), /pi/)
    } finally {
      bare.cleanup()
    }
  })
})

describe('the payload that ships can actually run once installed', () => {
  it('keeps lib a sibling, so every relative import resolves', () => {
    const t = tempEnv()
    try {
      // The real bundled payload, not a fixture: this is exactly the tree a
      // user installs, and its own import strings are the specification.
      installHost('claude', t.env)
      const root = join(t.env.CONSENSFLOW_HOME, 'hosts', 'claude')
      const entries = [join(root, 'bin', 'cf.mjs')]

      let checked = 0
      for (const entry of entries) {
        assert.ok(existsSync(entry), `${entry} is part of the payload`)
        for (const match of readFileSync(entry, 'utf8').matchAll(/from ["'](\.[^"']+)["']/g)) {
          const target = resolve(dirname(entry), match[1])
          assert.ok(
            existsSync(target),
            `${entry} imports ${match[1]}, which is missing after install`,
          )
          checked += 1
        }
      }
      assert.ok(checked > 0, 'the payload really does import its own modules')
    } finally {
      t.cleanup()
    }
  })
})

describe('a machine without Node still runs what the payload installs', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('names the runtime absolutely, never a bare `node`', () => {
    installHost('claude', t.env)

    // No hooks any more, so the place a runtime is still named is the command
    // file — and it must not gamble on Node being on PATH.
    const commandFile = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md'),
      'utf8',
    )
    assert.doesNotMatch(
      commandFile,
      /^node "/m,
      'a bare `node` assumes a PATH the app does not need',
    )
    const [, runtime] = commandFile.match(/"([^"]+)"\s+"[^"]*cf\.mjs"/) ?? []
    if (runtime !== undefined) {
      assert.ok(existsSync(runtime), 'the runtime it names is the one doing the installing')
    }
  })
})
