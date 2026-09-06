import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { readFile, writeFile } from 'node:fs/promises'
import { delimiter, isAbsolute, join } from 'node:path'
import { createInterface } from 'node:readline'
import { after, before, describe, it } from 'node:test'
import { setTimeout as delay } from 'node:timers/promises'
import { childEnv } from '../hosts/lib/runners.js'
import {
  checkScope,
  controllerEnv,
  endLaunch,
  issueTicket,
  leadEnv,
  redeem,
  scopedToken,
  shellEnv,
} from '../src/launch.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

const LEAD_OPS = ['consult', 'say', 'attach', 'read', 'seen', 'notify.lead', 'panes']
const CONTROLLER_OPS = ['session.bind', 'progress.set', 'sent.record']
const APP_KEYS = [
  'CONSENSFLOW_APP',
  'CONSENSFLOW_APP_TOKEN',
  'CONSENSFLOW_TAB',
  'CONSENSFLOW_PANE_ID',
  'CONSENSFLOW_LEAD_ID',
  'CONSENSFLOW_LAUNCH',
]

function ownership(suffix = 'one') {
  return {
    tab: `tab-${suffix}`,
    pane: `pane-${suffix}`,
    lead: `tab:tab-${suffix}:3`,
    requester: `requester-${suffix}`,
    conversation: `conversation-${suffix}`,
    generation: 3,
  }
}

describe('launch roles expose only their authority', () => {
  it('builds the exact lead, controller and shell environment shapes', () => {
    const app = { url: 'http://127.0.0.1:43210', token: 'page-ui-token' }
    const env = leadEnv({
      tab: 'tab-a',
      pane: 'pane-lead',
      leadId: 'tab:tab-a:4',
      app,
    })

    assert.deepEqual(
      Object.keys(env).sort(),
      [
        'CONSENSFLOW_APP',
        'CONSENSFLOW_APP_TOKEN',
        'CONSENSFLOW_LEAD_ID',
        'CONSENSFLOW_PANE_ID',
        'CONSENSFLOW_TAB',
        'PATH',
      ].sort(),
    )
    assert.equal(env.CONSENSFLOW_APP, app.url)
    assert.notEqual(env.CONSENSFLOW_APP_TOKEN, app.token, 'the page token is never a lead token')
    assert.equal(env.CONSENSFLOW_LEAD_ID, 'tab:tab-a:4')
    assert.equal(env.CONSENSFLOW_TAB, 'tab-a')
    assert.equal(env.CONSENSFLOW_PANE_ID, 'pane-lead')
    assert.equal(env.CONSENSFLOW_CHILD, undefined)

    const firstPath = env.PATH.split(delimiter)[0]
    assert.equal(firstPath, join(import.meta.dirname, '..', 'bin'))
    assert.equal(isAbsolute(firstPath), true, 'the bundle bin path is absolute')
    for (const op of LEAD_OPS) {
      assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'tab-a', op }), true, op)
    }
    assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'tab-a', op: 'session.bind' }), false)

    assert.deepEqual(controllerEnv({ pane: 'pane-worker', app, ticket: 'single-use-ticket' }), {
      CONSENSFLOW_APP: app.url,
      CONSENSFLOW_LAUNCH: 'single-use-ticket',
      CONSENSFLOW_PANE_ID: 'pane-worker',
    })
    assert.deepEqual(shellEnv(), {})
  })

  it('strips every app credential before the harness child is spawned', () => {
    const base = Object.fromEntries(APP_KEYS.map((key) => [key, `base-${key}`]))
    const env = childEnv(
      { ...base, PATH: '/bin', CONSENSFLOW_CHILD: '1', UNRELATED: 'kept' },
      {
        env: {
          CONSENSFLOW_APP: 'override-app',
          CONSENSFLOW_APP_TOKEN: 'override-token',
        },
      },
    )

    for (const key of APP_KEYS) assert.equal(env[key], undefined, `${key} must be stripped`)
    assert.equal(env.CONSENSFLOW_CHILD, '1', 'the existing recursion guard remains')
    assert.equal(env.UNRELATED, 'kept')
    assert.equal(env.PATH, '/bin')
  })

  it('does not read ambient process.env while constructing roles or credentials', () => {
    const source = readFileSync(join(import.meta.dirname, '..', 'src', 'launch.js'), 'utf8')
    assert.doesNotMatch(source, /process\.env/)
  })

  it('mints a lead credential without accepting the page token', () => {
    const env = leadEnv({
      tab: 'tab-a',
      pane: 'pane-lead',
      leadId: 'tab:tab-a:4',
      app: { url: 'http://127.0.0.1:43210' },
    })

    assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'tab-a', op: 'consult' }), true)
  })
})

describe('launch tickets and scoped credentials', () => {
  it('redeems a ticket once into ownership and a launch-scoped controller capability', () => {
    const expected = ownership('single')
    const ticket = issueTicket(expected)

    assert.equal(typeof ticket, 'string')
    assert.ok(ticket.length >= 32)
    const redeemed = redeem(ticket)
    assert.deepEqual(
      { ...redeemed, launch: '<launch>', capability: '<capability>' },
      { ...expected, launch: '<launch>', capability: '<capability>' },
    )
    assert.equal(typeof redeemed.capability, 'string')
    assert.notEqual(redeemed.capability, ticket)
    assert.notEqual(redeemed.launch, ticket, 'the public launch id is not the bearer ticket')

    for (const op of CONTROLLER_OPS) {
      assert.equal(
        checkScope(redeemed.capability, {
          launch: redeemed.launch,
          generation: expected.generation,
          op,
        }),
        true,
        op,
      )
    }
    assert.equal(
      checkScope(redeemed.capability, {
        launch: 'another-launch',
        generation: expected.generation,
        op: 'session.bind',
      }),
      false,
    )
    assert.equal(
      checkScope(redeemed.capability, {
        launch: redeemed.launch,
        generation: expected.generation + 1,
        op: 'session.bind',
      }),
      false,
    )
    assert.equal(
      checkScope(redeemed.capability, {
        launch: redeemed.launch,
        generation: expected.generation,
        op: 'consult',
      }),
      false,
    )

    assert.throws(() => redeem(ticket), /already used|invalid/i)
    assert.equal(endLaunch(redeemed.launch), true)
    assert.equal(
      checkScope(redeemed.capability, {
        launch: redeemed.launch,
        generation: expected.generation,
        op: 'session.bind',
      }),
      false,
      'ending the launch revokes its capability',
    )

    const cancelled = issueTicket(ownership('cancelled'))
    assert.equal(endLaunch(cancelled), true)
    assert.throws(() => redeem(cancelled), /already used|invalid/i)
  })

  it('refuses an expired ticket', async () => {
    const ticket = issueTicket(ownership('expired'), { ticketMs: 5 })
    await delay(20)
    assert.throws(() => redeem(ticket), /expired/i)
  })

  it('sweeps expired tickets when issuing and redeeming', async () => {
    const sweptOnIssue = issueTicket(ownership('swept-on-issue'), { ticketMs: 5 })
    await delay(20)
    const live = issueTicket(ownership('live'))
    assert.throws(() => redeem(sweptOnIssue), /already used|invalid/i)
    assert.equal(endLaunch(live), true)

    const redeemedExpired = issueTicket(ownership('redeemed-expired'), { ticketMs: 5 })
    const sweptOnRedeem = issueTicket(ownership('swept-on-redeem'), { ticketMs: 5 })
    await delay(20)
    assert.throws(() => redeem(redeemedExpired), /expired/i)
    assert.throws(() => redeem(sweptOnRedeem), /already used|invalid/i)
  })

  it('refuses a token outside its tab or named operations', () => {
    const token = scopedToken({ tab: 'tab-a', ops: LEAD_OPS })

    assert.equal(checkScope(token, { tab: 'tab-a', op: 'consult' }), true)
    assert.equal(checkScope(token, { tab: 'tab-b', op: 'consult' }), false)
    assert.equal(checkScope(token, { tab: 'tab-a', op: 'session.bind' }), false)
  })
})

async function nextLine(lines, stderr) {
  let timer
  try {
    return await Promise.race([
      lines.next().then(({ value, done }) => {
        if (done) throw new Error(`cf ui exited before its handle line: ${stderr()}`)
        return value
      }),
      new Promise((_, reject) => {
        timer = setTimeout(() => {
          reject(new Error(`cf ui did not print a handle line: ${stderr()}`))
        }, 10_000)
      }),
    ])
  } finally {
    clearTimeout(timer)
  }
}

async function spawnScopedServer() {
  const t = tempEnv()
  chooseCmuxMode(t)
  const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
  const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
    env: { ...t.env, PATH: [t.env.PATH, '/usr/bin', '/bin'].join(delimiter) },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let stderr = ''
  child.stderr.on('data', (chunk) => {
    stderr += chunk
  })
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]()

  try {
    const handle = JSON.parse(await nextLine(lines, () => stderr))
    return {
      ...handle,
      home: t.env.CONSENSFLOW_HOME,
      workspace: join(t.root, 'workspace'),
      async close() {
        lines.return?.()
        if (child.exitCode === null && child.signalCode === null) {
          const exited = new Promise((resolve) => child.once('exit', resolve))
          child.kill()
          await exited
        }
        t.cleanup()
      },
    }
  } catch (cause) {
    child.kill()
    t.cleanup()
    throw cause
  }
}

describe('the real loopback server enforces role scope before pane routes exist', () => {
  let server

  before(async () => {
    server = await spawnScopedServer()
    const created = await api(server.token, '/api/tabs', {
      method: 'POST',
      body: { dir: server.workspace, harness: 'pi' },
    })
    assert.equal(created.status, 201)
    server.created = await created.json()

    const issued = await api(server.token, '/api/launch', {
      method: 'POST',
      body: {
        tab: server.created.tab.id,
        pane: server.created.tab.panes[0].id,
        conversation: 'zeus-coral-lane',
      },
    })
    assert.equal(issued.status, 201)
    server.issued = await issued.json()

    const redeemed = await api(null, '/api/launch/redeem', {
      method: 'POST',
      body: { ticket: server.issued.ticket },
    })
    assert.equal(redeemed.status, 200)
    server.controller = await redeemed.json()
  })
  after(async () => {
    await server.close()
  })

  function api(token, path, { method = 'GET', body } = {}) {
    return fetch(`${server.url.replace(/\/$/, '')}${path}`, {
      method,
      headers: {
        ...(token === null ? {} : { authorization: `Bearer ${token}` }),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
  }

  it('mints both launch roles inside the server through UI-token-only routes', async () => {
    assert.deepEqual(Object.keys(server.created).sort(), ['leadEnv', 'tab'])
    assert.equal(server.created.tab.directory, server.workspace)
    assert.equal(server.created.tab.lead.harness, 'pi')
    assert.equal(server.created.leadEnv.CONSENSFLOW_TAB, server.created.tab.id)
    assert.equal(server.created.leadEnv.CONSENSFLOW_PANE_ID, server.created.tab.panes[0].id)
    assert.notEqual(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, server.token)

    assert.deepEqual(Object.keys(server.issued).sort(), ['controllerEnv', 'ticket'])
    assert.deepEqual(server.issued.controllerEnv, {
      CONSENSFLOW_APP: server.url,
      CONSENSFLOW_LAUNCH: server.issued.ticket,
      CONSENSFLOW_PANE_ID: server.created.tab.panes[0].id,
    })
    assert.notEqual(server.controller.launch, server.issued.ticket)
    assert.equal(server.controller.tab, server.created.tab.id)
    assert.equal(server.controller.pane, server.created.tab.panes[0].id)
    assert.equal(server.controller.requester, server.created.tab.panes[0].id)
    assert.equal(typeof server.controller.capability, 'string')

    const second = await api(null, '/api/launch/redeem', {
      method: 'POST',
      body: { ticket: server.issued.ticket },
    })
    assert.equal(second.status, 401, 'the HTTP redemption is single-use too')

    assert.equal(
      (
        await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/tabs', {
          method: 'POST',
          body: { dir: server.workspace, harness: 'pi' },
        })
      ).status,
      403,
      'only the page token may create tabs',
    )
    assert.equal(
      (
        await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/launch', {
          method: 'POST',
          body: {
            tab: server.created.tab.id,
            pane: server.created.tab.panes[0].id,
            conversation: 'forged-launch',
          },
        })
      ).status,
      403,
      'only the page token may issue launch tickets',
    )
  })

  it('redeems tickets only by POST body and never accepts scoped credentials in URLs', async () => {
    const issued = await api(server.token, '/api/launch', {
      method: 'POST',
      body: {
        tab: server.created.tab.id,
        pane: server.created.tab.panes[0].id,
        conversation: 'zeus-hidden-ticket',
      },
    })
    assert.equal(issued.status, 201)
    const launch = await issued.json()

    assert.equal((await api(null, `/api/launch/${launch.ticket}`)).status, 401)
    assert.equal(
      (
        await api(null, '/api/launch/redeem', {
          method: 'POST',
          body: { ticket: launch.ticket },
        })
      ).status,
      200,
      'the rejected GET must not consume the ticket',
    )

    const leadQuery = `/api/panes/consult?token=${encodeURIComponent(server.created.leadEnv.CONSENSFLOW_APP_TOKEN)}`
    assert.equal(
      (
        await api(null, leadQuery, {
          method: 'POST',
          body: { tab: server.created.tab.id, agent: 'zeus', task: 'review this' },
        })
      ).status,
      401,
    )
  })

  it('keeps existing UI-token routes working but refuses that token on pane routes', async () => {
    assert.equal((await api(server.token, '/api/agents')).status, 200)
    assert.equal((await api(server.token, '/api/panes')).status, 403)
    assert.equal((await api(server.token, '/api/panes/consult')).status, 403)
  })

  it('accepts a valid lead operation up to the not-yet-implemented route', async () => {
    const response = await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/panes/consult', {
      method: 'POST',
      body: { tab: server.created.tab.id, agent: 'zeus', task: 'review this' },
    })

    assert.equal(response.status, 501)
    assert.deepEqual(await response.json(), { error: 'not yet' })
  })

  it('refuses a lead token on UI-only and controller operations', async () => {
    for (const [path, options] of [
      ['/api/agents', {}],
      ['/api/mode', { method: 'POST', body: { mode: 'cmux' } }],
      ['/api/reset', { method: 'POST', body: { confirm: true } }],
    ]) {
      assert.equal(
        (await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, path, options)).status,
        403,
        path,
      )
    }

    for (const op of CONTROLLER_OPS) {
      const response = await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, `/api/panes/${op}`, {
        method: 'POST',
        body: { launch: server.controller.launch, generation: server.controller.generation },
      })
      assert.equal(response.status, 403, op)
    }
  })

  it('rejects caller-supplied identity and a foreign tab as bad requests', async () => {
    for (const field of ['by', 'lead', 'owner']) {
      const response = await api(
        server.created.leadEnv.CONSENSFLOW_APP_TOKEN,
        '/api/panes/consult',
        {
          method: 'POST',
          body: {
            tab: server.created.tab.id,
            agent: 'zeus',
            task: 'review this',
            [field]: 'forged',
          },
        },
      )
      assert.equal(response.status, 400, field)
    }

    const foreign = await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/panes/consult', {
      method: 'POST',
      body: { tab: 'tab-foreign', agent: 'zeus', task: 'review this' },
    })
    assert.equal(foreign.status, 400)
  })

  it('rejects every non-object JSON request body before scope checks', async () => {
    for (const body of [[1, 2], 'a string', null, 42]) {
      const response = await api(
        server.created.leadEnv.CONSENSFLOW_APP_TOKEN,
        '/api/panes/consult',
        { method: 'POST', body },
      )
      assert.equal(response.status, 400, JSON.stringify(body))
    }
  })

  it('accepts a controller capability only for its launch, generation and operations', async () => {
    const { capability, launch, generation } = server.controller
    for (const op of CONTROLLER_OPS) {
      const response = await api(capability, `/api/panes/${op}`, {
        method: 'POST',
        body: { launch, generation },
      })
      assert.equal(response.status, 501, op)
    }

    assert.equal(
      (
        await api(capability, '/api/panes/session.bind', {
          method: 'POST',
          body: { launch: 'foreign-launch', generation },
        })
      ).status,
      403,
    )
    assert.equal(
      (
        await api(capability, '/api/panes/session.bind', {
          method: 'POST',
          body: { launch, generation: generation + 1 },
        })
      ).status,
      403,
    )
    assert.equal(
      (
        await api(capability, '/api/panes/consult', {
          method: 'POST',
          body: { launch, generation },
        })
      ).status,
      403,
    )
  })

  it('refuses scoped pane calls that omit their credential dimensions', async () => {
    assert.equal(
      (
        await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/panes/consult', {
          method: 'POST',
          body: { agent: 'zeus', task: 'review this' },
        })
      ).status,
      403,
      'a lead operation must bind to the token tab',
    )

    for (const [field, value] of [
      ['launch', server.controller.launch],
      ['generation', server.controller.generation],
    ]) {
      const response = await api(
        server.created.leadEnv.CONSENSFLOW_APP_TOKEN,
        '/api/panes/consult',
        {
          method: 'POST',
          body: { tab: server.created.tab.id, agent: 'zeus', task: 'review this', [field]: value },
        },
      )
      assert.equal(response.status, 403, field)
    }

    for (const body of [
      {},
      { launch: server.controller.launch },
      { generation: server.controller.generation },
    ]) {
      const response = await api(server.controller.capability, '/api/panes/session.bind', {
        method: 'POST',
        body,
      })
      assert.equal(response.status, 403, JSON.stringify(body))
    }
  })

  it('classifies credential, validation and internal failures', async () => {
    const badTicket = await api(null, '/api/launch/redeem', {
      method: 'POST',
      body: { ticket: 'not-a-ticket' },
    })
    assert.equal(badTicket.status, 401)
    assert.deepEqual(await badTicket.json(), { error: 'unauthorized' })

    const badRequest = await api(server.token, '/api/launch', {
      method: 'POST',
      body: { tab: 'missing-tab', pane: 'missing-pane', conversation: 'zeus-missing-tab' },
    })
    assert.equal(badRequest.status, 400)

    const tabsPath = join(server.home, 'app', 'tabs.json')
    const tabs = JSON.parse(await readFile(tabsPath, 'utf8'))
    const tab = tabs.tabs.find((candidate) => candidate.id === server.created.tab.id)
    tab.panes[0].kind = 'worker'
    await writeFile(tabsPath, `${JSON.stringify(tabs, null, 2)}\n`)

    const invariant = await api(server.token, '/api/launch', {
      method: 'POST',
      body: {
        tab: server.created.tab.id,
        pane: server.created.tab.panes[0].id,
        conversation: 'zeus-no-lead',
      },
    })
    assert.equal(invariant.status, 500)
    assert.deepEqual(await invariant.json(), { error: 'internal_error' })
  })
})
