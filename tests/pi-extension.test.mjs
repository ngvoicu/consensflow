import assert from 'node:assert/strict'
import { access, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { currentSession, probeEditor } from '../src/channels/pi.js'
import { send } from '../src/channels.js'

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
    editorGuard,
    editor = '',
    hasUI = true,
    mode = 'tui',
    leaf,
    pendingMessages = false,
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
    editorGuard,
    ackTimeoutMs,
    logger: { error: (...args) => logs.push(args.join(' ')) },
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
  it('probes the live Pi conversation after native new-session replacement without sending', async () => {
    const s = await setup(null, { editorGuard: 1 })
    const config = {
      kind: 'pi-extension',
      editorGuard: 1,
      inbox: s.inbox,
      ack: s.ack,
      launchId: 'launch-pi-test',
    }
    try {
      assert.equal(await currentSession(config), 'native-pi-session')
      await s.pi.handlers.get('session_shutdown')({ reason: 'new' })
      const next = { ...s.ctx, ...context(() => false, { sessionId: 'new-native-session' }) }
      await s.pi.handlers.get('session_start')({ reason: 'new' }, next)
      assert.equal(await currentSession(config), 'new-native-session', 'identity works while busy')
      next.mode = 'rpc'
      assert.equal(await currentSession(config), null, 'only the native TUI is an app lead')
      next.mode = 'tui'
      assert.equal(await currentSession({ ...config, launchId: 'wrong-launch' }), null)
      await s.pi.handlers.get('session_shutdown')({ reason: 'quit' })
      assert.equal(
        await currentSession(config),
        null,
        'a stopped extension leaves no reusable identity',
      )
      assert.deepEqual(s.pi.sent, [])
      assert.deepEqual(await readdir(s.inbox), [])
      assert.deepEqual(await readdir(s.ack), [])
    } finally {
      await s.close()
    }
  })

  for (const corrupt of ['id', 'launchId', 'expiresAt', 'sessionId']) {
    it(`rejects a Pi identity response with invalid ${corrupt}`, async () => {
      const s = await setup(null, { editorGuard: 1 })
      await s.pi.handlers.get('session_shutdown')({ reason: 'quit' })
      const config = {
        kind: 'pi-extension',
        editorGuard: 1,
        inbox: s.inbox,
        ack: s.ack,
        launchId: 'launch-pi-test',
      }
      let responder
      try {
        responder = setInterval(async () => {
          for (const file of await readdir(s.inbox)) {
            if (!file.endsWith('.json')) continue
            const request = JSON.parse(await readFile(join(s.inbox, file), 'utf8'))
            const response = { ...request, sessionId: 'forged-session' }
            response[corrupt] = corrupt === 'expiresAt' ? 0 : ''
            await mkdir(s.ack, { recursive: true })
            await writeFile(join(s.ack, file), JSON.stringify(response))
            clearInterval(responder)
          }
        }, 10)
        assert.equal(await currentSession(config), null)
      } finally {
        clearInterval(responder)
        await s.close()
      }
    })
  }

  it('answers fresh native editor probes without persisting any editor text', async () => {
    const s = await setup(null, { editorGuard: 1 })
    const config = {
      kind: 'pi-extension',
      editorGuard: 1,
      inbox: s.inbox,
      ack: s.ack,
      launchId: 'launch-pi-test',
    }
    try {
      assert.deepEqual(await probeEditor(config, 'native-pi-session'), { ready: true })
      s.setEditor('private unfinished text')
      assert.deepEqual(await probeEditor(config, 'native-pi-session'), {
        ready: false,
        reason: 'draft open',
      })
      assert.deepEqual(await probeEditor(config, 'old-session'), {
        ready: false,
        reason: 'native session changed',
      })
      s.setEditor('')
      s.ctx.mode = 'rpc'
      assert.deepEqual(await probeEditor(config, 'native-pi-session'), {
        ready: false,
        reason: 'native editor unavailable',
      })
      s.ctx.mode = 'tui'
      delete s.ctx.ui.getEditorText
      assert.deepEqual(await probeEditor(config, 'native-pi-session'), {
        ready: false,
        reason: 'native editor unavailable',
      })
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('handles a probe while a busy delivery stays queued unchanged', async () => {
    const s = await setup({ ...envelopeRecord, text: envelope }, { editorGuard: 1, idle: false })
    try {
      const file = join(s.inbox, 'm-00000000000000000000000000000051.json')
      const before = await readFile(file, 'utf8')
      assert.deepEqual(
        await probeEditor(
          {
            kind: 'pi-extension',
            editorGuard: 1,
            inbox: s.inbox,
            ack: s.ack,
            launchId: 'launch-pi-test',
          },
          'native-pi-session',
        ),
        { ready: false, reason: 'lead busy' },
      )
      assert.equal(await readFile(file, 'utf8'), before)
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  for (const [name, options, reason] of [
    ['unsent text', { editor: 'my unfinished question' }, 'draft open'],
    ['whitespace draft', { editor: ' ' }, 'draft open'],
    ['headless empty-editor fallback', { hasUI: false }, 'native editor unavailable'],
    ['RPC empty-editor fallback', { mode: 'rpc' }, 'native editor unavailable'],
    ['missing editor text', { editor: null }, 'native editor unavailable'],
  ]) {
    it(`refuses ${name} at the native send boundary without touching the editor`, async () => {
      const s = await setup(
        { ...envelopeRecord, text: envelope, session: 'native-pi-session' },
        { editorGuard: 1, ...options },
      )
      try {
        assert.deepEqual(s.pi.sent, [])
        assert.equal(
          (await waitFor(join(s.ack, 'm-00000000000000000000000000000051.json'))).reason,
          reason,
        )
        assert.equal(
          s.ctx.ui.getEditorText(),
          Object.hasOwn(options, 'editor') ? options.editor : '',
        )
      } finally {
        await s.close()
      }
    })
  }

  it('refuses a guarded delivery addressed to a different native Pi session', async () => {
    const s = await setup(
      { ...envelopeRecord, text: envelope, session: 'previous-session' },
      { editorGuard: 1 },
    )
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

  it('rechecks the native editor after busy work settles', async () => {
    const s = await setup(
      { ...envelopeRecord, text: envelope, session: 'native-pi-session' },
      { editorGuard: 1, idle: false },
    )
    try {
      s.setEditor('new draft')
      s.setIdle(true)
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      assert.equal(
        (await waitFor(join(s.ack, 'm-00000000000000000000000000000051.json'))).admitted,
        false,
      )
      assert.deepEqual(s.pi.sent, [])
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
      { ackTimeoutMs: 20 },
    )
    try {
      assert.deepEqual(await waitFor(join(s.ack, 'm-00000000000000000000000000000054.json')), {
        id: 'm-00000000000000000000000000000054',
        admitted: null,
        reason: 'admission-unknown',
      })
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

      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      assert.equal(await exists(join(s.settled, 'launch-pi-test.json')), true)
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
  const s = await setup(null, { editorGuard: 1 })
  try {
    const result = await send(
      'pi-extension',
      {
        session: 'native-pi-session',
        pane: 'lead-pane',
        generation: 1,
        epoch: 0,
        claimEpoch: async () => {
          s.ctx.sessionManager.getSessionId = () => 'new-pi-session'
          return { ok: true }
        },
        launch: {
          channel: {
            kind: 'pi-extension',
            editorGuard: 1,
            inbox: s.inbox,
            ack: s.ack,
            launchId: 'launch-pi-test',
            ackTimeoutMs: 1000,
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
