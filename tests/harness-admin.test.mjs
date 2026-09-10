import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'
import { HarnessAdmin } from '../src/harness-admin.js'
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

test('version and update probes do not manufacture integration success; checks cache and refresh', async () => {
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
    assert.equal(row.integration.state, 'unverified')
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
    assert.equal(row.integration.state, 'unverified')
    await assert.rejects(admin.check('../invalid'), /Unknown harness/)
  } finally {
    t.cleanup()
  }
})

test('integration reports current native receipt evidence independently from version checks', async () => {
  const t = tempEnv()
  try {
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    writeFileSync(join(t.env.PATH, 'codex'), '#!/bin/sh\nprintf "unknown version\\n"\n')
    chmodSync(join(t.env.PATH, 'codex'), 0o755)
    const admin = new HarnessAdmin(t.env, {
      latest: async () => '1.0.0',
      integration: async (id) => ({
        state: 'ok',
        reason: `Complete result observed in the current ${id} lead`,
        checkedAt: 100,
      }),
    })
    const [row] = await admin.check('codex')
    assert.equal(row.version.state, 'unknown')
    assert.equal(row.integration.state, 'ok')
    assert.match(row.integration.reason, /Complete result observed/)
  } finally {
    t.cleanup()
  }
})

test('only a native receipt addressed to the current live lead proves integration', async () => {
  const { integrationEvidence } = await import('../src/harness-admin.js')
  const tabs = [
    {
      id: 't-1',
      role: 'lead',
      lead: { harness: 'codex', generation: 2, nativeSession: 'native-current' },
      panes: [{ id: 'p-1', kind: 'lead', generation: 2 }],
    },
  ]
  const target = { tab: 't-1', pane: 'p-1', generation: 2, session: 'native-current' }
  const receipt = { state: 'accepted', target, acceptedAt: 100, evidenceIds: ['native-result-1'] }
  for (const candidate of [
    { ...receipt, state: 'submitting' },
    { ...receipt, target: { ...target, generation: 1 } },
    { ...receipt, target: { ...target, session: 'old' } },
    { ...receipt, evidenceIds: [] },
  ]) {
    assert.equal(integrationEvidence('codex', tabs, [candidate]).state, 'unverified')
  }
  assert.equal(integrationEvidence('codex', tabs, [receipt]).state, 'ok')
  assert.equal(
    integrationEvidence('codex', [{ ...tabs[0], closed: true }], [receipt]).state,
    'unverified',
  )
  assert.equal(integrationEvidence('pi', tabs, [receipt]).state, 'unverified')
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
