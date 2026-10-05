/**
 * The daemon's file formats as Node writes them, for the Rust daemon's tests
 * to be held to line for line (`files.json`, which `dataFiles` of `files.mjs`
 * makes beside the traces): the lines of `daemon.log` (`src/core/log.js`) and
 * of `events.jsonl` (`src/core/trace.js`), the rotation of both past their
 * limit, and what `forget` leaves of a trace. The traces hold these files as
 * the suites wrote them; this holds the formats themselves, each case once,
 * which no suite writes every corner of.
 *
 * The clock is fixed while it is made: every time Node would read is
 * 2026-10-05T10:00:00.123Z.
 */
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { daemonLog } from '../../../src/core/log.js'
import { eventTrace } from '../../../src/core/trace.js'

const FIXED = Date.UTC(2026, 9, 5, 10, 0, 0, 123)
const RealDate = Date

/** The error Node's log would take, with a stack of fixed words. */
function errorWith(stack, message = 'boom') {
  return Object.assign(new Error(message), { stack })
}

/** What Node's log writes under a line for `error` (`log.js:15-18`). */
const causeText = (error) =>
  error === undefined
    ? null
    : error instanceof Error
      ? (error.stack ?? error.message)
      : String(error)

const LOG_CASES = [
  { name: 'a start', level: 'info', message: 'start pid 4242 node v26.8.1 home /tmp/consensflow' },
  { name: 'a stop', level: 'info', message: 'stop: stdin ended; rss 61 MB' },
  { name: 'a slow pass', level: 'warn', message: 'a pass took 6000 ms' },
  {
    name: 'an error with its stack',
    level: 'error',
    message: 'a pass failed',
    error: errorWith(
      'Error: boom\n    at step (/src/core/dispatcher.js:10:5)\n    at pass (/src/core/dispatcher.js:99:7)',
    ),
  },
  {
    name: 'an error that is text',
    level: 'error',
    message: 'the agents file could not be used',
    error: 'plain words',
  },
  {
    name: 'a cause with a blank line and a trailing newline',
    level: 'error',
    message: 'a cause of several lines',
    error: 'first\n\nthird\n',
  },
  {
    name: 'an error with no stack says its message',
    level: 'warn',
    message: 'something odd',
    error: errorWith(undefined, 'only a message'),
  },
  {
    name: 'a message of its own lines and characters',
    level: 'info',
    message: 'café ☕ \u{1F600}\nsecond line "quoted" \\ back',
  },
]

const LEDGER_EVENT = (project, kind, data) => ({
  at: '2026-10-05T09:59:00.000Z',
  project,
  kind,
  data,
})

const TRACE_CASES = [
  {
    name: 'a ledger event, with the time it carries',
    entry: LEDGER_EVENT(2, 'task.created', { number: 2, to: 'zeus', title: 'Café ☕' }),
  },
  {
    name: 'a ledger event with nested data',
    entry: LEDGER_EVENT(1, 'conversation.bound', {
      a: [1, null, true],
      b: { c: 'x' },
      n: 1.5,
      big: 12345678,
    }),
  },
  {
    name: 'a ledger event with characters JSON writes as escapes',
    entry: LEDGER_EVENT(3, 'message.sent', {
      body: 'tab\there\nnew "quote" \\ \u0001 \u007f   \u{1F600}',
    }),
  },
  {
    name: 'a window going idle',
    entry: {
      kind: 'window.activity',
      project: 1,
      participant: 'chief',
      state: 'idle',
      reason: null,
    },
  },
  {
    name: 'a window working for a reason',
    entry: {
      kind: 'window.activity',
      project: 4,
      participant: 'zeus',
      state: 'waiting',
      reason: 'a question',
    },
  },
  {
    name: 'a kill that failed',
    entry: { kind: 'window.kill_failed', project: null, participant: null, error: null },
  },
  {
    name: 'a delivery held',
    entry: {
      kind: 'delivery.held',
      project: 2,
      participant: 'zeus',
      message: 7,
      reason: 'the window is not ready for a paste: a paste is on its way',
    },
  },
  {
    name: 'enter pressed again',
    entry: { kind: 'delivery.enter_again', project: 2, participant: 'zeus', message: 7 },
  },
  {
    name: 'a project deleted',
    entry: {
      kind: 'project.deleted',
      project: null,
      data: {
        id: 3,
        name: 'Parser',
        directory: '/work/parser',
        createdAt: '2026-09-19T11:00:00.000Z',
        members: 1,
        sessions: 2,
        tasks: 4,
        messages: 9,
      },
    },
  },
  {
    name: 'an error nobody caught',
    entry: {
      kind: 'daemon.error',
      project: null,
      reason: 'a pass failed: it said "no"\nsecond line',
    },
  },
]

const read = (file) => {
  try {
    return readFileSync(file, 'utf8')
  } catch {
    return null
  }
}

function logGoldens(home) {
  const cases = LOG_CASES.map(({ name, level, message, error }) => {
    const folder = home()
    const log = daemonLog(folder, { now: () => new RealDate(FIXED) })
    if (level === 'info') log.info(message)
    else log[level](message, error)
    return {
      name,
      level,
      message,
      cause: causeText(error),
      expected: read(join(folder, 'daemon.log')),
    }
  })
  // Past its limit the log is moved aside once: one previous file.
  const folder = home()
  let tick = 0
  const log = daemonLog(folder, { limit: 100, now: () => new RealDate(FIXED + 1000 * tick++) })
  for (let line = 0; line < 7; line++) log.info(`line ${line}`)
  const rotation = {
    limit: 100,
    lines: 7,
    current: read(join(folder, 'daemon.log')),
    aside: read(join(folder, 'daemon.log.1')),
  }
  return { cases, rotation }
}

function traceGoldens(home) {
  const cases = TRACE_CASES.map(({ name, entry }) => {
    const folder = home()
    eventTrace(folder)(entry)
    return { name, entry, expected: read(join(folder, 'events.jsonl')) }
  })
  // Past its limit the trace is moved aside once.
  const folder = home()
  const trace = eventTrace(folder, { limit: 120 })
  const entries = Array.from({ length: 8 }, (_, at) =>
    LEDGER_EVENT(at + 1, 'project.created', { n: at }),
  )
  for (const entry of entries) trace(entry)
  const rotation = {
    limit: 120,
    entries,
    current: read(join(folder, 'events.jsonl')),
    aside: read(join(folder, 'events.jsonl.1')),
  }
  // A deleted project's lines leave both files; a line that is no JSON, or no object, stays.
  const forgetting = home()
  const before = [
    '{"at":"a","project":1,"kind":"x"}',
    '{"at":"b","project":2,"kind":"x"}',
    'not json',
    '',
    '[2]',
    '{"at":"c","project":"2","kind":"x"}',
    '{"at":"d","project":2.0,"kind":"y"}',
    '{"at":"e","project":null,"kind":"z"}',
    '{"at":"f","project":3,"kind":"x"}',
    '',
  ].join('\n')
  const beforeAside = '{"at":"g","project":2,"kind":"x"}\n{"at":"h","project":4,"kind":"x"}\n'
  writeFileSync(join(forgetting, 'events.jsonl'), before)
  writeFileSync(join(forgetting, 'events.jsonl.1'), beforeAside)
  eventTrace(forgetting).forget(2)
  const forget = {
    project: 2,
    before,
    beforeAside,
    after: read(join(forgetting, 'events.jsonl')),
    afterAside: read(join(forgetting, 'events.jsonl.1')),
  }
  // A file with nothing of the project is not written: its mtime stays, and so do its bytes.
  return { cases, rotation, forget }
}

/** The text of `files.json`: every format case, as Node's log and trace write it. */
export function formats() {
  const scratch = []
  const home = () => {
    const folder = mkdtempSync(join(tmpdir(), 'cf-daemon-goldens-'))
    scratch.push(folder)
    return folder
  }
  // `new Date()` with no arguments is the fixed moment, as the trace reads it.
  globalThis.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length === 0 ? [FIXED] : args))
    }
  }
  try {
    const goldens = {
      clock: new RealDate(FIXED).toISOString(),
      log: logGoldens(home),
      trace: traceGoldens(home),
    }
    return `${JSON.stringify(goldens, null, 2)}\n`
  } finally {
    globalThis.Date = RealDate
    for (const folder of scratch) rmSync(folder, { recursive: true, force: true })
  }
}
