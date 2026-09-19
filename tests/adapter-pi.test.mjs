import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { piAdapter } from '../src/adapters/pi.js'

/**
 * The Pi adapter (TEST-BDC-05, IMPL-BDC-07): Pi runs with ConsensFlow's
 * extension, on a session name of ours, with the first message as its last
 * argument; later messages go through the extension's inbox, and the
 * extension's own settled marker says when a turn is over.
 */
async function withHome(fn) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-pi-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    PATH: path.join(root, 'bin'),
  }
  await mkdir(env.PATH, { recursive: true })
  const executable = path.join(env.PATH, 'pi')
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
  harness: 'pi',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  directory: '/work/app',
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
  agent: { model: 'opencode-go/muse-spark-1.3-contributor', thinking: 'high' },
  ...overrides,
})

describe('the Pi adapter', () => {
  it('launches Pi with its extension, a session name of ours and the task last', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await piAdapter({ env }).prepare(request())
      assert.match(plan.nativeSession, /^cf-1-zeus-[0-9a-f]{8}$/)
      const extension = plan.argv[2]
      assert.match(
        extension,
        /extensions\/pi\/[0-9a-f]+\/hosts\/pi-extension\/consensflow-delivery\.mjs$/,
      )
      assert.deepEqual(plan.argv, [
        executable,
        '--extension',
        extension,
        '--session-id',
        plan.nativeSession,
        '--model',
        'opencode-go/muse-spark-1.3-contributor',
        '--thinking',
        'high',
        '--approve',
        '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
      ])
      const root = path.join(env.CONSENSFLOW_HOME, 'integrations', 'pi', 'launch-1')
      assert.equal(plan.env.CF_DELIVERY_INBOX, path.join(root, 'inbox'))
      assert.equal(plan.env.CF_DELIVERY_SETTLED, path.join(root, 'settled'))
      assert.equal(plan.env.CF_DELIVERY_LAUNCH_ID, 'launch-1')
      assert.equal(plan.env.CONSENSFLOW_CHILD, undefined, 'cf stays usable inside the window')
    })
  })

  it('resumes the session it has', async () => {
    await withHome(async ({ env }) => {
      const plan = await piAdapter({ env }).prepare(
        request({ resume: 'cf-1-zeus-0000abcd', message: null }),
      )
      assert.equal(plan.nativeSession, 'cf-1-zeus-0000abcd')
      assert.deepEqual(plan.argv.slice(3), [
        '--session-id',
        'cf-1-zeus-0000abcd',
        '--model',
        'opencode-go/muse-spark-1.3-contributor',
        '--approve',
      ])
    })
  })

  it('delivers through the extension inbox with the window input epoch', async () => {
    await withHome(async ({ env }) => {
      const sent = []
      const adapter = piAdapter({
        env,
        send: async (target, text) => {
          sent.push([target, text])
          return { ok: true, admitted: true }
        },
      })
      const { launch } = await adapter.prepare(request())
      const host = { request: async () => ({ ok: true, inputEpoch: 3 }) }
      const pane = { id: 's1-zeus', generation: 2 }
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
        admitted: true,
      })
      const [target] = sent[0]
      assert.deepEqual(
        { ...target, launch: target.launch.kind, bridge: target.bridge === host },
        { launch: 'pi-extension', session: launch.nativeSession, bridge: true, pane, epoch: 3 },
      )
    })
  })

  it("reads each turn's end from the extension's settled marker", async () => {
    await withHome(async ({ env }) => {
      const calls = []
      const adapter = piAdapter({
        env,
        answers: async (...call) => {
          calls.push(call)
          return { items: [], inFlight: false, settlement: { state: 'unknown' } }
        },
      })
      const { launch } = await adapter.prepare(request())
      assert.equal((await adapter.observe({ launch })).settled, true)
      const [kind, session, , options] = calls[0]
      assert.deepEqual([kind, session], ['pi', launch.nativeSession])
      assert.deepEqual(options.piSettlement, {
        directory: path.join(env.CONSENSFLOW_HOME, 'integrations', 'pi', 'launch-1', 'settled'),
        launchId: 'launch-1',
      })
    })
  })
})
