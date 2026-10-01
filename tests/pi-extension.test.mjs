import assert from 'node:assert/strict'
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { send } from '../src/channels/pi.js'

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
    ackTimeoutMs = 1000,
    editor = '',
    hasUI = true,
    mode = 'tui',
    leaf,
    pendingMessages = false,
    watchInbox,
    pollMs,
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
    ackTimeoutMs,
    logger: { error: (...args) => logs.push(args.join(' ')) },
    ...(watchInbox ? { watchInbox } : {}),
    ...(pollMs ? { pollMs } : {}),
  })
  let currentIdle = idle
  const ctx = context(() => currentIdle)
  ctx.hasUI = hasUI
  ctx.mode = mode
  ctx.hasPendingMessages = () => pendingMessages
  ctx.sessionManager.getLeafEntry = () => leaf
  ctx.ui = { getEditorText: () => editor }
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

it('a Pi session switch at admission reports a retryable zero-byte refusal', async () => {
  const s = await setup(null)
  try {
    const result = await send(
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
