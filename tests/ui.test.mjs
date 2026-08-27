import assert from 'node:assert/strict'
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { Script } from 'node:vm'
import { listAgents } from '../src/roster.js'
import { startUiServer } from '../src/ui.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('a host program can start the editor and be told where it is', () => {
  it('prints one machine-readable line, then serves', async () => {
    const t = tempEnv()
    chooseCmuxMode(t)
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
    chooseCmuxMode(t)
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
    // Deliberate still, just in-page: arm on the first click, act on the second.
    assert.match(script, /Click again to turn off/)
    assert.match(script, /Click again to destroy/)
  })

  it('explains the commands, and how the lead follows a window', async () => {
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()

    for (const verb of ['cf run @name', 'cf sessions', 'cf catchup', 'cf last', 'cf attach']) {
      assert.ok(html.includes(verb), `the page should explain ${verb}`)
    }
    // The one thing a reader must know: a consult IS the agent's window now,
    // and the coding agent follows it through the harness's own session.
    assert.match(html, /own\s+window/i)
    assert.match(html, /--wait/)
  })

  it('serves a page that can do the whole job, not half of it', async () => {
    const html = await (await fetch(`${server.url}/?token=${server.token}`)).text()
    // Everything the CLI can do has an affordance here.
    for (const marker of [
      'id="integrations"',
      'id="update"',
      'id="terminal"',
      'id="off"',
      'Edit',
    ]) {
      assert.ok(html.includes(marker), `the page is missing ${marker}`)
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
      body: JSON.stringify({ model: 'claude-fable-5' }),
    })
    assert.equal(edited.status, 200)
    assert.equal(listAgents(t.env)[0].model, 'claude-fable-5')

    const listed = await api('/api/agents')
    const payload = await listed.json()
    assert.equal(payload.agents.length, 1)
    // Permission is gone from the product: no select, no field, no API.
    assert.equal(payload.permissions, undefined)
    assert.ok(Array.isArray(payload.catalog.claude))
  })

  it('installs and regenerates the skill on every change — no separate step', async () => {
    // Choosing the path is what installs; from then on every roster change
    // keeps it current with no separate step.
    await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode: 'cmux' }) })
    await api('/api/agents/zeus', {
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
      system.harnesses.map((a) => a.id),
      ['claude'],
    )
    assert.equal(system.harnesses[0].native, false)
    assert.equal(typeof system.skills.owned, 'number')
    assert.equal(system.agents, 1)
  })

  it('reports the mode and what it means for the machine', async () => {
    const res = await api('/api/system')
    const system = await res.json()

    // Whatever this suite has chosen by now, the report describes it: the
    // no-mode case has its own test in the CLI suite.
    assert.ok(system.mode.current === null || system.mode.available.includes(system.mode.current))
    assert.equal(system.mode.labels.cmux, 'cmux (pi, cc, codex, opencode, kimi)')
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
    // A mode is a scope, and the page says so. It used to promise Claude Code
    // "your live conversation as context", which nothing has delivered since
    // the session stashing was deleted.
    assert.match(byId.claude.summary, /only claude code/i)
    assert.match(byId.cmux.summary, /every coding harness/i)
    for (const integration of system.integrations) {
      assert.doesNotMatch(integration.summary, /conversation/i, 'no promise nothing keeps')
      // Every card says something about THIS machine — which harnesses the
      // mode would reach, or that it would reach none. "not the active mode"
      // next to a Use-this button tells the reader nothing they cannot see.
      assert.ok(Array.isArray(integration.reach), 'each mode reports its reach')
      assert.doesNotMatch(integration.detail, /not the active mode/)
    }
    const claude = byId.claude
    assert.deepEqual(claude.reach, ['claude'], 'a host mode reaches exactly its harness')
    assert.ok(byId.cmux.reach.length >= claude.reach.length, 'cmux reaches at least as many')
  })

  it('counts a host integration as installed, not just skill files', async () => {
    const system = await (await api('/api/system')).json()
    for (const integration of system.integrations) {
      assert.equal(typeof integration.files, 'number')
    }
  })

  it('installs and updates the skills from the page', async () => {
    const res = await api('/api/skills/install', { method: 'POST', body: JSON.stringify({}) })
    assert.equal(res.status, 200)
    const body = await res.json()

    // Installed, updated, or already exactly right — all three are success.
    // An install that changes nothing reports `unchanged`, not a silent pass.
    assert.ok(body.report.length > 0, 'the install reported on every target')
    assert.ok(
      body.report.every((r) => ['installed', 'updated', 'unchanged'].includes(r.action)),
      'nothing was refused',
    )
    assert.ok(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      'the generated skill is on disk',
    )
  })

  it('installs no cmux skills from any button — ConsensFlow ships one skill', async () => {
    // The cloning era installed cmux's skills tree here; the buttons now only
    // take back what it left. A git on PATH that would clone happily proves
    // nothing reaches for it.
    const git = join(t.env.PATH, 'git')
    writeFileSync(git, '#!/bin/sh\necho "git should never run" >&2\nexit 1\n')
    chmodSync(git, 0o755)

    const body = await (
      await api('/api/skills/install', { method: 'POST', body: JSON.stringify({}) })
    ).json()

    assert.equal(body.cmuxCommit, null, 'there is no cmux clone to report')
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')),
      false,
      'no pane-control tree appears',
    )
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
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

  it('can put the terminal command on PATH, and take it back', async () => {
    // cmux mode installs the launcher itself (its skill says `cf run`), so the
    // page's own toggle is tested from a known state rather than an assumed one.
    await api('/api/terminal-command', { method: 'POST', body: JSON.stringify({ remove: true }) })
    const before = await (await api('/api/system')).json()
    assert.equal(before.terminal.installed, false)

    const installed = await (
      await api('/api/terminal-command', { method: 'POST', body: JSON.stringify({}) })
    ).json()
    assert.equal(installed.system.terminal.installed, true)
    assert.match(installed.system.terminal.path, /consensflow$/)

    const removed = await (
      await api('/api/terminal-command', { method: 'POST', body: JSON.stringify({ remove: true }) })
    ).json()
    assert.equal(removed.system.terminal.installed, false)
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
    // Off cleared everything, mode included: choosing a path again is what
    // puts the skill back, so removal has a target.
    await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode: 'cmux' }) })
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

describe('the page can reset the machine, and says what that destroys', () => {
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

  it('offers the reset as its own button, not a variant of off', async () => {
    const page = await (await api('/')).text()

    assert.match(page, /id="reset"/)
    // It has to name what only this button destroys, or it reads as a louder
    // "off" and someone loses a roster they typed by hand. The host cannot be
    // asked for a dialog here, so the warning lives on the button itself and
    // the arming label counts the cost before the second click.
    assert.match(page, /cannot be undone/i)
    assert.match(page, /run artifact/i)
    assert.match(page, /Click again to destroy/)
  })

  it('refuses without a deliberate confirmation', async () => {
    const refused = await api('/api/reset', { method: 'POST', body: JSON.stringify({}) })

    assert.equal(refused.status, 400)
    assert.match((await refused.json()).error, /confirm/)
  })

  it('removes everything and reports what it counted', async () => {
    await api('/api/agents', {
      method: 'POST',
      body: JSON.stringify({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }),
    })
    await api('/api/mode', { method: 'POST', body: JSON.stringify({ mode: 'claude' }) })
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))

    const done = await api('/api/reset', {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    })

    assert.equal(done.status, 200)
    const body = await done.json()
    assert.equal(body.removed.agents, 1)
    assert.match(body.report.join(' '), /1 agent/)
    assert.equal(body.system.mode.current, null)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
    assert.equal(existsSync(t.env.CONSENSFLOW_HOME), false, 'the whole root is gone')
  })
})
