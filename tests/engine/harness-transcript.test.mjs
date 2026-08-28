import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { harnessTurns } from '../../hosts/lib/harness-transcript.js'

/**
 * Fixtures, not the developer's real stores.
 *
 * These readers parse four tools' internal session files. The shapes are copied
 * from real ones (surveyed 2026-08-24) but the tests must never depend on the
 * machine having them, and must never read a stranger's conversations.
 */
async function withStores(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-htr-'))
  try {
    return await fn({ HOME: dir, XDG_DATA_HOME: path.join(dir, '.local', 'share') })
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

const write = async (file, text) => {
  await mkdir(path.dirname(file), { recursive: true })
  await writeFile(file, text, 'utf8')
}

test('harness transcript: codex is read from its rollout file', async () => {
  await withStores(async (env) => {
    const id = '01a03068-3530-7173-a123-009f15591007'
    await write(
      path.join(
        env.HOME,
        '.codex',
        'sessions',
        '2026',
        '08',
        '23',
        `rollout-2026-08-23T23-55-30-${id}.jsonl`,
      ),
      [
        JSON.stringify({ type: 'session_meta' }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'user',
            content: [{ type: 'input_text', text: 'tell me a joke' }],
          },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            type: 'message',
            role: 'assistant',
            content: [{ type: 'output_text', text: 'light attracts bugs' }],
          },
        }),
        'not json at all',
      ].join('\n'),
    )

    const turns = await harnessTurns('codex', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: 'tell me a joke' },
      { role: 'assistant', text: 'light attracts bugs' },
    ])
  })
})

test('harness transcript: claude is read from the file named after the session', async () => {
  await withStores(async (env) => {
    const id = '4ea93ea2-dd16-442d-879e-3a14d48b48a9'
    await write(
      path.join(env.HOME, '.claude', 'projects', '-Users-x-proj', `${id}.jsonl`),
      [
        JSON.stringify({ type: 'user', message: { role: 'user', content: 'what changed?' } }),
        JSON.stringify({
          type: 'assistant',
          message: { role: 'assistant', content: [{ type: 'text', text: 'two files' }] },
        }),
      ].join('\n'),
    )

    const turns = await harnessTurns('claude-code', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: 'what changed?' },
      { role: 'assistant', text: 'two files' },
    ])
  })
})

test('harness transcript: pi is read from the uuid in its filename', async () => {
  await withStores(async (env) => {
    const id = '019eac84-de84-74e9-b402-61918f2eaf6c'
    await write(
      path.join(
        env.HOME,
        '.pi',
        'agent',
        'sessions',
        '--Users-x-proj--',
        `2026-06-09T13-14-09-412Z_${id}.jsonl`,
      ),
      [
        JSON.stringify({ type: 'session' }),
        JSON.stringify({
          type: 'message',
          message: { role: 'user', content: [{ type: 'text', text: 'ping' }] },
        }),
        JSON.stringify({
          type: 'message',
          message: { role: 'assistant', content: [{ type: 'text', text: 'pong' }] },
        }),
      ].join('\n'),
    )

    const turns = await harnessTurns('pi', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: 'ping' },
      { role: 'assistant', text: 'pong' },
    ])
  })
})

test('harness transcript: opencode is assembled from its message and part files', async () => {
  await withStores(async (env) => {
    const id = 'ses_46bcb2ee4ffe2pXrmy1mnDJbNZ'
    const store = path.join(env.XDG_DATA_HOME, 'opencode', 'storage')
    await write(
      path.join(store, 'message', id, 'msg_aaa.json'),
      JSON.stringify({ id: 'msg_aaa', sessionID: id, role: 'user', time: { created: 1 } }),
    )
    await write(
      path.join(store, 'message', id, 'msg_bbb.json'),
      JSON.stringify({ id: 'msg_bbb', sessionID: id, role: 'assistant', time: { created: 2 } }),
    )
    await write(
      path.join(store, 'part', 'msg_aaa', 'prt_1.json'),
      JSON.stringify({ type: 'text', text: 'is it safe?' }),
    )
    await write(
      path.join(store, 'part', 'msg_bbb', 'prt_2.json'),
      JSON.stringify({ type: 'text', text: 'yes, with a caveat' }),
    )

    const turns = await harnessTurns('opencode', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: 'is it safe?' },
      { role: 'assistant', text: 'yes, with a caveat' },
    ])
  })
})

test('harness transcript: a store we cannot find is empty, never an error', async () => {
  await withStores(async (env) => {
    // These are undocumented internal files: a harness may move them in a
    // patch release. Catching up is a convenience — it must degrade to
    // "nothing to show", never break the caller.
    for (const kind of ['codex', 'claude-code', 'pi', 'opencode']) {
      assert.deepEqual(await harnessTurns(kind, 'no-such-session', env), [])
    }
    assert.deepEqual(await harnessTurns('image', 'x', env), [])
    assert.deepEqual(await harnessTurns('codex', '', env), [])
  })
})

test('harness transcript: the harness own injected context is not a turn', async () => {
  await withStores(async (env) => {
    const id = 'inject-test'
    await write(
      path.join(env.HOME, '.codex', 'sessions', '2026', '08', '24', `rollout-${id}.jsonl`),
      [
        // codex injects these as `user` messages. They are the environment
        // talking to itself, and showing them as "you asked" is a lie. Shape
        // copied from a real rollout on 2026-08-27, closing tag included —
        // the abbreviated version this fixture used to carry was what made
        // "starts with <" look like a safe test for "is injected context".
        JSON.stringify({
          type: 'response_item',
          payload: {
            role: 'user',
            content: [
              { text: '<recommended_plugins>\nHere is a list of plugins…\n</recommended_plugins>' },
            ],
          },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: {
            role: 'user',
            content: [{ text: '<skills_instructions>\n## Skills\n</skills_instructions>' }],
          },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: { role: 'user', content: [{ text: 'a real question' }] },
        }),
      ].join('\n'),
    )

    const turns = await harnessTurns('codex', id, env)

    assert.deepEqual(turns, [{ role: 'user', text: 'a real question' }])
  })
})

test('harness transcript: our own packet is shown as the question inside it', async () => {
  await withStores(async (env) => {
    const id = 'packet-test'
    const packet = [
      '# ConsensFlow Packet',
      'Created: 2026-08-24T00:00:00.000Z',
      'Workspace: /tmp/ws',
      '',
      '## How to work',
      'You can read and modify this workspace — edit files and run commands as needed.',
      '',
      '## Message from the user',
      'Tell me a joke.',
      '',
      'Respond directly and conversationally. There is no required format.',
    ].join('\n')
    await write(
      path.join(env.HOME, '.codex', 'sessions', '2026', '08', '24', `rollout-${id}.jsonl`),
      JSON.stringify({
        type: 'response_item',
        payload: { role: 'user', content: [{ text: packet }] },
      }),
    )

    const turns = await harnessTurns('codex', id, env)

    // Reading back a conversation should show what was asked, not the
    // scaffolding we wrapped it in.
    assert.deepEqual(turns, [{ role: 'user', text: 'Tell me a joke.' }])
  })
})

test('harness transcript: kimi is assembled from its wire log, parts joined per turn', async () => {
  await withStores(async (env) => {
    const id = 'session_ba239627-2ddf-40b9-9a57-b9ae8a33fd30'
    // The shape below is copied from a real Kimi Code 0.38.0 session
    // (probed 2026-08-24): the id names a DIRECTORY, and the log is events.
    await write(
      path.join(
        env.HOME,
        '.kimi-code',
        'sessions',
        'wd_proj_abc',
        id,
        'agents',
        'main',
        'wire.jsonl',
      ),
      [
        JSON.stringify({ type: 'metadata' }),
        JSON.stringify({
          type: 'turn.prompt',
          input: [{ type: 'text', text: 'Reply with exactly: pong' }],
          origin: { kind: 'user' },
        }),
        JSON.stringify({
          type: 'context.append_loop_event',
          event: { type: 'content.part', turnId: '0', part: { type: 'think', think: 'simple' } },
        }),
        // One answer arriving as several parts — counting each as a turn would
        // inflate the number `--unread` and `--wait` both key on.
        JSON.stringify({
          type: 'context.append_loop_event',
          event: { type: 'content.part', turnId: '0', part: { type: 'text', text: 'po' } },
        }),
        JSON.stringify({
          type: 'context.append_loop_event',
          event: { type: 'content.part', turnId: '0', part: { type: 'text', text: 'ng' } },
        }),
        // The environment's own injected block arrives under a user role.
        JSON.stringify({
          type: 'context.append_message',
          message: { role: 'user', content: [{ type: 'text', text: '<system-reminder>…' }] },
        }),
        JSON.stringify({
          type: 'turn.prompt',
          input: [{ type: 'text', text: 'and again?' }],
          origin: { kind: 'user' },
        }),
        JSON.stringify({
          type: 'context.append_loop_event',
          event: { type: 'content.part', turnId: '1', part: { type: 'text', text: 'pong again' } },
        }),
      ].join('\n'),
    )

    const turns = await harnessTurns('kimi', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: 'Reply with exactly: pong' },
      { role: 'assistant', text: 'pong' },
      { role: 'user', text: 'and again?' },
      { role: 'assistant', text: 'pong again' },
    ])
  })
})

test('harness transcript: a kimi session we cannot find is empty, never an error', async () => {
  await withStores(async (env) => {
    assert.deepEqual(await harnessTurns('kimi', 'session_nope', env), [])
  })
})

test('harness transcript: opencode is read from its database, not the frozen files', async () => {
  // opencode migrated its store into SQLite on 2026-01-06 and the JSON tree
  // stopped being written. The file reader kept passing against fixtures in
  // the old shape while returning nothing for any real conversation — green
  // tests over a dead format. This one uses the shape opencode writes today.
  const { DatabaseSync } = await import('node:sqlite')
  await withStores(async (env) => {
    const dir = path.join(env.XDG_DATA_HOME, 'opencode')
    await mkdir(dir, { recursive: true })
    const db = new DatabaseSync(path.join(dir, 'opencode.db'))
    db.exec(`
      create table session (id text primary key, directory text, time_created integer);
      create table message (id text primary key, session_id text, time_created integer, data text);
      create table part (id text primary key, message_id text, session_id text, time_created integer, data text);
    `)
    db.exec(`insert into session values ('ses_new', '/work/here', 5000)`)
    db.exec(
      `insert into message values ('m1', 'ses_new', 1, '${JSON.stringify({ role: 'user' })}')`,
    )
    db.exec(
      `insert into message values ('m2', 'ses_new', 2, '${JSON.stringify({ role: 'assistant' })}')`,
    )
    db.exec(
      `insert into part values ('p1', 'm1', 'ses_new', 1, '${JSON.stringify({ type: 'text', text: 'is it safe?' })}')`,
    )
    // One answer, many parts — joined per message, as the store splits them.
    db.exec(
      `insert into part values ('p2', 'm2', 'ses_new', 2, '${JSON.stringify({ type: 'text', text: 'yes,' })}')`,
    )
    db.exec(
      `insert into part values ('p3', 'm2', 'ses_new', 3, '${JSON.stringify({ type: 'text', text: 'with a caveat' })}')`,
    )
    db.close()

    assert.deepEqual(await harnessTurns('opencode', 'ses_new', env), [
      { role: 'user', text: 'is it safe?' },
      { role: 'assistant', text: 'yes,\nwith a caveat' },
    ])

    const { discoverOpencodeSession } = await import('../../hosts/lib/harness-transcript.js')
    assert.equal(await discoverOpencodeSession('/work/here', 4000, env), 'ses_new')
    assert.equal(await discoverOpencodeSession('/work/here', 6000, env), null, 'nothing since')
    assert.equal(await discoverOpencodeSession('/work/elsewhere', 0, env), null)
  })
})

test('harness transcript: a conversation older than the migration still reads', async () => {
  // No database on this machine at all: the frozen JSON layout is still the
  // record for anything from before 2026-01-06.
  await withStores(async (env) => {
    const id = 'ses_before_migration'
    const store = path.join(env.XDG_DATA_HOME, 'opencode', 'storage')
    await write(
      path.join(store, 'message', id, 'msg_a.json'),
      JSON.stringify({ id: 'msg_a', sessionID: id, role: 'user', time: { created: 1 } }),
    )
    await write(
      path.join(store, 'part', 'msg_a', 'prt_1.json'),
      JSON.stringify({ type: 'text', text: 'from the old days' }),
    )

    assert.deepEqual(await harnessTurns('opencode', id, env), [
      { role: 'user', text: 'from the old days' },
    ])
  })
})

test('harness transcript: a tag is not injected context — real text starting with < survives', async () => {
  // Live 2026-08-27: `readable` dropped ANY turn whose text began with `<`,
  // for both roles. So a question about markup vanished, and so did an answer
  // that opened with one — while our own packet tells the agent to "return
  // only the requested output", which is exactly how an agent asked for HTML
  // replies. What is stripped now is a COMPLETE <tag>…</tag> block, which is
  // what an environment injects; a lone opening tag is somebody talking.
  await withStores(async (env) => {
    const id = 'markup-test'
    await write(
      path.join(env.HOME, '.codex', 'sessions', '2026', '08', '24', `rollout-${id}.jsonl`),
      [
        JSON.stringify({
          type: 'response_item',
          payload: { role: 'user', content: [{ text: '<div> tags are escaping wrong, why?' }] },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: { role: 'assistant', content: [{ text: '<!doctype html>\n<html>…</html>' }] },
        }),
        // A block with a real question after it keeps the question.
        JSON.stringify({
          type: 'response_item',
          payload: {
            role: 'user',
            content: [{ text: '<skills_instructions>\nx\n</skills_instructions>\nand now mine' }],
          },
        }),
      ].join('\n'),
    )

    const turns = await harnessTurns('codex', id, env)

    assert.deepEqual(turns, [
      { role: 'user', text: '<div> tags are escaping wrong, why?' },
      { role: 'assistant', text: '<!doctype html>\n<html>…</html>' },
      { role: 'user', text: 'and now mine' },
    ])
  })
})

// --- finding the session codex minted --------------------------------------

/** A rollout as codex writes one: metadata first, then the turns. */
const rollout = (created, cwd, turns = []) =>
  [
    JSON.stringify({ type: 'session_meta', payload: { cwd, timestamp: created } }),
    ...turns.map((text) =>
      JSON.stringify({
        type: 'response_item',
        payload: { type: 'message', role: 'user', content: [{ type: 'input_text', text }] },
      }),
    ),
  ].join('\n')

test('discovery: codex is matched on when its session was created, not when its file was written', async () => {
  // The lead asking for an agent is often itself a codex in the same
  // directory. The moment it takes a turn its rollout is the most recently
  // WRITTEN file there, and ranking by mtime named the lead's own conversation
  // as the agent's (live, 2026-08-28). Both files here are written now; only
  // the age of the sessions inside them differs.
  await withStores(async (env) => {
    const { discoverCodexSession } = await import('../../hosts/lib/harness-transcript.js')
    const dir = path.join(env.HOME, '.codex', 'sessions', '2026', '08', '28')
    const lead = '01a048fa-9c3d-7941-9860-00000000lead'
    const ours = '01a048fa-9c3d-7941-9860-00000000ours'
    const iso = (offset) => new Date(Date.now() + offset).toISOString()
    await write(path.join(dir, `rollout-x-${lead}.jsonl`), rollout(iso(-3_600_000), '/work/here'))
    await write(path.join(dir, `rollout-x-${ours}.jsonl`), rollout(iso(0), '/work/here'))

    const since = Date.now() - 60_000
    assert.equal(await discoverCodexSession('/work/here', since, env), ours)
    assert.equal(await discoverCodexSession('/work/elsewhere', since, env), null)
    assert.equal(
      await discoverCodexSession('/work/here', Date.now() + 60_000, env),
      null,
      'nothing created since',
    )
  })
})

test('discovery: with a seed, ours is the session carrying it — whatever else appeared', async () => {
  // codex can sit half an hour on its trust prompt before a session exists, so
  // the search has to outlast a person, and anything else that shows up in the
  // directory meanwhile belongs to somebody else. The stranger here is the
  // EARLIER of the two, so no ordering rule alone would tell them apart.
  await withStores(async (env) => {
    const { discoverCodexSession } = await import('../../hosts/lib/harness-transcript.js')
    const dir = path.join(env.HOME, '.codex', 'sessions', '2026', '08', '28')
    const stranger = '01a048fa-9c3d-7941-9860-00000000them'
    const ours = '01a048fa-9c3d-7941-9860-00000000ours'
    const iso = (offset) => new Date(Date.now() + offset).toISOString()
    await write(
      path.join(dir, `rollout-x-${stranger}.jsonl`),
      rollout(iso(-1000), '/work/here', ['what the lead was already talking about']),
    )
    await write(
      path.join(dir, `rollout-x-${ours}.jsonl`),
      rollout(iso(0), '/work/here', ['## Your brief\nreview the retry path']),
    )

    const since = Date.now() - 60_000
    const seeded = await discoverCodexSession('/work/here', since, env, {
      // Wrapped by the window, but the same text — whitespace is not identity.
      seed: '## Your brief    review the retry path',
    })
    assert.equal(seeded, ours)
    assert.equal(
      await discoverCodexSession('/work/here', since, env),
      stranger,
      'without a seed, the earliest session created since is the best guess left',
    )
    assert.equal(
      await discoverCodexSession('/work/here', since, env, { seed: 'a question nobody asked' }),
      null,
      'and an exact search that matches nothing takes nothing',
    )
  })
})
