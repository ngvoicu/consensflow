import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { runHook, selectedSession } from '../hosts/devin-hooks.mjs'
import { prepareDevinIntegration } from '../src/devin-install.js'
import { roleConfiguration } from '../src/role-skills.js'
import { tempEnv } from './helpers.mjs'

function fixture() {
  let selected = 'native-a'
  const options = {
    selectedSession: async () => selected,
    instructions: 'Only the PM writes specifications. Advisors return advice to the PM.',
  }
  const event = (name, extra = {}) => ({ hook_event_name: name, session_id: selected, ...extra })
  return {
    options,
    event,
    select: (id) => {
      selected = id
    },
  }
}

test('a session starts with its role text, and no turn event says anything', async () => {
  const s = fixture()
  for (const source of ['startup', 'resume', 'clear']) {
    assert.deepEqual(await runHook(s.event('SessionStart', { source }), s.options), {
      code: 0,
      stdout: {
        hookSpecificOutput: {
          hookEventName: 'SessionStart',
          additionalContext: s.options.instructions,
        },
      },
    })
  }
  for (const name of ['UserPromptSubmit', 'Stop', 'SessionEnd'])
    assert.deepEqual(await runHook(s.event(name), s.options), { code: 0 })
  assert.deepEqual(await runHook(s.event('SessionStart'), { ...s.options, instructions: '' }), {
    code: 0,
  })
})

test('only the conversation the window shows is answered; subagents and unknown events are inert', async () => {
  const s = fixture()
  s.select('native-b')
  assert.deepEqual(
    await runHook({ hook_event_name: 'SessionStart', session_id: 'native-a' }, s.options),
    { code: 0 },
  )
  for (const event of [
    s.event('SessionStart', { agent_id: 'subagent' }),
    s.event('SessionStart', { parent_session_id: 'native-a' }),
    s.event('FileChanged'),
    { hook_event_name: 'SessionStart', session_id: '../bad' },
  ])
    assert.deepEqual(await runHook(event, s.options), { code: 0 })
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
    assert.match(hook.command, /devin-hooks\.mjs/)
    assert.equal(hook.timeout, 5)
    assert.equal(hook.async, undefined)
  }
  assert.equal(copy.hooks.FileChanged, undefined)
  // Devin's question tool is answered from the board through cf's hook.
  assert.deepEqual(copy.hooks.PreToolUse.at(-1), {
    matcher: 'ask_user_question',
    hooks: [{ type: 'command', command: 'cf hook devin', timeout: 3600 }],
  })
  assert.equal(copy.auto_update, false)
  assert.equal(await fs.readFile(native, 'utf8'), original)
  assert.ok(
    configuration.env.CHISEL_PURE_ACP_WIRE_LOG.startsWith(f.env.CONSENSFLOW_HOME + path.sep),
  )
  const role = await roleConfiguration('devin', {
    role: 'advisor',
    env: f.env,
    launch: 'launch-a',
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
