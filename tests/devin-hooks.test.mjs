import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { PANE_CF } from '../src/core/pane-cf.js'
import { prepareDevinIntegration } from '../src/devin-install.js'
import { selectedSession } from '../src/devin-wire.js'
import { devinFolders } from '../src/harnesses.js'
import { roleConfiguration } from '../src/role-skills.js'
import { tempEnv } from './helpers.mjs'

/** The line Devin's wire log gains when its window configures a conversation it opens. */
const shows = (sessionId) =>
  `${JSON.stringify({
    sessionId,
    update: { sessionUpdate: 'config_option_update', configOptions: [{ id: 'mode' }] },
  })}\n`

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

test('private installation preserves native defaults and hooks, and does not edit project/global files', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const native = path.join(devinFolders(f.env).config, 'config.json')
  await fs.mkdir(path.dirname(native), { recursive: true })
  const original =
    '{ // native JSONC\n "theme_mode": "dark", "hooks": {"Stop": [{"hooks": [{"type":"command","command":"user-hook"}]}]}, "agent":{"model":"native-default"}, }'
  await fs.writeFile(native, original)
  const configuration = await prepareDevinIntegration(f.env, { launchId: 'launch-a' })
  assert.equal(configuration.args[0], '--config')
  assert.ok(configuration.args[1].startsWith(f.env.CONSENSFLOW_HOME + path.sep))
  const copy = JSON.parse(await fs.readFile(configuration.args[1], 'utf8'))
  assert.equal(copy.theme_mode, 'dark')
  assert.equal(copy.agent.model, 'native-default')
  const start = copy.hooks.SessionStart.at(-1).hooks[0]
  // The bundle's own cf answers it, named in full (crates/cf-harness, devin::session_hook).
  assert.equal(start.command, `'${PANE_CF}' hook devin-session`)
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

test("on Windows the owner's own Devin config is read from %APPDATA%\\devin", async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  const env = { ...f.env, OS: 'Windows_NT', APPDATA: path.join(f.root, 'AppData', 'Roaming') }
  const native = path.join(env.APPDATA, 'devin', 'config.json')
  await fs.mkdir(path.dirname(native), { recursive: true })
  await fs.writeFile(native, '{ "theme_mode": "dark", "agent": { "model": "native-default" } }')
  const configuration = await prepareDevinIntegration(env, { launchId: 'launch-a' })
  const copy = JSON.parse(await fs.readFile(configuration.args[1], 'utf8'))
  assert.equal(copy.theme_mode, 'dark')
  assert.equal(copy.agent.model, 'native-default')
})

test('invalid launch identity or malformed native configuration fails without replacing native files', async (t) => {
  const f = tempEnv()
  t.after(f.cleanup)
  await assert.rejects(prepareDevinIntegration(f.env, { launchId: '../escape' }), /launch/)
  const native = path.join(devinFolders(f.env).config, 'config.json')
  await fs.mkdir(path.dirname(native), { recursive: true })
  await fs.writeFile(native, '{ invalid')
  await assert.rejects(prepareDevinIntegration(f.env, { launchId: 'launch-a' }), /configuration/)
  assert.equal(await fs.readFile(native, 'utf8'), '{ invalid')
})
