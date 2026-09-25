import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { installTerminalCommand, terminalCommandStatus, terminalRuntime } from '../src/terminal.js'
import { tempEnv } from './helpers.mjs'

/** A launcher is `cf` on POSIX and `cf.cmd` on Windows. */
const CMD = process.platform === 'win32' ? '.cmd' : ''

describe('the app can put its own CLI on your PATH', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const bin = join(t.root, 'bin')
  mkdirSync(bin, { recursive: true })

  it('reports nothing installed to begin with', () => {
    assert.equal(terminalCommandStatus(t.env, { candidates: [bin] }).installed, false)
  })

  it('writes a launcher that runs this very copy', () => {
    const outcome = installTerminalCommand(t.env, { candidates: [bin] })

    assert.equal(outcome.installed, true)
    const launcher = join(bin, `consensflow${CMD}`)
    assert.ok(existsSync(launcher))

    // It must point at the harness and sources running right now, so the
    // terminal and the app can never drift apart.
    const script = readFileSync(launcher, 'utf8')
    assert.ok(script.includes(process.execPath))
    assert.ok(script.includes('cf.mjs'))
    if (!CMD) assert.ok((statSync(launcher).mode & 0o111) !== 0, 'must be executable')
  })

  it('says where it went, and whether that place is on PATH', () => {
    const status = terminalCommandStatus({ ...t.env, PATH: bin }, { candidates: [bin] })
    assert.equal(status.installed, true)
    assert.equal(status.path, join(bin, `consensflow${CMD}`))
    assert.equal(status.onPath, true)

    const elsewhere = terminalCommandStatus({ ...t.env, PATH: '/nowhere' }, { candidates: [bin] })
    assert.equal(elsewhere.onPath, false)
  })

  it('says whether the command runs THIS copy, or another ConsensFlow', () => {
    installTerminalCommand(t.env, { candidates: [bin] })
    const ours = terminalRuntime(t.env, { candidates: [bin] })
    assert.equal(ours.runtime, process.execPath)
    assert.equal(ours.exists, true)
    assert.equal(ours.mine, true)

    // Two ConsensFlows on one machine — an app beside a repo build — and the
    // command names the other one. It exists, so every other check calls this
    // healthy while every `cf` the skill teaches runs the other one's code.
    const other = join(t.root, 'Other.app', 'node')
    mkdirSync(dirname(other), { recursive: true })
    writeFileSync(other, '')
    const otherCli = join(t.root, 'Other.app', 'cf.mjs')
    writeFileSync(
      join(bin, `consensflow${CMD}`),
      CMD
        ? `@echo off\r\nREM Installed by ConsensFlow.\r\n"${other}" "${otherCli}" %*\r\n`
        : `#!/bin/sh\n# Installed by ConsensFlow.\nexec "${other}" "${otherCli}" "$@"\n`,
    )

    const theirs = terminalRuntime(t.env, { candidates: [bin] })

    assert.equal(theirs.exists, true, 'it is there — which is what made this invisible')
    assert.equal(theirs.mine, false)
    // And it says WHICH copy, because a developer syncing a build has to
    // write into the one the command runs, not the one they just built.
    assert.equal(theirs.entry, join(t.root, 'Other.app', 'cf.mjs'))
  })

  it('says when the command runs the installed release, which development must never write into', {
    skip:
      process.platform === 'win32' &&
      'the release bundle is macOS-shaped; the Windows installer has its own',
  }, () => {
    const bundle = (name, identifier) => {
      const contents = join(t.root, `${name}.app`, 'Contents')
      mkdirSync(join(contents, 'Resources', 'cli', 'bin'), { recursive: true })
      writeFileSync(
        join(contents, 'Info.plist'),
        `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0">\n<dict>\n\t<key>CFBundleIdentifier</key>\n\t<string>${identifier}</string>\n</dict>\n</plist>\n`,
      )
      const entry = join(contents, 'Resources', 'cli', 'bin', 'cf.mjs')
      writeFileSync(entry, '')
      writeFileSync(
        join(bin, 'consensflow'),
        `#!/bin/sh\n# Installed by ConsensFlow.\nexec "${process.execPath}" "${entry}" "$@"\n`,
      )
    }
    bundle('Live', 'dev.ngvoicu.consensflow')
    assert.equal(terminalRuntime(t.env, { candidates: [bin] }).live, true)
    bundle('Candidate', 'dev.ngvoicu.consensflow.candidate')
    assert.equal(terminalRuntime(t.env, { candidates: [bin] }).live, false)
    installTerminalCommand(t.env, { candidates: [bin] })
    assert.equal(
      terminalRuntime(t.env, { candidates: [bin] }).live,
      false,
      'a checkout is not a bundle',
    )
  })

  it('keeps a separate home when run from a terminal that does not name one', () => {
    // The candidate's launcher is run from an ordinary terminal, where no
    // CONSENSFLOW_HOME is set: without the pin it would fall back to the
    // live ~/.consensflow.
    installTerminalCommand(t.env, { candidates: [bin] })
    const script = readFileSync(join(bin, `cf${CMD}`), 'utf8')
    assert.ok(
      script.includes(
        CMD
          ? `set "CONSENSFLOW_HOME=${t.env.CONSENSFLOW_HOME}"`
          : `export CONSENSFLOW_HOME="${t.env.CONSENSFLOW_HOME}"`,
      ),
    )
    assert.equal(terminalRuntime(t.env, { candidates: [bin] }).mine, true)

    const { CONSENSFLOW_HOME, ...defaults } = t.env
    const plain = join(t.root, 'plain-bin')
    mkdirSync(plain)
    installTerminalCommand(defaults, { candidates: [plain] })
    assert.ok(!readFileSync(join(plain, `cf${CMD}`), 'utf8').includes('CONSENSFLOW_HOME'))
  })

  it('explains itself when no candidate directory can be written', () => {
    // A file where a directory is wanted cannot be written into, anywhere.
    const blocked = join(t.root, 'blocked')
    writeFileSync(blocked, '')
    assert.throws(
      () => installTerminalCommand(t.env, { candidates: [join(blocked, 'bin')] }),
      /could not write|no writable/i,
    )
  })
})

it('keeps default CLI launchers in ConsensFlow home despite a project bin override', () => {
  const t = tempEnv()
  try {
    t.env.CONSENSFLOW_BIN_DIR = join(t.root, 'project', 'bin')
    const outcome = installTerminalCommand(t.env)
    assert.equal(outcome.dir, join(t.env.CONSENSFLOW_HOME, 'bin'))
    assert.equal(existsSync(t.env.CONSENSFLOW_BIN_DIR), false)
    assert.equal(existsSync(join(t.env.HOME, '.local', 'bin')), false)
    assert.equal(existsSync(join(outcome.dir, `cf${CMD}`)), true)
  } finally {
    t.cleanup()
  }
})
