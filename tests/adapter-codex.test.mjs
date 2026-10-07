import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises'
import { createServer } from 'node:http'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { codexAdapter } from '../src/adapters/codex.js'
import { BUNDLE_CF } from '../src/core/pane-cf.js'
import { fakeNodeExecutable } from './helpers.mjs'

/**
 * The Codex adapter (TEST-BDC-05, IMPL-BDC-07): Codex runs under ConsensFlow's
 * supervisor, `cf codex-session` (its app-server, a broker that knows the
 * thread and its queue, and the TUI attached to both), in full-permission
 * mode, with the first message as its last argument. The broker names the
 * thread and queues every later message; a Codex without the native queue is
 * refused.
 */
async function withHome(fn, { queue = true } = {}) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-codex-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    PATH: path.join(root, 'bin'),
  }
  await mkdir(env.PATH, { recursive: true })
  // A Codex that answers the three things a launch asks of it: its version,
  // whether it has the native queue, and its effective instructions over the
  // app-server. It writes down what it is asked, a line of arguments each
  // (`asked`).
  const asking = path.join(root, 'asked.log')
  const executable = fakeNodeExecutable(
    path.join(env.PATH, 'codex'),
    `#!${process.execPath}
import { appendFileSync } from 'node:fs'
import { createInterface } from 'node:readline'
appendFileSync(${JSON.stringify(asking)}, process.argv.slice(2).join(' ') + '\\n')
if (process.argv[2] === '--version') {
  console.log('codex-cli 0.150.0')
} else if (process.argv[2] === 'queue') {
  ${queue ? "console.log('Usage: codex queue --thread <id> --message <text>')" : 'process.exit(2)'}
} else if (process.argv[2] === 'app-server') {
  createInterface({ input: process.stdin }).on('line', (line) => {
    const request = JSON.parse(line)
    if (request.method === 'initialize') console.log(JSON.stringify({ id: request.id, result: {} }))
    else if (request.method === 'config/read')
      console.log(JSON.stringify({ id: request.id, result: { config: { developer_instructions: '' } } }))
    else if (request.method !== 'initialized') process.exit(20)
  })
}
`,
  )
  const asked = async () =>
    (await readFile(asking, 'utf8').catch(() => '')).split('\n').filter(Boolean)
  try {
    await fn({ env, executable, asked })
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
/** The launch arguments without the role text, which every window carries as `-c developer_instructions=…`. */
const withoutRole = (argv) => {
  const at = argv.findIndex((arg) => arg.startsWith('developer_instructions='))
  assert.notEqual(at, -1, 'the role text rides along')
  assert.equal(argv[at - 1], '-c')
  return [...argv.slice(0, at - 1), ...argv.slice(at + 1)]
}

const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  // A real directory: the role text is read back through Codex's app-server, started there.
  directory: os.tmpdir(),
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
  agent: { model: 'gpt-5.6-luna', effort: 'low' },
  ...overrides,
})

describe('the Codex adapter', () => {
  it('runs Codex under its supervisor, in full-permission mode, with the task last', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await codexAdapter({ env }).prepare(request())
      assert.equal(plan.nativeSession, null, 'the broker names the thread')
      assert.deepEqual(withoutRole(plan.argv), [
        BUNDLE_CF,
        'codex-session',
        executable,
        '--enable',
        'default_mode_request_user_input',
        '-c',
        'suppress_unstable_features_warning=true',
        '-c',
        'check_for_update_on_startup=false',
        '-c',
        'allow_login_shell=false',
        '--model',
        'gpt-5.6-luna',
        '-c',
        'model_reasoning_effort="low"',
        '--dangerously-bypass-approvals-and-sandbox',
        '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
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
      assert.deepEqual(withoutRole(plan.argv).slice(2), [
        executable,
        '--enable',
        'default_mode_request_user_input',
        '-c',
        'suppress_unstable_features_warning=true',
        '-c',
        'check_for_update_on_startup=false',
        '-c',
        'allow_login_shell=false',
        'resume',
        thread,
        '--model',
        'gpt-5.6-luna',
        '-c',
        'model_reasoning_effort="low"',
        '--dangerously-bypass-approvals-and-sandbox',
      ])
    })
  })

  it("opens an image agent's window on Codex's own model, whose image tool draws: no model or effort of its agent", async () => {
    await withHome(async ({ env, executable }) => {
      const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
      const designer = { model: 'codex-image', effort: 'high', designer: true }
      const fresh = await codexAdapter({ env }).prepare(request({ agent: designer }))
      assert.deepEqual(withoutRole(fresh.argv).slice(2), [
        executable,
        '--enable',
        'default_mode_request_user_input',
        '-c',
        'suppress_unstable_features_warning=true',
        '-c',
        'check_for_update_on_startup=false',
        '-c',
        'allow_login_shell=false',
        '--dangerously-bypass-approvals-and-sandbox',
        '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
      ])
      const resumed = await codexAdapter({ env }).prepare(
        request({ agent: designer, resume: thread, message: null }),
      )
      assert.deepEqual(withoutRole(resumed.argv).slice(-3), [
        'resume',
        thread,
        '--dangerously-bypass-approvals-and-sandbox',
      ])
      assert.deepEqual(fresh.dropEnv, ['OPENAI_API_KEY'])
    })
  })

  it('learns the thread from its broker', async () => {
    await withHome(async ({ env }) => {
      const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
      let asked = 0
      const adapter = codexAdapter({
        env,
        discoverEveryMs: 5,
        sessionState: async () => ({ sessionId: ++asked < 3 ? null : thread, available: true }),
      })
      const { launch } = await adapter.prepare(request())
      assert.deepEqual(await adapter.started({ launch }), { nativeSession: thread })
      assert.equal(launch.nativeSession, thread)
    })
  })

  it("starts a member with Codex's MCP servers as it starts the chief: neither command line switches them off", async () => {
    await withHome(async ({ env }) => {
      const adapter = codexAdapter({ env })
      const member = await adapter.prepare(request())
      const chief = await adapter.prepare(request({ role: 'chief', agent: null, message: null }))
      // What a command line could say of them: a flag, or a `-c` setting, that names MCP.
      const switches = (plan) =>
        withoutRole(plan.argv).filter(
          (arg, at, argv) => (arg.startsWith('-') || argv[at - 1] === '-c') && /mcp/i.test(arg),
        )
      assert.deepEqual(switches(chief), [])
      assert.deepEqual(switches(member), switches(chief))
    })
  })

  it("lists no MCP server of Codex's for a member's launch: there is none to switch off", async () => {
    await withHome(async ({ env, asked }) => {
      await codexAdapter({ env }).prepare(request())
      const asks = await asked()
      assert.ok(asks.includes('queue --help'), 'the stand-in tells what a launch asks of Codex')
      assert.deepEqual(
        asks.filter((ask) => /^mcp\b/.test(ask)),
        [],
      )
    })
  })

  it('gives the chief no question tool: it asks the human in plain words in its window', async () => {
    await withHome(async ({ env }) => {
      const adapter = codexAdapter({ env })
      const member = await adapter.prepare(request())
      assert.ok(member.argv.includes('default_mode_request_user_input'), 'a member asks the board')
      const chief = await adapter.prepare(request({ role: 'chief', agent: null, message: null }))
      assert.ok(!chief.argv.includes('default_mode_request_user_input'))
      assert.ok(!chief.argv.includes('suppress_unstable_features_warning=true'))
    })
  })

  it('holds a message while its broker cannot take one: a window starting, resuming or reconnecting', async () => {
    await withHome(async ({ env }) => {
      const thread = '0f8fad5b-d9cb-469f-a165-70867728950e'
      let available = false
      const adapter = codexAdapter({
        env,
        sessionState: async () => ({ sessionId: thread, available }),
      })
      const { launch } = await adapter.prepare(request({ resume: thread, message: null }))
      assert.match(await adapter.ready({ launch }), /cannot take a message yet/)
      available = true
      assert.equal(await adapter.ready({ launch }), true)
    })
  })

  it('follows the window to the thread a /new or /resume left it on, holding while it shows none', async () => {
    await withHome(async ({ env }) => {
      const first = '0f8fad5b-d9cb-469f-a165-70867728950e'
      const next = '7c9e6679-7425-40de-944b-e07fc1f90ae7'
      let shown = { sessionId: first, available: true }
      const read = []
      const adapter = codexAdapter({
        env,
        sessionState: async () => shown,
        answers: async (_kind, thread) => {
          read.push(thread)
          return {
            items: [{ id: `${thread}-1`, role: 'user', text: 'hello' }],
            inFlight: false,
            settlement: { state: 'settled' },
          }
        },
      })
      const launch = { nativeSession: first, channel: { kind: 'codex-queue' } }
      assert.equal((await adapter.observe({ launch })).settled, true)
      assert.equal(await adapter.ready({ launch }), true)

      // /new: while Codex starts the new thread, the broker names none...
      shown = { sessionId: null, available: false }
      const switching = await adapter.observe({ launch })
      assert.equal(switching.unnamed, true)
      assert.match(switching.waiting?.reason ?? '', /cannot take a message yet/)
      assert.equal(await adapter.ready({ launch }), switching.waiting.reason)
      // ...then names it.
      shown = { sessionId: next, available: true }
      const observed = await adapter.observe({ launch })
      assert.deepEqual(observed.switched, { nativeSession: next })
      assert.equal(observed.settled, false)
      assert.equal(await adapter.ready({ launch }), 'the window shows another conversation')

      // The dispatcher follows the window: the new thread's record is read.
      launch.nativeSession = next
      const followed = await adapter.observe({ launch })
      assert.equal(followed.switched, undefined)
      assert.equal(followed.settled, true)
      assert.equal(read.at(-1), next)
      assert.equal(await adapter.ready({ launch }), true)
    })
  })

  it('queues a message through the real channel: a claim, then the broker', async () => {
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
            claims.push([op, body])
            return { ok: true }
          },
        }
        const pane = { id: 's1-diana', generation: 2 }
        assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
          admitted: true,
          queued: true,
        })
        assert.deepEqual(claims, [['pane.claim', { pane: 's1-diana', generation: 2 }]])
        assert.equal(posted[0].url, '/deliver')
        assert.deepEqual([posted[0].body.sessionId, posted[0].body.text], [thread, 'hi'])
        // Codex's app-server refuses half a character as the pane host does.
        await adapter.deliver({
          launch,
          pane,
          host,
          text: 'half \ud83d of it, \u001b[31mred\u001b[0m and 50%\r60%',
        })
        assert.equal(posted[1].body.text, 'half  of it, ␛[31mred␛[0m and 50%␍60%')
      } finally {
        await new Promise((resolve) => broker.close(resolve))
      }
    })
  })

  it('refuses to open a Codex without the native queue, naming its version', async () => {
    await withHome(
      async ({ env }) => {
        await assert.rejects(
          codexAdapter({ env }).prepare(request()),
          /Codex 0\.150\.0 has no native queue, which ConsensFlow needs to reach its window: update Codex/,
        )
      },
      { queue: false },
    )
  })

  it("passes the harness's word on its quota through", async () => {
    await withHome(async ({ env }) => {
      const quota = { state: 'low', usedPercent: 96, resetsAt: '2026-09-26T08:29:53.000Z' }
      const adapter = codexAdapter({
        env,
        sessionState: async () => ({ sessionId: 'thread-1', available: true }),
        answers: async () => ({
          items: [],
          inFlight: false,
          settlement: { state: 'settled' },
          quota,
        }),
      })
      const channel = { kind: 'codex-queue' }
      const observed = await adapter.observe({ launch: { nativeSession: 'thread-1', channel } })
      assert.deepEqual([observed.settled, observed.quota], [true, quota])
      assert.equal(
        (await adapter.observe({ launch: { nativeSession: null, channel } })).quota,
        null,
      )
    })
  })
})
