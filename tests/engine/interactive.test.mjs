import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { discoverOpencodeSession } from '../../hosts/lib/harness-transcript.js'
import { childEnv, interactiveResume, interactiveStart } from '../../hosts/lib/runners.js'

const AGENTS = {
  claude: { id: 'zeus', kind: 'claude-code', model: 'claude-opus-5', effort: 'max' },
  pi: { id: 'athena', kind: 'pi', model: 'openrouter/qwen/qwen3.8-27b', thinking: 'max' },
  codex: { id: 'hyperion', kind: 'codex', model: 'gpt-5.6-sol', effort: 'ultra' },
  opencode: { id: 'mani', kind: 'opencode', model: 'openrouter/moonshotai/kimi-k3' },
}

// --- a window from the very first turn -------------------------------------

test('window: claude opens fresh on an id we mint, seeded with the packet', () => {
  const w = interactiveStart(AGENTS.claude, 'uuid-1', 'the packet text')

  assert.equal(w.command, 'claude')
  assert.deepEqual(w.args.slice(0, 2), ['--session-id', 'uuid-1'])
  assert.equal(w.args.at(-1), 'the packet text', 'the seed is the last positional')
  assert.ok(
    w.args.includes('--model') && w.args.includes('--effort'),
    'same model and effort as a run',
  )
})

test('window: pi opens fresh on the id we mint — --session-id creates it', () => {
  const w = interactiveStart(AGENTS.pi, 'jade-waves', 'the packet text')

  assert.equal(w.command, 'pi')
  assert.deepEqual(w.args.slice(0, 2), ['--session-id', 'jade-waves'])
  assert.ok(w.args.includes('--thinking'), 'effort travels as pi thinking')
  assert.equal(w.args.at(-1), 'the packet text')
})

test('window: opencode opens without an id — the store will tell us later', () => {
  const w = interactiveStart(AGENTS.opencode, null, 'the packet text')

  assert.equal(w.command, 'opencode')
  assert.ok(!w.args.includes('--session'), 'there is no id to give yet')
  const at = w.args.indexOf('--prompt')
  assert.equal(w.args[at + 1], 'the packet text', 'the seed rides --prompt')
})

test('window: codex cannot pre-set an interactive id — stream first, then resume', () => {
  // No flag exists. The caller runs turn 1 through the one-shot machinery
  // (which captures thread_id) and opens `codex resume <id>` on it.
  assert.equal(interactiveStart(AGENTS.codex, 'anything', 'seed'), null)
  assert.equal(interactiveStart({ kind: 'image' }, 'x', 'seed'), null)
})

test('window: claude and pi refuse to open fresh without the id they need', () => {
  assert.equal(interactiveStart(AGENTS.claude, undefined, 'seed'), null)
  assert.equal(interactiveStart(AGENTS.pi, null, 'seed'), null)
})

// --- resuming a window, now with a first message ---------------------------

test('window: every resume can carry the follow-up as its seed', () => {
  assert.equal(interactiveResume(AGENTS.codex, 'thread-1', 'again?').args.at(-1), 'again?')
  assert.equal(interactiveResume(AGENTS.claude, 'sess-1', 'again?').args.at(-1), 'again?')
  assert.equal(interactiveResume(AGENTS.pi, 'jade-waves', 'again?').args.at(-1), 'again?')
  const oc = interactiveResume(AGENTS.opencode, 'ses_1', 'again?')
  assert.equal(oc.args[oc.args.indexOf('--prompt') + 1], 'again?')
})

test('window: a resume without a seed stays exactly the hand-over it was', () => {
  assert.deepEqual(interactiveResume(AGENTS.codex, 'thread-1').args, ['resume', 'thread-1'])
})

// --- the guards hold on the attached path ----------------------------------

test('window: the billing guard is the same one the one-shot carries', () => {
  // For a while `cf attach` spawned with the full environment: every attached
  // turn could silently switch a subscription login to API billing.
  assert.deepEqual(interactiveStart(AGENTS.claude, 'u', 's').dropEnv, ['ANTHROPIC_API_KEY'])
  assert.deepEqual(interactiveResume(AGENTS.claude, 'u').dropEnv, ['ANTHROPIC_API_KEY'])
  assert.deepEqual(interactiveResume(AGENTS.codex, 't').dropEnv, ['OPENAI_API_KEY'])
})

test('window: every window is marked a child, so agents never nest', () => {
  for (const w of [
    interactiveStart(AGENTS.claude, 'u', 's'),
    interactiveStart(AGENTS.pi, 'n', 's'),
    interactiveStart(AGENTS.opencode, null, 's'),
    interactiveResume(AGENTS.codex, 't'),
  ]) {
    assert.equal(w.env.CONSENSFLOW_CHILD, '1')
  }
})

test('childEnv applies the guards a window declares', () => {
  const base = {
    PATH: '/bin',
    ANTHROPIC_API_KEY: 'leak',
    CMUX_SOCKET_CAPABILITY: 'token',
    CMUX_CLAUDE_HOOK_CMUX_BIN: '/x',
    CMUX_SURFACE_ID: 'pane-1',
  }
  const env = childEnv(base, interactiveStart(AGENTS.claude, 'u', 's'))

  assert.equal(env.ANTHROPIC_API_KEY, undefined, 'billing key stripped')
  assert.equal(env.CMUX_SOCKET_CAPABILITY, undefined, 'pane control stripped')
  assert.equal(env.CMUX_CLAUDE_HOOK_CMUX_BIN, undefined)
  assert.equal(env.CMUX_SURFACE_ID, 'pane-1', 'identity vars survive — only control is stripped')
  assert.equal(env.CONSENSFLOW_CHILD, '1')
  assert.equal(env.PATH, '/bin')
})

// --- finding the session opencode minted -----------------------------------

test('discovery: the newest opencode session in this directory, born since the spawn', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-disc-'))
  try {
    const env = { HOME: dir, XDG_DATA_HOME: path.join(dir, '.local', 'share') }
    const store = path.join(env.XDG_DATA_HOME, 'opencode', 'storage', 'session', 'proj-hash')
    await mkdir(store, { recursive: true })
    const write = (id, directory, created) =>
      writeFile(
        path.join(store, `${id}.json`),
        JSON.stringify({ id, directory, time: { created } }),
      )
    await write('ses_old', '/tmp/ws', 100)
    await write('ses_other', '/tmp/elsewhere', 900)
    await write('ses_new', '/tmp/ws', 800)

    assert.equal(await discoverOpencodeSession('/tmp/ws', 500, env), 'ses_new')
    assert.equal(await discoverOpencodeSession('/tmp/ws', 900, env), null, 'nothing born since')
    assert.equal(await discoverOpencodeSession('/nowhere', 0, env), null)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

test('discovery: a missing store is null, never an error', async () => {
  assert.equal(await discoverOpencodeSession('/tmp/ws', 0, { HOME: '/nonexistent-cf' }), null)
})
