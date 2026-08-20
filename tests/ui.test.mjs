import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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
