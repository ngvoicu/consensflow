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
    assert.deepEqual([...system.mode.available].sort(), ['claude', 'pi', 'standalone'])
    assert.ok(Array.isArray(system.mode.report))
  })

  it('switches mode from the page, saying who gains and loses access', async () => {
    const res = await api('/api/mode', {
      method: 'POST',
      body: JSON.stringify({ mode: 'standalone' }),
    })
    assert.equal(res.status, 200)
    const body = await res.json()

    assert.equal(body.mode, 'standalone')
    assert.match(body.report.join(' '), /claude/)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('refuses a mode that does not exist', async () => {
    const res = await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode: 'emacs' }) })
    assert.equal(res.status, 400)
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

  it('removes them again, but only when the click was deliberate', async () => {
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
