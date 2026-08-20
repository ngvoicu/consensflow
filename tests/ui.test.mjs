import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { listParticipants } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv } from './helpers.mjs'

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('a host program can start the editor and be told where it is', () => {
  it('prints one machine-readable line, then serves', async () => {
    const t = tempEnv()
    const { spawn } = await import('node:child_process')
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    // No stdin at all: the editor serves until it is killed.
    const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
      env: t.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    try {
      const line = await new Promise((resolve, reject) => {
        let buffer = ''
        child.stdout.on('data', (chunk) => {
          buffer += chunk
          const end = buffer.indexOf('\n')
          if (end !== -1) resolve(buffer.slice(0, end))
        })
        child.on('error', reject)
        setTimeout(() => reject(new Error('no handle line')), 10_000)
      })

      const handle = JSON.parse(line)
      assert.ok(handle.url.length > 0)
      assert.match(handle.url, /^http:\/\/127\.0\.0\.1:\d+\/$/)
      assert.equal(typeof handle.token, 'string')

      // The address it printed is really serving.
      const res = await fetch(`${handle.url}?token=${handle.token}`)
      assert.equal(res.status, 200)
    } finally {
      child.kill()
      t.cleanup()
    }
  })

  it('shuts down when the program that started it goes away', async () => {
    const t = tempEnv()
    const { spawn } = await import('node:child_process')
    const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
    const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
      env: t.env,
      stdio: ['pipe', 'pipe', 'ignore'],
    })
    try {
      await new Promise((resolve, reject) => {
        child.stdout.once('data', resolve)
        setTimeout(() => reject(new Error('never started')), 10_000)
      })

      // A parent that holds a pipe owns the lifetime: closing it says "gone".
      child.stdin.end()
      const code = await new Promise((resolve, reject) => {
        child.once('exit', resolve)
        setTimeout(() => reject(new Error('the editor kept serving')), 10_000)
      })
      assert.equal(code, 0)
    } finally {
      child.kill()
      t.cleanup()
    }
  })
})

describe('the roster UI is loopback, token-gated and ephemeral', () => {
  const t = tempEnv()
  let server

  before(async () => {
    stubCli(t, 'claude')
    server = await startUiServer(t.env)
  })
  after(async () => {
    await server.close()
    t.cleanup()
  })

  function api(path, options = {}) {
    return fetch(`${server.url}${path}`, {
      ...options,
      headers: {
        authorization: `Bearer ${server.token}`,
        'content-type': 'application/json',
        ...options.headers,
      },
    })
  }

  it('binds loopback only and refuses requests without the token', async () => {
    assert.match(server.url, /^http:\/\/127\.0\.0\.1:/)
    const res = await fetch(`${server.url}/api/participants`)
    assert.equal(res.status, 401)
  })

  it('serves a page whose script actually ran through the template', async () => {
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    // An escaped `\${…}` ships a page that dies on load: the token never
    // interpolates and the browser hits a syntax error before fetching data.
    assert.ok(html.includes(`const TOKEN = "${server.token}"`))
    assert.ok(!html.includes('${JSON.stringify'))
  })

  it('serves the editor page', async () => {
    const res = await fetch(`${server.url}/?token=${server.token}`)
    assert.equal(res.status, 200)
    const html = await res.text()
    assert.match(html, /ConsensFlow/i)
    assert.match(html, /participant/i)
  })

  it('adds, edits and removes participants through the API, persisting each', async () => {
    const added = await api('/api/participants', {
      method: 'POST',
      body: JSON.stringify({ name: 'zeus', runtime: 'claude', model: 'claude-opus-5' }),
    })
    assert.equal(added.status, 201)
    assert.equal(listParticipants(t.env)[0].name, 'zeus')

    const edited = await api('/api/participants/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-fable-5' }),
    })
    assert.equal(edited.status, 200)
    assert.equal(listParticipants(t.env)[0].model, 'claude-fable-5')

    const listed = await api('/api/participants')
    const payload = await listed.json()
    assert.equal(payload.participants.length, 1)
    // Permission is gone from the product: no select, no field, no API.
    assert.equal(payload.permissions, undefined)
    assert.ok(Array.isArray(payload.catalog.claude))
  })

  it('installs and regenerates the skill on every change — no separate step', async () => {
    // The add in the previous test already installed it; the edit refreshes.
    await api('/api/participants/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-opus-5' }),
    })

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /claude-opus-5/)
  })

  it('reports the same system state cf doctor prints', async () => {
    const res = await api('/api/system')
    assert.equal(res.status, 200)
    const system = await res.json()

    assert.match(system.version, /^3\./)
    assert.deepEqual(
      system.agents.map((a) => a.id),
      ['claude'],
    )
    assert.equal(system.agents[0].native, false)
    assert.equal(typeof system.skills.owned, 'number')
    assert.equal(system.participants, 1)
  })

  it('reports the mode and what it means for the machine', async () => {
    const res = await api('/api/system')
    const system = await res.json()

    assert.equal(system.mode.current, null)
    assert.equal(system.mode.labels.cmux, 'cmux (pi, cc, codex, opencode)')
    assert.deepEqual([...system.mode.available].sort(), ['claude', 'cmux', 'pi'])
    assert.ok(Array.isArray(system.mode.report))
  })

  it('switches mode from the page, saying who gains and loses access', async () => {
    const res = await api('/api/mode', {
      method: 'POST',
      body: JSON.stringify({ mode: 'cmux' }),
    })
    assert.equal(res.status, 200)
    const body = await res.json()

    assert.equal(body.mode, 'cmux')
    assert.match(body.report.join(' '), /claude/)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('refuses a mode that does not exist', async () => {
    const res = await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode: 'emacs' }) })
    assert.equal(res.status, 400)
  })

  it('reports each integration: what it is, and where it stands', async () => {
    const system = await (await api('/api/system')).json()

    const byId = Object.fromEntries(system.integrations.map((i) => [i.id, i]))
    assert.deepEqual(Object.keys(byId).sort(), ['claude', 'cmux', 'pi'])

    // Each one says what it gives you and whether it is the active path.
    for (const integration of system.integrations) {
      assert.ok(integration.title.length > 0)
      assert.ok(integration.summary.length > 0)
      assert.equal(typeof integration.active, 'boolean')
      assert.equal(typeof integration.present, 'boolean')
    }
    assert.match(byId.cmux.title, /cmux/)
    assert.match(byId.claude.summary, /conversation/i)
  })

  it('installs and updates the skills from the page', async () => {
    const res = await api('/api/skills/install', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(res.status, 200)
    const body = await res.json()

    assert.ok(body.report.some((r) => r.action === 'installed' || r.action === 'updated'))
    assert.ok(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      'the generated skill is on disk',
    )
  })

  it('turns everything off from the page, deliberately', async () => {
    const refused = await api('/api/off', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(refused.status, 400)

    const done = await api('/api/off', { method: 'POST', body: JSON.stringify({ confirm: true }) })
    assert.equal(done.status, 200)
    assert.equal((await done.json()).system.mode.current, null)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
  })

  it('removes them again, but only when the click was deliberate', async () => {
    // Off cleared everything; put the skill back so removal has a target.
    await api('/api/skills/install', { method: 'POST', body: JSON.stringify({}) })
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))

    const refused = await api('/api/skills/uninstall', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(refused.status, 400)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))

    const done = await api('/api/skills/uninstall', {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    })
    assert.equal(done.status, 200)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
  })

  it('never exposes a way to run arbitrary commands', async () => {
    for (const path of ['/api/exec', '/api/run', '/api/shell']) {
      const res = await api(path, { method: 'POST', body: JSON.stringify({ command: 'id' }) })
      assert.equal(res.status, 404, `${path} must not exist`)
    }
  })

  it('surfaces validation errors as JSON, not crashes', async () => {
    const res = await api('/api/participants', {
      method: 'POST',
      body: JSON.stringify({ name: 'Bad Name', runtime: 'claude', model: 'm' }),
    })
    assert.equal(res.status, 400)
    assert.match((await res.json()).error, /names/)
  })

  it('removes a participant', async () => {
    const res = await api('/api/participants/zeus', { method: 'DELETE' })
    assert.equal(res.status, 204)
    assert.deepEqual(listParticipants(t.env), [])
  })
})
