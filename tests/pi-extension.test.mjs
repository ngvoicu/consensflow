import assert from 'node:assert/strict'
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { envelope as makeEnvelope, pointer } from '../hosts/lib/deliveries.js'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { deliver, enabledChannels, launchConfiguration } from '../src/channels.js'

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

async function setup(record, { idle = true, ackTimeoutMs = 1000 } = {}) {
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
  })
  let currentIdle = idle
  const ctx = context(() => currentIdle)
  const payload =
    typeof record.id === 'string' && /^d-\d+$/.test(record.id) && record.expiresAt === undefined
      ? { ...record, expiresAt: Date.now() + ackTimeoutMs }
      : record
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
    ctx,
  }
}

const envelopeRecord = {
  id: 'd-51',
  answerId: 'answer-51',
  conversation: 'worker',
  agent: 'pi',
  answer: 'body',
  channel: 'pi-extension',
}
const envelope = makeEnvelope(envelopeRecord)

describe('consensflow Pi extension', () => {
  it('delivers an inbox arrival immediately when Pi is already idle', async () => {
    const s = await setup({ ...envelopeRecord, text: envelope })
    try {
      assert.deepEqual(s.pi.sent, [envelope])
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text: envelope }] } },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'd-51.json')), {
        id: 'd-51',
        admitted: true,
        mode: 'tui',
      })
      assert.equal(await exists(join(s.inbox, 'd-51.json')), false)
    } finally {
      await s.close()
    }
  })

  it('leaves a busy arrival queued and delivers it after agent_settled', {
    timeout: 2000,
  }, async () => {
    const s = await setup(
      { ...envelopeRecord, id: 'd-52', text: envelope.replaceAll('d-51', 'd-52') },
      { idle: false },
    )
    try {
      assert.deepEqual(s.pi.sent, [])
      s.setIdle(true)
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
      // A filesystem notification can already be consuming the inbox; observe
      // the send itself instead of assuming this handler owns the drain.
      await s.pi.sentOnce
      assert.deepEqual(s.pi.sent, [envelope.replaceAll('d-51', 'd-52')])
      await s.pi.handlers.get('message_start')(
        {
          message: {
            role: 'user',
            content: [{ type: 'text', text: envelope.replaceAll('d-51', 'd-52') }],
          },
        },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'd-52.json')), {
        id: 'd-52',
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
      file: 'invalid',
      text: '[consensflow delivery invalid]\n',
    })
    try {
      await new Promise((resolve) => setTimeout(resolve, 25))
      assert.equal(await exists(join(s.inbox, 'invalid.json')), false)
      assert.equal(await exists(join(s.quarantine, 'invalid.json')), true)
      assert.equal(await exists(join(s.ack, '..', 'escaped-ack.json')), false)
      assert.equal(s.logs.filter((line) => line.includes('invalid delivery id')).length, 1)
      assert.deepEqual(JSON.parse(await readFile(join(s.quarantine, 'invalid.json'), 'utf8')), {
        id: '../escaped-ack',
        file: 'invalid',
        text: '[consensflow delivery invalid]\n',
      })
      await s.extension.consume()
      assert.equal(s.logs.filter((line) => line.includes('invalid delivery id')).length, 1)
    } finally {
      await s.close()
    }
  })

  it('refuses a bare answer without an envelope before send and acknowledges the refusal', async () => {
    const s = await setup({ id: 'd-53', answer: 'bare answer' })
    try {
      assert.deepEqual(s.pi.sent, [])
      assert.deepEqual(await waitFor(join(s.ack, 'd-53.json')), {
        id: 'd-53',
        admitted: false,
        reason: 'missing-envelope',
      })
      assert.equal(await exists(join(s.inbox, 'd-53.json')), false)
      assert.match(s.logs.join('\n'), /missing envelope/)
    } finally {
      await s.close()
    }
  })

  it('acks unknown admission when no user message event arrives and then removes the inbox record', async () => {
    const s = await setup(
      { ...envelopeRecord, id: 'd-54', text: envelope.replaceAll('d-51', 'd-54') },
      { ackTimeoutMs: 20 },
    )
    try {
      assert.deepEqual(await waitFor(join(s.ack, 'd-54.json')), {
        id: 'd-54',
        admitted: null,
        reason: 'admission-unknown',
      })
      assert.equal(await exists(join(s.inbox, 'd-54.json')), false)
    } finally {
      await s.close()
    }
  })

  it('moves an expired inbox record aside and refuses it without sending', async () => {
    const s = await setup({
      ...envelopeRecord,
      id: 'd-58',
      text: envelope.replaceAll('d-51', 'd-58'),
      expiresAt: Date.now() - 1,
    })
    try {
      assert.deepEqual(await waitFor(join(s.ack, 'd-58.json')), {
        id: 'd-58',
        admitted: false,
        reason: 'expired-before-send',
      })
      assert.equal(await exists(join(s.inbox, 'd-58.json')), false)
      assert.equal(await exists(join(s.expired, 'd-58.json')), true)
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('writes settlement evidence tied to the launch, session and leaf, then invalidates it on new work', async () => {
    const s = await setup({ ...envelopeRecord, id: 'd-59', text: envelope })
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

  it('accepts a pointer rebuilt from the record even when no answer body is present', async () => {
    const delivery = {
      id: 'd-60',
      answerId: 'answer-60',
      conversation: 'worker',
      agent: 'pi',
      channel: 'cf-read',
      expiresAt: Date.now() + 100,
    }
    const text = pointer(delivery)
    const s = await setup({ ...delivery, text })
    try {
      assert.deepEqual(s.pi.sent, [text])
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text }] } },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'd-60.json')), {
        id: 'd-60',
        admitted: true,
        mode: 'tui',
      })
    } finally {
      await s.close()
    }
  })

  it('delivers a canonical cf-read pointer when Pi is already idle', async () => {
    const delivery = {
      id: 'd-56',
      answerId: 'answer-56',
      conversation: 'worker',
      agent: 'pi',
      answer: 'body',
      channel: 'cf-read',
    }
    const text = pointer(delivery)
    const s = await setup({ ...delivery, text })
    try {
      assert.deepEqual(s.pi.sent, [text])
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text }] } },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, 'd-56.json')), {
        id: 'd-56',
        admitted: true,
        mode: 'tui',
      })
      assert.equal(await exists(join(s.inbox, 'd-56.json')), false)
    } finally {
      await s.close()
    }
  })

  it('rejects a noncanonical pointer instead of sending it to Pi', async () => {
    const delivery = {
      id: 'd-57',
      answerId: 'answer-57',
      conversation: 'worker',
      agent: 'pi',
      answer: 'body',
      channel: 'cf-read',
    }
    const s = await setup({ ...delivery, text: `${pointer(delivery)}\n` })
    try {
      assert.deepEqual(s.pi.sent, [])
      assert.deepEqual(await waitFor(join(s.ack, 'd-57.json')), {
        id: 'd-57',
        admitted: false,
        reason: 'missing-envelope',
      })
      assert.equal(await exists(join(s.inbox, 'd-57.json')), false)
      assert.match(s.logs.join('\n'), /missing envelope or pointer/)
    } finally {
      await s.close()
    }
  })

  it('reads the production Pi timeout split and receives its unknown ack at the shared expiry', async () => {
    const root = await mkdtemp(join(tmpdir(), 'consensflow-pi-production-shape-'))
    const configuration = await launchConfiguration('pi', {
      launchId: 'launch-pi-negative-1',
      workspace: root,
    })
    assert.ok(Number.isFinite(configuration.channel.ackTimeoutMs))
    assert.equal(configuration.channel.ackTimeoutMs, 30_000)
    assert.ok(configuration.channel.extensionAckTimeoutMs < configuration.channel.ackTimeoutMs)
    assert.equal(configuration.channel.extensionAckTimeoutMs, 24_000)
    assert.equal(
      configuration.env.CF_DELIVERY_ACK_TIMEOUT_MS,
      String(configuration.channel.ackTimeoutMs),
    )
    assert.equal(
      configuration.env.CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS,
      String(configuration.channel.extensionAckTimeoutMs),
    )
    assert.equal(configuration.env.CF_DELIVERY_SETTLED, configuration.channel.settled)
    assert.equal(configuration.env.CF_DELIVERY_EXPIRED, configuration.channel.expired)
    assert.equal(configuration.env.CF_DELIVERY_LAUNCH_ID, configuration.channel.launchId)
    const pi = fakePi()
    const extension = createDeliveryExtension(pi, {
      inbox: configuration.env.CF_DELIVERY_INBOX,
      ack: configuration.env.CF_DELIVERY_ACK,
      quarantine: configuration.env.CF_DELIVERY_QUARANTINE,
    })
    const contextValue = context(() => true)
    await pi.handlers.get('session_start')({}, contextValue)
    const pump = setInterval(() => void extension.consume(), 5)
    pump.unref()
    const started = Date.now()
    try {
      const answer = await deliver(
        'pi-extension',
        {
          ...context(true),
          enabledChannels: enabledChannels('pi'),
          pane: 'lead-pane',
          generation: 1,
          epoch: 0,
          claimEpoch: async () => ({ ok: true }),
          launch: {
            ...configuration,
            channel: { ...configuration.channel, ackTimeoutMs: 40 },
          },
        },
        {
          id: 'd-55',
          answerId: 'answer-55',
          conversation: 'worker',
          agent: 'pi',
          answer: 'body',
          expiresAt: Date.now() + 40,
        },
      )
      assert.deepEqual(answer, {
        ok: false,
        admitted: null,
        error: 'uncertain',
        cause: 'admission-unknown',
        ack: { id: 'd-55', admitted: null, reason: 'admission-unknown' },
      })
      assert.ok(Date.now() - started < 200)
    } finally {
      clearInterval(pump)
      await pi.handlers.get('session_shutdown')()
      await rm(root, { recursive: true, force: true })
    }
  })
})
