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
        '--thinking',
        'high',
        '--approve',
      ])
    })
  })

  it('delivers through the real channel: a claim, then the extension inbox', async () => {
    await withHome(async ({ env }) => {
      const adapter = piAdapter({ env })
      const { launch } = await adapter.prepare(request())
      const claims = []
      const host = {
        async request(op, body) {
          claims.push([op, body])
          return { ok: true }
        },
      }
      // The extension's part: take each record from the inbox, acknowledge it.
      const { inbox, ack } = launch.channel
      const texts = []
      let busy = false
      const extension = setInterval(async () => {
        if (busy) return
        busy = true
        try {
          const names = await readdir(inbox).catch(() => [])
          for (const name of names.filter((n) => n.endsWith('.json'))) {
            const record = JSON.parse(await readFile(path.join(inbox, name), 'utf8'))
            texts.push(record.text)
            await mkdir(ack, { recursive: true })
            await writeFile(
              path.join(ack, `${record.id}.json`),
              JSON.stringify({ id: record.id, admitted: true, mode: 'tui' }),
            )
            await rm(path.join(inbox, name), { force: true })
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
        assert.deepEqual(claims, [['pane.claim', { pane: 's1-zeus', generation: 2 }]])
        // Pi hands the text to its model's API, which refuses half a character too.
        await adapter.deliver({
          launch,
          pane,
          host,
          text: 'half \ud83d of it, \u001b[31mred\u001b[0m and 50%\r60%',
        })
        assert.deepEqual(texts, ['hi', 'half  of it, ␛[31mred␛[0m and 50%␍60%'])
      } finally {
        clearInterval(extension)
      }
    })
  })

  it('follows the window to the conversation a /new or /resume left it on, holding until it names one', async () => {
    await withHome(async ({ env }) => {
      const read = []
      const adapter = piAdapter({
        env,
        answers: async (_kind, session) => {
          read.push(session)
          return {
            items: [{ id: `${session}-1`, role: 'user', text: 'hello' }],
            inFlight: false,
            settlement: { state: 'settled' },
          }
        },
      })
      const { launch } = await adapter.prepare(request())
      const shows = async (sessionId) => {
        await mkdir(launch.channel.settled, { recursive: true })
        await writeFile(
          path.join(launch.channel.settled, 'launch-1.shown.json'),
          JSON.stringify({ launchId: 'launch-1', sessionId }),
        )
      }
      // Until its extension starts, Pi has not said which conversation it shows.
      const unnamed = await adapter.observe({ launch })
      assert.match(unnamed.waiting?.reason ?? '', /Pi has not said/)
      assert.equal(await adapter.ready({ launch }), unnamed.waiting.reason)
      await shows(launch.nativeSession)
      assert.equal((await adapter.observe({ launch })).settled, true)
      assert.equal(await adapter.ready({ launch }), true)

      // /new: the extension says the window shows another conversation.
      const fresh = '0199a6f0-4cc1-7d3e-9f7a-3c5b2e1d0a98'
      await shows(fresh)
      const observed = await adapter.observe({ launch })
      assert.deepEqual(observed.switched, { nativeSession: fresh })
      assert.equal(observed.settled, false)
      assert.equal(await adapter.ready({ launch }), 'the window shows another conversation')

      // The dispatcher follows the window: the new conversation's record is read.
      launch.nativeSession = fresh
      const followed = await adapter.observe({ launch })
      assert.equal(followed.switched, undefined)
      assert.equal(read.at(-1), fresh)
      assert.equal(await adapter.ready({ launch }), true)
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
