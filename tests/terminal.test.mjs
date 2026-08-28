import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  installTerminalCommand,
  removeTerminalCommand,
  terminalCommandStatus,
  terminalRuntime,
} from '../src/terminal.js'
import { tempEnv } from './helpers.mjs'

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
    const launcher = join(bin, 'consensflow')
    assert.ok(existsSync(launcher))

    // It must point at the harness and sources running right now, so the
    // terminal and the app can never drift apart.
    const script = readFileSync(launcher, 'utf8')
    assert.ok(script.includes(process.execPath))
    assert.ok(script.includes('cf.mjs'))
    assert.ok((statSync(launcher).mode & 0o111) !== 0, 'must be executable')
  })

  it('says where it went, and whether that place is on PATH', () => {
    const status = terminalCommandStatus({ ...t.env, PATH: bin }, { candidates: [bin] })
    assert.equal(status.installed, true)
    assert.equal(status.path, join(bin, 'consensflow'))
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
    writeFileSync(
      join(bin, 'consensflow'),
      `#!/bin/sh\n# Installed by ConsensFlow.\nexec "${other}" "${join(t.root, 'Other.app', 'cf.mjs')}" "$@"\n`,
    )

    const theirs = terminalRuntime(t.env, { candidates: [bin] })

    assert.equal(theirs.exists, true, 'it is there — which is what made this invisible')
    assert.equal(theirs.mine, false)
  })

  it('removes it again', () => {
    removeTerminalCommand(t.env, { candidates: [bin] })
    assert.equal(existsSync(join(bin, 'consensflow')), false)
    assert.equal(terminalCommandStatus(t.env, { candidates: [bin] }).installed, false)
  })

  it('explains itself when no candidate directory can be written', () => {
    assert.throws(
      () => installTerminalCommand(t.env, { candidates: ['/System/nope'] }),
      /could not write|no writable/i,
    )
  })
})
