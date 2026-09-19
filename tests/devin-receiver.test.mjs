import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { runHook, selectedSession } from '../hosts/devin-receiver.mjs'
import { prepareDevinIntegration } from '../src/devin-install.js'
import { roleConfiguration } from '../src/role-skills.js'
import { tempEnv } from './helpers.mjs'

function fixture() {
  let selected = 'native-a'
  let receiver = null
  const calls = [],
    queue = []
  const options = {
    selectedSession: async () => selected,
    instructions: 'Only the PM writes specifications. Advisors return advice to the PM.',
    request: async (op, body) => {
      calls.push({ op, body })
      if (op === 'state') return receiver
      if (op === 'register') {
        receiver = { lease: `lease-${body.session}`, session: body.session }
        return receiver
      }
      if (op === 'claim') return queue.shift() ?? null
      return {}
    },
  }
  const event = (name, extra = {}) => ({ hook_event_name: name, session_id: selected, ...extra })
  return {
    options,
    calls,
    queue,
    event,
    select: (id) => {
      selected = id
    },
  }
}

test('native startup loads role instructions without a prompt or model wake', async () => {
  const s = fixture()
  const output = await runHook(s.event('SessionStart', { source: 'startup' }), s.options)
  assert.ok(output.stdout.hookSpecificOutput.additionalContext.startsWith(s.options.instructions))
  assert.match(output.stdout.hookSpecificOutput.additionalContext, /next human prompt/)
  assert.equal(output.stdout.hookSpecificOutput.hookEventName, 'SessionStart')
  assert.deepEqual(
    s.calls.map((c) => c.op),
    ['state', 'register'],
  )
  assert.equal(output.code, 0)
})

test('advisors load their role without coordinator inbox authority', async () => {
  const s = fixture()
  delete s.options.request
  const output = await runHook(s.event('SessionStart'), s.options)
  assert.match(output.stdout.hookSpecificOutput.additionalContext, /Only the PM/)
  assert.deepEqual(await runHook(s.event('Stop'), s.options), { code: 0 })
  assert.equal(s.calls.length, 0)
})

test('each Stop can fetch a different full reply; repeated hooks never claim receipt', async () => {
  const s = fixture()
  await runHook(s.event('SessionStart'), s.options)
  for (let n = 1; n <= 3; n++) {
    s.queue.push({ id: `c-${n}`, result: `d-${n}`, text: `Complete framed reply ${n}` })
    const output = await runHook(s.event('Stop', { stop_hook_active: n > 1 }), s.options)
    assert.deepEqual(output.stdout, { decision: 'block', reason: `Complete framed reply ${n}` })
  }
  assert.equal(s.calls.filter((c) => c.op === 'begin').length, 3)
  assert.equal(s.calls.filter((c) => c.op === 'receipt').length, 0)
  assert.deepEqual(await runHook(s.event('Stop'), s.options), { code: 0 })
})

test('next human prompt collects late replies as native context without replacing the prompt', async () => {
  const s = fixture()
  await runHook(s.event('SessionStart'), s.options)
  s.queue.push({ id: 'c-late', result: 'd-late', text: 'Complete late reply' })
  const output = await runHook(
    s.event('UserPromptSubmit', { prompt: 'my own question' }),
    s.options,
  )
  assert.deepEqual(output.stdout, {
    hookSpecificOutput: {
      hookEventName: 'UserPromptSubmit',
      additionalContext: 'Complete late reply',
    },
  })
  assert.equal(s.calls.filter((c) => c.op === 'wake').length, 0)
})

test('new and resume register the selected native session; stale hooks cannot take ownership', async () => {
  const s = fixture()
  await runHook(s.event('SessionStart'), s.options)
  s.select('native-b')
  assert.deepEqual(
    await runHook({ hook_event_name: 'SessionStart', session_id: 'native-a' }, s.options),
    { code: 0 },
  )
  await runHook(s.event('SessionStart', { source: 'startup' }), s.options)
  s.select('native-a')
  await runHook(s.event('SessionStart', { source: 'resume' }), s.options)
  assert.deepEqual(
    s.calls.filter((c) => c.op === 'register').map((c) => c.body.session),
    ['native-a', 'native-b', 'native-a'],
  )
  assert.deepEqual(
    s.calls.filter((c) => c.op === 'register').map((c) => c.body.previous),
    [null, 'lease-native-a', 'lease-native-b'],
  )
})

test('selection changing before output releases only the unsubmitted claim', async () => {
  const s = fixture()
  await runHook(s.event('SessionStart'), s.options)
  s.queue.push({ id: 'c', result: 'd', text: 'must not enter the successor' })
  const request = s.options.request
  s.options.request = async (op, body) => {
    const result = await request(op, body)
    if (op === 'begin') s.select('native-b')
    return result
  }
  assert.deepEqual(await runHook(s.event('Stop'), s.options), { code: 0 })
  const release = s.calls.find((c) => c.op === 'release').body
  assert.equal(release.admitted, false)
  assert.equal(release.bytesWritten, 0)
})

test('session end retires only the selected receiver; subagents and unknown events are inert', async () => {
  const s = fixture()
  await runHook(s.event('SessionStart'), s.options)
  const before = s.calls.length
  for (const event of [
    s.event('SessionStart', { agent_id: 'subagent' }),
    s.event('FileChanged'),
    { hook_event_name: 'Stop', session_id: '../bad' },
  ])
    assert.deepEqual(await runHook(event, s.options), { code: 0 })
  assert.equal(s.calls.length, before)
  await runHook(s.event('SessionEnd'), s.options)
  assert.equal(s.calls.at(-1).op, 'retire')
})

test('unavailable app never manufactures a wake or a receipt', async () => {
  const s = fixture()
  s.options.request = async () => {
    throw new Error('app unavailable')
  }
  await assert.rejects(runHook(s.event('Stop'), s.options), /app unavailable/)
})

test('role instructions still load when the app is temporarily unavailable', async () => {
  const s = fixture()
  s.options.request = async () => {
    throw new Error('app unavailable')
  }
  const output = await runHook(s.event('SessionStart'), s.options)
  assert.ok(output.stdout.hookSpecificOutput.additionalContext.startsWith(s.options.instructions))
  assert.match(output.stdout.hookSpecificOutput.additionalContext, /next human prompt/)
})

test('selection uses complete native records and ignores an in-progress final write', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const file = path.join(f.env.CONSENSFLOW_HOME, 'wire.jsonl')
  await fs.mkdir(path.dirname(file), { recursive: true })
  const select = (sessionId) =>
    JSON.stringify({
      sessionId,
      update: {
        sessionUpdate: 'config_option_update',
        configOptions: [{ id: 'mode' }],
      },
    }) + '\n'
  await fs.writeFile(file, select('native-a') + select('native-b') + '{"sessionId":')
  assert.equal(await selectedSession(file), 'native-b')
  await fs.writeFile(file, select('native-a') + '{invalid}\n' + select('native-b'))
  await assert.rejects(selectedSession(file), /JSON|Unexpected|Expected/)
})

test('private installation preserves native defaults and hooks, and does not edit project/global files', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const native = path.join(f.env.XDG_CONFIG_HOME, 'devin', 'config.json')
  await fs.mkdir(path.dirname(native), { recursive: true })
  const original =
    '{ // native JSONC\n "theme_mode": "dark", "hooks": {"Stop": [{"hooks": [{"type":"command","command":"user-hook"}]}]}, "agent":{"model":"native-default"}, }'
  await fs.writeFile(native, original)
  const configuration = await prepareDevinIntegration(f.env, {
    launchId: 'launch-a',
    node: process.execPath,
  })
  assert.equal(configuration.args[0], '--config')
  assert.ok(configuration.args[1].startsWith(f.env.CONSENSFLOW_HOME + path.sep))
  const copy = JSON.parse(await fs.readFile(configuration.args[1], 'utf8'))
  assert.equal(copy.theme_mode, 'dark')
  assert.equal(copy.agent.model, 'native-default')
  assert.equal(copy.hooks.Stop[0].hooks[0].command, 'user-hook')
  for (const name of ['SessionStart', 'UserPromptSubmit', 'Stop', 'SessionEnd']) {
    const hook = copy.hooks[name].at(-1).hooks[0]
    assert.match(hook.command, /devin-receiver\.mjs/)
    assert.equal(hook.timeout, 5)
    assert.equal(hook.async, undefined)
  }
  assert.equal(copy.hooks.FileChanged, undefined)
  assert.equal(copy.auto_update, false)
  assert.equal(await fs.readFile(native, 'utf8'), original)
  assert.ok(
    configuration.env.CHISEL_PURE_ACP_WIRE_LOG.startsWith(f.env.CONSENSFLOW_HOME + path.sep),
  )
  const role = await roleConfiguration('devin', {
    role: 'advisor',
    env: f.env,
    content: '# ConsensFlow advisor\n',
  })
  assert.match(await fs.readFile(role.env.CF_DEVIN_ROLE_FILE, 'utf8'), /advisor/i)
  assert.deepEqual(role.args, [])
})

test('invalid launch identity or malformed native configuration fails without replacing native files', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  await assert.rejects(
    prepareDevinIntegration(f.env, { launchId: '../escape', node: process.execPath }),
    /launch/,
  )
  const native = path.join(f.env.XDG_CONFIG_HOME, 'devin', 'config.json')
  await fs.mkdir(path.dirname(native), { recursive: true })
  await fs.writeFile(native, '{ invalid')
  await assert.rejects(
    prepareDevinIntegration(f.env, { launchId: 'launch-a', node: process.execPath }),
    /configuration/,
  )
  assert.equal(await fs.readFile(native, 'utf8'), '{ invalid')
})
