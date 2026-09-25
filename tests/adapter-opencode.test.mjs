import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm } from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { openCodeAdapter } from '../src/adapters/opencode.js'
import { fakeExecutable } from './helpers.mjs'

/**
 * The OpenCode adapter (TEST-BDC-05, IMPL-BDC-07): the conversation is created
 * before the window, the TUI runs its own server with ConsensFlow's plugin, the
 * first message goes through that server and later ones through the plugin.
 * The native calls are stand-ins here; the live bench runs the real OpenCode.
 */
async function withHome(fn) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-opencode-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    PATH: path.join(root, 'bin'),
  }
  await mkdir(env.PATH, { recursive: true })
  const executable = fakeExecutable(path.join(env.PATH, 'opencode'))
  try {
    await fn({ env, executable })
  } finally {
    await rm(root, { recursive: true, force: true })
  }
}

const participant = {
  id: 3,
  projectId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'opencode',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: os.tmpdir(),
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
  agent: { model: 'opencode/muse-spark-1.3-contributor-free', effort: 'high' },
  ...overrides,
})

describe('the OpenCode adapter', () => {
  it('creates the conversation first and opens the TUI on its own server, in full-permission mode', async () => {
    await withHome(async ({ env, executable }) => {
      const created = []
      const adapter = openCodeAdapter({
        env,
        createSession: async (input) => {
          created.push(input)
          return 'ses_abc123'
        },
      })
      const plan = await adapter.prepare(request())
      assert.equal(plan.nativeSession, 'ses_abc123')
      const port = plan.argv[plan.argv.indexOf('--port') + 1]
      assert.match(port, /^\d+$/)
      assert.deepEqual(plan.argv, [
        executable,
        '--port',
        port,
        '--hostname',
        '127.0.0.1',
        '--session',
        'ses_abc123',
        '--model',
        'opencode/muse-spark-1.3-contributor-free',
        '--auto',
      ])
      assert.equal(plan.env.OPENCODE_SERVER_USERNAME, 'opencode')
      assert.equal(plan.env.OPENCODE_SERVER_PASSWORD, plan.launch.channel.password)
      assert.match(
        plan.env.OPENCODE_TUI_CONFIG.replaceAll('\\', '/'),
        /extensions\/opencode\/[0-9a-f]+\/hosts\/opencode-extension\/tui\.json$/,
      )
      assert.equal(JSON.parse(plan.env.CF_OPENCODE_SESSION_BRIDGE).launchId, 'launch-1')
      assert.equal(created.length, 1)
      assert.equal(created[0].cwd, os.tmpdir())
      assert.equal(created[0].configuration.channel.endpoint, plan.launch.channel.endpoint)
    })
  })

  it('resumes the conversation it has without creating another', async () => {
    await withHome(async ({ env }) => {
      let created = 0
      const adapter = openCodeAdapter({
        env,
        createSession: async () => {
          created += 1
          return 'ses_new'
        },
      })
      const plan = await adapter.prepare(request({ resume: 'ses_old', message: null }))
      assert.equal(created, 0)
      assert.equal(plan.nativeSession, 'ses_old')
      assert.deepEqual(plan.argv.slice(5), ['--session', 'ses_old', '--auto'])
    })
  })

  it('submits the first message through its own server once the window is up', async () => {
    await withHome(async ({ env }) => {
      const seeded = []
      const adapter = openCodeAdapter({
        env,
        createSession: async () => 'ses_abc123',
        seedSession: async (input) => {
          seeded.push(input)
        },
      })
      const fresh = await adapter.prepare(request())
      await adapter.started({ launch: fresh.launch })
      assert.deepEqual(
        { ...seeded[0], channel: seeded[0].channel.kind },
        {
          channel: 'opencode-server',
          sessionId: 'ses_abc123',
          cwd: os.tmpdir(),
          text: '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
          model: 'opencode/muse-spark-1.3-contributor-free',
          variant: 'high',
        },
      )
      const resumed = await adapter.prepare(request({ resume: 'ses_old' }))
      await adapter.started({ launch: resumed.launch })
      assert.equal(seeded[1].resume, true, 'a resumed session keeps its own model')
      assert.equal(seeded[1].model, undefined)
      const empty = await adapter.prepare(request({ message: null }))
      await adapter.started({ launch: empty.launch })
      assert.equal(seeded.length, 2, 'a window without a task submits nothing')
    })
  })

  it('delivers through the real channel: an epoch claim, then the plugin', async () => {
    await withHome(async ({ env }) => {
      const posted = []
      let answer = { ok: true, admitted: true }
      const plugin = createServer(async (request, response) => {
        let body = ''
        for await (const chunk of request) body += chunk
        posted.push({
          url: request.url,
          auth: request.headers.authorization,
          body: JSON.parse(body),
        })
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end(JSON.stringify(answer))
      })
      await new Promise((resolve) => plugin.listen(0, '127.0.0.1', resolve))
      try {
        const adapter = openCodeAdapter({ env, createSession: async () => 'ses_abc123' })
        const { launch } = await adapter.prepare(request())
        launch.channel.sessionBridge = {
          endpoint: `http://127.0.0.1:${plugin.address().port}`,
          token: 't'.repeat(32),
        }
        const claims = []
        const host = {
          async request(op, body) {
            if (op === 'pane.snapshot') return { ok: true, inputEpoch: 7 }
            claims.push([op, body])
            return { ok: true }
          },
        }
        const pane = { id: 's1-zeus', generation: 2 }
        assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
          admitted: true,
          queued: true,
        })
        assert.deepEqual(claims, [
          ['pane.claim_native_epoch', { pane: 's1-zeus', generation: 2, epoch: 7 }],
        ])
        assert.equal(posted[0].url, '/deliver')
        assert.equal(posted[0].auth, `Bearer ${'t'.repeat(32)}`)
        assert.deepEqual(
          { ...posted[0].body, expiresAt: typeof posted[0].body.expiresAt },
          { launchId: 'launch-1', sessionId: 'ses_abc123', text: 'hi', expiresAt: 'number' },
        )
        answer = { ok: false, admitted: false, bytesWritten: 0, error: 'native-session-changed' }
        assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
          admitted: false,
          reason: 'native-session-changed',
        })
      } finally {
        await new Promise((resolve) => plugin.close(resolve))
      }
    })
  })

  it('is ready for a message only once its plugin shows the conversation', async () => {
    await withHome(async ({ env }) => {
      let shown
      const adapter = openCodeAdapter({
        env,
        sessionState: async () =>
          shown === undefined ? undefined : { sessionId: shown, status: null },
        answers: async () => ({ items: [], inFlight: false, settlement: { state: 'unknown' } }),
      })
      const launch = { nativeSession: 'ses_abc123', channel: { kind: 'opencode-server' } }
      assert.equal(await adapter.ready({ launch }), false, 'the TUI has not loaded its plugin yet')
      assert.equal((await adapter.observe({ launch })).settled, false)
      shown = 'ses_other'
      assert.equal(await adapter.ready({ launch }), false, 'the human is looking at another one')
      shown = 'ses_abc123'
      assert.equal(await adapter.ready({ launch }), true)
      assert.equal((await adapter.observe({ launch })).settled, true)
    })
  })

  it('reads a conversation with no messages yet as idle, and one mid-turn as working', async () => {
    await withHome(async ({ env }) => {
      let record = { items: [], inFlight: false, settlement: { state: 'unknown' } }
      const adapter = openCodeAdapter({
        env,
        answers: async () => record,
        sessionState: async () => ({ sessionId: 'ses_abc123', status: { type: 'busy' } }),
      })
      const launch = { nativeSession: 'ses_abc123' }
      assert.equal((await adapter.observe({ launch })).settled, true)
      record = {
        items: [{ id: 'u', role: 'user' }],
        inFlight: true,
        settlement: { state: 'in-flight' },
      }
      assert.equal((await adapter.observe({ launch })).settled, false)
      record = {
        items: [{ id: 'u', role: 'user' }],
        inFlight: false,
        settlement: { state: 'settled' },
        quota: { state: 'exhausted', resetsAt: null },
      }
      const observed = await adapter.observe({ launch })
      assert.deepEqual(
        [observed.settled, observed.quota],
        [true, { state: 'exhausted', resetsAt: null }],
      )
    })
  })

  it("reads OpenCode waiting out a usage or rate limit as the member's quota spent", async () => {
    await withHome(async ({ env }) => {
      const now = Date.now()
      let shown = { sessionId: 'ses_abc123', status: { type: 'busy' } }
      const adapter = openCodeAdapter({
        env,
        sessionState: async () => shown,
        answers: async () => ({
          items: [{ id: 'u', role: 'user' }],
          inFlight: true,
          settlement: { state: 'in-flight' },
        }),
      })
      const launch = { nativeSession: 'ses_abc123' }
      const retry = (message, next, action) => ({
        sessionId: 'ses_abc123',
        status: { type: 'retry', attempt: 1, message, next, ...(action ? { action } : {}) },
      })
      assert.equal((await adapter.observe({ launch })).quota, null, 'busy is not a refusal')
      // OpenCode 1.18.31 on a spent free tier: it waits until the reset it names.
      const midnight = now + 5 * 3_600_000
      shown = retry('Free usage exceeded, subscribe to Go', midnight, {
        reason: 'free_tier_limit',
      })
      const spent = await adapter.observe({ launch })
      assert.deepEqual(
        [spent.settled, spent.quota],
        [false, { state: 'exhausted', at: null, resetsAt: new Date(midnight).toISOString() }],
      )
      // A rate limit OpenCode retries in seconds is backoff: the window is at
      // work, and its task stays with it. Only a reset a minute or more away
      // is a spent quota.
      shown = retry('Rate limit exceeded. Please try again later.', now + 4_000)
      const backoff = await adapter.observe({ launch })
      assert.deepEqual([backoff.quota, backoff.settled], [null, false], 'backoff, not a quota')
      shown = retry('Rate limit exceeded. Please try again later.', now + 20 * 60_000)
      assert.deepEqual(
        (await adapter.observe({ launch })).quota,
        { state: 'exhausted', at: null, resetsAt: new Date(now + 20 * 60_000).toISOString() },
        'a retry twenty minutes away is a limit waited out',
      )
      shown = retry('Provider is overloaded', now + 4_000)
      assert.equal((await adapter.observe({ launch })).quota, null, 'an overload is no quota')
      shown = { ...retry('Rate limit exceeded', midnight), sessionId: 'ses_other' }
      assert.equal(
        (await adapter.observe({ launch })).quota,
        null,
        "another conversation's status says nothing about this one",
      )
    })
  })

  it('reads a turn OpenCode never finished as over once OpenCode says the window is idle', async () => {
    await withHome(async ({ env }) => {
      // A window lost mid-answer and reopened on its conversation: the store
      // keeps the unfinished answer for good, and OpenCode does not retry it.
      let status = { type: 'busy' }
      const adapter = openCodeAdapter({
        env,
        sessionState: async () => ({ sessionId: 'ses_abc123', status }),
        answers: async () => ({
          items: [
            { id: 'u', role: 'user', complete: true },
            { id: 'a', role: 'assistant', text: '', complete: false },
          ],
          inFlight: true,
          settlement: { state: 'in-flight' },
        }),
      })
      const launch = { nativeSession: 'ses_abc123' }
      assert.equal((await adapter.observe({ launch })).settled, false, 'busy: still working')
      status = null
      assert.equal(
        (await adapter.observe({ launch })).settled,
        false,
        'no word from the window: the store decides',
      )
      status = { type: 'idle' }
      assert.equal((await adapter.observe({ launch })).settled, true, 'idle: the turn is over')
    })
  })
})
