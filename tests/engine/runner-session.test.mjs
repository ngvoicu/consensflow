import assert from 'node:assert/strict'
import test from 'node:test'
import { buildRunnerInvocation, extractSessionId } from '../../hosts/lib/runners.js'

const PACKET = '/tmp/packet.md'
const CWD = '/tmp/ws'

const AGENTS = {
  pi: { id: 'ares', kind: 'pi', model: 'openrouter/x-ai/grok-4.6', thinking: 'high' },
  claude: { id: 'zeus', kind: 'claude-code', model: 'claude-opus-5', effort: 'max' },
  codex: { id: 'hyperion', kind: 'codex', model: 'gpt-5.6-sol', effort: 'ultra' },
  opencode: { id: 'mani', kind: 'opencode', model: 'openrouter/moonshotai/kimi-k3' },
}

const build = (agent, session) => buildRunnerInvocation(agent, PACKET, CWD, session)

// --- one-shot: unchanged when no conversation is being continued -----------

test('runner: with no session, every harness keeps its one-shot flags', () => {
  assert.ok(build(AGENTS.pi).args.includes('--no-session'))
  assert.ok(build(AGENTS.claude).args.includes('--no-session-persistence'))
  assert.ok(build(AGENTS.codex).args.includes('--ephemeral'))
  // opencode never asked for a session in the first place.
  assert.ok(!build(AGENTS.opencode).args.includes('--session'))
})

// --- resuming: the kill-session flag is replaced, never merely added ------

test('runner: pi resumes by an id we mint', () => {
  const args = build(AGENTS.pi, { sessionId: 'bubble-sky-1' }).args

  assert.deepEqual(
    [args[args.indexOf('--session-id')], args[args.indexOf('--session-id') + 1]],
    ['--session-id', 'bubble-sky-1'],
  )
  assert.ok(!args.includes('--no-session'), 'asking for a session and refusing one are exclusive')
})

test('runner: claude resumes by the id it gave us', () => {
  const args = build(AGENTS.claude, { sessionId: 'sess-abc' }).args

  assert.deepEqual(
    [args[args.indexOf('--resume')], args[args.indexOf('--resume') + 1]],
    ['--resume', 'sess-abc'],
  )
  assert.ok(!args.includes('--no-session-persistence'))
})

test('runner: codex resumes through its own subcommand', () => {
  const args = build(AGENTS.codex, { sessionId: 'thread-xyz' }).args

  // `codex exec resume <id>` — the id is positional, not a flag.
  assert.deepEqual(args.slice(0, 3), ['exec', 'resume', 'thread-xyz'])
  assert.ok(!args.includes('--ephemeral'), 'ephemeral would discard the session we just asked for')
})

test('runner: opencode resumes by session id', () => {
  const args = build(AGENTS.opencode, { sessionId: 'oc-123' }).args

  assert.deepEqual(
    [args[args.indexOf('--session')], args[args.indexOf('--session') + 1]],
    ['--session', 'oc-123'],
  )
})

// --- the guards must survive both shapes ----------------------------------

test('runner: billing guards and permissions are identical either way', () => {
  for (const [name, agent] of Object.entries(AGENTS)) {
    const cold = build(agent)
    const warm = build(agent, { sessionId: 'whatever' })

    assert.deepEqual(warm.dropEnv, cold.dropEnv, `${name}: billing guard changed`)
    assert.deepEqual(warm.env, cold.env, `${name}: child env changed`)
    assert.equal(warm.command, cold.command)
    assert.equal(warm.stdinMode, cold.stdinMode, `${name}: how the packet arrives changed`)

    for (const flag of [
      '--dangerously-skip-permissions',
      '--dangerously-bypass-approvals-and-sandbox',
      '--auto',
    ]) {
      assert.equal(
        warm.args.includes(flag),
        cold.args.includes(flag),
        `${name}: permission flag ${flag} differs between cold and warm`,
      )
    }
  }
})

test('runner: the model and effort still reach the child when resuming', () => {
  const args = build(AGENTS.codex, { sessionId: 'thread-xyz' }).args

  assert.ok(args.includes('gpt-5.6-sol'))
  assert.ok(args.some((arg) => String(arg).includes('ultra')))
})

// --- [TEST-THR-07] capturing the id each harness hands back ---------------

test('runner: each harness session id is read from its own stream shape', () => {
  assert.equal(
    extractSessionId('claude-code', { type: 'system', session_id: 'claude-sess-1' }),
    'claude-sess-1',
  )
  assert.equal(
    extractSessionId('codex', { type: 'thread.started', thread_id: 'codex-thread-1' }),
    'codex-thread-1',
  )
  assert.equal(
    extractSessionId('opencode', { type: 'message', sessionID: 'oc-sess-1' }),
    'oc-sess-1',
  )
})

test('runner: pi needs no capture — we minted its id', () => {
  assert.equal(extractSessionId('pi', { type: 'message_end', sessionId: 'ignored' }), null)
})

test('runner: a line without an id is not an error', () => {
  for (const line of [null, undefined, {}, { type: 'text' }, 'a string', 42]) {
    assert.equal(extractSessionId('claude-code', line), null)
    assert.equal(extractSessionId('codex', line), null)
    assert.equal(extractSessionId('opencode', line), null)
  }
})

test('runner: an id is only read from the harness that uses that field', () => {
  // codex emits thread_id; claude must not accept it, or a mixed roster would
  // cross-wire two conversations.
  assert.equal(extractSessionId('claude-code', { thread_id: 'codex-thread-1' }), null)
  assert.equal(extractSessionId('codex', { session_id: 'claude-sess-1' }), null)
})
