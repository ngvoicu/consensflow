import assert from 'node:assert/strict'
import { chmod, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { deliver, send } from '../src/channels/codex.js'

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

function record(overrides = {}) {
  return {
    id: 'd-33',
    answerId: 'answer-7',
    conversation: 'worker-one',
    agent: 'zeus',
    answer: 'The answer is complete.',
    channel: 'pty-inline',
    expiresAt: Date.now() + 3_000,
    ...overrides,
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

it('uses the canonical envelope and cf-read pointer without rewriting the text', async () => {
  for (const channel of ['pty-inline', 'cf-read']) {
    const f = await fixture()
    f.target.claimEpoch = async () => ({ ok: true })
    try {
      await deliver('codex-queue', f.target, record({ channel }))
      const [entry] = await f.capture()
      assert.equal(entry.argv[0], 'queue')
      assert.equal(entry.argv[1], '--thread')
      assert.equal(entry.argv[2], SESSION)
      assert.equal(entry.argv[3], '--message')
      if (channel === 'cf-read') {
        assert.equal(
          entry.argv[4],
          '@zeus answered in worker-one — run: cf read d-33  (it prints everything; read all of it)',
        )
      } else {
        assert.equal(
          entry.argv[4],
          '[consensflow delivery d-33 from worker-one #answer-7]\n' +
            'The answer is complete.\n' +
            '[end of delivery d-33]\n',
        )
      }
    } finally {
      await f.cleanup()
    }
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

it('refuses an expired delivery before the native claim or helper spawn', async () => {
  const f = await fixture()
  let claims = 0
  f.target.claimEpoch = async () => {
    claims += 1
    return { ok: true }
  }
  try {
    assert.deepEqual(
      await deliver('codex-queue', f.target, record({ expiresAt: Date.now() - 1 })),
      {
        ok: false,
        admitted: false,
        error: 'expired',
        bytesWritten: 0,
      },
    )
    assert.equal(claims, 0)
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
