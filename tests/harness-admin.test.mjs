import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { HarnessAdmin } from '../src/harness-admin.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv } from './helpers.mjs'

test('administration lists missing harnesses without probing or installing them', async () => {
  const t = tempEnv()
  try {
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        throw new Error('must not call')
      },
    })
    const rows = await admin.check()
    assert.equal(rows.length, 5)
    assert.ok(rows.every((r) => r.installed === false && r.version.state === 'not-installed'))
    assert.equal(rows.find((r) => r.id === 'kimi').lead, false)
  } finally {
    t.cleanup()
  }
})

test('version and update checks cache results and refresh on request', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    writeFileSync(join(t.env.PATH, 'codex'), '#!/bin/sh\nprintf "codex-cli 99.1.0\\n"\n')
    chmodSync(join(t.env.PATH, 'codex'), 0o755)
    let calls = 0
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        calls++
        return '99.2.0'
      },
    })
    const [row] = await admin.check('codex')
    assert.equal(row.version.value, '99.1.0')
    assert.equal(row.update.state, 'available')
    assert.equal(Object.hasOwn(row, 'integration'), false)
    await admin.check('codex')
    assert.equal(calls, 1)
    await admin.check('codex', { refresh: true })
    assert.equal(calls, 2)
  } finally {
    t.cleanup()
  }
})

test('offline and invalid version output are explicit failures, not latest or incompatible', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    writeFileSync(join(t.env.PATH, 'claude'), '#!/bin/sh\necho unusual\n')
    chmodSync(join(t.env.PATH, 'claude'), 0o755)
    const admin = new HarnessAdmin(t.env, {
      latest: async () => {
        throw new Error('offline')
      },
    })
    const [row] = await admin.check('claude')
    assert.equal(row.version.state, 'unknown')
    assert.equal(row.update.state, 'error')
    assert.equal(Object.hasOwn(row, 'integration'), false)
    await assert.rejects(admin.check('../invalid'), /Unknown harness/)
  } finally {
    t.cleanup()
  }
})

test('update source follows the detected Homebrew distribution instead of npm latest', async () => {
  const { releaseSource } = await import('../src/harness-admin.js')
  const t = tempEnv()
  try {
    assert.equal(
      releaseSource('codex', '/opt/homebrew/Caskroom/codex/0.1/bin/codex', t.env).url,
      'https://formulae.brew.sh/api/cask/codex.json',
    )
    assert.equal(
      releaseSource('opencode', '/opt/homebrew/Cellar/opencode/1/bin/opencode', t.env).url,
      'https://formulae.brew.sh/api/formula/opencode.json',
    )
    assert.equal(
      releaseSource('claude', '/opt/homebrew/Caskroom/claude-code@latest/2/bin/claude', t.env).url,
      'https://formulae.brew.sh/api/cask/claude-code@latest.json',
    )
    assert.equal(
      releaseSource(
        'pi',
        '/usr/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js',
        t.env,
      ).distribution,
      'npm',
    )
  } finally {
    t.cleanup()
  }
})

test('harness checks do not require session storage or expose receipt diagnostics', async () => {
  const t = tempEnv()
  mkdirSync(t.env.HOME, { recursive: true })
  mkdirSync(t.env.PATH, { recursive: true })
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  writeFileSync(join(t.env.PATH, 'codex'), '#!/bin/sh\necho 1.2.3\n')
  chmodSync(join(t.env.PATH, 'codex'), 0o755)
  mkdirSync(join(t.env.CONSENSFLOW_HOME, 'app'), { recursive: true })
  const tabs = join(t.env.CONSENSFLOW_HOME, 'app', 'tabs.json')
  const server = await startUiServer(t.env, { harnessLatest: async () => '1.2.4' })
  const saved = existsSync(tabs) ? readFileSync(tabs) : null
  try {
    writeFileSync(tabs, 'unreadable session state')
    const response = await fetch(`${server.url}/api/harnesses/check`, {
      method: 'POST',
      headers: { authorization: `Bearer ${server.token}`, 'content-type': 'application/json' },
      body: '{}',
    })
    assert.equal(response.status, 200)
    const { harnesses } = await response.json()
    assert.equal(harnesses.find((row) => row.id === 'codex').update.state, 'available')
    assert.ok(
      harnesses.every(
        (row) => !Object.hasOwn(row, 'integration') && !Object.hasOwn(row, 'instructions'),
      ),
    )
    assert.equal(readFileSync(tabs, 'utf8'), 'unreadable session state')
  } finally {
    if (saved === null) rmSync(tabs, { force: true })
    else writeFileSync(tabs, saved)
    await server.close()
    t.cleanup()
  }
})
