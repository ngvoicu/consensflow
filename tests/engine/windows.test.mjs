import assert from 'node:assert/strict'
import test from 'node:test'
import { childEnv, interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'

const AGENTS = {
  claude: { id: 'zeus', kind: 'claude-code', model: 'claude-opus-5', effort: 'max' },
  pi: { id: 'athena', kind: 'pi', model: 'openrouter/qwen/qwen3.8-27b', thinking: 'max' },
  codex: { id: 'hyperion', kind: 'codex', model: 'gpt-5.6-sol', effort: 'ultra' },
  opencode: { id: 'mani', kind: 'opencode', model: 'openrouter/moonshotai/kimi-k3' },
}

/**
 * How each harness's own window opens and resumes (`hosts/lib/windows.js`):
 * the command every adapter builds a pane from, its full-permission flag,
 * the billing guard it carries and the environment it must not inherit.
 */

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

test('window: codex opens cold on a positional prompt, id found afterwards', () => {
  // `codex [PROMPT]` opens the real window seeded with it. Unlike `exec
  // --json` it announces no id — an interactive codex talks to a person — so
  // the caller finds the session in codex's own rollout store, as for opencode.
  const w = interactiveStart(AGENTS.codex, null, 'the packet text')

  assert.equal(w.command, 'codex')
  assert.ok(!w.args.includes('exec'), 'the window is the interface, not the one-shot')
  assert.ok(!w.args.some((a) => String(a).startsWith('--session')), 'no id to give yet')
  assert.equal(w.args.at(-1), 'the packet text', 'the seed is the last positional')
  assert.ok(w.args.includes('--dangerously-bypass-approvals-and-sandbox'))
  assert.deepEqual(w.dropEnv, ['OPENAI_API_KEY'], 'the billing guard holds in a window too')
})

test('window: kimi is the one that cannot be seeded; an image agent opens Codex on its default model', () => {
  // `-p` is defined as non-interactive and kimi has no positional prompt, so
  // no flag it owns can open a seeded window. It streams turn one instead.
  assert.equal(interactiveStart({ id: 'ilmarinen', kind: 'kimi' }, 'anything', 'seed'), null)
  // The image designer draws with Codex's image tool, whatever model answers:
  // its window is Codex's own, with no model or effort of its own.
  const image = interactiveStart({ kind: 'image', model: 'codex-image' }, null, 'seed')
  assert.deepEqual(
    [image.command, image.args, image.dropEnv],
    ['codex', ['--dangerously-bypass-approvals-and-sandbox', 'seed'], ['OPENAI_API_KEY']],
  )
  assert.deepEqual(interactiveResume({ kind: 'image' }, 'thread-9').args.slice(0, 2), [
    'resume',
    'thread-9',
  ])
})

test('window: claude and pi refuse to open fresh without the id they need', () => {
  assert.equal(interactiveStart(AGENTS.claude, undefined, 'seed'), null)
  assert.equal(interactiveStart(AGENTS.pi, null, 'seed'), null)
})

test('window: every harness opens and resumes in full-permission mode', () => {
  // Gabriel, 2026-09-19: every harness opens in yolo mode, for every role.
  // Before this only a fresh Codex window did; OpenCode stalled on prompts.
  const devin = { id: 'odin', kind: 'devin', model: 'swe-1-6-slow' }
  const kimi = { id: 'ilmarinen', kind: 'kimi' }
  const has = (w, ...flags) => {
    const at = w.args.indexOf(flags[0])
    return at !== -1 && flags.every((flag, n) => w.args[at + n] === flag)
  }
  for (const w of [
    interactiveStart(AGENTS.claude, 'u', 's'),
    interactiveResume(AGENTS.claude, 'u', 's'),
  ])
    assert.ok(has(w, '--permission-mode', 'bypassPermissions'), w.args.join(' '))
  for (const w of [
    interactiveStart(AGENTS.codex, null, 's'),
    interactiveResume(AGENTS.codex, 't', 's'),
  ])
    assert.ok(has(w, '--dangerously-bypass-approvals-and-sandbox'), w.args.join(' '))
  for (const w of [
    interactiveStart(AGENTS.opencode, 'ses_1'),
    interactiveResume(AGENTS.opencode, 'ses_1'),
  ])
    assert.ok(has(w, '--auto'), w.args.join(' '))
  for (const w of [interactiveStart(AGENTS.pi, 'p', 's'), interactiveResume(AGENTS.pi, 'p', 's')])
    assert.ok(has(w, '--approve'), w.args.join(' '))
  for (const w of [interactiveStart(devin, null, 's'), interactiveResume(devin, 'd', 's')]) {
    assert.ok(has(w, '--permission-mode', 'dangerous'), w.args.join(' '))
    assert.ok(has(w, '--respect-workspace-trust', 'false'), w.args.join(' '))
  }
  assert.ok(has(interactiveResume(kimi, 'k'), '--auto'), 'kimi is paused, not exempt')
  // Seeds stay the last positional.
  assert.equal(interactiveStart(AGENTS.claude, 'u', 's').args.at(-1), 's')
  assert.equal(interactiveResume(AGENTS.codex, 't', 's').args.at(-1), 's')
})

test('window: a resume names the recorded native session on every kind', () => {
  // A resume reopens the identity the store recorded — never a fresh one,
  // never the last session in the folder.
  assert.deepEqual(interactiveResume(AGENTS.pi, 't-1-chief-x').args.slice(0, 2), [
    '--session-id',
    't-1-chief-x',
  ])
  const oc = interactiveResume(AGENTS.opencode, 'ses_recorded')
  assert.deepEqual(
    [oc.args[oc.args.indexOf('--session')], oc.args[oc.args.indexOf('--session') + 1]],
    ['--session', 'ses_recorded'],
  )
  assert.ok(!oc.args.includes('--continue'), 'resuming one session never continues the last')
  assert.deepEqual(interactiveResume(AGENTS.codex, 'thread-1').args.slice(0, 2), [
    'resume',
    'thread-1',
  ])
  assert.deepEqual(interactiveResume(AGENTS.claude, 'sess-1').args.slice(0, 2), [
    '--resume',
    'sess-1',
  ])
})

test('window: every resume can carry the follow-up as its seed', () => {
  assert.equal(interactiveResume(AGENTS.codex, 'thread-1', 'again?').args.at(-1), 'again?')
  assert.equal(interactiveResume(AGENTS.claude, 'sess-1', 'again?').args.at(-1), 'again?')
  assert.equal(interactiveResume(AGENTS.pi, 'jade-waves', 'again?').args.at(-1), 'again?')
  const oc = interactiveResume(AGENTS.opencode, 'ses_1', 'again?')
  assert.equal(
    oc.args.includes('--prompt'),
    false,
    'OpenCode receives tasks through its native API',
  )
})

test('window: a resume without a seed stays exactly the hand-over it was', () => {
  assert.deepEqual(interactiveResume(AGENTS.codex, 'thread-1').args, [
    'resume',
    'thread-1',
    '--dangerously-bypass-approvals-and-sandbox',
  ])
})

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

test('window: OpenCode opens the exact empty native session without plugins or a prompt', () => {
  const w = interactiveStart(AGENTS.opencode, 'ses_created')
  assert.equal(w.args[w.args.indexOf('--session') + 1], 'ses_created')
  assert.equal(w.args.includes('--prompt'), false)
  assert.equal(w.env.OPENCODE_CONFIG_CONTENT, undefined)
  assert.equal(w.env.CF_OPENCODE_LAUNCH_NONCE, undefined)
})

test('window: OpenCode worker opens its exact native session and leaves task delivery to the API', () => {
  const w = interactiveStart(AGENTS.opencode, 'ses_created', 'Tell me a joke.')
  assert.equal(w.args[w.args.indexOf('--session') + 1], 'ses_created')
  assert.equal(w.args.includes('--prompt'), false, '--session ignores CLI prompts')
  assert.deepEqual(Object.keys(w.env), ['CONSENSFLOW_CHILD'])
})

test('kimi: full permissions are IMPLIED by -p, so no flag is the correct shape', () => {
  // Probed 2026-08-24: `--auto` and `--yolo` are both REFUSED alongside `-p`,
  // and the session log records "Auto permission mode is active". A missing
  // danger flag here is the verified answer, not an omission.
  const agent = { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3' }
  const args = interactiveStart(agent, 'x', 's')

  assert.equal(args, null, 'kimi cannot open a window on an id it was given')
})

test('kimi: the window is its own interactive session, resumed', () => {
  const window = interactiveResume({ id: 'ilmarinen', kind: 'kimi' }, 'session_abc')

  assert.equal(window.command, 'kimi')
  assert.deepEqual(window.args, ['-S', 'session_abc', '--auto'])
  // No seed: an interactive kimi takes no first message, so a follow-up sent
  // this way arrives as a pane the user types into.
  assert.deepEqual(interactiveResume({ kind: 'kimi' }, 'session_abc', 'seed').args, [
    '-S',
    'session_abc',
    '--auto',
  ])
})

test('kimi: the selected K3 effort reaches a resumed window through its environment', () => {
  for (const effort of ['low', 'high', 'max']) {
    const agent = { id: 'ilmarinen', kind: 'kimi', model: 'moonshot-ai/kimi-k3', effort }
    const window = interactiveResume(agent, 'session_abc')
    assert.deepEqual(window.env, { CONSENSFLOW_CHILD: '1', KIMI_MODEL_THINKING_EFFORT: effort })
    assert.ok(!window.args.includes('--effort'), 'this CLI uses an environment control')
    const env = childEnv(
      { HOME: '/user', KIMI_CODE_HOME: '/kimi', KIMI_MODEL_THINKING_EFFORT: 'off' },
      window,
    )
    assert.equal(
      env.KIMI_MODEL_THINKING_EFFORT,
      effort,
      'saved selection wins over inherited override',
    )
    assert.equal(env.KIMI_CODE_HOME, '/kimi', 'native config and credentials stay in place')
  }
  for (const effort of ['medium', 'xhigh', 'ultra', 'off', 'on', 0, false]) {
    const agent = { kind: 'kimi', model: 'moonshot-ai/kimi-k3', effort }
    assert.throws(() => interactiveResume(agent, 'session_abc'), /low.*high.*max/)
  }
})

test('window: devin joins a family and its level into the model id it writes', () => {
  // Devin writes the level into the id (claude-opus-5-5-max); a catalog row
  // names the family and the effort (2026-10-01).
  const model = (agent) => {
    const w = interactiveStart({ kind: 'devin', ...agent }, null, 's')
    const at = w.args.indexOf('--model')
    return at === -1 ? null : w.args[at + 1]
  }
  assert.equal(model({ model: 'claude-opus-5-5', effort: 'max' }), 'claude-opus-5-5-max')
  assert.equal(model({ model: 'gpt-6-1-sol', effort: 'low' }), 'gpt-6-1-sol-low')
  assert.equal(model({ model: 'swe-1-6-slow' }), 'swe-1-6-slow', 'an id without a level as it is')
  assert.equal(model({ model: 'default', effort: 'max' }), null, "Devin's own setting")
})
