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
  const ctx = context(() => currentIdle)
  ctx.hasUI = hasUI
  ctx.mode = mode
  ctx.hasPendingMessages = () => pendingMessages
  ctx.sessionManager.getLeafEntry = () => leaf
  ctx.ui = { getEditorText: () => editor }
  // Pi's own context reading and compaction (ExtensionContext, Pi 1.4.0 and 1.5.0): the
  // reading the test gives, and each compaction asked, which the test ends itself.
  let currentUsage = usage
  const compactions = []
  ctx.getContextUsage = () => currentUsage
  ctx.compact = (options) => compactions.push(options)
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

  it('refuses a delivery as busy while its compaction runs, and delivers it once it is over', async () => {
    const s = await setup(null, { compaction: 100_000, usage: past })
    try {
      await settle(s)
      assert.equal(s.compactions.length, 1)
      await writeFile(
        join(s.inbox, `${envelopeRecord.id}.json`),
        `${JSON.stringify({ ...envelopeRecord, expiresAt: Date.now() + 30_000 })}\n`,
      )
      await s.extension.consume()
      assert.deepEqual(s.pi.sent, [], 'nothing goes in while Pi compacts')
      // Pi's own word for a window at work: no attempt of the message is spent on it.
      assert.equal((await waitFor(join(s.ack, `${envelopeRecord.id}.json`))).reason, 'chief busy')
      s.compactions[0].onComplete({ tokensBefore: 150_005, summary: 'the summary' })
      await rm(join(s.ack, `${envelopeRecord.id}.json`))
      await writeFile(
        join(s.inbox, 'm-00000000000000000000000000000052.json'),
        `${JSON.stringify({ ...envelopeRecord, id: 'm-00000000000000000000000000000052', expiresAt: Date.now() + 30_000 })}\n`,
      )
      await s.extension.consume()
      assert.deepEqual(s.pi.sent, [envelope])
    } finally {
      await s.close()
    }
  })

  it('asks again at a later rest after a compaction that failed', async () => {
    const s = await setup(null, { compaction: 100_000, usage: past })
    try {
      await settle(s)
      s.compactions[0].onError(new Error('Nothing to compact (session too small)'))
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
