import assert from 'node:assert/strict'
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import {
  compactionOf,
  createDeliveryExtension,
} from '../hosts/pi-extension/consensflow-delivery.mjs'
import { cargoMissing, rustPi } from './rust-channels.mjs'

function fakePi() {
  const handlers = new Map()
  const sent = []
  let firstSend
  const sentOnce = new Promise((resolve) => {
    firstSend = resolve
  })
  return {
    sentOnce,
    handlers,
    sent,
    on(event, handler) {
      handlers.set(event, handler)
    },
    sendUserMessage(content) {
      sent.push(content)
      firstSend()
    },
  }
}

function context(isIdle, { sessionId = 'native-pi-session', leafId = 'leaf-1' } = {}) {
  return {
    isIdle,
    sessionManager: {
      getSessionId: () => sessionId,
      getLeafId: () => leafId,
    },
  }
}

async function exists(path) {
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}

async function waitFor(path, timeoutMs = 300) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await exists(path)) return JSON.parse(await readFile(path, 'utf8'))
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error(`timed out waiting for ${path}`)
}

async function setup(
  record,
  {
    idle = true,
    // The record's life, as the channel gives it: long, so a busy runner reads
    // it before it expires (1 s once expired one before its other refusal);
    // a test about expiry sets its own.
    ackTimeoutMs = 30_000,
    editor = '',
    hasUI = true,
    mode = 'tui',
    leaf,
    pendingMessages = false,
    watchInbox,
    pollMs,
    compaction,
    usage,
  } = {},
) {
  const root = await mkdtemp(join(tmpdir(), 'consensflow-pi-extension-'))
  const inbox = join(root, 'inbox')
  const ack = join(root, 'ack')
  const quarantine = join(root, 'quarantine')
  const settled = join(root, 'settled')
  const expired = join(root, 'expired')
  await Promise.all([mkdir(inbox), mkdir(ack), mkdir(quarantine)])
  const pi = fakePi()
  const logs = []
  const extension = createDeliveryExtension(pi, {
    inbox,
    ack,
    quarantine,
    settled,
    expired,
    launchId: 'launch-pi-test',
    logger: { error: (...args) => logs.push(args.join(' ')) },
    ...(watchInbox ? { watchInbox } : {}),
    ...(pollMs ? { pollMs } : {}),
    ...(compaction !== undefined ? { compaction } : {}),
  })
  let currentIdle = idle
  // Pi's own compaction state, as Pi 1.4.0 and 1.5.0 keep it: `isIdle()` is false from the
  // moment a compaction begins until its state is cleared (`!this._isAgentRunActive &&
  // !this.isCompacting`, dist/core/agent-session.js), whoever began it.
  let piCompacting = false
  const ctx = context(() => currentIdle && !piCompacting)
  ctx.hasUI = hasUI
  ctx.mode = mode
  ctx.hasPendingMessages = () => pendingMessages
  ctx.sessionManager.getLeafEntry = () => leaf
  ctx.ui = { getEditorText: () => editor }
  // Pi's own context reading and compaction (ExtensionContext, Pi 1.4.0 and 1.5.0): the
  // reading the test gives, and each compaction, which the test ends itself in Pi's order.
  let currentUsage = usage
  const compactions = []
  ctx.getContextUsage = () => currentUsage
  const tell = (name, event) => pi.handlers.get(name)?.(event, ctx)
  // A compaction, begun by `ctx.compact()` (reason `manual`, its `options`) or by Pi itself at
  // its own point (`threshold`, `overflow`): Pi is not idle from its start; the extension is
  // told `session_before_compact`. It ends complete (`session_compact`, told while Pi is still
  // compacting, then its state cleared), failed or cancelled (its state cleared, then
  // `session_compact_failed`), and only then the caller's callback, as `compact()` returns.
  const begin = (reason, options) => {
    piCompacting = true
    void tell('session_before_compact', {
      type: 'session_before_compact',
      reason,
      willRetry: false,
    })
    const handle = {
      options,
      reason,
      complete: async (result = { tokensBefore: 150_005, summary: 'the summary' }) => {
        await tell('session_compact', { type: 'session_compact', reason, willRetry: false })
        piCompacting = false
        options?.onComplete?.(result)
      },
      fail: async (message = 'the summary model failed') => {
        piCompacting = false
        await tell('session_compact_failed', {
          type: 'session_compact_failed',
          reason,
          errorMessage: `Compaction failed: ${message}`,
          aborted: false,
          willRetry: false,
        })
        options?.onError?.(new Error(message))
      },
      // Pi clears its state and tells no extension (it tells `session_compact` only where it
      // finds the entry it saved).
      endUntold: async () => {
        piCompacting = false
      },
      cancel: async () => {
        piCompacting = false
        await tell('session_compact_failed', {
          type: 'session_compact_failed',
          reason,
          aborted: true,
          willRetry: false,
        })
        options?.onError?.(new Error('Compaction cancelled'))
      },
    }
    compactions.push(handle)
    return handle
  }
  ctx.compact = (options) => {
    begin('manual', options)
  }
  const payload =
    typeof record?.id === 'string' &&
    /^m-[a-f0-9]+$/.test(record.id) &&
    record.expiresAt === undefined
      ? { ...record, expiresAt: Date.now() + ackTimeoutMs }
      : record
  if (record !== null)
    await writeFile(join(inbox, `${record.file ?? record.id}.json`), `${JSON.stringify(payload)}\n`)
  await pi.handlers.get('session_start')({}, ctx)
  return {
    ack,
    quarantine,
    settled,
    expired,
    close: async () => {
      await pi.handlers.get('session_shutdown')()
      await rm(root, { recursive: true, force: true })
    },
    extension,
    inbox,
    logs,
    pi,
    setIdle(value) {
      currentIdle = value
    },
    setEditor(value) {
      editor = value
    },
    setUsage(value) {
      currentUsage = value
    },
    compactions,
    /** Pi compacts by itself, at its own point. */
    piCompacts: (reason = 'threshold') => begin(reason),
    ctx,
  }
}

const envelopeRecord = {
  id: 'm-00000000000000000000000000000051',
  type: 'message',
  launchId: 'launch-pi-test',
  session: 'native-pi-session',
  text: 'exact task text',
}
const envelope = envelopeRecord.text

describe('consensflow Pi extension', () => {
  // Text the human left in the editor holds nothing and stays there (the
  // owner's choice, 2026-10-01): Pi's own send never touches the editor.
  for (const [name, editor] of [
    ['unsent text', 'my unfinished question'],
    ['a whitespace draft', ' '],
    ['no editor text at all', null],
  ]) {
    it(`delivers past ${name} and leaves the editor as it was`, async () => {
      const s = await setup(
        { ...envelopeRecord, text: envelope, session: 'native-pi-session' },
        { editor },
      )
      try {
        assert.deepEqual(s.pi.sent, [envelope])
        assert.equal(s.ctx.ui.getEditorText(), editor)
      } finally {
        await s.close()
      }
    })
  }

  for (const [name, options] of [
    ['a headless run', { hasUI: false }],
    ['an RPC run', { mode: 'rpc' }],
  ]) {
    it(`refuses ${name} at the native send boundary`, async () => {
      const s = await setup(
        { ...envelopeRecord, text: envelope, session: 'native-pi-session' },
        options,
      )
      try {
        assert.deepEqual(s.pi.sent, [])
        assert.equal(
          (await waitFor(join(s.ack, 'm-00000000000000000000000000000051.json'))).reason,
          'native TUI unavailable',
        )
      } finally {
        await s.close()
      }
    })
  }

  it('refuses a delivery addressed to a different native Pi session', async () => {
    const s = await setup({ ...envelopeRecord, text: envelope, session: 'previous-session' })
    try {
      assert.deepEqual(s.pi.sent, [])
      assert.equal(
        (await waitFor(join(s.ack, 'm-00000000000000000000000000000051.json'))).reason,
        'native session changed',
      )
    } finally {
      await s.close()
    }
  })

  it('delivers once busy work settles, leaving text typed meanwhile in the editor', async () => {
    const s = await setup(
      { ...envelopeRecord, text: envelope, session: 'native-pi-session' },
      { idle: false },
    )
    try {
      assert.deepEqual(s.pi.sent, [], 'nothing goes in while Pi works')
      s.setEditor('new draft')
      s.setIdle(true)
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      const deadline = Date.now() + 2_000
      while (s.pi.sent.length === 0 && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10))
      }
      assert.deepEqual(s.pi.sent, [envelope])
      assert.equal(s.ctx.ui.getEditorText(), 'new draft')
    } finally {
      await s.close()
    }
  })

  it('delivers an inbox arrival immediately when Pi is already idle', async () => {
    const s = await setup({ ...envelopeRecord, text: envelope })
    try {
      assert.deepEqual(s.pi.sent, [envelope])
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text: envelope }] } },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'm-00000000000000000000000000000051.json')), {
        id: 'm-00000000000000000000000000000051',
        admitted: true,
        mode: 'tui',
      })
      assert.equal(await exists(join(s.inbox, 'm-00000000000000000000000000000051.json')), false)
    } finally {
      await s.close()
    }
  })

  it('delivers an inbox arrival whose watch event was lost: the inbox is read anyway', async () => {
    // A watcher that never reports, as a macOS watch that lost the event under load.
    const s = await setup(null, {
      watchInbox: () => ({ close() {} }),
      pollMs: 50,
    })
    try {
      await writeFile(
        join(s.inbox, `${envelopeRecord.id}.json`),
        `${JSON.stringify({ ...envelopeRecord, expiresAt: Date.now() + 5000 })}\n`,
      )
      const deadline = Date.now() + 2000
      while (s.pi.sent.length === 0 && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 20))
      }
      assert.deepEqual(s.pi.sent, [envelope])
    } finally {
      await s.close()
    }
  })

  it('leaves a busy arrival queued and delivers it after agent_settled', {
    timeout: 2000,
  }, async () => {
    const s = await setup(
      {
        ...envelopeRecord,
        id: 'm-00000000000000000000000000000052',
        text: envelope.replaceAll(
          'm-00000000000000000000000000000051',
          'm-00000000000000000000000000000052',
        ),
      },
      { idle: false },
    )
    try {
      assert.deepEqual(s.pi.sent, [])
      s.setIdle(true)
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      // A filesystem notification can already be consuming the inbox; observe
      // the send itself instead of assuming this handler owns the drain.
      await s.pi.sentOnce
      assert.deepEqual(s.pi.sent, [
        envelope.replaceAll(
          'm-00000000000000000000000000000051',
          'm-00000000000000000000000000000052',
        ),
      ])
      await s.pi.handlers.get('message_start')(
        {
          message: {
            role: 'user',
            content: [
              {
                type: 'text',
                text: envelope.replaceAll(
                  'm-00000000000000000000000000000051',
                  'm-00000000000000000000000000000052',
                ),
              },
            ],
          },
        },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'm-00000000000000000000000000000052.json')), {
        id: 'm-00000000000000000000000000000052',
        admitted: true,
        mode: 'tui',
      })
    } finally {
      await s.close()
    }
  })

  it('quarantines an invalid id once without using it in any ack path', async () => {
    const s = await setup({
      id: '../escaped-ack',
      type: 'message',
      file: 'invalid',
      text: '[consensflow delivery invalid]\n',
    })
    try {
      await new Promise((resolve) => setTimeout(resolve, 25))
      assert.equal(await exists(join(s.inbox, 'invalid.json')), false)
      assert.equal(await exists(join(s.quarantine, 'invalid.json')), true)
      assert.equal(await exists(join(s.ack, '..', 'escaped-ack.json')), false)
      assert.equal(s.logs.filter((line) => line.includes('invalid message id')).length, 1)
      assert.deepEqual(JSON.parse(await readFile(join(s.quarantine, 'invalid.json'), 'utf8')), {
        id: '../escaped-ack',
        type: 'message',
        file: 'invalid',
        text: '[consensflow delivery invalid]\n',
      })
      await s.extension.consume()
      assert.equal(s.logs.filter((line) => line.includes('invalid message id')).length, 1)
    } finally {
      await s.close()
    }
  })

  it('acks unknown admission when no user message event arrives and then removes the inbox record', async () => {
    const s = await setup(
      {
        ...envelopeRecord,
        id: 'm-00000000000000000000000000000054',
        text: envelope.replaceAll(
          'm-00000000000000000000000000000051',
          'm-00000000000000000000000000000054',
        ),
      },
      // The admission window is also the record's expiry: 20 ms expired it on
      // a loaded machine before the inbox was even read, and it was refused.
      { ackTimeoutMs: 500 },
    )
    try {
      assert.deepEqual(
        await waitFor(join(s.ack, 'm-00000000000000000000000000000054.json'), 3000),
        {
          id: 'm-00000000000000000000000000000054',
          admitted: null,
          reason: 'admission-unknown',
        },
      )
      assert.equal(await exists(join(s.inbox, 'm-00000000000000000000000000000054.json')), false)
    } finally {
      await s.close()
    }
  })

  it('moves an expired inbox record aside and refuses it without sending', async () => {
    const s = await setup({
      ...envelopeRecord,
      id: 'm-00000000000000000000000000000058',
      text: envelope.replaceAll(
        'm-00000000000000000000000000000051',
        'm-00000000000000000000000000000058',
      ),
      expiresAt: Date.now() - 1,
    })
    try {
      assert.deepEqual(await waitFor(join(s.ack, 'm-00000000000000000000000000000058.json')), {
        id: 'm-00000000000000000000000000000058',
        admitted: false,
        bytesWritten: 0,
        reason: 'expired-before-send',
      })
      assert.equal(await exists(join(s.inbox, 'm-00000000000000000000000000000058.json')), false)
      assert.equal(await exists(join(s.expired, 'm-00000000000000000000000000000058.json')), true)
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('restores native settlement on an idle resumed Pi TUI without a new model turn', async () => {
    const leaf = {
      id: 'leaf-1',
      type: 'message',
      message: { role: 'assistant', stopReason: 'stop' },
    }
    const s = await setup(null, { leaf })
    try {
      const evidence = await waitFor(join(s.settled, 'launch-pi-test.json'))
      assert.equal(evidence.sessionId, 'native-pi-session')
      assert.deepEqual(evidence.frontier, { id: leaf.id })
      assert.equal(evidence.launchId, 'launch-pi-test')
      assert.equal(s.pi.sent.length, 0)
      await s.pi.handlers.get('agent_start')({}, s.ctx)
      assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), false)
    } finally {
      await s.close()
    }
  })

  for (const options of [
    { idle: false },
    { pendingMessages: true },
    { mode: 'rpc' },
    { hasUI: false },
    {
      leaf: {
        id: 'wrong-leaf',
        type: 'message',
        message: { role: 'assistant', stopReason: 'stop' },
      },
    },
    {
      leaf: { id: 'leaf-1', type: 'message', message: { role: 'assistant', stopReason: 'error' } },
    },
    { leaf: { id: 'leaf-1', type: 'message', message: { role: 'user' } } },
  ]) {
    it(`does not restore Pi startup settlement for ${JSON.stringify(options)}`, async () => {
      const leaf = {
        id: 'leaf-1',
        type: 'message',
        message: { role: 'assistant', stopReason: 'stop' },
      }
      const s = await setup(null, { leaf, ...options })
      try {
        assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), false)
      } finally {
        await s.close()
      }
    })
  }

  it('says which conversation the window shows, at start and after a /new or /resume', async () => {
    const s = await setup(null)
    try {
      assert.deepEqual(await waitFor(join(s.settled, 'launch-pi-test.shown.json')), {
        launchId: 'launch-pi-test',
        sessionId: 'native-pi-session',
      })
      // Pi opens the other conversation in a new runtime of the extension.
      await s.pi.handlers.get('session_shutdown')({ reason: 'new' })
      s.ctx.sessionManager.getSessionId = () => 'new-pi-session'
      await s.pi.handlers.get('session_start')({ reason: 'new' }, s.ctx)
      assert.deepEqual(
        JSON.parse(await readFile(join(s.settled, 'launch-pi-test.shown.json'), 'utf8')),
        { launchId: 'launch-pi-test', sessionId: 'new-pi-session' },
      )
    } finally {
      await s.close()
    }
  })

  it('writes settlement evidence tied to the launch, session and leaf, then invalidates it on new work', async () => {
    const s = await setup({
      ...envelopeRecord,
      id: 'm-00000000000000000000000000000059',
      text: envelope,
    })
    try {
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      const evidence = await waitFor(join(s.settled, 'launch-pi-test.json'))
      assert.equal(evidence.launchId, 'launch-pi-test')
      assert.equal(evidence.sessionId, 'native-pi-session')
      assert.deepEqual(evidence.frontier, { id: 'leaf-1' })
      assert.equal(typeof evidence.settledAt, 'number')

      await s.pi.handlers.get('turn_start')({}, s.ctx)
      assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), false)
      // A turn on, before Pi has saved anything: the watchdog's evidence.
      const working = await waitFor(join(s.settled, 'launch-pi-test.working.json'))
      assert.deepEqual(
        [working.launchId, working.sessionId, typeof working.startedAt],
        ['launch-pi-test', 'native-pi-session', 'number'],
      )

      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), true)
      assert.equal(await exists(join(s.settled, 'launch-pi-test.working.json')), false)
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text: 'new prompt' }] } },
        s.ctx,
      )
      assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), false)
    } finally {
      await s.close()
    }
  })
})

it('a Pi session switch at admission reports a retryable zero-byte refusal', {
  skip: cargoMissing,
}, async () => {
  const s = await setup(null)
  try {
    const result = await rustPi().send(
      {
        session: 'native-pi-session',
        pane: 'chief-pane',
        generation: 1,
        claim: async () => {
          s.ctx.sessionManager.getSessionId = () => 'new-pi-session'
          return { ok: true }
        },
        launch: {
          channel: {
            kind: 'pi-extension',
            inbox: s.inbox,
            ack: s.ack,
            launchId: 'launch-pi-test',
            // The refusal comes back at once; the window is for a loaded machine.
            ackTimeoutMs: 10_000,
          },
        },
      },
      envelopeRecord.text,
    )
    assert.equal(result.error, 'failed-with-zero-bytes')
    assert.equal(result.ack.reason, 'native session changed')
    assert.equal(result.admitted, false)
    assert.equal(result.bytesWritten, 0)
    assert.deepEqual(s.pi.sent, [])
  } finally {
    await s.close()
  }
})

// The size a project chose for its members to compact at (smaller-contexts, SCX-28 to
// SCX-30, T-1265): Pi has no flag or variable for it, so ConsensFlow's extension asks Pi
// to compact at the window's next rest once its context is past the size. `ctx.compact()`
// aborts a turn at work, so it is asked only at a rest: idle, nothing pending, the turn not
// aborted by the human. A size at or above the model's window leaves Pi's own point.
describe('compaction at a rest', () => {
  const window = 1_000_000
  const past = { tokens: 150_005, contextWindow: window, percent: 15 }

  const settle = (s, event = {}) => s.pi.handlers.get('agent_settled')(event, s.ctx)

  it('asks Pi once to compact at a rest once the context is past the size', async () => {
    const s = await setup(null, { compaction: 100_000, usage: past })
    try {
      await settle(s)
      assert.equal(s.compactions.length, 1)
      // A second rest while the first compaction is at work asks for none.
      await settle(s)
      assert.equal(s.compactions.length, 1)
    } finally {
      await s.close()
    }
  })

  for (const [name, options, event] of [
    ['a project that chose no size', { usage: past }, {}],
    ['a context at the size or under it', { compaction: 150_005, usage: past }, {}],
    [
      'a size at or above the model window',
      // Pi's estimate can run past the window before Pi's own overflow handling: the size is
      // passed, but a size at the window is no point of ours.
      { compaction: 262_144, usage: { tokens: 270_000, contextWindow: 262_144, percent: 103 } },
      {},
    ],
    [
      'a context Pi cannot say yet (just after a compaction)',
      { compaction: 100_000, usage: { tokens: null, contextWindow: window, percent: null } },
      {},
    ],
    ['no reading at all', { compaction: 100_000, usage: undefined }, {}],
    ['a turn the human aborted', { compaction: 100_000, usage: past }, { aborted: true }],
    ['a window not idle', { compaction: 100_000, usage: past, idle: false }, {}],
    ['messages pending', { compaction: 100_000, usage: past, pendingMessages: true }, {}],
  ]) {
    it(`asks for no compaction for ${name}`, async () => {
      const s = await setup(null, options)
      try {
        await settle(s, event)
        assert.equal(s.compactions.length, 0)
      } finally {
        await s.close()
      }
    })
  }

  const idOf = (n) => `m-${String(n).padStart(32, '0')}`
  const arrive = (s, n, more = {}) =>
    writeFile(
      join(s.inbox, `${idOf(n)}.json`),
      `${JSON.stringify({ ...envelopeRecord, id: idOf(n), expiresAt: Date.now() + 30_000, ...more })}\n`,
    )
  const ackOf = (s, n) => waitFor(join(s.ack, `${idOf(n)}.json`))
  const sentWithin = async (s, count, ms = 2_000) => {
    const deadline = Date.now() + ms
    while (s.pi.sent.length < count && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
  }
  // Who began the compaction: ConsensFlow's extension at a rest past the size, or Pi itself at
  // its own point (no size chosen).
  const sources = {
    ours: async () => {
      const s = await setup(null, { compaction: 100_000, usage: past })
      await settle(s)
      assert.equal(s.compactions.length, 1)
      return [s, s.compactions[0]]
    },
    "Pi's own": async () => {
      const s = await setup(null)
      return [s, s.piCompacts('threshold')]
    },
  }

  for (const [source, start] of Object.entries(sources)) {
    // ConsensFlow's own compaction always ends in its callback; only Pi may end one untold.
    const ends =
      source === 'ours'
        ? ['complete', 'fail', 'cancel']
        : ['complete', 'fail', 'cancel', 'endUntold']
    for (const end of ends) {
      it(`refuses every delivery as busy while ${source} compaction runs, and delivers exactly one after it ends (${end})`, async () => {
        const [s, compaction] = await start()
        try {
          // Pi is not idle for the whole compaction, as real Pi keeps it.
          assert.equal(s.ctx.isIdle(), false)
          for (const n of [1, 2, 3]) {
            await arrive(s, n)
            await s.extension.consume()
            // Pi's own word for a window at work: no attempt of the message is spent on it.
            assert.equal((await ackOf(s, n)).reason, 'chief busy', `attempt ${n}`)
          }
          assert.deepEqual(s.pi.sent, [], 'nothing goes in while Pi compacts')
          await compaction[end]()
          await arrive(s, 4)
          await s.extension.consume()
          await sentWithin(s, 1)
          assert.deepEqual(s.pi.sent, [envelope])
          // None of the refused ones comes back later.
          await new Promise((resolve) => setTimeout(resolve, 100))
          await s.extension.consume()
          assert.deepEqual(s.pi.sent, [envelope])
        } finally {
          await s.close()
        }
      })
    }
  }

  it('keeps the expiry and session checks while Pi compacts', async () => {
    const s = await setup(null)
    try {
      s.piCompacts('threshold')
      await arrive(s, 1, { session: 'previous-session' })
      await arrive(s, 2, { expiresAt: Date.now() - 1 })
      await s.extension.consume()
      assert.equal((await ackOf(s, 1)).reason, 'native session changed')
      assert.equal(
        (await waitFor(join(s.expired, '..', 'ack', `${idOf(2)}.json`))).reason,
        'expired-before-send',
      )
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('leaves a delivery waiting, unanswered, while Pi works a turn and compacts nothing', async () => {
    const s = await setup(null, { idle: false })
    try {
      await arrive(s, 1)
      await s.extension.consume()
      await assert.rejects(ackOf(s, 1), /timed out/)
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('asks again at a later rest after a compaction that failed', async () => {
    const s = await setup(null, { compaction: 100_000, usage: past })
    try {
      await settle(s)
      await s.compactions[0].fail('Nothing to compact (session too small)')
      await settle(s)
      assert.equal(s.compactions.length, 2)
      assert.ok(s.logs.some((line) => line.includes('Nothing to compact')))
    } finally {
      await s.close()
    }
  })

  it('reads the size its launch gave it from CF_COMPACTION_TOKENS, and nothing else as one', () => {
    assert.equal(compactionOf({ CF_COMPACTION_TOKENS: '400000' }), 400_000)
    for (const text of [undefined, '', '0', '-400000', '400k', '4e5', '400000.5', ' 400000']) {
      assert.equal(compactionOf({ CF_COMPACTION_TOKENS: text }), undefined, String(text))
    }
    assert.equal(compactionOf({ CF_COMPACTION: '400000' }), undefined, 'another name')
    assert.equal(compactionOf(undefined), undefined)
  })
})

// The same, through the real Rust channel (`pi-send`, the daemon's own handover): a Pi window
// that compacts, whoever began it, answers Busy to every handover however many lifetimes the
// compaction outlasts, which the adapter reads as Busy and the engine gives back the attempt
// for (`Admission::Busy`); once it ends, completed, failed or cancelled, the next handover goes
// in, once. Before, real Pi's `isIdle()` was false all along, the extension left each message
// unanswered, and each ended `expired-before-send`, a failed attempt (T-1272).
for (const source of ['ours', "Pi's own"]) {
  for (const end of ['complete', 'fail', 'cancel']) {
    it(`hands over nothing but Busy while ${source} compaction runs, and one message after it ends (${end}), through the real channel`, {
      skip: cargoMissing,
    }, async () => {
      const usage = { tokens: 150_005, contextWindow: 1_000_000, percent: 15 }
      const s =
        source === 'ours'
          ? await setup(null, { compaction: 100_000, usage, pollMs: 10 })
          : await setup(null, { pollMs: 10 })
      // Real Pi says it took a message as its turn begins (`message_start`).
      s.pi.sendUserMessage = (text) => {
        s.pi.sent.push(text)
        setTimeout(
          () =>
            s.pi.handlers.get('message_start')(
              { message: { role: 'user', content: [{ type: 'text', text }] } },
              s.ctx,
            ),
          5,
        )
      }
      try {
        let compaction
        if (source === 'ours') {
          await s.pi.handlers.get('agent_settled')({}, s.ctx)
          compaction = s.compactions[0]
        } else {
          compaction = s.piCompacts('threshold')
        }
        const channel = rustPi()
        const handover = (ackTimeoutMs) =>
          channel.send(
            {
              session: 'native-pi-session',
              pane: 'member-pane',
              generation: 1,
              claim: async () => ({ ok: true }),
              launch: {
                channel: {
                  kind: 'pi-extension',
                  inbox: s.inbox,
                  ack: s.ack,
                  launchId: 'launch-pi-test',
                  ackTimeoutMs,
                },
              },
            },
            envelopeRecord.text,
          )
        // Three handovers with a short life, each after the one before has expired: the
        // compaction outlasts them all.
        for (const attempt of [1, 2, 3]) {
          const result = await handover(150)
          assert.equal(
            result.ack?.reason,
            'chief busy',
            `attempt ${attempt}: ${JSON.stringify(result)}`,
          )
          assert.equal(result.admitted, false, `attempt ${attempt}`)
          assert.equal(result.bytesWritten, 0, `attempt ${attempt}`)
          await new Promise((resolve) => setTimeout(resolve, 200))
        }
        assert.deepEqual(s.pi.sent, [], 'nothing went in while Pi compacted')
        await compaction[end]()
        const delivered = await handover(10_000)
        assert.equal(delivered.admitted, true, JSON.stringify(delivered))
        await new Promise((resolve) => setTimeout(resolve, 200))
        assert.deepEqual(s.pi.sent, [envelopeRecord.text], 'exactly one delivery after it')
      } finally {
        await s.close()
      }
    })
  }
}
