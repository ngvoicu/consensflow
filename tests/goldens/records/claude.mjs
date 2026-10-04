/**
 * Claude Code's cases of `tests/engine/completion.test.mjs`, as scenarios:
 * late ancestors, fragments, its queue, interrupts and compaction, and its
 * API errors.
 */
import { fixtureLines } from './fixtures.mjs'
import { once, staged } from './suite.mjs'

const CLAUDE = '15fba934-d727-4777-8791-123675a63649'
const LATE = '4e761651-511b-4065-8a65-6ff21582faad'
const LATE_FIXTURE = 'claude-code/v268-late-ancestors.jsonl'
const QUEUED = '1b09fb15-feb1-4595-9f47-5eb9ff768191'

export function claudeScenarios() {
  return [...claudeCases(), ...claudeQueueCases()]
}

function claudeCases() {
  const late = staged('claude-code', LATE, LATE_FIXTURE)
  const lateRecords = fixtureLines(LATE_FIXTURE).map((line) => JSON.parse(line))
  const lateCases = [
    ['missing attachment', (rows) => rows.pop()],
    ['unrelated user', (rows) => (rows[6].uuid = 'another-user')],
    ['cyclic attachments', (rows) => (rows[7].parentUuid = rows[10].uuid)],
    ['duplicate attachment ID', (rows) => rows.push({ ...rows[7] })],
    ['duplicate user ID', (rows) => rows.push({ ...rows[6] })],
    ['foreign attachment', (rows) => (rows[8].sessionId = 'foreign')],
    ['sidechain attachment', (rows) => (rows[8].isSidechain = true)],
    ['unknown ancestry record', (rows) => (rows[8].type = 'unknown')],
    ['sidechain assistant', (rows) => (rows[3].isSidechain = true)],
    ['wrong boundary parent', (rows) => (rows[4].parentUuid = 'another-assistant')],
    ['stop hooks not complete', (rows) => rows.splice(4, 1)],
    ['open tool', (rows) => rows[3].message.content.push({ type: 'tool_use', id: 'open-tool' })],
    [
      'next user with earlier timestamp',
      (rows) => rows.push({ ...rows[6], uuid: 'next-user', parentUuid: rows[3].uuid }),
    ],
    [
      'next user before late ancestors',
      (rows) => rows.splice(6, 0, { ...rows[6], uuid: 'next-user', parentUuid: rows[3].uuid }),
    ],
    [
      'later queue matching old prompt',
      (rows) =>
        rows.splice(6, 0, {
          type: 'queue-operation',
          operation: 'enqueue',
          content: rows[6].message.content,
        }),
    ],
  ]
  return [
    {
      name: 'claude-code: late native user ancestors preserve completion and item positions',
      env: late.env,
      steps: [
        { mkdir: '$ROOT/projects' },
        {
          write: late.file,
          text: `${lateRecords
            .slice(0, 6)
            .map((record) => JSON.stringify(record))
            .join('\n')}\n`,
        },
        { look: 'fresh', kind: 'claude-code', session: LATE },
        {
          append: late.file,
          text: `${lateRecords
            .slice(6)
            .map((record) => JSON.stringify(record))
            .join('\n')}\n`,
        },
        { look: 'fresh', kind: 'claude-code', session: LATE },
      ],
    },
    ...lateCases.map(([label, mutate]) =>
      once(
        `claude-code: late ancestry cannot settle unrelated or unproven work: ${label}`,
        'claude-code',
        LATE,
        LATE_FIXTURE,
        {
          mutate: (rows) => {
            mutate(rows)
            return rows
          },
        },
      ),
    ),
    once(
      'claude-code: fragments before its stop hooks reported',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
      { take: 5 },
    ),
    once(
      'claude-code: fragments share message.id, server tool result is lossless, hook settles',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
    ),
    once(
      'claude-code: fragment identity supports growth and repeated equal text',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
      {
        mutate(records) {
          const first = records[0]
          const last = records[4]
          first.message.content[0].text = 'A'
          const grown = structuredClone(first)
          grown.message.content[0].text = 'AB'
          last.message.content[0].text = 'AB'
          records.splice(1, 0, grown)
          return records
        },
      },
    ),
    once(
      'claude-code: mid-session version metadata does not affect settlement',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
      {
        mutate(records) {
          records.at(-1).version = '2.1.250'
          return records
        },
      },
    ),
  ]
}

function claudeQueueCases() {
  const history = fixtureLines('claude-code/frontier-history.jsonl').map((line) => JSON.parse(line))
  const envelope = (attributes) =>
    `<cross-session-message from="uds:/tmp/cc-socks/0000.sock"${attributes} from-name="worker-01">worker result</cross-session-message>`
  return [
    once(
      'claude-code: historical interrupt, advisor, and removed queue do not poison the current frontier',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
      {
        mutate(records) {
          records.unshift(...history.slice(0, 3))
          records.splice(-1, 0, ...history.slice(3))
          return records
        },
      },
    ),
    once(
      'claude-code: a queued message still to come',
      'claude-code',
      QUEUED,
      'claude-code/queued-turn.jsonl',
      { take: 4 },
    ),
    once(
      'claude-code: queued work prevents a settled window through dequeue and hooks',
      'claude-code',
      QUEUED,
      'claude-code/queued-turn.jsonl',
    ),
    once(
      'claude-code: a removed cross-session message clears its queue entry when the envelope attributes differ',
      'claude-code',
      QUEUED,
      'claude-code/queued-turn.jsonl',
      {
        mutate: ([enqueue, assistant, , stopHooks]) => [
          { ...enqueue, content: envelope(' hop-chain="ce0e9fe0365a224d1a5ef3fe"') },
          assistant,
          { ...enqueue, operation: 'remove', content: envelope('') },
          stopHooks,
        ],
      },
    ),
    once(
      'claude-code: the captured interrupt cancels',
      'claude-code',
      QUEUED,
      'claude-code/interrupted.jsonl',
    ),
    once(
      'claude-code: a quoted interrupt marker is an ordinary user turn',
      'claude-code',
      QUEUED,
      'claude-code/interrupted.jsonl',
      {
        mutate(records) {
          records[0].uuid = 'ordinary-user-quoted-marker'
          records[0].message.content[0].text =
            'Please quote "[Request interrupted by user]" in the report.'
          delete records[0].interruptedMessageId
          return records
        },
      },
    ),
    once(
      'claude-code: compaction keeps prior answers',
      'claude-code',
      QUEUED,
      'claude-code/compaction.jsonl',
    ),
    ...[5, 8, undefined].map((take) =>
      once(
        `claude-code: popAll consumes every popped item and later queue history reconciles (${take ?? 'all'} records)`,
        'claude-code',
        QUEUED,
        'claude-code/queue-pop-all.jsonl',
        take === undefined ? {} : { take },
      ),
    ),
    once(
      'claude-code: native API error settles incomplete as failure, not cancellation',
      'claude-code',
      '33383216-87a0-4e6d-a273-07c4b229cdb1',
      'claude-code/provider-429.jsonl',
    ),
  ]
}
