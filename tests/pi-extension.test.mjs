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
  return {
    handlers,
    sent,
    on(event, handler) {
      handlers.set(event, handler)
    },
    sendUserMessage(content) {
      sent.push(content)
    },
  }
}

function context(isIdle) {
  return {
    isIdle,
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

async function setup(record, { idle = true, ackTimeoutMs = 50 } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'consensflow-pi-extension-'))
  const inbox = join(root, 'inbox')
  const ack = join(root, 'ack')
  const quarantine = join(root, 'quarantine')
  await Promise.all([mkdir(inbox), mkdir(ack), mkdir(quarantine)])
  const pi = fakePi()
  const logs = []
  const extension = createDeliveryExtension(pi, {
    inbox,
    ack,
    quarantine,
    ackTimeoutMs,
    logger: { error: (...args) => logs.push(args.join(' ')) },
  })
  let currentIdle = idle
  const ctx = context(() => currentIdle)
  await writeFile(join(inbox, `${record.file ?? record.id}.json`), `${JSON.stringify(record)}\n`)
  await pi.handlers.get('session_start')({}, ctx)
  return {
    ack,
    quarantine,
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

  it('leaves a busy arrival queued and delivers it after agent_settled', async () => {
    const s = await setup(
      { ...envelopeRecord, id: 'd-52', text: envelope.replaceAll('d-51', 'd-52') },
      { idle: false },
    )
    try {
      assert.deepEqual(s.pi.sent, [])
      s.setIdle(true)
      await s.pi.handlers.get('agent_settled')({}, s.ctx)
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

  it('refuses a bare answer without an envelope and leaves it in the inbox', async () => {
    const s = await setup({ id: 'd-53', answer: 'bare answer' })
    try {
      await new Promise((resolve) => setTimeout(resolve, 25))
      assert.deepEqual(s.pi.sent, [])
      assert.equal(await exists(join(s.inbox, 'd-53.json')), true)
      assert.match(s.logs.join('\n'), /missing envelope/)
    } finally {
      await s.close()
    }
  })

  it('acks non-admission when no user message event arrives and then removes the inbox record', async () => {
    const s = await setup(
      { ...envelopeRecord, id: 'd-54', text: envelope.replaceAll('d-51', 'd-54') },
      { ackTimeoutMs: 20 },
    )
    try {
      assert.deepEqual(await waitFor(join(s.ack, 'd-54.json')), {
        id: 'd-54',
        admitted: false,
        reason: 'user-message-not-observed',
      })
      assert.equal(await exists(join(s.inbox, 'd-54.json')), false)
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
      await new Promise((resolve) => setTimeout(resolve, 25))
      assert.deepEqual(s.pi.sent, [])
      assert.equal(await exists(join(s.inbox, 'd-57.json')), true)
      assert.match(s.logs.join('\n'), /missing envelope or pointer/)
    } finally {
      await s.close()
    }
  })

  it('reads the production Pi timeout split and receives its negative ack before adapter expiry', async () => {
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
    const pi = fakePi()
    const extension = createDeliveryExtension(pi, {
      inbox: configuration.env.CF_DELIVERY_INBOX,
      ack: configuration.env.CF_DELIVERY_ACK,
      quarantine: configuration.env.CF_DELIVERY_QUARANTINE,
      ackTimeoutMs: Number(configuration.env.CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS),
    })
    const contextValue = context(() => true)
    await pi.handlers.get('session_start')({}, contextValue)
    const pump = setInterval(() => void extension.consume(), 5)
    pump.unref()
    const started = Date.now()
    try {
      const answer = await deliver(
        'pi-extension',
        { enabledChannels: enabledChannels('pi'), launch: configuration },
        { id: 'd-55', answerId: 'answer-55', conversation: 'worker', agent: 'pi', answer: 'body' },
      )
      assert.deepEqual(answer, {
        ok: true,
        admitted: false,
        ack: { id: 'd-55', admitted: false, reason: 'user-message-not-observed' },
      })
      assert.ok(Date.now() - started < configuration.channel.ackTimeoutMs)
    } finally {
      clearInterval(pump)
      await pi.handlers.get('session_shutdown')()
      await rm(root, { recursive: true, force: true })
    }
  })
})
