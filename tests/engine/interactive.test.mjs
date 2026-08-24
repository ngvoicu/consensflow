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

// --- the seed is a message, not a packet -----------------------------------

test('seed: a bare task travels bare — the window needs no scene-setting', async () => {
  const { createWindowSeed } = await import('../../hosts/lib/packets.js')

  const seed = createWindowSeed({ task: 'Tell me a joke.' })

  assert.equal(seed, 'Tell me a joke.')
})

test('seed: no packet scaffolding, whatever rides along', async () => {
  const { createWindowSeed } = await import('../../hosts/lib/packets.js')

  const seed = createWindowSeed({ task: 'review this', brief: 'GDPR pass', handoff: 'we said X' })

  // The window is a full interactive session: it knows how to work, and its
  // seed is the first thing the USER sees in their pane.
  assert.doesNotMatch(seed, /How to work/)
  assert.doesNotMatch(seed, /ConsensFlow Packet/)
  assert.doesNotMatch(seed, /Respond directly and conversationally/)
  assert.match(seed, /GDPR pass/)
  assert.match(seed, /we said X/)
})

test('seed: the message marker survives sections, so catchup unwraps to the question', async () => {
  const { createWindowSeed } = await import('../../hosts/lib/packets.js')

  const seed = createWindowSeed({ task: 'the actual question', brief: 'a brief' })

  const marker = seed.indexOf('## Message from the user')
  assert.ok(marker > 0, 'sections present → marker present')
  assert.equal(seed.slice(marker + '## Message from the user'.length).trim(), 'the actual question')
})

// --- kimi: the fifth harness, and the second that cannot open cold ---------

test('kimi: a one-shot carries the packet in argv, because nothing else works', async () => {
  const { buildRunnerInvocation } = await import('../../hosts/lib/runners.js')
  const agent = { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3' }

  const cold = buildRunnerInvocation(agent, '/tmp/packet.md', '/repo', undefined, 'THE PACKET')

  assert.equal(cold.command, 'kimi')
  assert.deepEqual(cold.args.slice(0, 2), ['-p', 'THE PACKET'])
  assert.equal(cold.stdinMode, 'none', 'there is no --prompt-file, and `-p -` is a literal dash')
  assert.ok(cold.args.includes('--output-format') && cold.args.includes('stream-json'))
  assert.deepEqual(
    [cold.args[cold.args.indexOf('-m')], cold.args[cold.args.indexOf('-m') + 1]],
    ['-m', 'moonshot-ai/kimi-k3'],
  )
  assert.ok(!cold.args.includes('-S'), 'a first turn has nothing to resume')
})

test('kimi: full permissions are IMPLIED by -p, so no flag is the correct shape', () => {
  // Probed 2026-08-24: `--auto` and `--yolo` are both REFUSED alongside `-p`,
  // and the session log records "Auto permission mode is active". A missing
  // danger flag here is the verified answer, not an omission.
  const agent = { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3' }
  const args = interactiveStart(agent, 'x', 's')

  assert.equal(args, null, 'kimi cannot open a window on an id it was given')
})

test('kimi: effort is ignored, because Kimi Code has no effort flag', async () => {
  const { buildRunnerInvocation } = await import('../../hosts/lib/runners.js')
  // `default_effort` lives per-model in the user's own config.toml — the file
  // that also holds their API key. Inventing a flag would fail the run.
  const args = buildRunnerInvocation(
    { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'max' },
    '/tmp/packet.md',
    '/repo',
    undefined,
    'p',
  ).args

  assert.ok(!args.some((a) => String(a).includes('effort')))
  assert.ok(!args.includes('max'))
})

test('kimi: no billing guard exists to carry — its key is in a config file', async () => {
  const { buildRunnerInvocation } = await import('../../hosts/lib/runners.js')
  const invocation = buildRunnerInvocation(
    { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3' },
    '/tmp/packet.md',
    '/repo',
    undefined,
    'p',
  )

  // claude and codex strip an env var whose presence silently switches
  // billing. Kimi Code authenticates from config.toml, so there is nothing to
  // strip: the empty guard is a finding, and this pins it as one.
  assert.deepEqual(invocation.dropEnv, [])
  assert.equal(invocation.env.CONSENSFLOW_CHILD, '1')
})

test('kimi: resuming replaces nothing and adds -S', async () => {
  const { buildRunnerInvocation } = await import('../../hosts/lib/runners.js')
  const args = buildRunnerInvocation(
    { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3' },
    '/tmp/packet.md',
    '/repo',
    { sessionId: 'session_abc' },
    'follow up',
  ).args

  assert.deepEqual([args[args.indexOf('-S')], args[args.indexOf('-S') + 1]], ['-S', 'session_abc'])
})

test('kimi: the session id is read from its resume hint', async () => {
  const { extractSessionId } = await import('../../hosts/lib/runners.js')

  assert.equal(
    extractSessionId('kimi', {
      role: 'meta',
      type: 'session.resume_hint',
      session_id: 'session_1',
    }),
    'session_1',
  )
  // It arrives at the END of kimi's stream, unlike codex's, so a run killed
  // early simply records none — never a wrong one.
  assert.equal(extractSessionId('kimi', { role: 'assistant', content: 'hi' }), null)
})

test('kimi: the window is its own interactive session, resumed', async () => {
  const { interactiveResume } = await import('../../hosts/lib/runners.js')
  const window = interactiveResume({ id: 'ilmarinen', kind: 'kimi' }, 'session_abc')

  assert.equal(window.command, 'kimi')
  assert.deepEqual(window.args, ['-S', 'session_abc'])
  // No seed: an interactive kimi takes no first message, so a follow-up sent
  // this way arrives as a pane the user types into.
  assert.deepEqual(interactiveResume({ kind: 'kimi' }, 'session_abc', 'seed').args, [
    '-S',
    'session_abc',
  ])
})
