import assert from 'node:assert/strict'
import { chmod, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { test } from 'node:test'
import { Bridge } from '../src/bridge.js'
import { startUiServer } from '../src/ui.js'

test('the editor bridge no longer exposes native version inspection', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cf-update-bridge-'))
  const env = { ...process.env, HOME: root, CONSENSFLOW_HOME: join(root, 'state'), PATH: root }
  for (const name of ['claude', 'codex', 'pi', 'opencode', 'kimi']) {
    const file = join(root, name)
    await writeFile(file, '#!/bin/sh\nprintf "2.1.267\\n"\n')
    await chmod(file, 0o755)
  }
  const server = await startUiServer(env)
  const outgoing = new PassThrough()
  const incoming = new PassThrough()
  const node = new Bridge({ input: incoming, output: outgoing, idPrefix: 'n-', peerIdPrefix: 'r-' })
  const rust = new Bridge({ input: outgoing, output: incoming, idPrefix: 'r-', peerIdPrefix: 'n-' })
  try {
    server.attachBridge(node)
    const response = await rust.request('updates.compatibility', {})
    assert.equal(response.ok, false)
    assert.equal(response.compatibility, undefined)
  } finally {
    await server.drain()
    node.close()
    rust.close()
    await server.close()
    await rm(root, { recursive: true, force: true })
  }
})
