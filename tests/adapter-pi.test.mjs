import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { piAdapter } from '../src/adapters/pi.js'
import { fakeExecutable } from './helpers.mjs'

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
  const executable = fakeExecutable(path.join(env.PATH, 'pi'))
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
  harness: 'pi',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
  agent: { model: 'openrouter/meta/muse-spark-1.3', thinking: 'high' },
  ...overrides,
})

describe('the Pi adapter', () => {
  it('launches Pi with its extension, a session name of ours and the task last', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await piAdapter({ env }).prepare(request())
      assert.match(plan.nativeSession, /^cf-1-zeus-[0-9a-f]{8}$/)
      const extension = plan.argv[2]
      assert.match(
        extension.replaceAll('\\', '/'),
        /extensions\/pi\/[0-9a-f]+\/hosts\/pi-extension\/consensflow-delivery\.mjs$/,
      )
      const skill = path.join(
        env.CONSENSFLOW_HOME,
        'roles',
        'worker',
        '.claude',
        'skills',
        'consensflow-worker',
        'SKILL.md',
      )
      assert.deepEqual(plan.argv, [
        executable,
        '--extension',
        extension,
        '--skill',
        skill,
        '--append-system-prompt',
        '# ConsensFlow worker\n\nRole text for the test.',
        '--session-id',
        plan.nativeSession,
        '--model',
        'openrouter/meta/muse-spark-1.3',
        '--thinking',
        'high',
        '--approve',
        '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
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
      assert.deepEqual(plan.argv.slice(7), [
        '--session-id',
        'cf-1-zeus-0000abcd',
        '--model',
        'openrouter/meta/muse-spark-1.3',
        '--approve',
      ])
    })
  })

  it('delivers through the real channel: an epoch claim, then the extension inbox', async () => {
    await withHome(async ({ env }) => {
      const adapter = piAdapter({ env })
      const { launch } = await adapter.prepare(request())
      const claims = []
      const host = {
        async request(op, body) {
          if (op === 'pane.snapshot') return { ok: true, inputEpoch: 3 }
          claims.push([op, body])
          return { ok: true }
        },
      }
      // The extension's part: take the record from the inbox, acknowledge it.
      const { inbox, ack } = launch.channel
      let busy = false
      const extension = setInterval(async () => {
        if (busy) return
        busy = true
        try {
          const names = await readdir(inbox).catch(() => [])
          for (const name of names.filter((n) => n.endsWith('.json'))) {
            const record = JSON.parse(await readFile(path.join(inbox, name), 'utf8'))
            await mkdir(ack, { recursive: true })
            await writeFile(
              path.join(ack, `${record.id}.json`),
              JSON.stringify({ id: record.id, admitted: true, mode: 'tui' }),
            )
            await rm(path.join(inbox, name), { force: true })
            clearInterval(extension)
          }
        } finally {
          busy = false
        }
      }, 5)
      try {
        const pane = { id: 's1-zeus', generation: 2 }
        assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
          admitted: true,
          queued: true,
        })
        assert.deepEqual(claims, [
          ['pane.claim_native_epoch', { pane: 's1-zeus', generation: 2, epoch: 3 }],
        ])
      } finally {
        clearInterval(extension)
      }
    })
  })

  it("reads each turn's end from the extension's settled marker", async () => {
    await withHome(async ({ env }) => {
      const calls = []
      const adapter = piAdapter({
        env,
        answers: async (...call) => {
          calls.push(call)
          return {
            items: [],
            inFlight: false,
            settlement: { state: 'unknown' },
            quota: { state: 'exhausted', resetsAt: '2026-09-22T10:00:00.000Z' },
          }
        },
      })
      const { launch } = await adapter.prepare(request())
      const observed = await adapter.observe({ launch })
      assert.equal(observed.settled, true)
      assert.deepEqual(observed.quota, { state: 'exhausted', resetsAt: '2026-09-22T10:00:00.000Z' })
      const [kind, session, , options] = calls[0]
      assert.deepEqual([kind, session], ['pi', launch.nativeSession])
      assert.deepEqual(options.piSettlement, {
        directory: path.join(env.CONSENSFLOW_HOME, 'integrations', 'pi', 'launch-1', 'settled'),
        launchId: 'launch-1',
      })
    })
  })
})
