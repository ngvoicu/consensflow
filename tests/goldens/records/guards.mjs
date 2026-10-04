/**
 * The cases of `tests/engine/completion.test.mjs` that cross harnesses, and
 * its guards and quota cases, as scenarios.
 *
 * Three of its tests have no scenario. `env is mandatory` and the unknown
 * kind are ruled out by the Rust signature. The source checks read the
 * JavaScript itself. `readOn`'s `only` is a unit of the read-on core, and
 * its test is ported there.
 */
import { stagedDevin } from './devin.mjs'
import { stagedOpencode } from './opencode.mjs'
import { staged } from './suite.mjs'

const PI_QUIET_MS = 120_000
const CODEX = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
const CLAUDE = '15fba934-d727-4777-8791-123675a63649'
const FORK = '01a077fa-5968-7b62-8fdd-043410a3d4b9'
const QUEUED = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
const WINDOW = 'ses_f88c0c7cdffeANJRVLwiBceADi'
const TOOLS = 'ses_f87e22f72ffewC2qJ2dAyyfPe1'

/** Two fresh looks at one staged record, as the identity test takes them. */
function twice(name, kind, session, { steps, env }) {
  const look = { look: 'fresh', kind, session }
  return { name, env, steps: [...steps, look, look] }
}

/** One fresh look at one staged record. */
function once(name, kind, session, { steps, env }) {
  return { name, env, steps: [...steps, { look: 'fresh', kind, session }] }
}

export function guardScenarios() {
  return [
    ...identity(),
    ...longLeaves(),
    ...versions(),
    ...jsonlGuards(),
    ...quotas(),
    ...storedNulls(),
  ]
}

/**
 * Stored data that is JSON `null`, where Node's readers read a property of
 * it: each reading is unknown, for a reason that is V8's.
 */
function storedNulls() {
  const nulled = (pick) => ({
    mutate(rows) {
      pick(rows).data = 'null'
    },
  })
  return [
    once(
      'stored null: an OpenCode event',
      'opencode',
      TOOLS,
      stagedOpencode(
        'opencode/tool-result.json',
        nulled(({ events }) => events.at(-1)),
      ),
    ),
    once(
      'stored null: an OpenCode message',
      'opencode',
      TOOLS,
      stagedOpencode(
        'opencode/tool-result.json',
        nulled(({ messages }) => messages[0]),
      ),
    ),
    once(
      'stored null: an OpenCode part',
      'opencode',
      TOOLS,
      stagedOpencode(
        'opencode/tool-result.json',
        nulled(({ parts }) => parts[0]),
      ),
    ),
    once('stored null: a Devin wire line', 'devin', 'calm-river', stagedDevin('Done.', [null])),
    {
      name: 'stored null: a Devin message',
      ...(() => {
        const { steps, env, store } = stagedDevin('Done.', [])
        return {
          env,
          steps: [
            ...steps,
            { db: 'edit', open: store },
            {
              db: 'edit',
              run: "UPDATE message_nodes SET chat_message = 'null' WHERE node_id = 'n-2'",
            },
            { db: 'edit', close: true },
            { look: 'fresh', kind: 'devin', session: 'calm-river' },
          ],
        }
      })(),
    },
  ]
}

function identity() {
  const quiet = { ageMs: PI_QUIET_MS + 1_000 }
  return [
    twice(
      'identity: codex completed',
      'codex',
      CODEX,
      staged('codex', CODEX, 'codex/completed.jsonl'),
    ),
    twice('identity: codex forked', 'codex', FORK, staged('codex', FORK, 'codex/forked.jsonl')),
    twice(
      'identity: claude-code fragments',
      'claude-code',
      CLAUDE,
      staged('claude-code', CLAUDE, 'claude-code/fragments.jsonl'),
    ),
    twice(
      'identity: claude-code queue popAll',
      'claude-code',
      QUEUED,
      staged('claude-code', QUEUED, 'claude-code/queue-pop-all.jsonl'),
    ),
    twice(
      'identity: pi tool loop, quiet',
      'pi',
      'hazy-ridge',
      staged('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', quiet),
    ),
    twice(
      'identity: pi after a 429, quiet',
      'pi',
      'triton-jade-fern',
      staged('pi', 'triton-jade-fern', 'pi/provider-429.jsonl', quiet),
    ),
    twice(
      'identity: opencode tool result',
      'opencode',
      TOOLS,
      stagedOpencode('opencode/tool-result.json'),
    ),
    twice(
      'identity: opencode completion window, after',
      'opencode',
      WINDOW,
      stagedOpencode('opencode/completion-window.json', { snapshot: 'after' }),
    ),
  ]
}

function longLeaves() {
  const longText = 'L'.repeat(60000)
  return [
    once(
      'a 60,000-character text leaf whole: claude-code',
      'claude-code',
      CLAUDE,
      staged('claude-code', CLAUDE, 'claude-code/fragments.jsonl', {
        mutate(records) {
          records[4].message.content[0].text = longText
          return records
        },
      }),
    ),
    once(
      'a 60,000-character text leaf whole: pi',
      'pi',
      'hazy-ridge',
      staged('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
        mutate(records) {
          records.at(-1).message.content[0].text = longText
          return records
        },
      }),
    ),
    once(
      'a 60,000-character text leaf whole: opencode',
      'opencode',
      TOOLS,
      stagedOpencode('opencode/tool-result.json', {
        mutate({ parts }) {
          const part = parts.find((candidate) => candidate.id === 'prt_07834fd70001aAGLSN74ke5QX3')
          part.data = JSON.stringify({ ...JSON.parse(part.data), text: longText })
        },
      }),
    ),
  ]
}

function versions() {
  const absent = (records) => {
    for (const record of records) {
      delete record.version
      if (record.payload) delete record.payload.cli_version
    }
    return records
  }
  return [
    once(
      'versions: codex ignores an undeclared cli_version',
      'codex',
      CODEX,
      staged('codex', CODEX, 'codex/completed.jsonl', {
        mutate(records) {
          records[0].payload.cli_version = '0.154.0'
          return records
        },
      }),
    ),
    once(
      'versions: claude-code ignores an undeclared version',
      'claude-code',
      CLAUDE,
      staged('claude-code', CLAUDE, 'claude-code/fragments.jsonl', {
        mutate(records) {
          for (const record of records) record.version = '2.2.0'
          return records
        },
      }),
    ),
    once(
      'versions: pi ignores an undeclared version',
      'pi',
      'hazy-ridge',
      staged('pi', 'hazy-ridge', 'pi/tool-loop.jsonl', {
        mutate(records) {
          records[0].version = 4
          return records
        },
      }),
    ),
    once(
      'versions: opencode ignores an undeclared version',
      'opencode',
      WINDOW,
      stagedOpencode('opencode/completion-window.json', { version: '1.19.0' }),
    ),
    ...[
      ['codex', CODEX, 'codex/completed.jsonl'],
      ['claude-code', CLAUDE, 'claude-code/fragments.jsonl'],
      ['pi', 'hazy-ridge', 'pi/tool-loop.jsonl'],
    ].map(([kind, session, name]) =>
      once(
        `versions: absent version metadata preserves extraction: ${kind}`,
        kind,
        session,
        staged(kind, session, name, { mutate: absent }),
      ),
    ),
  ]
}

function jsonlGuards() {
  const finalAppends = [
    '\n{"type":"event_msg"',
    '\ndefinitely-not-json',
    '\n{"type":!}',
    ...['\v', ' ', '\f'].flatMap((whitespace) => [`\n${whitespace}`, `\n{"type":${whitespace}`]),
  ]
  const malformed = staged('codex', CODEX, 'codex/completed.jsonl')
  const malformedLines = malformed.steps.at(-1).text.trimEnd().split('\n')
  malformedLines.splice(2, 0, '{"malformed":')
  const empty = '$ROOT/empty'
  return [
    ...finalAppends.map((finalAppend) =>
      once(
        `JSONL streams tolerate only an incomplete final append: ${JSON.stringify(finalAppend)}`,
        'codex',
        CODEX,
        staged('codex', CODEX, 'codex/completed.jsonl', { finalAppend }),
      ),
    ),
    {
      name: 'JSONL: a malformed whole line fails closed, naming its record',
      env: malformed.env,
      steps: [
        ...malformed.steps.slice(0, -1),
        { write: malformed.steps.at(-1).write, text: `${malformedLines.join('\n')}\n` },
        { look: 'fresh', kind: 'codex', session: CODEX },
      ],
    },
    {
      name: 'readable corrupted storage fails closed',
      env: { CODEX_HOME: '$ROOT' },
      steps: [
        { mkdir: '$ROOT/sessions' },
        { write: '$ROOT/sessions/rollout-corrupt.jsonl', text: '{bad}\n' },
        { look: 'fresh', kind: 'codex', session: 'corrupt' },
      ],
    },
    {
      name: 'absent storage fails closed',
      env: { HOME: empty, XDG_DATA_HOME: empty },
      steps: [
        { mkdir: empty },
        ...['codex', 'claude-code', 'pi', 'opencode'].map((kind) => ({
          look: 'fresh',
          kind,
          session: 'no-such-session',
        })),
        { look: 'fresh', kind: 'codex', session: '' },
      ],
    },
  ]
}

function quotas() {
  const codexSession = '01a077c2-5d0f-7452-a2be-ace096bbe3be'
  const limits = (used, reached = null) => ({
    timestamp: '2026-09-06T17:27:06.000Z',
    ordinal: 2490,
    type: 'event_msg',
    payload: {
      type: 'token_count',
      info: null,
      rate_limits: {
        limit_id: 'codex',
        primary: { used_percent: used, window_minutes: 10080, resets_at: 1790423393 },
        secondary: { used_percent: 12.5, window_minutes: 300, resets_at: 1789900000 },
        rate_limit_reached_type: reached,
      },
    },
  })
  const claudeSession = '33383216-87a0-4e6d-a273-07c4b229cdb1'
  const named = (records) => {
    const error = records.findLast((record) => record.isApiErrorMessage === true)
    error.message.content = [{ type: 'text', text: "You've hit your limit. Resets in 2 hours." }]
    error.timestamp = '2026-09-19T10:00:00.000Z'
    return error
  }
  const piSession = 'hazy-429'
  const upToError = (records) => {
    const last = records.findLastIndex((record) => record.message?.stopReason === 'error')
    return records.slice(0, last + 1)
  }
  const piError = (message, timestamp) => (records) => {
    const cut = upToError(records)
    cut.at(-1).message.errorMessage = message
    if (timestamp !== undefined) cut.at(-1).message.timestamp = Date.parse(timestamp)
    return cut
  }
  return [
    ...[
      ['ok', limits(5)],
      ['low', limits(97)],
      ['exhausted', limits(100, 'primary')],
    ].map(([label, record]) =>
      once(
        `quota/codex: the rollout says how much of the window is used (${label})`,
        'codex',
        codexSession,
        staged('codex', codexSession, 'codex/completed.jsonl', {
          mutate: (records) => [...records, record],
        }),
      ),
    ),
    once(
      'quota/codex: no token_count, no word on it',
      'codex',
      codexSession,
      staged('codex', codexSession, 'codex/completed.jsonl'),
    ),
    once(
      'quota/claude-code: a 429 is exhaustion',
      'claude-code',
      claudeSession,
      staged('claude-code', claudeSession, 'claude-code/provider-429.jsonl'),
    ),
    once(
      'quota/claude-code: the refusal is history once a turn succeeds',
      'claude-code',
      claudeSession,
      staged('claude-code', claudeSession, 'claude-code/provider-429.jsonl', {
        mutate: (records) => {
          const error = named(records)
          return [
            ...records,
            {
              ...error,
              uuid: 'after-reset',
              parentUuid: error.uuid,
              timestamp: '2026-09-19T13:00:00.000Z',
              isApiErrorMessage: undefined,
              apiErrorStatus: undefined,
              error: undefined,
              message: {
                ...error.message,
                id: 'msg_after_reset',
                content: [{ type: 'text', text: 'Back to work.' }],
              },
            },
          ]
        },
      }),
    ),
    once(
      'quota/claude-code: a 429 that names its reset',
      'claude-code',
      claudeSession,
      staged('claude-code', claudeSession, 'claude-code/provider-429.jsonl', {
        mutate: (records) => {
          named(records)
          return records
        },
      }),
    ),
    ...[
      [
        'a 429 names its reset in the message',
        piError(
          '429: {"type":"GoUsageLimitError","message":"Weekly usage limit reached. Resets in 3 days."}',
          '2026-09-19T10:00:00.000Z',
        ),
      ],
      ['a 500 is not quota', piError('500: provider down')],
      [
        "Pi's other shape of a 429",
        piError(
          'OpenAI API error (429): {"type":"GoUsageLimitError","message":"Weekly usage limit reached. Resets in 2 days."}',
          '2026-09-19T10:00:00.000Z',
        ),
      ],
      ['the fixture retries and succeeds', (records) => records],
    ].map(([label, mutate]) =>
      once(
        `quota/pi: ${label}`,
        'pi',
        piSession,
        staged('pi', piSession, 'pi/provider-429.jsonl', { mutate }),
      ),
    ),
    once(
      'quota/opencode: a completed turn without an error says nothing of quota',
      'opencode',
      WINDOW,
      stagedOpencode('opencode/completion-window.json', { snapshot: 'after' }),
    ),
  ]
}
