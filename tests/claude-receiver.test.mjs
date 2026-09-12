import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { runHook } from '../hosts/claude-receiver.mjs'
import { prepareClaudeReceiver } from '../src/claude-install.js'
import { tempEnv } from './helpers.mjs'

function fixture() {
  let receiver = { session: 'current', lease: 'epoch' },
    queued = true
  const calls = []
  return {
    calls,
    select: (session) => {
      receiver = { session, lease: 'next' }
    },
    request: async (op, body) => {
      calls.push({ op, body })
      if (op === 'state') return receiver
      if (op === 'register') {
        receiver = { session: body.session, lease: 'registered' }
        return receiver
      }
      if (op === 'wake') return { wake: queued }
      if (op === 'claim') {
        if (!queued) return null
        queued = false
        return { id: 'c-one', result: 'd-1', receiver, text: 'complete framed result' }
      }
      return {}
    },
  }
}
test('Claude async notification contains no answer; current synchronous hook owns full receipt body', async () => {
  const s = fixture(),
    options = { request: s.request, signal: '/private/signal' }
  const wake = await runHook(
    { hook_event_name: 'FileChanged', session_id: 'current', file_path: options.signal },
    options,
  )
  assert.equal(wake.code, 2)
  assert.doesNotMatch(wake.stderr, /complete framed/)
  assert.equal(
    s.calls.some((call) => call.op === 'claim'),
    false,
  )
  s.select('successor')
  const body = await runHook(
    { hook_event_name: 'UserPromptSubmit', session_id: 'successor' },
    options,
  )
  assert.equal(body.stdout.hookSpecificOutput.additionalContext, 'complete framed result')
  assert.deepEqual(
    s.calls.slice(-3).map((call) => call.op),
    ['claim', 'begin', 'state'],
  )
  const stale = await runHook(
    { hook_event_name: 'UserPromptSubmit', session_id: 'current' },
    options,
  )
  assert.equal(stale.stdout, undefined)
})
test('Claude subagents, retired sessions and unrelated file changes never fetch or wake', async () => {
  for (const event of [
    { hook_event_name: 'SessionStart', session_id: 'current', agent_id: 'independent-agent' },
    { hook_event_name: 'UserPromptSubmit', session_id: 'old' },
    { hook_event_name: 'FileChanged', session_id: 'current', file_path: '/unrelated' },
  ]) {
    const s = fixture()
    assert.equal((await runHook(event, { request: s.request, signal: '/private/signal' })).code, 0)
    assert.equal(
      s.calls.some((call) => ['claim', 'wake', 'register'].includes(call.op)),
      false,
    )
  }
})
test('Claude startup installs a private native file watch; selection change after begin releases without output', async () => {
  const s = fixture()
  const options = {
    signal: '/private/signal',
    request: async (op, body) => {
      const result = await s.request(op, body)
      if (op === 'begin') s.select('changed')
      return result
    },
  }
  const start = await runHook(
    { hook_event_name: 'SessionStart', session_id: 'current', source: 'resume' },
    options,
  )
  assert.deepEqual(start.stdout.hookSpecificOutput.watchPaths, ['/private/signal'])
  const body = await runHook(
    { hook_event_name: 'UserPromptSubmit', session_id: 'current' },
    options,
  )
  assert.equal(body.stdout, undefined)
  assert.equal(s.calls.at(-1).op, 'release')
  assert.equal(s.calls.at(-1).body.bytesWritten, 0)
})

test('Claude receiver preparation ships immutable process-local hooks and signal under configured home', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const prepared = await prepareClaudeReceiver(f.env, 'launch-one', process.execPath)
  assert.ok(prepared.args[1].startsWith(f.env.CONSENSFLOW_HOME + path.sep))
  const settings = JSON.parse(await fs.readFile(prepared.args[1], 'utf8'))
  assert.equal(prepared.args[0], '--settings')
  assert.ok(prepared.env.CF_RESULT_SIGNAL.startsWith(f.env.CONSENSFLOW_HOME + path.sep))
  assert.equal(await fs.readFile(prepared.env.CF_RESULT_SIGNAL, 'utf8'), '')
  for (const name of ['SessionStart', 'UserPromptSubmit', 'FileChanged', 'Stop', 'SessionEnd']) {
    const hook = settings.hooks[name][0].hooks[0]
    assert.ok(hook.command.includes(f.env.CONSENSFLOW_HOME))
    assert.equal(hook.asyncRewake, ['FileChanged', 'Stop'].includes(name) ? true : undefined)
  }
  assert.equal(
    (await prepareClaudeReceiver(f.env, 'launch-one', process.execPath)).args[1],
    prepared.args[1],
  )
  await assert.rejects(prepareClaudeReceiver(f.env, '../outside', process.execPath), /launch/)
})
