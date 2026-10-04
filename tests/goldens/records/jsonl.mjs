/**
 * The JSONL harnesses' scenarios of `tests/engine/completion-chunked.test.mjs`,
 * as data: each transcript written the way its harness writes it, a piece at
 * a time, with a cached look and a fresh one after each piece; and the
 * transcripts that shrink, are replaced, move or go, read again from their
 * start.
 */
import { both, fixtureLines, pieces, transcript } from './fixtures.mjs'

const PI_QUIET_MS = 120_000

/**
 * `lines` written a piece at a time, a look after each. Pi is read three
 * ways: just written, quiet past its window, and with its extension's
 * evidence naming the last record.
 */
function inPieces(kind, session, lines, label) {
  const ways = kind === 'pi' ? ['fresh', 'quiet', 'evidence'] : ['fresh']
  return ways.map((way) => {
    const { dir, file, env } = transcript(kind, session)
    const steps = [{ mkdir: dir }]
    let options = {}
    if (way === 'evidence') {
      const frontier = { id: JSON.parse(lines.at(-1)).id }
      steps.push(
        { mkdir: '$ROOT/settled' },
        {
          write: '$ROOT/settled/launch-1.json',
          text: JSON.stringify({ launchId: 'launch-1', sessionId: session, frontier }),
        },
      )
      options = { piSettlement: { directory: '$ROOT/settled', launchId: 'launch-1' } }
    }
    for (const piece of pieces(lines)) {
      steps.push({ append: file, text: piece })
      if (way === 'quiet') steps.push({ mtime: file, ago: PI_QUIET_MS + 1_000 })
      steps.push(both(kind, session, options))
    }
    return { name: `${label}, read in pieces (${way})`, env, steps }
  })
}

const TRANSCRIPTS = [
  ['codex', 'codex/completed.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/errored-task-complete.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/interrupted.jsonl', '01a077f6-6663-7bc2-81cd-e287ccaabdbd'],
  ['codex', 'codex/forked.jsonl', '01a077fa-5968-7b62-8fdd-043410a3d4b9'],
  ['codex', 'codex/big-answer.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['claude-code', 'claude-code/fragments.jsonl', '15fba934-d727-4777-8791-123675a63649'],
  ['claude-code', 'claude-code/frontier-history.jsonl', '15fba934-d727-4777-8791-123675a63649'],
  ['claude-code', 'claude-code/queued-turn.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/queue-pop-all.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/interrupted.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/provider-429.jsonl', '33383216-87a0-4e6d-a273-07c4b229cdb1'],
  ['claude-code', 'claude-code/compaction.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/v263-tool-loop.jsonl', '5cbf8973-f472-448a-8763-59fb4268a9d7'],
  ['claude-code', 'claude-code/v265-tool-loop.jsonl', '17499106-8778-48e1-a306-87bd186c9f7e'],
  ['claude-code', 'claude-code/v266-tool-loop.jsonl', '47b1090f-b1c7-4d19-95b9-24c09a7f164a'],
  ['claude-code', 'claude-code/v268-clear.jsonl', 'fb561379-bcab-4045-92d2-d460bb19ed36'],
  ['claude-code', 'claude-code/v268-late-ancestors.jsonl', '4e761651-511b-4065-8a65-6ff21582faad'],
  ['pi', 'pi/between-tool-steps.jsonl', 'hazy-ridge'],
  ['pi', 'pi/tool-loop.jsonl', 'hazy-ridge'],
  ['pi', 'pi/provider-429.jsonl', 'triton-jade-fern'],
]

/** The JSONL harnesses' records, read in pieces and read again when they change under the reader. */
export function jsonlSequences() {
  const scenarios = TRANSCRIPTS.flatMap(([kind, name, session]) =>
    inPieces(kind, session, fixtureLines(name), name),
  )

  // Late ancestors decide whether a user record opened a turn; a record that
  // arrives later under a uuid that decision looked up can decide it otherwise.
  const lateSession = '4e761651-511b-4065-8a65-6ff21582faad'
  const records = fixtureLines('claude-code/v268-late-ancestors.jsonl').map((line) =>
    JSON.parse(line),
  )
  for (const [name, later] of [
    ['a duplicate user', records[6]],
    ['a duplicate attachment', records[7]],
    ['a next user', { ...records[6], uuid: 'next-user', parentUuid: records[3].uuid }],
  ]) {
    scenarios.push(
      ...inPieces(
        'claude-code',
        lateSession,
        [...records, later].map((record) => JSON.stringify(record)),
        `claude-code: late ancestors, then ${name}`,
      ),
    )
  }
  scenarios.push(
    ...inPieces(
      'codex',
      '01a074ec-7aff-74b0-8cf6-aa00d8e451cb',
      [
        ...fixtureLines('codex/errored-task-complete.jsonl'),
        ...fixtureLines('codex/completed.jsonl'),
      ],
      'codex: an answer after an errored turn',
    ),
  )
  return [
    ...scenarios,
    shrunkReplacedMoved(),
    unterminatedThenGrown(),
    unterminatedWrittenOver(),
    piEvidenceArrives(),
    looksTakeTurns(),
  ]
}

function shrunkReplacedMoved() {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const completed = fixtureLines('codex/completed.jsonl')
  const errored = fixtureLines('codex/errored-task-complete.jsonl')
  const { dir, file, env } = transcript('codex', session)
  const at = both('codex', session)
  const moved = '$ROOT/sessions/2026/09/07'
  const sameEnd = [...errored, ...completed, ...completed]
    .join('\n')
    .replaceAll('deferred', 'DEFERRED')
  return {
    name: 'codex: a transcript that shrinks, is rewritten, is replaced or moves is read again from its start',
    env,
    steps: [
      { mkdir: dir },
      { write: file, text: `${[...errored, ...completed].join('\n')}\n` },
      at,
      // Shorter: a rewrite that shrank it.
      { write: file, text: `${errored.slice(0, 11).join('\n')}\n` },
      at,
      // As long and longer, but other bytes where the last look stopped.
      { write: file, text: `${[...completed, ...errored].join('\n')}\n` },
      at,
      // Another file in its place.
      { replace: file, text: `${[...errored, ...completed, ...completed].join('\n')}\n` },
      at,
      // Another one again, the same but for a word early on: its end is the same bytes.
      { replace: file, text: `${sameEnd}\n` },
      at,
      // Gone from where it was, and found where it is now.
      { mkdir: moved },
      { move: file, to: `${moved}/rollout-2026-09-06T00-00-00-${session}.jsonl` },
      at,
      { remove: `${moved}/rollout-2026-09-06T00-00-00-${session}.jsonl` },
      at,
    ],
  }
}

function unterminatedThenGrown() {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = fixtureLines('codex/completed.jsonl')
  const { dir, file, env } = transcript('codex', session)
  const at = both('codex', session)
  return {
    name: 'codex: a whole unterminated last record that then grows into something else is read again',
    env,
    steps: [
      { mkdir: dir },
      { write: file, text: lines.join('\n') },
      at,
      { append: file, text: '   ' },
      at,
      { append: file, text: '{"type":"event_msg"}\n' },
      at,
    ],
  }
}

function unterminatedWrittenOver() {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = fixtureLines('codex/completed.jsonl')
  const { dir, file, env } = transcript('codex', session)
  const at = both('codex', session)
  // The same bytes before it; in its place, another record just as long.
  const other = lines.at(-1).replace('the merge.', 'the merge!')
  return {
    name: 'codex: a whole unterminated last record written over before its newline is read again',
    env,
    steps: [
      { mkdir: dir },
      { write: file, text: lines.join('\n') },
      at,
      { write: file, text: `${[...lines.slice(0, -1), other].join('\n')}\n` },
      at,
    ],
  }
}

function piEvidenceArrives() {
  const lines = fixtureLines('pi/tool-loop.jsonl')
  const { dir, file, env } = transcript('pi', 'hazy-ridge')
  const options = { piSettlement: { directory: '$ROOT/settled', launchId: 'launch-1' } }
  const at = both('pi', 'hazy-ridge', options)
  return {
    name: 'pi: evidence that arrives while the transcript does not change settles the next look',
    env,
    steps: [
      { mkdir: dir },
      { mkdir: '$ROOT/settled' },
      { write: file, text: `${lines.join('\n')}\n` },
      at,
      {
        write: '$ROOT/settled/launch-1.json',
        text: JSON.stringify({
          launchId: 'launch-1',
          sessionId: 'hazy-ridge',
          frontier: { id: '3f9b029e' },
        }),
      },
      at,
    ],
  }
}

/** Three looks in a row each time, where Node's cache had them take turns. */
function looksTakeTurns() {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const { dir, file, env } = transcript('claude-code', session)
  const steps = [{ mkdir: dir }]
  for (const line of fixtureLines('claude-code/queue-pop-all.jsonl')) {
    steps.push(
      { append: file, text: `${line}\n` },
      { look: 'cached', kind: 'claude-code', session },
      { look: 'cached', kind: 'claude-code', session },
      { look: 'both', kind: 'claude-code', session },
    )
  }
  return { name: 'claude-code: looks at one conversation one after another', env, steps }
}
