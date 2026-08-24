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
        // talking to itself, and showing them as "you asked" is a lie.
        JSON.stringify({
          type: 'response_item',
          payload: { role: 'user', content: [{ text: '<recommended_plugins>\nAirtable…' }] },
        }),
        JSON.stringify({
          type: 'response_item',
          payload: { role: 'user', content: [{ text: '<skills_instructions>\n## Skills' }] },
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
