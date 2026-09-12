import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { launchConfiguration } from '../src/channels.js'
import { HarnessAdmin } from '../src/harness-admin.js'
import { installEverywhere } from '../src/install.js'
import { tempEnv } from './helpers.mjs'

function detected(t) {
  const f = tempEnv()
  t.after(() => f.cleanup())
  mkdirSync(f.env.PATH, { recursive: true })
  writeFileSync(join(f.env.PATH, 'opencode'), '#!/bin/sh\necho 1.18.30\n')
  chmodSync(join(f.env.PATH, 'opencode'), 0o755)
  return f
}

test('OpenCode preparation is private, immutable, importable and installed with the app', async (t) => {
  const { prepareOpenCodeExtension } = await import('../src/opencode-install.js')
  const f = detected(t)
  const global = join(f.env.HOME, '.config', 'opencode', 'tui.json')
  mkdirSync(dirname(global), { recursive: true })
  writeFileSync(global, '{"plugin":["user-plugin"]}')
  const extension = prepareOpenCodeExtension(f.env)
  assert.equal(extension.state, 'installed-unverified')
  assert.ok(extension.path.startsWith(f.env.CONSENSFLOW_HOME))
  assert.equal(typeof (await import(extension.path)).tui, 'function')
  assert.deepEqual(prepareOpenCodeExtension(f.env), extension)
  assert.equal(installEverywhere(f.env).opencodeExtension.path, extension.path)
  assert.equal(readFileSync(global, 'utf8'), '{"plugin":["user-plugin"]}')
  writeFileSync(extension.path, 'drifted')
  assert.equal(prepareOpenCodeExtension(f.env).state, 'error')
  assert.equal(readFileSync(extension.path, 'utf8'), 'drifted')
})

test('Harnesses prepares OpenCode and reports preparation failures for retry', async (t) => {
  const f = detected(t)
  const admin = new HarnessAdmin(f.env, { latest: async () => '1.18.30' })
  const [row] = await admin.check('opencode')
  assert.equal(row.extension.state, 'installed-unverified')
  writeFileSync(row.extension.path, 'drifted')
  assert.equal((await admin.check('opencode', { refresh: true }))[0].extension.state, 'error')
})

test('OpenCode launch loads bundled TUI integration with unique private admission credentials', async (t) => {
  const f = detected(t)
  const { prepareOpenCodeExtension } = await import('../src/opencode-install.js')
  const extension = prepareOpenCodeExtension(f.env)
  const input = {
    launchId: 'owned-launch',
    workspace: f.root,
    env: f.env,
    extensionPath: extension.path,
  }
  const a = await launchConfiguration('opencode', input)
  const b = await launchConfiguration('opencode', { ...input, launchId: 'other-launch' })
  assert.ok(existsSync(a.env.OPENCODE_TUI_CONFIG))
  const config = JSON.parse(readFileSync(a.env.OPENCODE_TUI_CONFIG, 'utf8'))
  assert.equal(config.plugin.length, 1)
  assert.match(config.plugin[0], /consensflow-session\.mjs$/)
  const options = JSON.parse(a.env.CF_OPENCODE_SESSION_BRIDGE)
  assert.equal(options.launchId, 'owned-launch')
  assert.equal(a.channel.sessionBridge.token, options.token)
  assert.notEqual(a.channel.sessionBridge.token, b.channel.sessionBridge.token)
  assert.match(a.channel.sessionBridge.endpoint, /^http:\/\/127\.0\.0\.1:/)
  assert.equal(f.env.OPENCODE_TUI_CONFIG, undefined)
})
