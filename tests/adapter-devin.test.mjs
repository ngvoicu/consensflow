import assert from 'node:assert/strict'
import { appendFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { devinAdapter, devinRoleText } from '../src/adapters/devin.js'
import { SHOWS_ANOTHER } from '../src/adapters/shared.js'
import { consoleText } from '../src/console-text.js'
import { fakeExecutable, fakeNodeExecutable } from './helpers.mjs'

/**
 * The Devin adapter (TEST-BDC-05, IMPL-BDC-07): Devin runs on a config of our
 * own per launch (its hooks log each turn), in full-permission mode, with the
 * first message in a prompt file. Devin names its session itself, and its own
 * wire log says which one this window opened. Messages are pasted behind
 * whatever the input box holds, never while Devin shows another conversation.
 */
async function withHome(fn) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-devin-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    XDG_CONFIG_HOME: path.join(root, 'config'),
    PATH: path.join(root, 'bin'),
    CONSENSFLOW_NODE: process.execPath,
  }
  await mkdir(env.PATH, { recursive: true })
  const executable = fakeExecutable(path.join(env.PATH, 'devin'), { output: 'devin 3000.10.22' })
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
  harness: 'devin',
}
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser',
  agent: { model: 'swe-1-6-slow' },
  ...overrides,
})
const integration = (env) => path.join(env.CONSENSFLOW_HOME, 'integrations', 'devin', 'launch-1')

const selection = (sessionId) =>
  `${JSON.stringify({ sessionId, update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] } })}\n`

async function selects(env, sessionId) {
  await writeFile(path.join(integration(env), 'wire.jsonl'), selection(sessionId))
}

describe('the Devin adapter', () => {
  it('launches Devin on its own config, in full-permission mode, with the task in a prompt file', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await devinAdapter({ env }).prepare(request())
      const root = integration(env)
      assert.equal(plan.nativeSession, null, 'Devin names the session itself')
      assert.deepEqual(plan.argv, [
        executable,
        '--config',
        path.join(root, 'config.json'),
        '--model',
        'swe-1-6-slow',
        '--permission-mode',
        'dangerous',
        '--respect-workspace-trust',
        'false',
        '--prompt-file',
        path.join(root, 'prompt.txt'),
      ])
      assert.equal(await readFile(path.join(root, 'prompt.txt'), 'utf8'), request().message)
      assert.equal(plan.env.CHISEL_PURE_ACP_WIRE_LOG, path.join(root, 'wire.jsonl'))
      const config = JSON.parse(await readFile(path.join(root, 'config.json'), 'utf8'))
      assert.deepEqual(Object.keys(config.hooks).sort(), ['PreToolUse', 'SessionStart'])
      assert.equal(config.hooks.PreToolUse[0].matcher, 'ask_user_question')
      assert.equal(config.auto_update, false)
    })
  })

  it("leaves the chief's question tool to Devin's own dialog, where the human answers it", async () => {
    await withHome(async ({ env }) => {
      await devinAdapter({ env }).prepare(
        request({
          role: 'chief',
          participant: { ...participant, handle: 'chief', role: 'chief', agent: null },
          message: null,
        }),
      )
      const config = JSON.parse(await readFile(path.join(integration(env), 'config.json'), 'utf8'))
      assert.deepEqual(config.hooks.PreToolUse, [])
    })
  })

  it('asks Devin its version once, not at every launch, and refuses one too old', async () => {
    await withHome(async ({ env }) => {
      const runs = path.join(env.PATH, 'version-runs')
      const devin = (version) =>
        fakeNodeExecutable(
          path.join(env.PATH, 'devin'),
          `#!${process.execPath}
import { appendFileSync } from 'node:fs'
appendFileSync(${JSON.stringify(runs)}, 'x')
console.log('devin ${version}')
`,
        )
      devin('3000.11.3 (9c803229faa4)')
      const adapter = devinAdapter({ env })
      await adapter.prepare(request({ launchId: 'launch-1' }))
      await adapter.prepare(request({ launchId: 'launch-2' }))
      assert.equal(await readFile(runs, 'utf8'), 'x', 'one question for an unchanged Devin')
      devin('3000.9.1 (00000000)')
      await assert.rejects(
        adapter.prepare(request({ launchId: 'launch-3' })),
        /3000\.10\.21 or newer/,
      )
    })
  })

  it("launches a catalog agent on Devin's own id: the family with its level", async () => {
    await withHome(async ({ env }) => {
      const plan = await devinAdapter({ env }).prepare(
        request({ agent: { model: 'claude-opus-5-5', effort: 'max' } }),
      )
      const at = plan.argv.indexOf('--model')
      assert.equal(plan.argv[at + 1], 'claude-opus-5-5-max')
    })
  })

  it('resumes the session it has', async () => {
    await withHome(async ({ env }) => {
      const plan = await devinAdapter({ env }).prepare(
        request({ resume: 'mild-coin', message: null }),
      )
      assert.equal(plan.nativeSession, 'mild-coin')
      assert.deepEqual(plan.argv.slice(3), [
        '--resume',
        'mild-coin',
        '--model',
        'swe-1-6-slow',
        '--permission-mode',
        'dangerous',
        '--respect-workspace-trust',
        'false',
      ])
    })
  })

  it('learns the session this window opened from its own wire log', async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({ env, discoverEveryMs: 5 })
      const { launch } = await adapter.prepare(request())
      setTimeout(() => selects(env, 'mild-coin'), 30)
      assert.deepEqual(await adapter.started({ launch }), { nativeSession: 'mild-coin' })
      assert.equal(launch.nativeSession, 'mild-coin')
    })
  })

  it('pastes only into the conversation it knows, whatever the human left unsent', async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({ env })
      const { launch } = await adapter.prepare(request({ resume: 'mild-coin', message: null }))
      const requests = []
      let pasteInFlight = false
      const host = {
        async request(op, body) {
          requests.push([op, body])
          if (op === 'pane.snapshot') return { ok: true, pasteInFlight }
          return { ok: true }
        },
      }
      const pane = { id: 's1-zeus', generation: 2 }
      await selects(env, 'another-one')
      const other = await adapter.deliver({ launch, pane, host, text: 'hi' })
      assert.deepEqual(other, {
        admitted: false,
        reason: 'Devin is displaying another conversation',
      })
      await selects(env, 'mild-coin')
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hi' }), {
        admitted: true,
      })
      assert.deepEqual(requests.at(-1), [
        'pane.write_paste',
        { id: 's1-zeus', generation: 2, body: 'hi' },
      ])
      await adapter.deliver({
        launch,
        pane,
        host,
        text: 'half \ud83d of it, \u001b[31mred\u001b[0m and 50%\r60%',
      })
      // On a Windows machine every window is Windows': there it goes in ASCII.
      const shown = 'half  of it, ␛[31mred␛[0m and 50%␍60%'
      assert.equal(
        requests.at(-1)[1].body,
        process.platform === 'win32' ? consoleText(shown) : shown,
      )
      // On Windows the console drops a paste's non-ASCII marks: they go in ASCII.
      await devinAdapter({ env: { ...env, OS: 'Windows_NT' } }).deliver({
        launch,
        pane,
        host,
        text: '[ConsensFlow m-3 · T-1 · answer from @chief]\nblue — not “red” → done…',
      })
      assert.equal(
        requests.at(-1)[1].body,
        '[ConsensFlow m-3 | T-1 | answer from @chief]\nblue -- not "red" -> done...',
      )
      assert.equal(await adapter.ready({ launch, pane, host }), true)
      pasteInFlight = true
      assert.equal(await adapter.ready({ launch, pane, host }), false)
    })
  })

  it('interrupts with Escape twice, and closes with a third the rewind the two open at an idle prompt', () => {
    // poker-lab, 2026-10-03: two Escapes at a Devin already stopped opened its
    // rewind, and the next message's Enter rewound the conversation.
    assert.deepEqual(devinAdapter({ env: {} }).interrupt, { presses: 2, closeAfterMs: 1_000 })
  })

  it('tells Devin on Windows to name files the way its file tools write them, and nowhere else', () => {
    const role = '# ConsensFlow worker\n\nRole text.'
    const windows = devinRoleText(role, { OS: 'Windows_NT' })
    assert.ok(windows.startsWith(role))
    assert.match(windows, /never \/c\/… paths/)
    // A Windows machine is Windows whatever its environment says.
    assert.equal(devinRoleText(role, { HOME: '/h' }), process.platform === 'win32' ? windows : role)
  })

  it('reads a wire log that was replaced from its start, with nothing of the old one carried', async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({ env })
      const { launch } = await adapter.prepare(request({ resume: 'mild-coin', message: null }))
      const host = { request: async () => ({ ok: true, pasteInFlight: false }) }
      const pane = { id: 's1-zeus', generation: 2 }
      const wire = path.join(integration(env), 'wire.jsonl')
      // A line half written when it was read: carried until the rest comes.
      await writeFile(
        wire,
        `${selection('another-one')}${selection('mild-coin')}{"sessionId":"mild`,
      )
      assert.equal(await adapter.ready({ launch, pane, host }), true)
      await writeFile(wire, selection('another-one'))
      assert.equal(await adapter.ready({ launch, pane, host }), SHOWS_ANOTHER)
    })
  })

  it('follows the window to the conversation a /new or /resume left it on, holding until it names one', async () => {
    await withHome(async ({ env }) => {
      const read = []
      const adapter = devinAdapter({
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
      const { launch } = await adapter.prepare(request({ resume: 'mild-coin', message: null }))
      const host = {
        async request(op) {
          return op === 'pane.snapshot' ? { ok: true, pasteInFlight: false } : { ok: true }
        },
      }
      const pane = { id: 's1-zeus', generation: 2 }
      // Devin has not said yet which conversation the window shows: a message waits.
      const before = await adapter.observe({ launch })
      assert.equal(before.unnamed, true)
      assert.match(before.waiting?.reason ?? '', /Devin has not said/)
      assert.equal(await adapter.ready({ launch, pane, host }), before.waiting.reason)
      await selects(env, 'mild-coin')
      assert.equal((await adapter.observe({ launch })).settled, true)
      assert.equal(await adapter.ready({ launch, pane, host }), true)

      // /new: Devin's own log now names another conversation.
      await appendFile(path.join(integration(env), 'wire.jsonl'), selection('fresh-leaf'))
      const observed = await adapter.observe({ launch })
      assert.deepEqual(observed.switched, { nativeSession: 'fresh-leaf' })
      assert.equal(observed.settled, false)
      assert.equal(
        await adapter.ready({ launch, pane, host }),
        'the window shows another conversation',
      )

      // The dispatcher follows the window: the new conversation's record is read.
      launch.nativeSession = 'fresh-leaf'
      const followed = await adapter.observe({ launch })
      assert.equal(followed.switched, undefined)
      assert.equal(read.at(-1), 'fresh-leaf')
      assert.equal(await adapter.ready({ launch, pane, host }), true)
    })
  })

  it('counts a window whose session is not known yet as not ready', async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({ env, answers: async () => assert.fail('nothing to read yet') })
      const { launch } = await adapter.prepare(request())
      assert.deepEqual(await adapter.observe({ launch }), {
        items: [],
        settled: false,
        waiting: null,
        failed: false,
        quota: null,
      })
    })
  })

  it('reads its own question dialog, still open, as waiting', async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({
        env,
        answers: async () => ({
          items: [],
          inFlight: true,
          asking: true,
          settlement: { state: 'in-flight' },
        }),
        discoverEveryMs: 5,
      })
      const { launch } = await adapter.prepare(request())
      await selects(env, 'dev-1')
      await adapter.started({ launch })
      assert.deepEqual((await adapter.observe({ launch })).waiting, {
        reason: 'its own question dialog is open',
      })
    })
  })

  it("reads Devin's own refusal from the wire log, until the next prompt", async () => {
    await withHome(async ({ env }) => {
      const adapter = devinAdapter({
        env,
        answers: async () => ({ items: [], inFlight: false, settlement: { state: 'settled' } }),
        discoverEveryMs: 5,
      })
      const { launch } = await adapter.prepare(request())
      await selects(env, 'dev-1')
      await adapter.started({ launch })
      const wire = path.join(integration(env), 'wire.jsonl')
      const line = (event) => appendFile(wire, `${JSON.stringify(event)}\n`)
      assert.equal((await adapter.observe({ launch })).quota, null)

      await line({
        jsonrpc: '2.0',
        id: 2,
        method: 'session/prompt',
        params: { sessionId: 'dev-1' },
      })
      await line({
        sessionId: 'dev-1',
        update: {
          sessionUpdate: 'agent_message_chunk',
          content: {
            type: 'text',
            text: 'Reached overall message rate limit. Your limit will reset in 35 minutes.',
          },
        },
      })
      const before = Date.now()
      const { quota } = await adapter.observe({ launch })
      assert.equal(quota.state, 'exhausted')
      const reset = Date.parse(quota.resetsAt) - before
      assert.ok(reset >= 34 * 60_000 && reset <= 36 * 60_000, `reset in ${reset} ms`)
      assert.equal(
        (await adapter.observe({ launch })).quota.state,
        'exhausted',
        'nothing new: still out',
      )

      await line({
        jsonrpc: '2.0',
        id: 3,
        method: 'session/prompt',
        params: { sessionId: 'dev-1' },
      })
      await line({
        sessionId: 'dev-1',
        update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'On it.' } },
      })
      assert.equal((await adapter.observe({ launch })).quota, null)

      // The prompt refused outright, in the words Devin 3000.11 carries for it.
      await line({
        jsonrpc: '2.0',
        id: 4,
        method: 'session/prompt',
        params: { sessionId: 'dev-1' },
      })
      await line({
        jsonrpc: '2.0',
        id: 4,
        error: {
          code: -32011,
          message: 'Quota exhausted.',
          data: { 'cognition.ai/errorKind': 'resource_exhausted', 'cognition.ai/retryable': true },
        },
      })
      assert.equal((await adapter.observe({ launch })).quota?.state, 'exhausted')
    })
  })
})
