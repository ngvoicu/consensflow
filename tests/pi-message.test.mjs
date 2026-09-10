import assert from 'node:assert/strict'
import { access, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { send } from '../src/channels/pi.js'

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

async function waitFor(path, timeoutMs = 500) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await exists(path)) return JSON.parse(await readFile(path, 'utf8'))
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error(`timed out waiting for ${path}`)
}

async function waitForInboxFile(inbox, timeoutMs = 500) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const files = (await readdir(inbox)).filter((name) => name.endsWith('.json'))
    if (files.length > 0) return join(inbox, files.sort()[0])
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error(`timed out waiting for an inbox record in ${inbox}`)
}

async function setup(
  { idle = true, ackTimeoutMs = 1000, editor = '', hasUI = true, mode = 'tui' } = {},
  { launchId = 'launch-pi-test' } = {},
) {
  const root = await mkdtemp(join(tmpdir(), 'consensflow-pi-message-'))
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
    launchId,
    editorGuard: 1,
    logger: { error: (...args) => logs.push(args.join(' ')) },
  })
  let currentIdle = idle
  const ctx = context(() => currentIdle)
  ctx.hasUI = hasUI
  ctx.mode = mode
  ctx.hasPendingMessages = () => false
  ctx.ui = { getEditorText: () => editor }
  await pi.handlers.get('session_start')({}, ctx)
  const targetFor = (overrides = {}) => ({
    session: 'native-pi-session',
    pane: 'lead-pane',
    generation: 1,
    epoch: 0,
    claimEpoch: async () => ({ ok: true }),
    launch: {
      channel: {
        kind: 'pi-extension',
        editorGuard: 1,
        launchId,
        inbox,
        ack,
        ackTimeoutMs,
      },
    },
    ...overrides,
  })
  return {
    root,
    inbox,
    ack,
    quarantine,
    settled,
    expired,
    launchId,
    ackTimeoutMs,
    pi,
    ctx,
    logs,
    extension,
    targetFor,
    setIdle(value) {
      currentIdle = value
    },
    setEditor(value) {
      editor = value
    },
    close: async () => {
      await pi.handlers.get('session_shutdown')()
      await rm(root, { recursive: true, force: true })
    },
  }
}

describe('consensflow Pi worker followup', () => {
  it('sends raw worker text verbatim with a bounded m-<hex> record', async () => {
    const s = await setup()
    const pump = setInterval(() => void s.extension.consume(), 5)
    pump.unref()
    try {
      const text = 'worker followup: the cache key is per conversation'
      const pending = send(s.targetFor(), text)
      const inboxFile = await waitForInboxFile(s.inbox)
      const record = JSON.parse(await readFile(inboxFile, 'utf8'))
      assert.match(record.id, /^m-[a-f0-9]{32}$/)
      assert.deepEqual(Object.keys(record).sort(), [
        'expiresAt',
        'id',
        'launchId',
        'session',
        'text',
        'type',
      ])
      assert.equal(record.type, 'message')
      assert.equal(record.launchId, 'launch-pi-test')
      assert.equal(record.session, 'native-pi-session')
      assert.equal(record.text, text)
      assert.ok(Number.isFinite(record.expiresAt) && record.expiresAt > Date.now())
      assert.ok(!('answer' in record) && !('envelope' in record))
      assert.ok(!record.text.includes('[consensflow delivery'))
      const sendDeadline = Date.now() + 500
      while (s.pi.sent.length === 0 && Date.now() < sendDeadline) {
        await new Promise((resolve) => setTimeout(resolve, 5))
      }
      assert.deepEqual(s.pi.sent, [text])
      await s.pi.handlers.get('message_start')(
        { message: { role: 'user', content: [{ type: 'text', text }] } },
        s.ctx,
      )
      const result = await pending
      assert.equal(result.ok, true)
      assert.equal(result.admitted, true)
      assert.match(result.ack.id, /^m-[a-f0-9]{32}$/)
      assert.equal(await exists(inboxFile), false)
    } finally {
      clearInterval(pump)
      await s.close()
    }
  })

  it('refuses a followup while unsent draft text exists without touching the draft', async () => {
    const s = await setup({ editor: 'my unfinished question' })
    const pump = setInterval(() => void s.extension.consume(), 5)
    pump.unref()
    try {
      const text = 'worker followup while drafting'
      const result = await send(s.targetFor(), text)
      assert.deepEqual(s.pi.sent, [])
      assert.equal(result.ok, false)
      assert.equal(result.admitted, false)
      assert.equal(result.error, 'failed-with-zero-bytes')
      assert.equal(result.bytesWritten, 0)
      assert.equal(result.ack.reason, 'draft open')
      assert.equal(s.ctx.ui.getEditorText(), 'my unfinished question')
    } finally {
      clearInterval(pump)
      await s.close()
    }
  })

  it('refuses a followup addressed to the wrong native session', async () => {
    const s = await setup()
    const pump = setInterval(() => void s.extension.consume(), 5)
    pump.unref()
    try {
      const result = await send(
        s.targetFor({ session: 'previous-session' }),
        'followup for another session',
      )
      assert.deepEqual(s.pi.sent, [])
      assert.equal(result.admitted, false)
      assert.equal(result.ack.reason, 'native session changed')
    } finally {
      clearInterval(pump)
      await s.close()
    }
  })

  it('refuses a followup from the wrong launch without sending', async () => {
    const s = await setup()
    const pump = setInterval(() => void s.extension.consume(), 5)
    pump.unref()
    try {
      const foreign = s.targetFor()
      foreign.launch.channel.launchId = 'other-launch'
      const result = await send(foreign, 'followup from another launch')
      assert.deepEqual(s.pi.sent, [])
      assert.equal(result.admitted, false)
      assert.equal(result.ack.reason, 'wrong-launch')
    } finally {
      clearInterval(pump)
      await s.close()
    }
  })

  it('moves an expired followup aside without sending', async () => {
    const s = await setup()
    try {
      const id = `m-${'a'.repeat(32)}`
      await writeFile(
        join(s.inbox, `${id}.json`),
        `${JSON.stringify({
          id,
          type: 'message',
          launchId: 'launch-pi-test',
          session: 'native-pi-session',
          text: 'stale followup',
          expiresAt: Date.now() - 1,
        })}\n`,
      )
      await s.extension.consume()
      assert.deepEqual(await waitFor(join(s.ack, `${id}.json`)), {
        id,
        admitted: false,
        bytesWritten: 0,
        reason: 'expired-before-send',
      })
      assert.equal(await exists(join(s.inbox, `${id}.json`)), false)
      assert.equal(await exists(join(s.expired, `${id}.json`)), true)
      assert.deepEqual(s.pi.sent, [])
    } finally {
      await s.close()
    }
  })

  it('delivers a duplicate message id exactly once', async () => {
    const s = await setup()
    try {
      const id = `m-${'b'.repeat(32)}`
      const record = {
        id,
        type: 'message',
        launchId: 'launch-pi-test',
        session: 'native-pi-session',
        text: 'duplicate followup',
        expiresAt: Date.now() + 1000,
      }
      await writeFile(join(s.inbox, `${id}.json`), `${JSON.stringify(record)}\n`)
      await Promise.all([s.extension.consume(), s.extension.consume()])
      assert.deepEqual(s.pi.sent, ['duplicate followup'])
      await s.pi.handlers.get('message_start')(
        {
          message: {
            role: 'user',
            content: [{ type: 'text', text: 'duplicate followup' }],
          },
        },
        s.ctx,
      )
      assert.deepEqual(await waitFor(join(s.ack, `${id}.json`)), {
        id,
        admitted: true,
        mode: 'tui',
      })
      assert.equal(s.pi.sent.length, 1)
    } finally {
      await s.close()
    }
  })

  it('reports uncertain on ack timeout without an automatic retry', async () => {
    const s = await setup({ ackTimeoutMs: 40 })
    const pump = setInterval(() => void s.extension.consume(), 5)
    pump.unref()
    try {
      const result = await send(s.targetFor(), 'followup nobody admits')
      assert.equal(result.ok, false)
      assert.equal(result.admitted, null)
      assert.equal(result.error, 'uncertain')
      assert.equal(result.cause, 'admission-unknown')
      assert.equal(result.ack?.admitted, null)
      assert.match(result.ack?.id ?? '', /^m-[a-f0-9]{32}$/)
      assert.deepEqual(s.pi.sent, ['followup nobody admits'])
      assert.equal((await readdir(s.inbox)).length, 0)
    } finally {
      clearInterval(pump)
      await s.close()
    }
  })
})
