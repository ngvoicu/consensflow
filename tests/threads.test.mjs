import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import {
  allSessionNames,
  loadThreads,
  newSessionName,
  removeThread,
  saveThread,
  threadsPath,
} from '../hosts/lib/threads.js'

/**
 * `hosts/lib` reads the config home from process.env directly (it is the
 * engine, shared with the host payloads that once existed), so isolation here
 * means pointing CONSENSFLOW_HOME at a temp dir for in-process calls — the
 * same shape the engine e2e tests use. Without it these tests would write into
 * the developer's real ~/.consensflow.
 */
async function withTempHome(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-threads-'))
  const previous = process.env.CONSENSFLOW_HOME
  process.env.CONSENSFLOW_HOME = path.join(dir, 'home')
  try {
    return await fn(dir)
  } finally {
    if (previous === undefined) delete process.env.CONSENSFLOW_HOME
    else process.env.CONSENSFLOW_HOME = previous
    await rm(dir, { recursive: true, force: true })
  }
}

const RECORD = {
  agent: 'ares',
  kind: 'pi',
  sessionId: 'sess-abc123',
  runs: 1,
  createdAt: '2026-08-23T20:00:00.000Z',
  lastRunAt: '2026-08-23T20:00:00.000Z',
  lastRunId: 'ask-2026-08-23T20-00-00-000Z-aaaa',
}

test('threads: a workspace with no conversations reads as empty', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    assert.deepEqual(await loadThreads(ws), {}, 'no file yet is not an error')
  })
})

test('threads: the store lives beside the run directories, not in the project', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    const file = threadsPath(ws)

    assert.ok(
      file.startsWith(process.env.CONSENSFLOW_HOME),
      'conversation state belongs in the one root, never inside the project',
    )
    assert.equal(path.basename(file), 'threads.json')
    assert.ok(file.includes('workspaces'), 'keyed by workspace, like runs/')
  })
})

test('threads: a saved conversation round-trips by name', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    await saveThread(ws, 'bubble-sky', RECORD)
    const loaded = await loadThreads(ws)

    assert.deepEqual(loaded['bubble-sky'], RECORD)
  })
})

test('threads: several conversations coexist, including two for one agent', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    await saveThread(ws, 'bubble-sky', RECORD)
    await saveThread(ws, 'lolipop-waves', { ...RECORD, sessionId: 'sess-def456' })
    await saveThread(ws, 'amber-moss', { ...RECORD, agent: 'hera', sessionId: 'sess-ghi789' })
    const loaded = await loadThreads(ws)

    assert.deepEqual(Object.keys(loaded).sort(), ['amber-moss', 'bubble-sky', 'lolipop-waves'])
    // The whole point of naming them: one agent can hold more than one.
    const ares = Object.values(loaded).filter((row) => row.agent === 'ares')
    assert.equal(ares.length, 2)
    assert.notEqual(ares[0].sessionId, ares[1].sessionId)
  })
})

test('threads: saving the same name again replaces that record only', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')
    await saveThread(ws, 'bubble-sky', RECORD)
    await saveThread(ws, 'amber-moss', { ...RECORD, agent: 'hera' })

    await saveThread(ws, 'bubble-sky', { ...RECORD, runs: 4, sessionId: 'sess-newer' })
    const loaded = await loadThreads(ws)

    assert.equal(loaded['bubble-sky'].runs, 4)
    assert.equal(loaded['bubble-sky'].sessionId, 'sess-newer')
    assert.equal(loaded['amber-moss'].agent, 'hera', 'the other conversation is untouched')
  })
})

test('threads: two workspaces never see each other', async () => {
  await withTempHome(async (dir) => {
    const one = path.join(dir, 'one')
    const two = path.join(dir, 'two')

    await saveThread(one, 'bubble-sky', RECORD)

    assert.deepEqual(await loadThreads(two), {}, 'names are unique per workspace')
  })
})

test('threads: removeThread drops one and leaves the rest', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')
    await saveThread(ws, 'bubble-sky', RECORD)
    await saveThread(ws, 'amber-moss', { ...RECORD, agent: 'hera' })

    await removeThread(ws, 'bubble-sky')
    const loaded = await loadThreads(ws)

    assert.deepEqual(Object.keys(loaded), ['amber-moss'])
  })
})

test('threads: removing something that was never there is not an error', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    await assert.doesNotReject(() => removeThread(ws, 'never-existed'))
  })
})

test('threads: the write is atomic — no temp file survives', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')

    await saveThread(ws, 'bubble-sky', RECORD)

    const { readdir } = await import('node:fs/promises')
    const files = await readdir(path.dirname(threadsPath(ws)))
    assert.deepEqual(
      files.filter((name) => name.includes('threads.json')),
      ['threads.json'],
      'tmp+rename, mirroring state.js — a crash mid-write must not leave a shard',
    )
  })
})

test('threads: a corrupt store reads as empty rather than throwing', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')
    await saveThread(ws, 'bubble-sky', RECORD)
    await writeFile(threadsPath(ws), '{ this is not json', 'utf8')

    // A conversation index is a convenience, not the user's data. Losing it
    // must degrade to "no conversations yet", never break every consult.
    assert.deepEqual(await loadThreads(ws), {})
  })
})

test('threads: a corrupt store is repaired by the next save', async () => {
  await withTempHome(async (dir) => {
    const ws = path.join(dir, 'ws')
    await saveThread(ws, 'bubble-sky', RECORD)
    await writeFile(threadsPath(ws), 'garbage', 'utf8')

    await saveThread(ws, 'amber-moss', { ...RECORD, agent: 'hera' })

    assert.deepEqual(Object.keys(await loadThreads(ws)), ['amber-moss'])
    const onDisk = await readFile(threadsPath(ws), 'utf8')
    assert.doesNotThrow(() => JSON.parse(onDisk), 'the file is valid JSON again')
  })
})

// --- [TEST-THR-03] naming -------------------------------------------------

test('threads: a new name is two hyphenated words', async () => {
  const name = newSessionName([], [])

  assert.match(name, /^[a-z]+-[a-z]+$/, 'two lowercase words, one hyphen')
})

test('threads: a new name never collides with one already in the workspace', () => {
  // Exhaust nearly everything: whatever is left must still be fresh.
  const taken = []
  for (let i = 0; i < 200; i += 1) {
    const name = newSessionName(taken, [])
    assert.ok(!taken.includes(name), `draw ${i} repeated ${name}`)
    assert.match(name, /^[a-z]+-[a-z]+$/)
    taken.push(name)
  }
})

test('threads: a new name is never a roster agent name', () => {
  // An agent is WHO answers; a session is WHICH conversation. If the two could
  // share a name, "ask ares in ares" would be ambiguous to a reader and to the
  // lead composing the command.
  const agents = ['ares', 'athena', 'bubble-sky']

  for (let i = 0; i < 100; i += 1) {
    assert.ok(!agents.includes(newSessionName([], agents)))
  }
})

test('threads: the vocabulary is concrete, not mythological', async () => {
  const { listPresetIds } = await import('../hosts/lib/presets.js')
  const mythological = new Set(listPresetIds())

  for (let i = 0; i < 100; i += 1) {
    for (const word of newSessionName([], []).split('-')) {
      assert.ok(!mythological.has(word), `${word} is a preset name — too easy to confuse`)
    }
  }
})

test('threads: it gives up loudly rather than looping forever', () => {
  // A caller that has somehow taken every name must get an error, not a hang.
  const everything = allSessionNames()

  assert.throws(() => newSessionName(everything, []), /no unused session name/i)
})

// --- who a conversation belongs to ---------------------------------------

test('lead: the harness session wins over the pane it sits in', async () => {
  const { leadId } = await import('../hosts/lib/threads.js')

  // Same pane, new Claude Code session: the pane id has not changed, so a
  // pane-first rule would hand the new lead the previous one's conversations.
  assert.equal(leadId({ CLAUDE_CODE_SESSION_ID: 'sess-1', CMUX_SURFACE_ID: 'pane-A' }), 'sess-1')
  assert.notEqual(
    leadId({ CLAUDE_CODE_SESSION_ID: 'sess-2', CMUX_SURFACE_ID: 'pane-A' }),
    leadId({ CLAUDE_CODE_SESSION_ID: 'sess-1', CMUX_SURFACE_ID: 'pane-A' }),
  )
})

test('lead: a harness that names no session is identified by its window', async () => {
  const { leadId } = await import('../hosts/lib/threads.js')

  // codex, pi and opencode publish no session id to their children. The pane
  // or terminal window they run in is one lead for as long as it is open.
  assert.equal(leadId({ CMUX_SURFACE_ID: 'pane-A' }), 'pane-A')
  assert.equal(leadId({ ITERM_SESSION_ID: 'w0t0p0:UUID' }), 'w0t0p0:UUID')
  assert.equal(leadId({ TERM_SESSION_ID: 'abc' }), 'abc')
})

test('lead: unidentified is nobody, never everybody', async () => {
  const { leadId } = await import('../hosts/lib/threads.js')

  // Two anonymous shells must not count as one lead — that is the reuse this
  // exists to stop, for the leads least able to notice it happening.
  assert.equal(leadId({}), null)
  assert.equal(leadId({ CLAUDE_CODE_SESSION_ID: '' }), null)
  assert.equal(leadId(), null)
})
