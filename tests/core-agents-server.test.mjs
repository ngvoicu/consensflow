import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path, { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { Script } from 'node:vm'
import { agentsUi } from '../src/core/agents-server.js'
import { Credentials, startApi } from '../src/core/api.js'
import { openLedger } from '../src/ledger/index.js'
import { listAgents } from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

/**
 * The human's agents screens on the new core (TEST-BDC-24): the roster editor
 * with its tags, the agent library and the harness diagnostics, served by
 * the same server the agents' API runs on, behind the UI token the app
 * checks. Carried over from the old daemon's tests.
 */
function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const file = join(t.env.PATH, name)
  writeFileSync(file, '#!/bin/sh\nexit 0\n')
  chmodSync(file, 0o755)
}

describe('the agents screens on the new core', () => {
  const t = tempEnv()
  const token = 'ui-token-for-the-tests'
  let dir
  let ledger
  let server

  before(async () => {
    stubCli(t, 'claude')
    dir = await mkdtemp(path.join(os.tmpdir(), 'cf-agents-ui-'))
    ledger = openLedger(path.join(dir, 'consensflow.db'))
    server = await startApi({
      ledger,
      credentials: new Credentials(),
      ui: agentsUi(t.env, { token }),
    })
  })
  after(async () => {
    await server.close()
    ledger.close()
    await rm(dir, { recursive: true, force: true })
    t.cleanup()
  })

  const api = (route, options = {}) =>
    fetch(`${server.url}${route}`, {
      ...options,
      headers: {
        authorization: `Bearer ${token}`,
        'content-type': 'application/json',
        ...options.headers,
      },
    })
  const scriptsParse = (html) => {
    const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)].map((m) => m[1])
    assert.ok(scripts.length > 0, 'the page ships at least one script')
    for (const [index, source] of scripts.entries()) {
      if (source.trim().length === 0) continue
      assert.doesNotThrow(() => new Script(source), `script #${index} must parse`)
    }
  }

  it('is loopback only and refuses the screens and their API without the token', async () => {
    assert.match(server.url, /^http:\/\/127\.0\.0\.1:/)
    for (const route of ['/', '/library', '/harnesses', '/api/agents']) {
      assert.equal((await fetch(`${server.url}${route}`)).status, 401, route)
    }
    assert.equal((await fetch(`${server.url}/?token=wrong`)).status, 401)
  })

  it("leaves the agents' own API to its own tokens", async () => {
    const whoami = await api('/api/whoami')
    assert.equal(whoami.status, 401, 'the UI token opens no window')
  })

  it('serves the roster editor with the token interpolated and valid, dialog-free JavaScript', async () => {
    const response = await fetch(`${server.url}/?token=${token}`)
    assert.equal(response.status, 200)
    assert.match(response.headers.get('content-type'), /text\/html/)
    const html = await response.text()
    assert.ok(html.includes(`const TOKEN = "${token}"`))
    assert.ok(!html.includes('${JSON.stringify'))
    scriptsParse(html)
    const script = html.match(/<script\b[^>]*>([\s\S]*?)<\/script>/)[1]
    for (const call of ['confirm(', 'alert(', 'prompt(']) {
      assert.ok(!script.includes(call), `the page must not call ${call}`)
    }
    for (const marker of ['id="roster"', 'aria-label="Your agents"', 'id="add"', 'name="tags"']) {
      assert.ok(html.includes(marker), `the page is missing ${marker}`)
    }
    assert.doesNotMatch(
      html,
      /Talking to an agent|class="cmds"|cf sessions|cf results|cf read|cf attach|id="catalog-section"|Check all harnesses|id="terminal"|\/api\/mode/,
    )
  })

  it('serves the agent library and the harness diagnostics as their own pages', async () => {
    const library = await (await api('/library')).text()
    assert.match(library, /aria-label="Agent library"/)
    assert.match(library, /id="catalog"/)
    assert.doesNotMatch(library, /id="roster-section"|id="add"/)
    scriptsParse(library)
    const harnesses = await (await api('/harnesses')).text()
    assert.match(harnesses, /<h1>Harnesses<\/h1>/)
    assert.match(harnesses, /Check all harnesses/)
    assert.ok(harnesses.includes(`const TOKEN = "${token}"`))
    scriptsParse(harnesses)
  })

  it('adds, edits, tags and removes agents through the API, persisting each', async () => {
    const added = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({
        name: 'zeus',
        harness: 'claude',
        model: 'claude-opus-5',
        tags: ['coding', 'rust'],
      }),
    })
    assert.equal(added.status, 201)
    assert.deepEqual(
      [listAgents(t.env)[0].name, listAgents(t.env)[0].tags],
      ['zeus', ['coding', 'rust']],
    )
    const edited = await api('/api/agents/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-fable-5-1', tags: ['review'] }),
    })
    assert.equal(edited.status, 200)
    assert.deepEqual(
      [listAgents(t.env)[0].model, listAgents(t.env)[0].tags],
      ['claude-fable-5-1', ['review']],
    )
    const bad = await api('/api/agents/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ tags: ['Not A Tag'] }),
    })
    assert.equal(bad.status, 400)
    assert.match((await bad.json()).error, /tags/)
    const listed = await (await api('/api/agents')).json()
    assert.equal(listed.agents.length, 1)
    assert.deepEqual(listed.agents[0].tags, ['review'])
    assert.ok(Array.isArray(listed.catalog.claude))
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false, 'no role files prepared')

    const invalid = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'Bad Name', harness: 'claude', model: 'm' }),
    })
    assert.equal(invalid.status, 400)
    assert.match((await invalid.json()).error, /names/)

    const removed = await api('/api/agents/zeus', { method: 'DELETE' })
    assert.equal(removed.status, 204)
    assert.deepEqual(listAgents(t.env), [])
  })

  it('offers the catalog update as a named operation', async () => {
    await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({
        name: 'diana',
        harness: 'codex',
        model: 'gpt-5.5',
        effort: 'xhigh',
        preset: 'diana',
      }),
    })
    const before = await (await api('/api/agents')).json()
    assert.ok(
      before.drift.find((d) => d.name === 'diana'),
      'the page is told the catalog moved',
    )
    const synced = await api('/api/agents/sync', {
      method: 'POST',
      body: JSON.stringify({ name: 'diana' }),
    })
    assert.equal(synced.status, 200)
    assert.equal((await synced.json()).agents.find((p) => p.name === 'diana').model, 'gpt-5.6-luna')
    await api('/api/agents/diana', { method: 'DELETE' })
  })

  it('exposes nothing that runs commands, and no retired route', async () => {
    for (const route of [
      '/api/exec',
      '/api/run',
      '/api/shell',
      '/api/mode',
      '/api/off',
      '/api/reset',
    ]) {
      const response = await api(route, { method: 'POST', body: JSON.stringify({ command: 'id' }) })
      // Under the UI token an unknown route is refused as a window's (401) or unknown (404); neither runs.
      assert.ok([401, 404].includes(response.status), `${route} must not exist`)
    }
    assert.ok([401, 404].includes((await api('/api/system')).status))
  })
})
