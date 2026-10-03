/**
 * A transcript read on, a piece at a time, says what a reading of it whole
 * says. Every JSONL fixture is written the way its harness writes it, each
 * line in two halves and then its newline, and after each piece one reader
 * that reads on is compared with a fresh reading of the whole transcript. A
 * transcript that shrinks or is replaced is read again from its start.
 */
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test, { after } from 'node:test'
import { fileURLToPath } from 'node:url'
import { answers, cachedAnswers } from '../../hosts/lib/completion.js'

const FIX = fileURLToPath(new URL('./fixtures/completion/', import.meta.url))
const PI_QUIET_MS = 120_000

/** Every temporary root a test here makes, removed when the file's tests end. */
const roots = []
after(() => Promise.all(roots.map((root) => fs.rm(root, { recursive: true, force: true }))))

async function tempRoot() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-chunked-'))
  roots.push(root)
  return root
}

/** A look by `read` that reads on, checked against a reading of the whole record. */
async function agrees(read, kind, session, env, options, label) {
  const whole = await answers(kind, session, env, options)
  assert.deepEqual(await read(kind, session, env, options), whole, label)
  return whole
}

/** Each line in two halves and then its newline, so a look finds every kind of last line. */
function pieces(lines) {
  return lines
    .flatMap((line) => {
      const half = Math.floor(line.length / 2)
      return [line.slice(0, half), line.slice(half), '\n']
    })
    .filter((piece) => piece.length > 0)
}

const fixtureLines = async (fixture) =>
  (await fs.readFile(path.join(FIX, fixture), 'utf8')).trimEnd().split('\n')

/** Where each JSONL harness keeps a session's transcript, under `root`. */
async function transcript(kind, session, root) {
  if (kind === 'codex') {
    const dir = path.join(root, 'sessions', '2026', '09', '06')
    await fs.mkdir(dir, { recursive: true })
    return {
      file: path.join(dir, `rollout-2026-09-06T00-00-00-${session}.jsonl`),
      env: { CODEX_HOME: root },
    }
  }
  if (kind === 'claude-code') {
    const dir = path.join(root, 'projects', '-work-app')
    await fs.mkdir(dir, { recursive: true })
    return { file: path.join(dir, `${session}.jsonl`), env: { CLAUDE_CONFIG_DIR: root } }
  }
  const dir = path.join(root, '.pi', 'agent', 'sessions', '--work-app--')
  await fs.mkdir(dir, { recursive: true })
  return { file: path.join(dir, `2026-09-06T00-00-00-000Z_${session}.jsonl`), env: { HOME: root } }
}

// ------------------------------------------------------------------ JSONL

const transcripts = [
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

/**
 * Writes `lines` a piece at a time, a look after each. Pi is read three
 * ways: just written, quiet past its window, and with its extension's
 * evidence naming the last record.
 */
async function readInPieces(kind, session, lines, label) {
  const ways = kind === 'pi' ? ['fresh', 'quiet', 'evidence'] : ['fresh']
  for (const way of ways) {
    const root = await tempRoot()
    const { file, env } = await transcript(kind, session, root)
    let options = {}
    if (way === 'evidence') {
      const directory = path.join(root, 'settled')
      await fs.mkdir(directory)
      const frontier = { id: JSON.parse(lines.at(-1)).id }
      await fs.writeFile(
        path.join(directory, 'launch-1.json'),
        JSON.stringify({ launchId: 'launch-1', sessionId: session, frontier }),
      )
      options = { piSettlement: { directory, launchId: 'launch-1' } }
    }
    const read = cachedAnswers()
    let written = 0
    for (const piece of pieces(lines)) {
      await fs.appendFile(file, piece)
      written += piece.length
      if (way === 'quiet') {
        const old = new Date(Date.now() - PI_QUIET_MS - 1_000)
        await fs.utimes(file, old, old)
      }
      await agrees(read, kind, session, env, options, `${label} ${way} at ${written}`)
    }
  }
}

for (const [kind, fixture, session] of transcripts) {
  test(`completion/${kind}: ${fixture} read in pieces is read whole`, async () => {
    await readInPieces(kind, session, await fixtureLines(fixture), fixture)
  })
}

test('completion/claude-code: a record read later that an earlier decision looked up has the transcript read again', async () => {
  // Late ancestors decide whether a user record opened a turn; a record that
  // arrives later under a uuid that decision looked up can decide it otherwise.
  const session = '4e761651-511b-4065-8a65-6ff21582faad'
  const records = (await fixtureLines('claude-code/v268-late-ancestors.jsonl')).map(JSON.parse)
  for (const [name, later] of [
    ['a duplicate user', records[6]],
    ['a duplicate attachment', records[7]],
    ['a next user', { ...records[6], uuid: 'next-user', parentUuid: records[3].uuid }],
  ]) {
    await readInPieces(
      'claude-code',
      session,
      [...records, later].map((record) => JSON.stringify(record)),
      name,
    )
  }
})

test('completion/codex: an answer after an errored turn, read in pieces, is read whole', async () => {
  const lines = [
    ...(await fixtureLines('codex/errored-task-complete.jsonl')),
    ...(await fixtureLines('codex/completed.jsonl')),
  ]
  await readInPieces(
    'codex',
    '01a074ec-7aff-74b0-8cf6-aa00d8e451cb',
    lines,
    'errored, then completed',
  )
})

// ------------------------------------------------- shrunk, replaced, moved

test('completion: a transcript that shrinks, is rewritten, is replaced or moves is read again from its start', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const completed = await fixtureLines('codex/completed.jsonl')
  const errored = await fixtureLines('codex/errored-task-complete.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  const look = (label) => agrees(read, 'codex', session, env, {}, label)

  await fs.writeFile(file, `${[...errored, ...completed].join('\n')}\n`)
  assert.equal((await look('whole')).settlement.state, 'settled')
  // Shorter: a rewrite that shrank it.
  await fs.writeFile(file, `${errored.slice(0, 11).join('\n')}\n`)
  assert.equal((await look('shrunk')).inFlight, true)
  // As long and longer, but other bytes where the last look stopped.
  await fs.writeFile(file, `${[...completed, ...errored].join('\n')}\n`)
  await look('rewritten in place')
  // Another file in its place.
  const other = `${file}.new`
  await fs.writeFile(other, `${[...errored, ...completed, ...completed].join('\n')}\n`)
  await fs.rename(other, file)
  await look('replaced')
  // Another one again, the same but for a word early on: its end is the same bytes.
  const earlier = [...errored, ...completed, ...completed]
    .join('\n')
    .replaceAll('deferred', 'DEFERRED')
  await fs.writeFile(other, `${earlier}\n`)
  await fs.rename(other, file)
  const sameEnd = await look('replaced, the same at its end')
  assert.ok(sameEnd.items.some((item) => item.text.includes('DEFERRED')))
  // Gone from where it was, and found where it is now.
  const moved = path.join(root, 'sessions', '2026', '09', '07')
  await fs.mkdir(moved, { recursive: true })
  await fs.rename(file, path.join(moved, path.basename(file)))
  await look('moved')
  await fs.rm(path.join(moved, path.basename(file)))
  assert.equal((await look('gone')).unknown, true)
})

test('completion: a whole unterminated last record that then grows into something else is read again', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = await fixtureLines('codex/completed.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  await fs.writeFile(file, lines.join('\n'))
  const unterminated = await agrees(read, 'codex', session, env, {}, 'whole but unterminated')
  assert.equal(unterminated.settlement.state, 'settled', 'its task_complete is read')
  await fs.appendFile(file, '   ')
  await agrees(read, 'codex', session, env, {}, 'whitespace after it')
  await fs.appendFile(file, '{"type":"event_msg"}\n')
  const malformed = await agrees(read, 'codex', session, env, {}, 'more JSON on its line')
  assert.equal(malformed.unknown, true)
})

test('completion: a whole unterminated last record written over before its newline is read again', async () => {
  const session = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
  const lines = await fixtureLines('codex/completed.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('codex', session, root)
  const read = cachedAnswers()
  await fs.writeFile(file, lines.join('\n'))
  await agrees(read, 'codex', session, env, {}, 'whole but unterminated')
  // The same bytes before it; in its place, another record just as long.
  const other = lines.at(-1).replace('the merge.', 'the merge!')
  await fs.writeFile(file, `${[...lines.slice(0, -1), other].join('\n')}\n`)
  const after = await agrees(read, 'codex', session, env, {}, 'written over')
  assert.notEqual(after.settlement.state, 'settled', 'its final answer no longer matches')
})

test('completion/pi: evidence that arrives while the transcript does not change settles the next look', async () => {
  const lines = await fixtureLines('pi/tool-loop.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('pi', 'hazy-ridge', root)
  const directory = path.join(root, 'settled')
  await fs.mkdir(directory)
  const options = { piSettlement: { directory, launchId: 'launch-1' } }
  const read = cachedAnswers()
  await fs.writeFile(file, `${lines.join('\n')}\n`)
  const before = await agrees(read, 'pi', 'hazy-ridge', env, options, 'no evidence yet')
  assert.equal(before.settlement.state, 'in-flight')
  await fs.writeFile(
    path.join(directory, 'launch-1.json'),
    JSON.stringify({ launchId: 'launch-1', sessionId: 'hazy-ridge', frontier: { id: '3f9b029e' } }),
  )
  const settled = await agrees(read, 'pi', 'hazy-ridge', env, options, 'its evidence written')
  assert.equal(settled.settlement.state, 'settled')
})

test('completion: looks at one conversation at the same time take turns', async () => {
  const session = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
  const lines = await fixtureLines('claude-code/queue-pop-all.jsonl')
  const root = await tempRoot()
  const { file, env } = await transcript('claude-code', session, root)
  const read = cachedAnswers()
  for (const line of lines) {
    await fs.appendFile(file, `${line}\n`)
    const looks = await Promise.all([1, 2, 3].map(() => read('claude-code', session, env)))
    const whole = await answers('claude-code', session, env)
    for (const look of looks) assert.deepEqual(look, whole)
  }
})
