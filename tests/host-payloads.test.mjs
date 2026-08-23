import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { retireHostPayloads, staleClaudeHooks } from '../src/host-payloads.js'
import { tempEnv } from './helpers.mjs'

/** A `pi` on PATH that records what it was asked to do. */
function piStub(t) {
  mkdirSync(t.env.PATH, { recursive: true })
  const log = join(t.root, 'pi-calls.log')
  const pi = join(t.env.PATH, 'pi')
  writeFileSync(pi, `#!/bin/sh\necho "$@" >> "${log}"\nexit 0\n`)
  chmodSync(pi, 0o755)
  return log
}

/** A machine as the host-payload era left it. */
function seedOldInstall(t, { pi = false } = {}) {
  const home = t.env.CONSENSFLOW_HOME
  const skill = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
  mkdirSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow'), { recursive: true })
  writeFileSync(skill, 'the old hand-written skill\n')

  for (const dir of ['claude', 'pi', 'lib']) {
    mkdirSync(join(home, 'hosts', dir), { recursive: true })
    writeFileSync(join(home, 'hosts', dir, 'file.js'), '// payload\n')
  }

  const hosts = {
    claude: { version: '2.9.0', payload: join(home, 'hosts', 'claude'), files: [skill] },
  }
  if (pi) hosts.pi = { version: '2.9.0', source: join(home, 'hosts', 'pi') }
  mkdirSync(home, { recursive: true })
  writeFileSync(join(home, 'hosts.json'), JSON.stringify({ hosts }, null, 2))
  return { skill, hostsJson: join(home, 'hosts.json') }
}

describe('the payload era is taken back, and nothing of it is reinstalled', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  it('removes the recorded skill, the payloads and the bookkeeping', () => {
    const { skill, hostsJson } = seedOldInstall(t)

    retireHostPayloads(t.env)

    // The old skill sat exactly where the generated one belongs. Left there,
    // installSkill would refuse it as unowned and the machine would end up
    // with no skill at all.
    assert.equal(existsSync(skill), false, 'the hand-written skill is gone')
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts')), false, 'no payloads left')
    assert.equal(existsSync(hostsJson), false, 'and no state claiming they exist')
  })

  it('asks pi to remove the extension it registered', () => {
    const log = piStub(t)
    seedOldInstall(t, { pi: true })

    retireHostPayloads(t.env)

    assert.match(readFileSync(log, 'utf8'), /^remove /m, 'pi is asked, through its own CLI')
  })

  it('does not call pi when no extension was ever registered', () => {
    const log = piStub(t)
    // An earlier test in this env already made one call; what matters is that
    // this run adds none.
    const before = existsSync(log) ? readFileSync(log, 'utf8') : ''
    seedOldInstall(t)

    retireHostPayloads(t.env)

    const after = existsSync(log) ? readFileSync(log, 'utf8') : ''
    assert.equal(after, before, 'nothing to remove, nobody asked')
  })

  it('survives a machine that never had a payload at all', () => {
    assert.doesNotThrow(() => retireHostPayloads(t.env))
  })
})

describe('the /consensflow command and the backup are ours to take back', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  const commands = () => join(t.env.CLAUDE_CONFIG_DIR, 'commands')
  const ours = () => join(commands(), 'consensflow.md')
  const OURS = '---\ndescription: "ConsensFlow: manage named agents"\n---\n'

  it('removes the command it wrote, and leaves other commands alone', () => {
    mkdirSync(commands(), { recursive: true })
    writeFileSync(ours(), OURS)
    writeFileSync(join(commands(), 'notes.md'), '---\ndescription: "my own"\n---\n')

    retireHostPayloads(t.env)

    assert.equal(existsSync(ours()), false)
    assert.ok(existsSync(join(commands(), 'notes.md')), "someone else's command is untouched")
  })

  it('never removes a same-named command it did not write', () => {
    writeFileSync(ours(), '---\ndescription: "mine, not theirs"\n---\n')

    retireHostPayloads(t.env)

    assert.ok(existsSync(ours()), 'no marker, not ours')
  })

  it('takes back the world-readable settings backup an older version dropped', () => {
    const backup = join(t.env.CLAUDE_CONFIG_DIR, 'settings.json.consensflow.bak')
    writeFileSync(backup, 'a copy of settings nobody ever read')

    retireHostPayloads(t.env)

    assert.equal(existsSync(backup), false)
  })
})

describe("Claude Code's settings are reported, never written", () => {
  const t = tempEnv()
  after(() => t.cleanup())

  const settings = () => join(t.env.CLAUDE_CONFIG_DIR, 'settings.json')

  function seed(value) {
    mkdirSync(t.env.CLAUDE_CONFIG_DIR, { recursive: true })
    writeFileSync(settings(), `${JSON.stringify(value, null, 2)}\n`)
  }

  it('names the events still holding a hook of ours', () => {
    seed({
      model: 'opus',
      hooks: {
        SessionStart: [{ hooks: [{ type: 'command', command: 'node /x/consensflow/hook.mjs' }] }],
        Stop: [{ hooks: [{ type: 'command', command: 'echo hello' }] }],
      },
    })

    const stale = staleClaudeHooks(t.env)

    assert.deepEqual(stale.events, ['SessionStart'], 'ours is named, theirs is not')
    assert.equal(stale.path, settings())
  })

  it('leaves the file untouched, byte for byte', () => {
    const before = readFileSync(settings(), 'utf8')

    staleClaudeHooks(t.env)
    retireHostPayloads(t.env)

    assert.equal(readFileSync(settings(), 'utf8'), before, 'not ours to write')
  })

  it('reports nothing for settings with no hooks, and never creates the file', () => {
    const t2 = tempEnv()
    try {
      assert.deepEqual(staleClaudeHooks(t2.env).events, [])
      retireHostPayloads(t2.env)
      assert.equal(existsSync(join(t2.env.CLAUDE_CONFIG_DIR, 'settings.json')), false)
    } finally {
      t2.cleanup()
    }
  })
})
