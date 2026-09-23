import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { claudeCodeAdapter, DRAFT_GRACE_MS } from '../src/adapters/claude-code.js'

/**
 * The Claude Code adapter (TEST-BDC-05, IMPL-BDC-06): how a Claude window is
 * launched, how a message reaches it, and what Claude's own records say about
 * it. Each test gets a throwaway home, Claude config directory and a stand-in
 * `claude` executable on PATH.
 */
async function withHome(fn) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'cf-claude-adapter-'))
  const env = {
    HOME: path.join(root, 'home'),
    CONSENSFLOW_HOME: path.join(root, 'consensflow'),
    CLAUDE_CONFIG_DIR: path.join(root, 'claude'),
    PATH: path.join(root, 'bin'),
  }
  await mkdir(env.PATH, { recursive: true })
  const executable = path.join(env.PATH, 'claude')
  await writeFile(executable, '#!/bin/sh\nexit 0\n')
  await chmod(executable, 0o755)
  try {
    await fn({ env, root, executable })
  } finally {
    await rm(root, { recursive: true, force: true })
  }
}

const worker = { id: 3, handle: 'zeus', role: 'worker', agent: 'zeus', harness: 'claude-code' }
const request = (overrides = {}) => ({
  launchId: 'launch-1',
  participant: worker,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
  agent: { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' },
  ...overrides,
})

/** One Claude transcript line, in the shape Claude Code writes. */
function record(sessionId, n, fields) {
  return `${JSON.stringify({
    sessionId,
    version: '2.1.277',
    timestamp: new Date(Date.parse('2026-09-19T12:00:00Z') + n * 1000).toISOString(),
    uuid: `${sessionId}-${n}`,
    ...fields,
  })}\n`
}

async function transcript(env, sessionId, lines) {
  const directory = path.join(env.CLAUDE_CONFIG_DIR, 'projects', '-work-app')
  await mkdir(directory, { recursive: true })
  await writeFile(path.join(directory, `${sessionId}.jsonl`), lines.join(''))
}

const userLine = (sessionId, n, text) =>
  record(sessionId, n, { type: 'user', message: { role: 'user', content: text } })
const answerLine = (sessionId, n, text) =>
  record(sessionId, n, {
    type: 'assistant',
    message: {
      id: `${sessionId}-message-${n}`,
      role: 'assistant',
      content: [{ type: 'text', text }],
      stop_reason: 'end_turn',
    },
  })
const stopLine = (sessionId, n) =>
  record(sessionId, n, {
    type: 'system',
    subtype: 'stop_hook_summary',
    preventedContinuation: false,
    hookCount: 1,
  })

async function status(env, sessionId, fields) {
  const directory = path.join(env.CLAUDE_CONFIG_DIR, 'sessions')
  await mkdir(directory, { recursive: true })
  await writeFile(
    path.join(directory, `${process.pid}.json`),
    JSON.stringify({ pid: process.pid, sessionId, kind: 'interactive', ...fields }),
  )
}

describe('the Claude Code adapter', () => {
  it('launches a fresh worker on its own session id, in full-permission mode, with the task last', async () => {
    await withHome(async ({ env, executable }) => {
      const plan = await claudeCodeAdapter({ env, peer: false }).prepare(request())
      assert.match(plan.nativeSession, /^[0-9a-f-]{36}$/)
      const settings = path.join(
        env.CONSENSFLOW_HOME,
        'integrations',
        'claude',
        'launch-1',
        'settings.json',
      )
      const roles = path.join(env.CONSENSFLOW_HOME, 'roles', 'worker')
      assert.deepEqual(plan.argv, [
        executable,
        '--settings',
        settings,
        '--add-dir',
        roles,
        '--append-system-prompt-file',
        path.join(roles, '.claude', 'skills', 'consensflow-worker', 'SKILL.md'),
        '--system-prompt-snapshot',
        'off',
        '--session-id',
        plan.nativeSession,
        '--model',
        'claude-sonnet-5',
        '--effort',
        'high',
        '--permission-mode',
        'bypassPermissions',
        '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser',
      ])
      assert.deepEqual(plan.env, {}, 'cf stays usable inside the window')
      assert.deepEqual(plan.dropEnv, ['ANTHROPIC_API_KEY'])
      const written = JSON.parse(await readFile(settings, 'utf8'))
      assert.equal(written.permissions.defaultMode, 'bypassPermissions')
      assert.equal(written.skipDangerousModePermissionPrompt, true)
      assert.equal(written.crossSessionInbound, 'accept')
      assert.deepEqual(written.hooks.Stop, [{ hooks: [{ type: 'command', command: 'exit 0' }] }])
      assert.deepEqual(
        written.hooks.PreToolUse,
        [
          {
            matcher: 'AskUserQuestion',
            hooks: [{ type: 'command', command: 'cf hook claude', timeout: 3600 }],
          },
        ],
        "Claude's question tool is answered from the board, for an hour, then in the window",
      )
    })
  })

  it('resumes a conversation on the session it already has', async () => {
    await withHome(async ({ env }) => {
      const session = '0f8fad5b-d9cb-469f-a165-70867728950e'
      await transcript(env, session, [userLine(session, 1, 'Write the parser')])
      const plan = await claudeCodeAdapter({ env, peer: false }).prepare(
        request({ resume: session, message: null }),
      )
      assert.equal(plan.nativeSession, session)
      assert.deepEqual(plan.argv.slice(9), [
        '--resume',
        session,
        '--model',
        'claude-sonnet-5',
        '--permission-mode',
        'bypassPermissions',
      ])
    })
  })

  it('starts afresh under the same id a conversation Claude never kept, instead of resuming nothing', async () => {
    await withHome(async ({ env }) => {
      // A window opened by hand and lost before anything was said in it:
      // Claude kept no record, and `--resume` would say "No conversation found".
      const session = '6a2f41ac-8c1d-4c55-9b4e-2f1e8a3d9c70'
      const plan = await claudeCodeAdapter({ env, peer: false }).prepare(
        request({ resume: session, message: 'Review T-1' }),
      )
      assert.equal(plan.nativeSession, session)
      assert.ok(!plan.argv.includes('--resume'))
      assert.deepEqual(
        plan.argv.slice(plan.argv.indexOf('--session-id'), plan.argv.indexOf('--session-id') + 2),
        ['--session-id', session],
      )
      assert.equal(plan.argv.at(-1), 'Review T-1', 'the brief goes in as its first message')
    })
  })

  it('gives a lead its role instructions and no model of its own', async () => {
    await withHome(async ({ env }) => {
      const plan = await claudeCodeAdapter({ env, peer: false }).prepare(
        request({
          participant: { ...worker, handle: 'lead', role: 'lead', agent: null },
          role: 'lead',
          message: null,
          agent: null,
          instructions: 'LEAD INSTRUCTIONS',
        }),
      )
      const at = plan.argv.indexOf('--append-system-prompt-file')
      assert.ok(at > 0)
      assert.match(plan.argv[at + 1], /roles\/lead\/.*SKILL\.md$/)
      assert.equal(await readFile(plan.argv[at + 1], 'utf8'), 'LEAD INSTRUCTIONS')
      assert.equal(plan.argv.includes('--model'), false)
    })
  })

  it('refuses to launch when Claude is not installed', async () => {
    await withHome(async ({ env }) => {
      await assert.rejects(
        claudeCodeAdapter({ env: { ...env, PATH: '/nowhere' }, peer: false }).prepare(request()),
        /claude is not installed/,
      )
    })
  })

  it('reads the conversation from the transcript and waits for Claude itself to say the window is idle', async () => {
    await withHome(async ({ env }) => {
      const adapter = claudeCodeAdapter({ env, peer: false })
      const session = '1b4e28ba-2fa1-41d2-883f-0016d3cca427'
      const launch = { nativeSession: session }
      await transcript(env, session, [
        userLine(session, 1, '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser'),
        answerLine(session, 2, 'Parser done'),
        stopLine(session, 3),
      ])
      // A transcript that settled before this window opened is not the
      // window at its prompt: Claude's own status says when it is.
      let observed = await adapter.observe({ launch })
      assert.equal(observed.settled, false, 'no word from Claude yet')
      await status(env, session, { status: 'idle' })
      observed = await adapter.observe({ launch })
      assert.equal(observed.settled, true)
      assert.equal(observed.waiting, null)
      assert.deepEqual(
        observed.items.map((item) => [item.role, item.text, item.complete]),
        [
          ['user', '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser', true],
          ['assistant', 'Parser done', true],
        ],
      )

      await status(env, session, { status: 'waiting', waitingFor: 'permission to run a command' })
      observed = await adapter.observe({ launch })
      assert.equal(observed.settled, false)
      assert.deepEqual(observed.waiting, { reason: 'permission to run a command' })

      await status(env, session, { status: 'busy' })
      assert.equal((await adapter.observe({ launch })).settled, false)
    })
  })

  it('counts a new window with no transcript yet as idle once Claude says so', async () => {
    await withHome(async ({ env }) => {
      const adapter = claudeCodeAdapter({ env, peer: false })
      const launch = { nativeSession: 'e2c56db5-dffb-48d2-b060-d0f5a71096e0' }
      const before = await adapter.observe({ launch })
      assert.deepEqual([before.settled, before.items], [false, []], 'still starting')
      await status(env, launch.nativeSession, { status: 'idle' })
      const after = await adapter.observe({ launch })
      assert.deepEqual([after.settled, after.items], [true, []])
    })
  })

  it('pastes a message into the window by default, and waits while a human is typing there', async () => {
    await withHome(async ({ env }) => {
      let clock = 1_000_000
      const adapter = claudeCodeAdapter({ env, now: () => clock })
      const requests = []
      let draftLatched = false
      let inputEpoch = 4
      const host = {
        async request(op, body) {
          requests.push([op, body])
          if (op === 'pane.snapshot') return { ok: true, inputEpoch, draftLatched }
          if (op === 'pane.write_paste') return { ok: true }
          if (op === 'draft.clear') return { ok: true, outcome: 'cleared' }
          return { ok: false, error: 'unexpected' }
        },
      }
      const pane = { id: 's1-zeus', generation: 7 }
      const launch = { nativeSession: 'e2c56db5-dffb-48d2-b060-d0f5a71096e0' }
      assert.equal(await adapter.ready({ launch, pane, host }), true)
      assert.deepEqual(await adapter.deliver({ launch, pane, host, text: 'hello' }), {
        admitted: true,
      })
      assert.deepEqual(requests.at(-1), [
        'pane.write_paste',
        { id: 's1-zeus', generation: 7, epoch: 4, body: 'hello' },
      ])
      draftLatched = true
      assert.match(await adapter.ready({ launch, pane, host }), /someone is typing in the window/)
      // A latch nobody types behind any more lets go after the grace; a
      // keystroke meanwhile starts it over.
      clock += DRAFT_GRACE_MS - 1
      assert.match(await adapter.ready({ launch, pane, host }), /typed in the window \d+s ago/)
      inputEpoch = 9
      clock += 10
      assert.match(
        await adapter.ready({ launch, pane, host }),
        /someone is typing in the window/,
        'typed again: a fresh grace',
      )
      clock += DRAFT_GRACE_MS
      assert.equal(await adapter.ready({ launch, pane, host }), true)
      assert.deepEqual(requests.at(-1), [
        'draft.clear',
        { id: 's1-zeus', generation: 7, epoch: 9, submission: 'stale-9' },
      ])
    })
  })

  it('reports a refused request as exhausted quota, with the reset its text names', async () => {
    await withHome(async ({ env }) => {
      const adapter = claudeCodeAdapter({ env, peer: false })
      const session = '2c1a6b64-0d2c-4f4e-9a7b-6f1c5f2e8d90'
      await transcript(env, session, [
        userLine(session, 1, '[ConsensFlow m-1 · T-1 · task from @lead]\nWrite the parser'),
        record(session, 2, {
          type: 'assistant',
          isApiErrorMessage: true,
          apiErrorStatus: 429,
          error: 'rate_limit',
          message: {
            id: `${session}-message-2`,
            role: 'assistant',
            content: [{ type: 'text', text: "You've hit your limit. Resets in 2 hours." }],
          },
        }),
      ])
      const observed = await adapter.observe({ launch: { nativeSession: session } })
      assert.deepEqual(observed.quota, {
        state: 'exhausted',
        at: '2026-09-19T12:00:02.000Z',
        resetsAt: '2026-09-19T14:00:02.000Z',
      })
      await transcript(env, session, [
        userLine(session, 1, 'hello'),
        answerLine(session, 2, 'Hi'),
        stopLine(session, 3),
      ])
      assert.equal((await adapter.observe({ launch: { nativeSession: session } })).quota, null)
    })
  })
})
