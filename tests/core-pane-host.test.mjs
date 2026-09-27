import assert from 'node:assert/strict'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { PaneHost } from '../src/core/pane-host.js'

describe('PaneHost.open', () => {
  const root = mkdtempSync(join(tmpdir(), 'cf-pane-host-'))
  after(() => rmSync(root, { recursive: true, force: true }))
  const sent = []
  const host = new PaneHost({
    request: async (op, body) => {
      sent.push([op, body.argv])
      return { ok: true }
    },
  })

  it('hands the host a program as it is', async () => {
    assert.deepEqual(await host.open({ id: 'p1-chief', argv: ['/usr/local/bin/pi', 'x'] }), {
      ok: true,
    })
    assert.deepEqual(sent.at(-1), ['pane.open', ['/usr/local/bin/pi', 'x']])
  })

  it('and an npm shim as its own program and script', async () => {
    const script = join(root, 'cli.js')
    writeFileSync(script, '')
    const shim = join(root, 'cli.cmd')
    writeFileSync(shim, `@echo off\r\n"${process.execPath}" "${script}" %*\r\n`)
    await host.open({ id: 'p1-chief', argv: [shim, '--model', 'm'] })
    assert.deepEqual(sent.at(-1), ['pane.open', [process.execPath, script, '--model', 'm']])
  })

  it('but refuses, as a failed open, a .cmd no window can run', async () => {
    const opaque = join(root, 'opaque.cmd')
    writeFileSync(opaque, '@echo off\r\n"%NODE_EXE%" "%NPM_CLI_JS%" %*\r\n')
    const count = sent.length
    await assert.rejects(host.open({ id: 'p1-chief', argv: [opaque] }), /is not an npm shim/)
    assert.equal(sent.length, count)
  })
})
