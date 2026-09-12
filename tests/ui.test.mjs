import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { Script } from 'node:vm'
import { listAgents } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { tempEnv, testRoleConfiguration } from './helpers.mjs'

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
    server = await startUiServer(t.env, { prepareRole: testRoleConfiguration })
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
    const res = await fetch(`${server.url}/api/agents`)
    assert.equal(res.status, 401)
  })

  it('serves a page whose script actually ran through the template', async () => {
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    // An escaped `\${…}` ships a page that dies on load: the token never
    // interpolates and the browser hits a syntax error before fetching data.
    assert.ok(html.includes(`const TOKEN = "${server.token}"`))
    assert.ok(!html.includes('${JSON.stringify'))
  })

  it('serves a page whose script is valid JavaScript', async () => {
    // The page script is built inside a template literal, so nothing here ever
    // parsed it as code — a stray backslash in one string shipped a window
    // where every JS-rendered section (roster, catalog, mode cards) was blank
    // while the static markup around them looked fine. Compile it, do not run it.
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)].map((m) => m[1])

    assert.ok(scripts.length > 0, 'the page ships at least one script')
    for (const [index, source] of scripts.entries()) {
      if (source.trim().length === 0) continue
      assert.doesNotThrow(() => new Script(source), `script #${index} must parse`)
    }
  })

  it('never asks the host for a dialog it does not implement', async () => {
    // The app is a WKWebView with no dialog panels wired on the Rust side, so
    // window.confirm returns false without showing anything — every
    // confirm-gated button was silently dead there while working in a browser.
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    const script = html.match(/<script\b[^>]*>([\s\S]*?)<\/script>/)[1]

    for (const call of ['confirm(', 'alert(', 'prompt(']) {
      assert.ok(!script.includes(call), `the page must not call ${call}`)
    }
  })

  it('omits the agent-facing command reference from the roster screen', async () => {
    const html = await (await api('/')).text()
    assert.doesNotMatch(
      html,
      /Talking to an agent|class="cmds"|ask hyperion|cf sessions|cf results|cf read|cf attach/,
    )
  })

  it('keeps the Agents page focused on roster editing', async () => {
    const html = await (await api('/')).text()
    for (const marker of [
      'id="roster"',
      'aria-label="Your agents"',
      'Model and reasoning',
      'id="add"',
      'Edit',
    ]) {
      assert.ok(html.includes(marker), `the page is missing ${marker}`)
    }
    assert.doesNotMatch(
      html,
      /id="catalog-section"|Check all harnesses|\/api\/harnesses|\/api\/system|id="system"|Role skills included|Turn off|Reset everything/,
    )
  })

  it('serves Agent library separately with authentication and valid JavaScript', async () => {
    assert.equal((await fetch(`${server.url}/library`)).status, 401)
    const response = await api('/library')
    assert.equal(response.status, 200)
    const html = await response.text()
    assert.match(html, /aria-label="Agent library"/)
    assert.match(html, /id="catalog"/)
    assert.match(html, /offer__actions/)
    assert.doesNotMatch(html, /id="roster-section"|id="add"/)
    for (const [, source] of html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)) {
      assert.doesNotThrow(() => new Script(source))
    }
  })

  it('serves Harnesses as its own authenticated page with valid JavaScript', async () => {
    assert.equal((await fetch(`${server.url}/harnesses`)).status, 401)
    const response = await api('/harnesses')
    assert.equal(response.status, 200)
    const html = await response.text()
    assert.match(html, /<h1>Harnesses<\/h1>/)
    assert.match(html, /Check all harnesses/)
    assert.ok(html.includes(`const TOKEN = "${server.token}"`))
    assert.doesNotMatch(
      html,
      /id="roster"|id="system"|Role skills included|Turn off|Reset everything/,
    )
    for (const [, source] of html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)) {
      assert.doesNotThrow(() => new Script(source))
    }
  })

  it('serves the editor page', async () => {
    const res = await fetch(`${server.url}/?token=${server.token}`)
    assert.equal(res.status, 200)
    const html = await res.text()
    assert.match(html, /ConsensFlow/i)
    assert.match(html, /agent/i)
  })

  it('adds, edits and removes agents through the API, persisting each', async () => {
    const added = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }),
    })
    assert.equal(added.status, 201)
    assert.equal(listAgents(t.env)[0].name, 'zeus')

    const edited = await api('/api/agents/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-fable-5-1' }),
    })
    assert.equal(edited.status, 200)
    assert.equal(listAgents(t.env)[0].model, 'claude-fable-5-1')

    const listed = await api('/api/agents')
    const payload = await listed.json()
    assert.equal(payload.agents.length, 1)
    // Permission is gone from the product: no select, no field, no API.
    assert.equal(payload.permissions, undefined)
    assert.ok(Array.isArray(payload.catalog.claude))
  })

  it('installs and regenerates the skill on every change — no separate step', async () => {
    await api('/api/agents/zeus', {
      method: 'PATCH',
      body: JSON.stringify({ model: 'claude-opus-5' }),
    })

    const installed = readFileSync(
      join(
        t.env.CONSENSFLOW_HOME,
        'roles',
        'lead',
        '.claude',
        'skills',
        'consensflow-lead',
        'SKILL.md',
      ),
      'utf8',
    )
    assert.match(installed, /claude-opus-5/)
  })

  it('retires the unused system status endpoint', async () => {
    assert.equal((await api('/api/system')).status, 404)
  })

  it('retires the mode endpoint without changing installation (TEST-PANE-47)', async () => {
    for (const mode of ['claude', 'pi', 'cmux']) {
      const res = await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode }) })
      assert.equal(res.status, 404)
    }
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'mode.json')), false)
  })

  it('keeps retired mode controls absent (TEST-PANE-47)', async () => {
    const html = await (await api('/')).text()
    assert.doesNotMatch(html, /id="integrations"|id="mode-lede"|\/api\/mode|cmux mode|--wait/)
  })

  it('retires the separate skills installer without creating unrelated files', async () => {
    const response = await api('/api/skills/install', { method: 'POST', body: '{}' })
    assert.equal(response.status, 410)
    assert.match((await response.json()).error, /included with ConsensFlow/)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')),
      false,
    )
  })

  it('offers the catalog update from the page, as a named operation', async () => {
    // A quick-add carries provenance, so the page can later offer the update.
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
    const moved = before.drift.find((d) => d.name === 'diana')
    assert.ok(moved, 'the page is told the catalog moved')
    assert.deepEqual(moved.changes, [
      { field: 'model', from: 'gpt-5.5', to: 'gpt-5.6-luna' },
      // JSON drops an undefined `from`: this row was added with no description.
      { field: 'description', to: 'Codex GPT 5.6 Luna XHIGH' },
    ])

    const synced = await api('/api/agents/sync', {
      method: 'POST',
      body: JSON.stringify({ name: 'diana' }),
    })
    assert.equal(synced.status, 200)
    const body = await synced.json()
    assert.equal(body.applied.length, 1)
    assert.equal(body.agents.find((p) => p.name === 'diana').model, 'gpt-5.6-luna')

    const after = await (await api('/api/agents')).json()
    assert.equal(after.drift.length, 0)
    await api('/api/agents/diana', { method: 'DELETE' })
  })

  it('offers no button for the terminal command — opening the app owns it', async () => {
    // The page used to install and remove the launcher. Removing it is a button
    // that undoes itself now: the app claims the command every time it opens,
    // so the next launch brings it straight back.
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    assert.ok(!html.includes('id="terminal"'), 'no command button')
    assert.ok(!html.includes('Remove terminal command'))
    assert.ok(!html.includes('Nothing here needs it'), 'nor the copy that called it optional')

    const gone = await api('/api/terminal-command', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(gone.status, 404, 'and no endpoint behind it')
  })

  it('removes them again, but only when the click was deliberate', async () => {
    // Opening the app prepares its private role document; the retired UI cannot.
    const { refreshInstalledSkill } = await import('../src/sync.js')
    refreshInstalledSkill(t.env)
    assert.ok(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
    )

    const refused = await api('/api/skills/uninstall', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(refused.status, 400)
    assert.ok(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
    )

    const done = await api('/api/skills/uninstall', {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    })
    assert.equal(done.status, 200)
    assert.equal(
      existsSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
      ),
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
    const res = await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'Bad Name', harness: 'claude', model: 'm' }),
    })
    assert.equal(res.status, 400)
    assert.match((await res.json()).error, /names/)
  })

  it('removes an agent', async () => {
    const res = await api('/api/agents/zeus', { method: 'DELETE' })
    assert.equal(res.status, 204)
    assert.deepEqual(listAgents(t.env), [])
  })
})

describe('retired destructive API routes leave saved data intact', () => {
  for (const route of ['/api/off', '/api/reset']) {
    it(`${route} is absent even with confirmation and force`, async () => {
      const t = tempEnv()
      const { healOnOpen } = await import('../src/sync.js')
      const { addAgent } = await import('../src/roster.js')
      addAgent({ name: 'zeus', harness: 'claude', model: 'example' }, t.env)
      healOnOpen(t.env)
      const saved = [
        join(t.env.CONSENSFLOW_HOME, 'agents.json'),
        join(t.env.CONSENSFLOW_BIN_DIR, 'cf'),
        join(
          t.env.CONSENSFLOW_HOME,
          'roles',
          'lead',
          '.claude',
          'skills',
          'consensflow-lead',
          'SKILL.md',
        ),
        join(t.env.CONSENSFLOW_HOME, 'tabs.json'),
      ]
      writeFileSync(saved[3], '{"version":1,"tabs":[]}')
      const before = saved.map((path) => readFileSync(path))
      const server = await startUiServer(t.env)
      try {
        const response = await fetch(`${server.url}${route}`, {
          method: 'POST',
          headers: { authorization: `Bearer ${server.token}`, 'content-type': 'application/json' },
          body: JSON.stringify({ confirm: true, force: true }),
        })
        assert.equal(response.status, 404)
        assert.deepEqual(
          saved.map((path) => readFileSync(path)),
          before,
        )
      } finally {
        await server.close()
        t.cleanup()
      }
    })
  }
})

it('harness administration is UI-authorized and exposes all harnesses with honest statuses', async () => {
  const t = tempEnv()
  const server = await startUiServer(t.env, { harnessLatest: async () => '99.0.0' })
  try {
    const path = `${server.url}/api/harnesses/check`
    assert.equal((await fetch(path, { method: 'POST' })).status, 401)
    const headers = { authorization: `Bearer ${server.token}`, 'content-type': 'application/json' }
    const response = await fetch(path, { method: 'POST', headers, body: '{}' })
    assert.equal(response.status, 200)
    const { harnesses } = await response.json()
    assert.equal(harnesses.length, 5)
    assert.ok(harnesses.every((h) => h.installed === false))
    const invalid = await fetch(path, {
      method: 'POST',
      headers,
      body: JSON.stringify({ id: 'unknown' }),
    })
    assert.equal(invalid.status, 400)
    const html = await (await fetch(`${server.url}/harnesses`, { headers })).text()
    assert.match(html, /Check all harnesses/)
    assert.doesNotMatch(html, /skills in each of|consults via the generated skill/)
  } finally {
    await server.close()
    t.cleanup()
  }
})

it('role skills ship with the app and have no separate UI installation action', async () => {
  const t = tempEnv()
  const server = await startUiServer(t.env)
  try {
    const headers = { authorization: `Bearer ${server.token}`, 'content-type': 'application/json' }
    const html = await (await fetch(server.url, { headers })).text()
    assert.doesNotMatch(html, /Update skills|id="update"/)
    assert.doesNotMatch(html, /Role skills included in ConsensFlow/)
    const response = await fetch(`${server.url}/api/skills/install`, {
      method: 'POST',
      headers,
      body: '{}',
    })
    assert.equal(response.status, 410)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'roles')), false)
  } finally {
    await server.close()
    t.cleanup()
  }
})

it('API profiles follow edited saved agents without rewriting descriptions or state', async () => {
  const { addAgent, editAgent, rosterPath } = await import('../src/roster.js')
  const t = tempEnv()
  addAgent(
    {
      name: 'renamed',
      harness: 'codex',
      model: 'gpt-6-astra',
      effort: 'medium',
      preset: 'astraeus',
      description: '<my custom description>',
    },
    t.env,
  )
  const server = await startUiServer(t.env)
  const read = async () =>
    (
      await fetch(`${server.url}/api/agents`, {
        headers: { authorization: `Bearer ${server.token}` },
      })
    ).json()
  try {
    const before = readFileSync(rosterPath(t.env), 'utf8')
    const data = await read()
    assert.deepEqual(data.agents[0].profile?.categories, ['coding', 'reviewer'])
    assert.equal(data.agents[0].description, '<my custom description>')
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), before)
    editAgent('renamed', { model: 'custom-model', effort: 'low' }, t.env)
    const edited = readFileSync(rosterPath(t.env), 'utf8')
    const row = (await read()).agents[0]
    assert.deepEqual(row.profile.categories, ['coding'])
    assert.equal(row.profile.modelLabel, 'custom-model')
    assert.equal(row.description, '<my custom description>')
    assert.equal(row.preset, 'astraeus')
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), edited)
  } finally {
    await server.close()
    t.cleanup()
  }
})

it('fills legacy saved profiles without changing operational data and refreshes external edits', async () => {
  const { rosterPath } = await import('../src/roster.js')
  const t = tempEnv()
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  const legacy = {
    id: 'personal',
    name: 'Personal',
    kind: 'pi',
    model: 'anthropic/claude-opus-5',
    thinking: 'medium',
    description: 'My notes',
    createdAt: 'old',
    updatedAt: 'old',
    custom: { keep: true },
  }
  writeFileSync(
    rosterPath(t.env),
    JSON.stringify({ schemaVersion: 1, customDocument: true, agents: [legacy] }),
  )
  const server = await startUiServer(t.env)
  const read = async () =>
    (
      await fetch(`${server.url}/api/agents`, {
        headers: { authorization: `Bearer ${server.token}` },
      })
    ).json()
  try {
    const data = await read()
    const saved = JSON.parse(readFileSync(rosterPath(t.env), 'utf8'))
    assert.deepEqual(saved.agents[0].profile, data.agents[0].profile)
    const { profile, ...rest } = saved.agents[0]
    assert.ok(profile?.routeLabel)
    assert.deepEqual(rest, legacy)
    assert.equal(saved.customDocument, true)
    const stable = readFileSync(rosterPath(t.env), 'utf8')
    await read()
    assert.equal(readFileSync(rosterPath(t.env), 'utf8'), stable)
    saved.agents[0].model = 'openai-codex/gpt-6-astra'
    writeFileSync(rosterPath(t.env), JSON.stringify(saved))
    const edited = await read()
    assert.deepEqual(edited.agents[0].profile.categories, ['coding', 'reviewer'])
    assert.deepEqual(
      JSON.parse(readFileSync(rosterPath(t.env), 'utf8')).agents[0].profile,
      edited.agents[0].profile,
    )
  } finally {
    await server.close()
    t.cleanup()
  }
})
