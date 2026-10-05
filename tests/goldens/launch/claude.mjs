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

/**
 * The `n`th session id a fresh Claude window draws: the `n`th 16 bytes of
 * the scripted stream (`runner.mjs`), as a version 4 uuid.
 */
function drawn(n) {
  const bytes = Buffer.from(
    Array.from({ length: 16 }, (_, index) => ((16 * n + index) * 7 + 3) % 256),
  )
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = bytes.toString('hex')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
const DRAWN = drawn(0)
/** A second launch's id. */
const SECOND = '1b2c3d4e-5f60-4172-8b9c-0d1e2f3a4b5c'

const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  CLAUDE_CONFIG_DIR: '$ROOT/claude',
  PATH: '$ROOT/bin',
}
/** An environment that names no home, nor Claude's folder. */
const HOMELESS = { CONSENSFLOW_HOME: '$ROOT/consensflow', PATH: '$ROOT/bin' }

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
    { prepare: launch(fields) },
    { opened: { pid: '$PID' } },
    ...steps,
  ],
})

const snapshot = (fields) => ({
  'pane.snapshot': [{ ok: true, pasteInFlight: false, unsent: false, ...fields }],
})

/** A look Rust answers unsettled with nothing in it, where Node answered otherwise, and why. */
const notSettled = (why) => ({
  why,
  answer: {
    answer: { items: [], settled: false, waiting: null, failed: false, quota: null },
  },
})

export function claudeScenarios() {
  const prepared = (name, fields, before = [], { env = ENV, after = [], kept } = {}) => ({
    name: `claude: ${name}`,
    harness: 'claude-code',
    env,
    steps: [...before, { prepare: launch(fields), ...(kept ? { kept } : {}) }, ...after],
  })
  const resumedLive = [
    { opened: { pid: '$PID' } },
    { status: { pid: '$PID', sessionId: SESSION, status: 'idle' } },
    { observe: {} },
    { ready: {}, answers: snapshot({}) },
  ]
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
      'a conversation Claude kept is resumed on its session, and its window looks there',
      { resume: SESSION, message: null },
      [installed, transcript(SESSION, [user(SESSION, 1, 'Write the parser')])],
      { after: resumedLive },
    ),
    prepared(
      'a resumed conversation is given its follow-up',
      { resume: SESSION, message: 'And the tests' },
      [installed, transcript(SESSION, [user(SESSION, 1, 'Write the parser')])],
    ),
    prepared(
      'a conversation Claude never kept starts afresh under the same id, its window there',
      { resume: SESSION, message: 'Review T-1' },
      [installed],
      { after: resumedLive },
    ),
    prepared('a window with no first message', { message: null }, [installed]),
    prepared('Claude not installed is refused, and nothing is written', {}),
    prepared(
      'a window with no role text is refused, after its settings were written',
      { instructions: '' },
      [installed],
    ),
    prepared("a ConsensFlow folder that is a file refuses the launch in Node's words", {}, [
      installed,
      { write: '$ROOT/consensflow', text: 'x' },
    ]),
    prepared('a role folder in the way refuses the launch, after its settings were written', {}, [
      installed,
      { write: `$ROOT/consensflow/integrations/claude/${LAUNCH}/role`, text: 'x' },
    ]),
    prepared(
      'a resumed conversation with no home to look in is refused, after its files were written',
      { resume: SESSION, message: null },
      [installed],
      { env: HOMELESS },
    ),
    prepared(
      'a fresh window with no home is refused, after its files were written',
      {},
      [installed],
      {
        env: HOMELESS,
        kept: {
          why: 'no home is "missing home in env": Node read the process\'s own, and opened a window whose status it read from another home',
          answer: { throws: 'missing home in env' },
        },
      },
    ),
    prepared(
      'a conversation of an empty id is refused',
      { resume: '', message: null },
      [installed],
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError: no ledger holds an empty id",
          answer: { throws: 'a Claude window opens on a session id' },
        },
      },
    ),
  ]
  const live = (fields) => ({
    status: { pid: '$PID', sessionId: DRAWN, kind: 'interactive', ...fields },
  })
  const looks = [
    opened('a new window with neither a transcript nor a status is not settled', [{ observe: {} }]),
    opened('a new window with no transcript is idle once Claude says so', [
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('a settled transcript and an idle Claude settle the window', [
      transcript(DRAWN, [
        user(DRAWN, 1, 'Write the parser'),
        answer(DRAWN, 2, 'Done.'),
        stopped(DRAWN, 3),
      ]),
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('a busy Claude is not settled, its transcript settled or not', [
      transcript(DRAWN, [
        user(DRAWN, 1, 'Write the parser'),
        answer(DRAWN, 2, 'Done.'),
        stopped(DRAWN, 3),
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
      { status: { pid: '$PID', sessionId: DRAWN, status: 'idle' } },
      { observe: {} },
      { status: { pid: '$PID', sessionId: OTHER, status: 'idle' } },
      { observe: {} },
    ]),
    opened("a dead process's status file and an unreadable one say nothing", [
      { status: { pid: '$DEAD', sessionId: DRAWN, status: 'idle' } },
      { write: '$ROOT/claude/sessions/12.json', text: '{not json' },
      { write: '$ROOT/claude/sessions/notes.json', text: '{}' },
      { observe: {} },
    ]),
    opened("the pane's own child is taken before another Claude naming the conversation", [
      { status: { pid: '$OTHER', sessionId: DRAWN, status: 'busy' } },
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened("a Claude keeps its place among the files, with its last file's status", [
      { opened: { pid: '$DEAD' } },
      { status: { pid: '$PID', sessionId: OTHER, status: 'idle' }, file: '1.json' },
      { status: { pid: '$OTHER', sessionId: DRAWN, status: 'busy' }, file: '2.json' },
      { status: { pid: '$PID', sessionId: DRAWN, status: 'idle' }, file: '3.json' },
      { observe: {} },
    ]),
    opened('the Claude a window found is kept when its file goes, never traded for another', [
      { opened: { pid: '$DEAD' } },
      { status: { pid: '$OTHER', sessionId: DRAWN, status: 'idle' }, file: '2.json' },
      { observe: {} },
      { remove: '$ROOT/claude/sessions/2.json' },
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('a Claude that names the conversation later is found then', [
      { opened: { pid: '$DEAD' } },
      { observe: {} },
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('an idle Claude whose transcript cannot be read is settled and empty', [
      { write: `$ROOT/claude/projects/-work-app/${DRAWN}.jsonl`, text: '{not json}\n' },
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened('an idle Claude with a turn its transcript has not finished is not settled', [
      transcript(DRAWN, [user(DRAWN, 1, 'Write the parser')]),
      live({ status: 'idle' }),
      { observe: {} },
    ]),
    opened("a switch keeps the old conversation's items, its failure and its quota", [
      transcript(DRAWN, [
        user(DRAWN, 1, 'Write the parser'),
        answer(DRAWN, 2, "You've hit your limit · resets in 2 hours", {
          isApiErrorMessage: true,
          apiErrorStatus: 429,
        }),
      ]),
      live({ status: 'waiting', waitingFor: 'a dialog', sessionId: OTHER }),
      { observe: {} },
    ]),
    opened('a status file of process 0 says nothing', [
      { status: { pid: 0, sessionId: DRAWN, status: 'idle' }, file: '7.json' },
      {
        observe: {},
        kept: notSettled(
          'Node asked the system of process 0, its own group or itself, and found it alive',
        ),
      },
    ]),
    opened("only Claude's own four words are a state", [
      { status: { pid: '$PID', sessionId: OTHER, status: 'constructor' } },
      {
        observe: {},
        kept: notSettled('Node looked the word up in an object, and found what every object has'),
      },
      { status: { pid: '$PID', sessionId: DRAWN, status: ['idle'] } },
      { observe: {}, kept: notSettled('Node took a list for its text') },
      { status: { pid: '$PID', sessionId: DRAWN, status: { toString: 1 } } },
      {
        observe: {},
        kept: notSettled('Node failed the look on an object with a toString of its own'),
      },
    ]),
    opened('a status file JSON writes past what a double or a depth holds says nothing', [
      {
        statusText: `{"pid":$PID,"sessionId":"${DRAWN}","status":"idle","big":1e400}`,
        file: '1.json',
      },
      {
        observe: {},
        kept: notSettled('Node read 1e400 as Infinity; JSON here holds no such number'),
      },
      {
        statusText: `{"pid":$PID,"sessionId":"${DRAWN}","status":"idle","deep":${'['.repeat(200)}${']'.repeat(200)}}`,
        file: '1.json',
      },
      {
        observe: {},
        kept: notSettled('Node read JSON at any depth; it is read here 127 levels deep'),
      },
    ]),
    opened('a refused request is exhausted quota, with the reset its text names', [
      transcript(DRAWN, [
        user(DRAWN, 1, 'Write the parser'),
        answer(DRAWN, 2, "You've hit your limit · resets in 2 hours", {
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
  const draws = [
    {
      name: 'claude: two fresh windows draw two sessions, the second opening on its own',
      harness: 'claude-code',
      env: ENV,
      steps: [
        { executable: 'claude' },
        { prepare: launch() },
        { prepare: launch({ launchId: SECOND }) },
        { opened: { pid: '$PID' } },
        { status: { pid: '$PID', sessionId: drawn(1), status: 'idle' } },
        { observe: {} },
      ],
    },
    opened('a status naming half a surrogate pair is read as Node read it but for that half', [
      { statusText: '{"pid":$PID,"sessionId":"\\ud800","status":"idle"}', file: '1.json' },
      {
        observe: {},
        kept: {
          why: 'a Rust text holds no half of a pair: JSON read here writes U+FFFD for it',
          answer: {
            answer: {
              items: [],
              settled: false,
              waiting: null,
              failed: false,
              quota: null,
              switched: { nativeSession: '\ufffd' },
            },
          },
        },
      },
    ]),
  ]
  const waits = [
    opened("a look reads Claude's status before the conversation it waits for", [
      live({ status: 'idle' }),
      { holdLooks: true },
      { observe: {} },
      { holdLooks: false },
      live({ status: 'busy' }),
      { release: 'look' },
    ]),
    opened('a window closed while its paste is held keeps the paste going, its files gone', [
      { deliver: 'Hello', answers: { 'pane.write_paste': [{ held: true }] } },
      { close: {} },
      { release: 'pane.write_paste', answer: { ok: true } },
    ]),
  ]
  const ready = [
    opened("a paste on its way is said before what the human has not sent, by JavaScript's truth", [
      { ready: {}, answers: snapshot({ pasteInFlight: true, unsent: true }) },
      { ready: {}, answers: snapshot({ pasteInFlight: [] }) },
      { ready: {}, answers: snapshot({ pasteInFlight: 0, unsent: {} }) },
      { ready: {}, answers: snapshot({ pasteInFlight: '', unsent: 'x' }) },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: 1 }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: 'true', error: '' }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false, error: 0 }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false, error: null }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false, error: false }] } },
      { ready: {}, answers: { 'pane.snapshot': [null] } },
      {
        ready: {},
        answers: { 'pane.snapshot': [{ ok: false, error: { toString: 1 } }] },
        kept: {
          why: "the host's word is written as a template writes a value; Node's template threw on an object with a toString of its own",
          answer: { answer: 'the window cannot be read: [object Object]' },
        },
      },
    ]),
    opened('a window with no status is ready when its snapshot says so', [
      { ready: {}, answers: snapshot({}) },
      { ready: {}, answers: snapshot({ pasteInFlight: true }) },
      { ready: {}, answers: snapshot({ unsent: true }) },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false, error: 'stale' }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ ok: false }] } },
      { ready: {}, answers: { 'pane.snapshot': [{ throws: 'bridge ended' }] } },
    ]),
    opened('a window that shows another conversation is not ready, its snapshot never asked', [
      live({ status: 'idle', sessionId: OTHER }),
      { ready: {} },
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
      {
        deliver: 'Taken, whatever else it says',
        answers: { 'pane.write_paste': [{ ok: true, admitted: false, error: 'odd' }] },
      },
      {
        deliver: 'Not taken, though it says so',
        answers: { 'pane.write_paste': [{ ok: false, admitted: true, error: 'odd' }] },
      },
      {
        deliver: 'Refused in no words',
        answers: { 'pane.write_paste': [{ ok: false, admitted: false, cause: '', error: 'x' }] },
      },
      {
        deliver: 'Refused in a number',
        answers: { 'pane.write_paste': [{ ok: false, admitted: false, cause: 0 }] },
        kept: {
          why: "the host's cause is its words, and a reason is text; Node passed any other value on as the reason itself",
          answer: { answer: { admitted: false, reason: '0' } },
        },
      },
    ]),
  ]
  return [...plans, ...draws, ...looks, ...waits, ...ready, ...deliveries]
}
