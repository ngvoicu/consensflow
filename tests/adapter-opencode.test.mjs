import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { openCodeAdapter } from '../src/adapters/opencode.js'

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
  const executable = path.join(env.PATH, 'opencode')
  await writeFile(executable, '#!/bin/sh\nexit 0\n')
  await chmod(executable, 0o755)
  try {
    await fn({ env, executable })
  } finally {
    await rm(root, { recursive: true, force: true })
  }
}

const participant = {
  id: 3,
  sessionId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'opencode',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
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
        plan.env.OPENCODE_TUI_CONFIG,
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

  it('delivers through the plugin with the window input epoch and reports the outcome', async () => {
    await withHome(async ({ env }) => {
      const sent = []
      let outcome = { ok: true }
      const adapter = openCodeAdapter({
        env,
        createSession: async () => 'ses_abc123',
        send: async (target, text) => {
          sent.push([target, text])
          return outcome
        },
      })
      const { launch } = await adapter.prepare(request())
      const host = { request: async () => ({ ok: true, inputEpoch: 7 }) }
      const pane = { id: 's1-zeus', generation: 2 }
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
        admitted: true,
      })
      const [target, text] = sent[0]
      assert.deepEqual(
        { ...target, launch: target.launch.kind, bridge: target.bridge === host },
        { launch: 'opencode-server', session: 'ses_abc123', bridge: true, pane, epoch: 7 },
      )
      assert.equal(text, 'hi')
      outcome = { ok: false, admitted: null, error: 'uncertain' }
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
        admitted: null,
        reason: 'uncertain',
      })
      outcome = { ok: false, admitted: false, error: 'native-session-changed' }
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
        admitted: false,
        reason: 'native-session-changed',
      })
    })
  })

  it('reads a conversation with no messages yet as idle, and one mid-turn as working', async () => {
    await withHome(async ({ env }) => {
      let record = { items: [], inFlight: false, settlement: { state: 'unknown' } }
      const adapter = openCodeAdapter({ env, answers: async () => record })
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
      }
      assert.equal((await adapter.observe({ launch })).settled, true)
    })
  })
})
