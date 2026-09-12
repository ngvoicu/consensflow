import assert from 'node:assert/strict'
import { chmod, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { send } from '../src/channels/codex.js'

const SESSION = '01a0817b-e6b0-7f32-8e11-370dc000cbc0'

async function fakeCli(root, mode = 'success') {
  const executable = join(root, 'codex-fake')
  await writeFile(
    executable,
    `#!${process.execPath}
import { appendFileSync } from 'node:fs'

const capture = process.env.CF_FAKE_CAPTURE
const entry = {
  kind: 'start',
  argv: process.argv.slice(2),
  cwd: process.cwd(),
  openaiApiKey: process.env.OPENAI_API_KEY ?? null,
  pid: process.pid,
}
appendFileSync(capture, JSON.stringify(entry) + '\\n')

if (process.env.CF_FAKE_MODE === 'large-failure') {
  process.stdout.write('o'.repeat(100_000))
  process.stderr.write('e'.repeat(100_000))
  process.exit(7)
}

if (process.env.CF_FAKE_MODE === 'failure') process.exit(7)

if (process.env.CF_FAKE_MODE === 'timeout') {
  process.on('SIGTERM', () => {
    appendFileSync(capture, JSON.stringify({ kind: 'stopped', pid: process.pid }) + '\\n')
    process.exit(143)
  })
  setInterval(() => {}, 1000)
}
`,
    'utf8',
  )
  await chmod(executable, 0o755)
  return { executable, mode }
}

async function fixture(mode = 'success', timeoutMs = 3_000) {
  const root = await realpath(await mkdtemp(join(tmpdir(), 'consensflow-codex-channel-')))
  const capture = join(root, 'capture.jsonl')
  const fake = await fakeCli(root, mode)
  const launch = {
    kind: 'codex-queue',
    executable: fake.executable,
    cwd: root,
    env: {
      CF_FAKE_CAPTURE: capture,
      CF_FAKE_MODE: mode,
      OPENAI_API_KEY: 'must-not-cross-the-boundary',
    },
    timeoutMs,
  }
  return {
    root,
    launch,
    target: {
      session: SESSION,
      pane: 'codex-pane',
      generation: 3,
      epoch: 8,
      deadlineMs: 3_000,
      launch,
    },
    capture: async () => {
      const text = await readFile(capture, 'utf8').catch(() => '')
      return text
        .trim()
        .split('\n')
        .filter(Boolean)
        .map((line) => JSON.parse(line))
    },
    cleanup: () => rm(root, { recursive: true, force: true }),
  }
}

it('sends the exact queue argv and canonical text from an absolute launch', async () => {
  const f = await fixture()
  const order = []
  f.target.claimEpoch = async (request, operation) => {
    order.push({ request, operation })
    return { ok: true }
  }
  try {
    assert.deepEqual(await send(f.target, 'a message with spaces\nand newlines'), {
      ok: true,
      admitted: true,
    })
    assert.deepEqual(order, [
      {
        request: { pane: 'codex-pane', generation: 3, epoch: 8 },
        operation: 'pane.claim_native_epoch',
      },
    ])
    const [entry] = await f.capture()
    assert.equal(entry.kind, 'start')
    assert.deepEqual(entry.argv, [
      'queue',
      '--thread',
      SESSION,
      '--message',
      'a message with spaces\nand newlines',
    ])
    assert.equal(entry.cwd, f.root)
    assert.equal(entry.openaiApiKey, null)
  } finally {
    await f.cleanup()
  }
})

it('rejects malformed native session UUIDs before claiming or spawning', async () => {
  for (const session of ['', 'not-a-uuid', 'aaaaaaaa-bbbb-4ccc-8ddd-40940940940', `${SESSION}\n`]) {
    const f = await fixture()
    let claims = 0
    f.target.session = session
    f.target.claimEpoch = async () => {
      claims += 1
      return { ok: true }
    }
    try {
      await assert.rejects(() => send(f.target, 'invalid session'), /UUID/)
      assert.equal(claims, 0)
      assert.deepEqual(await f.capture(), [])
    } finally {
      await f.cleanup()
    }
  }
})

it('rejects an unscoped launch or malformed pane contract before claiming or spawning', async () => {
  const cases = [
    [
      (target) => {
        target.launch = { ...target.launch, kind: 'codex' }
      },
      /codex-queue launch configuration/,
    ],
    [
      (target) => {
        target.launch = { ...target.launch, executable: 'codex' }
      },
      /absolute launch executable/,
    ],
    [
      (target) => {
        target.launch = { ...target.launch, cwd: 'workspace' }
      },
      /absolute launch cwd/,
    ],
    [
      (target) => {
        target.pane = ''
      },
      /pane \{id, generation\}/,
    ],
    [
      (target) => {
        target.generation = 0
      },
      /pane \{id, generation\}/,
    ],
    [
      (target) => {
        target.epoch = -1
      },
      /observed input epoch/,
    ],
  ]

  for (const [mutate, expected] of cases) {
    const f = await fixture()
    let claims = 0
    mutate(f.target)
    f.target.claimEpoch = async () => {
      claims += 1
      return { ok: true }
    }
    try {
      await assert.rejects(() => send(f.target, 'invalid caller'), expected)
      assert.equal(claims, 0)
      assert.deepEqual(await f.capture(), [])
    } finally {
      await f.cleanup()
    }
  }
})

it('turns a stale native claim into an affirmative zero-byte refusal without spawning', async () => {
  const f = await fixture()
  const order = []
  f.target.claimEpoch = async (_request, operation) => {
    order.push(operation)
    return { ok: false, error: 'stale' }
  }
  try {
    assert.deepEqual(await send(f.target, 'stale message'), {
      ok: false,
      admitted: false,
      error: 'failed-with-zero-bytes',
      bytesWritten: 0,
      cause: 'stale',
    })
    assert.deepEqual(order, ['pane.claim_native_epoch'])
    assert.deepEqual(await f.capture(), [])
  } finally {
    await f.cleanup()
  }
})

it('reports a nonzero helper exit as uncertain and performs no automatic retry', async () => {
  const f = await fixture('failure')
  f.target.claimEpoch = async () => ({ ok: true })
  try {
    const result = await send(f.target, 'possibly queued')
    assert.equal(result.ok, false)
    assert.equal(result.admitted, null)
    assert.equal(result.error, 'uncertain')
    assert.equal(result.exitCode, 7)
    assert.equal((await f.capture()).filter((entry) => entry.kind === 'start').length, 1)
  } finally {
    await f.cleanup()
  }
})

it('reports a helper spawn error as uncertain after the spawn boundary', async () => {
  const f = await fixture()
  f.target.claimEpoch = async () => ({ ok: true })
  f.target.launch = { ...f.target.launch, executable: join(f.root, 'missing-codex') }
  try {
    const result = await send(f.target, 'transport may have started')
    assert.deepEqual(
      { ok: result.ok, admitted: result.admitted, error: result.error, cause: result.cause },
      { ok: false, admitted: null, error: 'uncertain', cause: 'transport' },
    )
    assert.deepEqual(await f.capture(), [])
  } finally {
    await f.cleanup()
  }
})

it('bounds helper output while preserving uncertain transport evidence', async () => {
  const f = await fixture('large-failure')
  f.target.claimEpoch = async () => ({ ok: true })
  try {
    const result = await send(f.target, 'large output')
    assert.equal(result.error, 'uncertain')
    assert.ok(Buffer.byteLength(result.stdout, 'utf8') <= 64 * 1024)
    assert.ok(Buffer.byteLength(result.stderr, 'utf8') <= 64 * 1024)
  } finally {
    await f.cleanup()
  }
})

it('stops a timed-out helper and does not retry the ambiguous queue operation', async () => {
  const f = await fixture('timeout', 2500)
  f.target.deadlineMs = 2500
  f.target.claimEpoch = async () => ({ ok: true })
  try {
    const result = await send(f.target, 'may already be queued')
    assert.deepEqual(
      { ok: result.ok, admitted: result.admitted, error: result.error, cause: result.cause },
      { ok: false, admitted: null, error: 'uncertain', cause: 'deadline' },
    )
    const entries = await f.capture()
    assert.equal(entries.filter((entry) => entry.kind === 'start').length, 1)
    assert.equal(entries.filter((entry) => entry.kind === 'stopped').length, 1)
  } finally {
    await f.cleanup()
  }
})

it('uses the owned bridge for exact identity and rejects a session switch after the pane claim', async () => {
  const { createServer } = await import('node:http')
  const { currentSession } = await import('../src/channels/codex.js')
  const f = await fixture()
  let selected = SESSION
  const received = []
  const server = createServer(async (request, response) => {
    response.setHeader('content-type', 'application/json')
    if (request.url === '/session')
      return response.end(JSON.stringify({ launchId: 'owned', sessionId: selected }))
    let text = ''
    for await (const chunk of request) text += chunk
    const input = JSON.parse(text)
    received.push(input)
    response.end(
      JSON.stringify(
        input.sessionId === selected
          ? { ok: true, admitted: true }
          : { ok: false, admitted: false, bytesWritten: 0, error: 'native-session-changed' },
      ),
    )
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  f.target.launch = {
    ...f.launch,
    launchId: 'owned',
    sessionBridge: {
      endpoint: `http://127.0.0.1:${server.address().port}`,
      token: 'private-owned-bridge-token-123',
    },
  }
  try {
    assert.equal(await currentSession(f.target.launch), SESSION)
    f.target.claimEpoch = async () => {
      selected = '01a09094-a559-7db0-bf50-e2309856c3c0'
      return { ok: true }
    }
    assert.deepEqual(await send(f.target, 'complete reply'), {
      ok: false,
      admitted: false,
      bytesWritten: 0,
      error: 'native-session-changed',
    })
    assert.equal(received[0].sessionId, SESSION)
    assert.deepEqual(await f.capture(), [], 'the old global queue helper must never be spawned')
    f.target.session = selected
    assert.deepEqual(await send(f.target, 'complete reply'), { ok: true, admitted: true })
    assert.equal(received[1].text, 'complete reply')
  } finally {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
    await f.cleanup()
  }
})
