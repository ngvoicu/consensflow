import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { runHook, selectedSession } from '../hosts/devin-hooks.mjs'
import { prepareDevinIntegration } from '../src/devin-install.js'
import { roleConfiguration } from '../src/role-skills.js'
import { tempEnv } from './helpers.mjs'

const HOOK = fileURLToPath(new URL('../hosts/devin-hooks.mjs', import.meta.url))

/** The line Devin's wire log gains when its window configures a conversation it opens. */
const shows = (sessionId) =>
  `${JSON.stringify({
    sessionId,
    update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
  })}\n`

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
  await fs.writeFile(file, `${shows('native-a')}${shows('native-b')}{"sessionId":`)
  assert.equal(await selectedSession(file), 'native-b')
  await fs.writeFile(file, `${shows('native-a')}{invalid}\n${shows('native-b')}`)
  await assert.rejects(selectedSession(file), /JSON|Unexpected|Expected/)
})

/**
 * One launch's files as Devin's hook command finds them in its environment:
 * the wire log naming the conversation the window shows, and the role text.
 */
async function launchFiles(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-devin-hook-'))
  t.after(() => fs.rm(root, { recursive: true, force: true }))
  const files = {
    wire: path.join(root, 'wire.jsonl'),
    role: path.join(root, 'SKILL.md'),
  }
  await fs.writeFile(files.wire, shows('native-a'))
  await fs.writeFile(files.role, '# ConsensFlow worker\n')
  const env = { CHISEL_PURE_ACP_WIRE_LOG: files.wire, CF_DEVIN_ROLE_FILE: files.role }
  return { ...files, env }
}

/** Devin running the hook command for one event: the event on stdin, the launch's files in the environment. */
function runAsDevin(input, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [HOOK], {
      env: { ...process.env, ...env },
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    let stdout = ''
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    child.on('error', reject)
    child.on('close', (code) => resolve({ code, stdout }))
    // A hook that stops reading early closes its input: that is its answer, not the test's error.
    child.stdin.on('error', () => {})
    child.stdin.end(typeof input === 'string' ? input : JSON.stringify(input))
  })
}

test("Devin's hook gives a session of the shown conversation its role text, and nothing else", async (t) => {
  const launch = await launchFiles(t)
  const event = (name, extra = {}) => ({ hook_event_name: name, session_id: 'native-a', ...extra })
  const role = {
    hookSpecificOutput: {
      hookEventName: 'SessionStart',
      additionalContext: '# ConsensFlow worker\n',
    },
  }
  const started = await runAsDevin(event('SessionStart', { source: 'startup' }), launch.env)
  assert.deepEqual([started.code, JSON.parse(started.stdout)], [0, role])

  // Another conversation's start, a subagent's, and every other event: nothing said.
  for (const other of [
    event('SessionStart', { session_id: 'native-b' }),
    event('SessionStart', { agent_id: 'subagent-1' }),
    event('SessionStart', { parent_session_id: 'native-a' }),
    event('UserPromptSubmit'),
    event('Stop'),
    event('PreToolUse'),
  ])
    assert.deepEqual(await runAsDevin(other, launch.env), { code: 0, stdout: '' })

  // After a /new the window shows native-b: its start is the one that gets the role text.
  await fs.appendFile(launch.wire, shows('native-b'))
  const renewed = await runAsDevin(
    event('SessionStart', { session_id: 'native-b', source: 'clear' }),
    launch.env,
  )
  assert.deepEqual([renewed.code, JSON.parse(renewed.stdout)], [0, role])
  assert.deepEqual(await runAsDevin(event('SessionStart'), launch.env), { code: 0, stdout: '' })
})

test("Devin's hook never stops Devin: no role text, no wire log, or input that is no event", async (t) => {
  const launch = await launchFiles(t)
  const start = { hook_event_name: 'SessionStart', session_id: 'native-a' }
  // A window with no role text starts plain.
  assert.deepEqual(await runAsDevin(start, { ...launch.env, CF_DEVIN_ROLE_FILE: '' }), {
    code: 0,
    stdout: '',
  })
  const missing = path.join(path.dirname(launch.role), 'gone.md')
  assert.deepEqual(await runAsDevin(start, { ...launch.env, CF_DEVIN_ROLE_FILE: missing }), {
    code: 0,
    stdout: '',
  })
  // Without the wire log nothing is known of the window: nothing said.
  const noWire = { ...launch.env, CHISEL_PURE_ACP_WIRE_LOG: path.join(missing, 'wire.jsonl') }
  assert.deepEqual(await runAsDevin(start, noWire), { code: 0, stdout: '' })
  assert.deepEqual(await runAsDevin('{"hook_event_name":', launch.env), { code: 0, stdout: '' })
  // Input past a mebibyte is refused before it is read as an event.
  const huge = { ...start, padding: 'x'.repeat(1024 * 1024) }
  assert.deepEqual(await runAsDevin(huge, launch.env), { code: 0, stdout: '' })
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
  const start = copy.hooks.SessionStart.at(-1).hooks[0]
  assert.match(start.command, /devin-hooks\.mjs/)
  assert.equal(start.timeout, 5)
  assert.equal(start.async, undefined)
  // Only a session's start needs ConsensFlow; the human's own hooks stay as they were.
  assert.deepEqual(copy.hooks.Stop, [{ hooks: [{ type: 'command', command: 'user-hook' }] }])
  for (const name of ['UserPromptSubmit', 'SessionEnd', 'FileChanged'])
    assert.equal(copy.hooks[name], undefined)
  assert.deepEqual(Object.keys(configuration.env), ['CHISEL_PURE_ACP_WIRE_LOG'])
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
