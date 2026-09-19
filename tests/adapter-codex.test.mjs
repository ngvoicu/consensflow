import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { codexAdapter } from '../src/adapters/codex.js'

/**
 * The Codex adapter (TEST-BDC-05, IMPL-BDC-07): Codex runs under ConsensFlow's
 * supervisor (its app-server, a broker that knows the thread and its queue,
 * and the TUI attached to both), in full-permission mode, with the first
 * message as its last argument. The broker names the thread and queues every
 * later message; a Codex without the native queue gets them pasted.
 */
const SUPERVISOR = fileURLToPath(new URL('../hosts/codex-session.mjs', import.meta.url))

async function withHome(fn, { queue = true } = {}) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-codex-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    PATH: path.join(root, 'bin'),
    CONSENSFLOW_NODE: process.execPath,
  }
  await mkdir(env.PATH, { recursive: true })
  const executable = path.join(env.PATH, 'codex')
  const help = queue ? 'echo "Usage: codex queue --thread <id> --message <text>"' : 'exit 2'
  await writeFile(executable, `#!/bin/sh\nif [ "$1" = queue ]; then ${help}; fi\n`)
  await chmod(executable, 0o755)
  try {
    await fn({ env, executable })
  } finally {
    await rm(root, { recursive: true, force: true })
  }
}

const participant = {
  id: 3,
  projectId: 1,
  handle: 'diana',
  role: 'worker',
  agent: 'diana',
  harness: 'codex',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  directory: '/work/app',
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
  agent: { model: 'gpt-5.6-luna', effort: 'low' },
  ...overrides,
})

describe('the Codex adapter', () => {
  it('runs Codex under its supervisor, in full-permission mode, with the task last', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await codexAdapter({ env }).prepare(request())
      assert.equal(plan.nativeSession, null, 'the broker names the thread')
      assert.deepEqual(plan.argv, [
        process.execPath,
        SUPERVISOR,
        executable,
        '--model',
        'gpt-5.6-luna',
        '-c',
        'model_reasoning_effort="low"',
        '--dangerously-bypass-approvals-and-sandbox',
        '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
      ])
      assert.equal(JSON.parse(plan.env.CF_CODEX_SESSION_BRIDGE).launchId, 'launch-1')
      assert.deepEqual(plan.dropEnv, ['OPENAI_API_KEY'])
    })
  })

  it('resumes its thread', async () => {
    await withHome(async ({ env, executable }) => {
      const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
      const plan = await codexAdapter({ env }).prepare(request({ resume: thread, message: null }))
      assert.equal(plan.nativeSession, thread)
      assert.deepEqual(plan.argv.slice(2), [
        executable,
        'resume',
        thread,
        '--dangerously-bypass-approvals-and-sandbox',
      ])
    })
  })

  it('learns the thread from its broker', async () => {
    await withHome(async ({ env }) => {
      const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
      let asked = 0
      const adapter = codexAdapter({
        env,
        discoverEveryMs: 5,
        currentSession: async () => (++asked < 3 ? null : thread),
      })
      const { launch } = await adapter.prepare(request())
      assert.deepEqual(await adapter.started({ launch }), { nativeSession: thread })
      assert.equal(launch.nativeSession, thread)
    })
  })

  it('queues a message through the real channel: an epoch claim, then the broker', async () => {
    await withHome(async ({ env }) => {
      const posted = []
      const broker = createServer(async (request, response) => {
        let body = ''
        for await (const chunk of request) body += chunk
        posted.push({ url: request.url, body: JSON.parse(body) })
        response.writeHead(200, { 'content-type': 'application/json' })
        response.end(JSON.stringify({ ok: true, admitted: true }))
      })
      await new Promise((resolve) => broker.listen(0, '127.0.0.1', resolve))
      try {
        const adapter = codexAdapter({ env })
        const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
        const { launch } = await adapter.prepare(request({ resume: thread, message: null }))
        launch.channel.sessionBridge = {
          endpoint: `http://127.0.0.1:${broker.address().port}`,
          token: 't'.repeat(32),
        }
        const claims = []
        const host = {
          async request(op, body) {
            if (op === 'pane.snapshot') return { ok: true, inputEpoch: 6 }
            claims.push([op, body])
            return { ok: true }
          },
        }
        const pane = { id: 's1-diana', generation: 2 }
        assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
          admitted: true,
        })
        assert.deepEqual(claims, [
          ['pane.claim_native_epoch', { pane: 's1-diana', generation: 2, epoch: 6 }],
        ])
        assert.equal(posted[0].url, '/deliver')
        assert.deepEqual([posted[0].body.sessionId, posted[0].body.text], [thread, 'hi'])
      } finally {
        await new Promise((resolve) => broker.close(resolve))
      }
    })
  })

  it('pastes into a Codex without the native queue', async () => {
    await withHome(
      async ({ env, executable }) => {
        const adapter = codexAdapter({ env })
        const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
        const plan = await adapter.prepare(request({ resume: thread, message: null }))
        assert.equal(plan.argv[0], executable, 'no supervisor without the queue')
        const requests = []
        const host = {
          async request(op, body) {
            requests.push([op, body])
            return op === 'pane.snapshot'
              ? { ok: true, inputEpoch: 1, draftLatched: false }
              : { ok: true }
          },
        }
        const pane = { id: 's1-diana', generation: 2 }
        assert.deepEqual(await adapter.deliver({ launch: plan.launch, pane, host, text: 'hi' }), {
          admitted: true,
        })
        assert.equal(requests.at(-1)[0], 'pane.write_paste')
      },
      { queue: false },
    )
  })
})
