/**
 * Claude Code's scenarios (`src/adapters/claude-code.js`, with
 * `src/claude-install.js` and `src/role-skills.js`): its launches, fresh,
 * resumed and refused, with the files each leaves; its looks, by Claude's
 * own status files and its transcript; whether it is ready for a paste; and
 * its deliveries. The cases of `tests/adapter-claude.test.mjs`, and more.
 */

/** The launch's id: a uuid, as the engine mints one. */
export const LAUNCH = '0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b'
const SESSION = '0f8fad5b-d9cb-469f-a165-70867728950e'
const OTHER = '6a2f41ac-8c1d-4c55-9b4e-2f1e8a3d9c70'

const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  CLAUDE_CONFIG_DIR: '$ROOT/claude',
  PATH: '$ROOT/bin',
}

const worker = {
  id: 3,
  projectId: 1,
  handle: 'zeus',
  role: 'worker',
  agent: 'zeus',
  harness: 'claude-code',
}
const chief = { ...worker, handle: 'chief', role: 'chief', agent: null }
const TASK = '[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser'

/** A launch to prepare, `fields` over a worker's. */
const launch = (fields = {}) => ({
  launchId: LAUNCH,
  participant: worker,
  role: 'worker',
  instructions: '# ConsensFlow worker\n\nRole text for the test.',
  directory: '/work/app',
  resume: null,
  message: TASK,
  agent: { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' },
  ...fields,
})

/** One Claude transcript line, in the shape Claude Code writes, at second `n`. */
function record(sessionId, n, fields) {
  return `${JSON.stringify({
    sessionId,
    version: '2.1.277',
    timestamp: new Date(Date.parse('2026-09-19T12:00:00Z') + n * 1000).toISOString(),
    uuid: `${sessionId}-${n}`,
    isSidechain: false,
    ...fields,
  })}\n`
}
const user = (sessionId, n, text) =>
  record(sessionId, n, { type: 'user', message: { role: 'user', content: text } })
const answer = (sessionId, n, text, extra = {}) =>
  record(sessionId, n, {
    type: 'assistant',
    message: {
      id: `${sessionId}-message-${n}`,
      role: 'assistant',
      content: [{ type: 'text', text }],
      stop_reason: 'end_turn',
    },
    ...extra,
  })
const stopped = (sessionId, n) =>
  record(sessionId, n, {
    type: 'system',
    subtype: 'stop_hook_summary',
    preventedContinuation: false,
    hookCount: 1,
  })
/** The transcript of `sessionId`, Claude's project folder for /work/app. */
const transcript = (sessionId, lines) => ({
  write: `$ROOT/claude/projects/-work-app/${sessionId}.jsonl`,
  text: lines.join(''),
})

/** A scenario of a fresh worker window: Claude installed, prepared, opened on this process. */
const opened = (name, steps, fields = {}) => ({
  name: `claude: ${name}`,
  harness: 'claude-code',
  env: ENV,
  steps: [
    { executable: 'claude' },
    { prepare: launch(fields), draws: { $SESSION: 'nativeSession' } },
    { opened: { pid: '$PID' } },
    ...steps,
  ],
})

const snapshot = (fields) => ({
  'pane.snapshot': [{ ok: true, pasteInFlight: false, unsent: false, ...fields }],
})

export function claudeScenarios() {
  const prepared = (name, fields, before = [], draws = { $SESSION: 'nativeSession' }) => ({
    name: `claude: ${name}`,
    harness: 'claude-code',
    env: ENV,
    steps: [...before, { prepare: launch(fields), draws }],
  })
  const installed = { executable: 'claude' }
  const plans = [
    prepared('a fresh worker opens on its own session id, in full permission, the task last', {}, [
      installed,
    ]),
    prepared(
      'the chief keeps its connectors, asks the human in its own window, and has no model of its own',
      { role: 'chief', participant: chief, agent: null, message: null },
      [installed],
    ),
    prepared(
      'a reviewer is given its task as a window takes text',
      {
        role: 'reviewer',
        message: 'Review\r\nthis \u001b[31mred\u001b[0m line\ttab\u0007bell\u0085',
        agent: { id: 'r', kind: 'claude-code', model: 'claude-opus-5', effort: '' },
      },
      [installed],
    ),
    prepared(
      'a model alone, and an effort alone',
      { agent: { kind: 'claude-code', model: 'claude-opus-5' } },
      [installed],
    ),
    prepared('an effort with no model', { agent: { kind: 'claude-code', effort: 'max' } }, [
      installed,
    ]),
    prepared(
      'a conversation Claude kept is resumed on its session',
      { resume: SESSION, message: null },
      [installed, transcript(SESSION, [user(SESSION, 1, 'Write the parser')])],
      {},
    ),
    prepared(
      'a resumed conversation is given its follow-up',
      { resume: SESSION, message: 'And the tests' },
      [installed, transcript(SESSION, [user(SESSION, 1, 'Write the parser')])],
      {},
    ),
    prepared(
      'a conversation Claude never kept starts afresh under the same id',
      { resume: OTHER, message: 'Review T-1' },
      [installed],
      {},
    ),
    prepared('a window with no first message', { message: null }, [installed]),
    prepared('Claude not installed is refused, and nothing is written', {}),
    prepared(
      'a window with no role text is refused, after its settings were written',
      { instructions: '' },
      [installed],
    ),
  ]
  const live = (fields) => ({
    status: { pid: '$PID', sessionId: '$SESSION', kind: 'interactive', ...fields },
  })
  const looks = [
    opened('a new window with neither a transcript nor a status is not settled', [{ observe: {} }]),
    opened('a new window with no transcript is idle once Claude says so', [
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('a settled transcript and an idle Claude settle the window', [
      transcript('$SESSION', [
        user('$SESSION', 1, 'Write the parser'),
        answer('$SESSION', 2, 'Done.'),
        stopped('$SESSION', 3),
      ]),
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('a busy Claude is not settled, its transcript settled or not', [
      transcript('$SESSION', [
        user('$SESSION', 1, 'Write the parser'),
        answer('$SESSION', 2, 'Done.'),
        stopped('$SESSION', 3),
      ]),
      live({ status: 'busy' }),
      { observe: {} },
    ]),
    opened('a Claude waiting says why', [
      live({ status: 'waiting', waitingFor: 'permission prompt' }),
      { observe: {} },
      live({ status: 'waiting' }),
      { observe: {} },
    ]),
    opened('a shell status is idle, an unknown one none', [
      live({ status: 'shell' }),
      { observe: {} },
      live({ status: 'sleeping' }),
      { observe: {} },
    ]),
    opened('a window a /clear left on another conversation is followed there', [
      live({ status: 'idle', sessionId: OTHER }),
      { observe: {} },
      { follow: OTHER },
      { observe: {} },
    ]),
    opened("a window's Claude found by the first status file naming its conversation", [
      { opened: { pid: '$DEAD' } },
      { status: { pid: '$PID', sessionId: '$SESSION', status: 'idle' } },
      { observe: {} },
      { status: { pid: '$PID', sessionId: OTHER, status: 'idle' } },
      { observe: {} },
    ]),
    opened("a dead process's status file and an unreadable one say nothing", [
      { status: { pid: '$DEAD', sessionId: '$SESSION', status: 'idle' } },
      { write: '$ROOT/claude/sessions/12.json', text: '{not json' },
      { write: '$ROOT/claude/sessions/notes.json', text: '{}' },
      { observe: {} },
    ]),
    opened('a refused request is exhausted quota, with the reset its text names', [
      transcript('$SESSION', [
        user('$SESSION', 1, 'Write the parser'),
        answer('$SESSION', 2, "You've hit your limit · resets in 2 hours", {
          isApiErrorMessage: true,
          apiErrorStatus: 429,
          message: {
            id: 'refused-2',
            role: 'assistant',
            content: [{ type: 'text', text: "You've hit your limit · resets in 2 hours" }],
            stop_reason: 'stop_sequence',
          },
        }),
      ]),
      live({ status: 'idle' }),
      { observe: {} },
    ]),
  ]
  const ready = [
    opened('a window with no status is ready when its snapshot says so', [
      { ready: {}, answers: snapshot({}) },
      { ready: {}, answers: snapshot({ pasteInFlight: true }) },
      { ready: {}, answers: snapshot({ unsent: true }) },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false, error: 'stale' }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ throws: 'bridge ended' }] } },
    ]),
    opened('a window that shows another conversation is not ready', [
      live({ status: 'idle', sessionId: OTHER }),
      { ready: {}, answers: snapshot({}) },
    ]),
  ]
  const deliveries = [
    opened('a message is pasted as a window takes text', [
      { deliver: 'Hello\r\nworld\u001b', answers: { 'pane.write_paste': [{ ok: true }] } },
      {
        deliver: 'Refused',
        answers: { 'pane.write_paste': [{ ok: false, admitted: false, error: 'stale pane' }] },
      },
      {
        deliver: 'Lost on the way',
        answers: { 'pane.write_paste': [{ throws: 'bridge ended', error: 'eof' }] },
      },
      {
        deliver: 'Half written',
        answers: { 'pane.write_paste': [{ ok: false, error: 'deadline' }] },
      },
      {
        deliver: 'Refused, no reason given',
        answers: { 'pane.write_paste': [{ ok: false, admitted: false }] },
      },
      {
        deliver: 'Unanswered',
        answers: { 'pane.write_paste': [{ ok: false }] },
      },
    ]),
  ]
  return [...plans, ...looks, ...ready, ...deliveries]
}
