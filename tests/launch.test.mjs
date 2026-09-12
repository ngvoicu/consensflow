import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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
  ownerOf,
  receiverEnv,
  redeem,
  scopedToken,
  shellEnv,
} from '../src/launch.js'
import { tempEnv } from './helpers.mjs'

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
  it('receiver credentials are separate, launch-scoped and revoked with their coordinator', () => {
    const app = { url: 'http://127.0.0.1:43210', token: 'page' }
    const env = receiverEnv({
      app,
      tab: 't-1',
      pane: 'p-1',
      launch: 'receiver-launch',
      generation: 2,
      kind: 'pi',
    })
    const config = JSON.parse(env.CF_RESULT_RECEIVER)
    assert.equal(
      checkScope(config.token, {
        tab: 't-1',
        launch: 'receiver-launch',
        generation: 2,
        op: 'receiver',
      }),
      true,
    )
    assert.equal(checkScope(config.token, { tab: 't-other', op: 'receiver' }), false)
    assert.equal(checkScope(config.token, { generation: 1, op: 'receiver' }), false)
    assert.equal(checkScope(config.token, { op: 'consult' }), false)
    const lead = leadEnv({
      app,
      tab: 't-1',
      pane: 'p-1',
      leadId: 'tab:t-1:2',
      node: '/bundle/node',
    })
    assert.equal(checkScope(lead.CONSENSFLOW_APP_TOKEN, { op: 'receiver' }), false)
    assert.equal(childEnv(env).CF_RESULT_RECEIVER, undefined)
    endLaunch('receiver-launch')
    assert.equal(checkScope(config.token, { op: 'receiver' }), false)
  })
  it('builds the exact lead, controller and shell environment shapes', () => {
    const app = { url: 'http://127.0.0.1:43210', token: 'page-ui-token' }
    const env = leadEnv({
      tab: 'tab-a',
      pane: 'pane-lead',
      leadId: 'tab:tab-a:4',
      app,
      node: '/opt/bundle/node',
    })

    assert.deepEqual(
      Object.keys(env).sort(),
      [
        'CONSENSFLOW_APP',
        'CONSENSFLOW_APP_TOKEN',
        'CONSENSFLOW_LEAD_ID',
        'CONSENSFLOW_NODE',
        'CONSENSFLOW_PANE_ID',
        'CONSENSFLOW_TAB',
        'PATH',
      ].sort(),
    )
    // The bundle's `bin/cf` shim runs `cf.mjs` with THIS runtime, never one
    // it finds on PATH: the first entry of PATH shadows a stale global `cf`,
    // and this says which node that shim must use.
    assert.equal(env.CONSENSFLOW_NODE, '/opt/bundle/node')
    assert.equal(isAbsolute(env.CONSENSFLOW_NODE), true)
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

  it('keeps a native channel’s own variables, which are the harness’s to read', () => {
    // The strip list exists to stop a child INHERITING our authority. A
    // channel variable is the opposite: it is what the harness needs to
    // serve the channel it was launched with, so stripping it would
    // silently disable the channel the launch just configured.
    const env = childEnv({
      CONSENSFLOW_APP_TOKEN: 'ours',
      OPENCODE_SERVER_PASSWORD: 'the channel’s',
      PATH: '/bin',
    })
    assert.equal(env.CONSENSFLOW_APP_TOKEN, undefined, 'our authority still goes')
    assert.equal(env.OPENCODE_SERVER_PASSWORD, 'the channel’s', 'the channel’s own stays')
  })

  it('does not read ambient process.env while constructing roles or credentials', () => {
    const source = readFileSync(join(import.meta.dirname, '..', 'src', 'launch.js'), 'utf8')
    assert.doesNotMatch(source, /process\.env/)
  })

  it('refuses a lead environment with no runtime to name', () => {
    const app = { url: 'http://127.0.0.1:43210' }
    assert.throws(
      () => leadEnv({ tab: 'tab-a', pane: 'pane-lead', leadId: 'tab:tab-a:4', app }),
      /runtime/,
      'a shim with no node to run is a shim that finds one on PATH',
    )
    assert.throws(
      () => leadEnv({ tab: 'tab-a', pane: 'pane-lead', leadId: 'tab:tab-a:4', app, node: 'node' }),
      /absolute/,
    )
  })

  it('mints a lead credential without accepting the page token', () => {
    const env = leadEnv({
      tab: 'tab-a',
      pane: 'pane-lead',
      leadId: 'tab:tab-a:4',
      app: { url: 'http://127.0.0.1:43210' },
      node: '/opt/bundle/node',
    })

    assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'tab-a', op: 'consult' }), true)
  })
})

describe('launch tickets and scoped credentials', () => {
  it('redeems a ticket once into ownership and a launch-scoped controller capability', () => {
    const expected = ownership('single')
    // The launch identity is minted WITH the ticket and handed back, so one
    // id serves the store's reservation, Rust's `pane.open` dedupe, the
    // lead's answer and `endLaunch` — nothing has to map between two.
    const { ticket, launch } = issueTicket(expected)

    assert.equal(typeof ticket, 'string')
    assert.ok(ticket.length >= 32)
    assert.equal(typeof launch, 'string')
    assert.notEqual(launch, ticket, 'the launch identity is not the bearer ticket')
    assert.deepEqual(ownerOf(launch), expected, 'the launch knows what it is for before redemption')

    const redeemed = redeem(ticket)
    assert.equal(redeemed.launch, launch, 'redemption names the launch the ticket was issued for')
    assert.deepEqual(
      { ...redeemed, launch: '<launch>', capability: '<capability>' },
      { ...expected, launch: '<launch>', capability: '<capability>' },
    )
    assert.equal(typeof redeemed.capability, 'string')
    assert.notEqual(redeemed.capability, ticket)

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
    assert.equal(ownerOf(redeemed.launch), null, 'an ended launch is for nothing')
    assert.equal(
      checkScope(redeemed.capability, {
        launch: redeemed.launch,
        generation: expected.generation,
        op: 'session.bind',
      }),
      false,
      'ending the launch revokes its capability',
    )

    // A launch ended before anyone redeemed it: the ticket dies with it,
    // rather than staying good until it expires.
    const cancelled = issueTicket(ownership('cancelled'))
    assert.equal(endLaunch(cancelled.launch), true)
    assert.throws(() => redeem(cancelled.ticket), /already used|invalid/i)
    assert.equal(ownerOf(cancelled.launch), null)
  })

  it('refuses an expired ticket', async () => {
    const { ticket } = issueTicket(ownership('expired'), { ticketMs: 5 })
    await delay(20)
    assert.throws(() => redeem(ticket), /expired/i)
  })

  it('sweeps expired tickets when issuing and redeeming', async () => {
    const { ticket: sweptOnIssue } = issueTicket(ownership('swept-on-issue'), { ticketMs: 5 })
    await delay(20)
    const live = issueTicket(ownership('live'))
    assert.throws(() => redeem(sweptOnIssue), /already used|invalid/i)
    assert.equal(endLaunch(live.launch), true)

    const { ticket: redeemedExpired } = issueTicket(ownership('redeemed-expired'), { ticketMs: 5 })
    const { ticket: sweptOnRedeem } = issueTicket(ownership('swept-on-redeem'), { ticketMs: 5 })
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
  const cf = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
  // A piped stdin is what makes this server speak the bridge, and a tab
  // cannot be created without a pane host any more: creating one opens its
  // lead's window. So this plays Rust for exactly that one frame.
  const shims = join(t.root, 'shims')
  mkdirSync(shims, { recursive: true })
  writeFileSync(join(shims, 'pi'), '#!/bin/sh\nexit 0\n')
  chmodSync(join(shims, 'pi'), 0o755)
  const child = spawn(process.execPath, [cf, 'ui', '--json', '--no-open'], {
    env: {
      ...t.env,
      PATH: [shims, t.env.PATH, '/usr/bin', '/bin'].join(delimiter),
    },
    stdio: ['pipe', 'pipe', 'pipe'],
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
      /**
       * Answers the next `pane.open` the server sends, skipping the
       * `state.changed` events the store emits after every mutation.
       */
      async answerNextOpen() {
        for (;;) {
          const frame = JSON.parse(await nextLine(lines, () => stderr))
          if (frame.kind !== 'req') continue
          child.stdin.write(
            `${JSON.stringify({
              v: 1,
              id: frame.id,
              kind: 'res',
              op: frame.op,
              body: { ok: true, id: frame.body.id, generation: frame.body.generation },
            })}\n`,
          )
          return frame
        }
      },
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
    const creating = api(server.token, '/api/tabs', {
      method: 'POST',
      body: { dir: server.workspace, harness: 'pi' },
    })
    await server.answerNextOpen()
    const created = await creating
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

    assert.deepEqual(Object.keys(server.issued).sort(), ['controllerEnv', 'launch', 'ticket'])
    assert.notEqual(server.issued.launch, server.issued.ticket)
    assert.equal(
      server.controller.launch,
      server.issued.launch,
      'redemption names the launch the route already reported',
    )
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

  it('lets a valid lead operation reach the route the scope allows', async () => {
    const response = await api(server.created.leadEnv.CONSENSFLOW_APP_TOKEN, '/api/panes/consult', {
      method: 'POST',
      body: { tab: server.created.tab.id, agent: 'zeus', task: 'review this' },
    })

    // What this suite proves is that the credential was accepted, so the
    // assertion has to name the answer the handler actually gives: any
    // status but 401 and 403 would also pass if every pane route answered
    // 500. This body carries no `opId`, and that is where the handler
    // stops.
    assert.equal(response.status, 400)
    assert.deepEqual(await response.json(), { error: 'opId is required' })
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
      // Accepted as a credential, and then answered by the handler itself:
      // the exact answer, so a route that stopped working could not pass
      // this test. `/api/launch` issues tickets for a conversation with no
      // row, so every controller op gets as far as the store and stops
      // there — except `sent.record`, which wants its `opId` first.
      assert.equal(response.status, 400, op)
      assert.deepEqual(
        await response.json(),
        {
          'sent.record': { error: 'opId is required' },
          'progress.set': { error: 'progress-refused', reason: 'progress state is required' },
          'session.bind': {
            // A refusal the store makes about the request: coded, so the
            // controller can tell it from a fault on the server's side,
            // which answers 500 and says nothing else.
            error: 'bind-refused',
            reason: 'no conversation named zeus-coral-lane in this workspace',
          },
        }[op],
        op,
      )
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

it('PM credentials authorize own advisors and manual lead handoff, never another group', () => {
  const env = leadEnv({
    tab: 'pm-tab',
    pane: 'pm-pane',
    leadId: 'tab:pm-tab:1',
    app: { url: 'http://127.0.0.1:1234', token: 'ui-token' },
    node: process.execPath,
    role: 'pm',
  })
  assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'pm-tab', op: 'lead.send' }), true)
  assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'pm-tab', op: 'lead.read' }), true)
  for (const op of ['consult', 'say', 'attach', 'read', 'results.list', 'panes', 'notify.lead']) {
    assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'pm-tab', op }), true, op)
    assert.equal(checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'parent-tab', op }), false, op)
  }
  assert.equal(
    checkScope(env.CONSENSFLOW_APP_TOKEN, { tab: 'another-tab', op: 'lead.read' }),
    false,
  )
})
