import assert from 'node:assert/strict'
import { existsSync, mkdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import {
  installTerminalCommand,
  removeTerminalCommand,
  terminalCommandStatus,
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
