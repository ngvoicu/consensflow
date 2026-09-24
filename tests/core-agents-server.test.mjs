import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path, { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { Script } from 'node:vm'
import { agentsUi } from '../src/core/agents-server.js'
import { Credentials, startApi } from '../src/core/api.js'
import { openLedger } from '../src/ledger/index.js'
import { listAgents, rosterPath } from '../src/roster.js'
import { tempEnv } from './helpers.mjs'

/**
 * The human's agents screens on the new core (TEST-BDC-24): the agents (the
 * catalog and the saved agents as one list) and the harness diagnostics,
 * served by the same server the agents' API runs on, behind the UI token the
 * app checks. Carried over from the old daemon's tests.
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
    for (const route of ['/', '/harnesses', '/api/agents']) {
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
    for (const marker of [
      'id="agents"',
      'aria-label="Agents"',
      'id="add"',
      'name="show"',
      'name="workTier"',
    ]) {
      assert.ok(html.includes(marker), `the page is missing ${marker}`)
    }
    assert.doesNotMatch(
      html,
      /name="tags"|Agent library|Your agents|PM candidate/,
      'no tags, one list',
    )
    assert.doesNotMatch(
      html,
      /Talking to an agent|class="cmds"|cf sessions|cf results|cf read|cf attach|id="catalog-section"|Check all harnesses|id="terminal"|\/api\/mode/,
    )
  })

  it('serves the harness diagnostics as their own page, and no library page any more', async () => {
    assert.equal(
      (await api('/library')).status,
      401,
      'no library page: the UI token opens nothing else',
    )
    const harnesses = await (await api('/harnesses')).text()
    assert.match(harnesses, /<h1>Harnesses<\/h1>/)
    assert.match(harnesses, /Check all harnesses/)
    assert.ok(harnesses.includes(`const TOKEN = "${token}"`))
    scriptsParse(harnesses)
  })

  it('keeps Claude and OpenAI models to their own harnesses when asked, and says so in the payload', async () => {
    const chosen = await api('/api/preferences', {
      method: 'POST',
      body: JSON.stringify({ ownHarnessOnly: true }),
    })
    assert.equal(chosen.status, 200)
    assert.deepEqual(await chosen.json(), { preferences: { ownHarnessOnly: true } })
    const listed = await (await api('/api/agents')).json()
    assert.deepEqual(listed.preferences, { ownHarnessOnly: true })
    assert.deepEqual(
      ['kronos', 'apollo'].map(
        (name) => listed.agents.find((p) => p.name === name).hidden === true,
      ),
      [true, false],
    )
    const html = await (await api('/')).text()
    assert.ok(html.includes('name="ownHarnessOnly"'))
    const bad = await api('/api/preferences', {
      method: 'POST',
      body: JSON.stringify({ ownHarnessOnly: 'yes' }),
    })
    assert.equal(bad.status, 400)
    await api('/api/preferences', {
      method: 'POST',
      body: JSON.stringify({ ownHarnessOnly: false }),
    })
  })

  it('adds, edits and removes agents through the API, persisting each', async () => {
    const added = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'mine', harness: 'claude', model: 'claude-opus-5' }),
    })
    assert.equal(added.status, 201)
    assert.deepEqual(
      [
        listAgents(t.env).find((p) => p.name === 'mine').name,
        listAgents(t.env).find((p) => p.name === 'mine').workTier,
      ],
      ['mine', undefined],
    )
    const edited = await api('/api/agents/mine', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-fable-5-1', workTier: 'complex' }),
    })
    assert.equal(edited.status, 200)
    assert.deepEqual(
      [
        listAgents(t.env).find((p) => p.name === 'mine').model,
        listAgents(t.env).find((p) => p.name === 'mine').workTier,
      ],
      ['claude-fable-5-1', 'complex'],
    )
    const bad = await api('/api/agents/mine', {
      method: 'PATCH',
      body: JSON.stringify({ workTier: 'huge' }),
    })
    assert.equal(bad.status, 400)
    assert.match((await bad.json()).error, /Work tier/)
    const listed = await (await api('/api/agents')).json()
    const mine = listed.agents.find((p) => p.name === 'mine')
    assert.deepEqual([mine.workTier, mine.custom], ['complex', true])
    assert.equal('tags' in mine, false, 'an agent carries no tags')
    assert.ok(Array.isArray(listed.harnesss), 'the harnesses the form offers')
    assert.equal(Object.hasOwn(listed, 'catalog'), false, 'the agents are the catalog')
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false, 'no role files prepared')

    const invalid = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'Bad Name', harness: 'claude', model: 'm' }),
    })
    assert.equal(invalid.status, 400)
    assert.match((await invalid.json()).error, /names/)

    const removed = await api('/api/agents/mine', { method: 'DELETE' })
    assert.equal(removed.status, 204)
    assert.equal(
      listAgents(t.env).some((p) => p.name === 'mine'),
      false,
    )
  })

  it('lists every catalog agent and refuses to edit, remove or redefine one', async () => {
    const listed = await (await api('/api/agents')).json()
    assert.ok(
      listed.agents.find((p) => p.name === 'gefjon'),
      'the catalog is the roster',
    )
    assert.equal(Object.hasOwn(listed, 'catalog'), false)
    const edited = await api('/api/agents/gefjon', {
      method: 'PATCH',
      body: JSON.stringify({ effort: 'low' }),
    })
    assert.equal(edited.status, 400)
    assert.match((await edited.json()).error, /catalog agent and stays/)
    assert.equal((await api('/api/agents/gefjon', { method: 'DELETE' })).status, 400)
    assert.equal(
      (await api('/api/agents/gefjon/reset', { method: 'POST', body: '{}' })).status,
      401,
      'no such route: nothing behind the token either',
    )
    const added = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'gefjon', harness: 'codex', model: 'm' }),
    })
    assert.equal(added.status, 400)
    assert.match((await added.json()).error, /catalog agent: pick another name/)
    const stored = existsSync(rosterPath(t.env))
      ? JSON.parse(readFileSync(rosterPath(t.env), 'utf8')).agents
      : []
    assert.equal(
      stored.some((row) => row.id === 'gefjon'),
      false,
      'nothing about it was written',
    )
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
