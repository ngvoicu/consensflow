import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { access, mkdir, mkdtemp, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { before, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createDeliveryExtension } from '../hosts/pi-extension/consensflow-delivery.mjs'
import { send as sendWithJavaScript } from '../src/channels/pi.js'

/**
 * Pi's channel in Rust (`crates/cf-harness`): the binary `pi-send`, built
 * once, which sends one message on the machine's own clock and randomness.
 * The cases below run against it as they run against JavaScript's `send`.
 */
function buildSendBinary() {
  const built = execFileSync(
    'cargo',
    [
      'build',
      '-p',
      'cf-harness',
      '--features',
      'test-support',
      '--bin',
      'pi-send',
      '--message-format=json',
    ],
    {
      cwd: fileURLToPath(new URL('..', import.meta.url)),
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  )
  const artifact = built
    .split('\n')
    .filter((line) => line.startsWith('{'))
    .map((line) => JSON.parse(line))
    .find((message) => message.reason === 'compiler-artifact' && message.executable)
  return artifact.executable
}

/**
 * `send` as the binary does it. The process cannot call back for a claim, so
 * what the claim answers (or the failure it throws) is asked for first.
 */
function sendWithRust(binary) {
  return async (target, text) => {
    const { launchId, inbox, ack, ackTimeoutMs } = target.launch.channel
    const claim = await target
      .claim({ pane: target.pane, generation: target.generation })
      .catch((cause) => ({ throws: cause.message, error: cause.error }))
    const asked = {
      channel: { launchId, inbox, ack, ackTimeoutMs },
      session: target.session,
      pane: target.pane,
      generation: target.generation,
      claim,
      text,
    }
    const answered = await new Promise((resolve, reject) => {
      const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'] })
      let output = ''
      child.stdout.setEncoding('utf8')
      child.stdout.on('data', (chunk) => {
        output += chunk
      })
      child.on('error', reject)
      child.on('close', (code) => {
        try {
          resolve(JSON.parse(output))
        } catch {
          reject(new Error(`pi-send ended with ${code} and said ${JSON.stringify(output)}`))
        }
      })
      child.stdin.end(JSON.stringify(asked))
    })
    if (answered.threw !== undefined) throw new Error(answered.threw)
    return answered
  }
}

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

async function waitFor(path, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (await exists(path)) return JSON.parse(await readFile(path, 'utf8'))
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error(`timed out waiting for ${path}`)
}

async function waitForInboxFile(inbox, timeoutMs = 5000) {
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
    pane: 'chief-pane',
    generation: 1,
    claim: async () => ({ ok: true }),
    launch: {
      channel: {
        kind: 'pi-extension',
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

/** Whether cargo is here: a machine without it holds the cases against JavaScript's `send` alone. */
function hasCargo() {
  try {
    execFileSync('cargo', ['--version'], { stdio: 'ignore' })
    return true
  } catch {
    return false
  }
}

for (const [implementation, choose, skip] of [
  ['JavaScript', () => sendWithJavaScript, false],
  ['Rust', () => sendWithRust(buildSendBinary()), hasCargo() ? false : 'cargo is not installed'],
]) {
  describe(`consensflow Pi worker followup, sent by ${implementation}`, { skip }, () => {
    let sender
    before(() => {
      sender = choose()
    })
    const send = (target, text) => sender(target, text)

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
        // An upper bound only: a send through Rust starts a process first.
        const sendDeadline = Date.now() + 5000
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

    // Unsent text holds nothing (the owner's choice, 2026-10-01); Pi's own send
    // leaves it in the editor.
    it('sends a followup past unsent text, leaving the text in the editor', async () => {
      const s = await setup({ editor: 'my unfinished question' })
      const pump = setInterval(() => void s.extension.consume(), 5)
      pump.unref()
      try {
        const text = 'worker followup while drafting'
        const pending = send(s.targetFor(), text)
        const sendDeadline = Date.now() + 2_000
        while (s.pi.sent.length === 0 && Date.now() < sendDeadline) {
          await new Promise((resolve) => setTimeout(resolve, 5))
        }
        assert.deepEqual(s.pi.sent, [text])
        await s.pi.handlers.get('message_start')(
          { message: { role: 'user', content: [{ type: 'text', text }] } },
          s.ctx,
        )
        const result = await pending
        assert.equal(result.admitted, true)
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
        // The acknowledgment is written before the file moves aside, and a pass
        // the inbox watcher started may be the one doing it (a Windows runner,
        // 2026-09-29): the move comes just after.
        const deadline = Date.now() + 2_000
        while ((await exists(join(s.inbox, `${id}.json`))) && Date.now() < deadline)
          await new Promise((resolve) => setTimeout(resolve, 5))
        assert.equal(await exists(join(s.inbox, `${id}.json`)), false, s.logs.join('; '))
        assert.equal(await exists(join(s.expired, `${id}.json`)), true, s.logs.join('; '))
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
          // Expiry is not under test here: a 1 s window expired before the
          // extension read it when the full suite loaded the machine.
          expiresAt: Date.now() + 60_000,
        }
        await writeFile(join(s.inbox, `${id}.json`), `${JSON.stringify(record)}\n`)
        // The extension's own inbox watcher may start the scan first; a consume
        // that finds a scan running only queues a rerun and returns, so wait for
        // the send instead of assuming these two calls made it.
        await Promise.all([s.extension.consume(), s.extension.consume()])
        const deadline = Date.now() + 2000
        while (s.pi.sent.length === 0 && Date.now() < deadline)
          await new Promise((resolve) => setTimeout(resolve, 5))
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

    it('never sends a message id again once it was acknowledged', async () => {
      // A scan that listed the file before the acknowledgement removed it, or a
      // second copy written with the same id, must not reach the model twice.
      const s = await setup()
      try {
        const id = `m-${'c'.repeat(32)}`
        const record = `${JSON.stringify({
          id,
          type: 'message',
          launchId: 'launch-pi-test',
          session: 'native-pi-session',
          text: 'acknowledged followup',
          expiresAt: Date.now() + 60_000,
        })}\n`
        await writeFile(join(s.inbox, `${id}.json`), record)
        await s.extension.consume()
        const deadline = Date.now() + 2000
        while (s.pi.sent.length === 0 && Date.now() < deadline)
          await new Promise((resolve) => setTimeout(resolve, 5))
        await s.pi.handlers.get('message_start')(
          { message: { role: 'user', content: [{ type: 'text', text: 'acknowledged followup' }] } },
          s.ctx,
        )
        await waitFor(join(s.ack, `${id}.json`))
        while ((await exists(join(s.inbox, `${id}.json`))) && Date.now() < deadline)
          await new Promise((resolve) => setTimeout(resolve, 5))

        await writeFile(join(s.inbox, `${id}.json`), record)
        await s.extension.consume()
        assert.deepEqual(s.pi.sent, ['acknowledged followup'])
        // A consume that found the first pass still running queues a rerun; the
        // copy goes when that rerun comes, not before.
        while ((await exists(join(s.inbox, `${id}.json`))) && Date.now() < deadline)
          await new Promise((resolve) => setTimeout(resolve, 5))
        assert.equal(await exists(join(s.inbox, `${id}.json`)), false, s.logs.join('; '))
      } finally {
        await s.close()
      }
    })

    it('ignores a late consume after its inbox is gone', async () => {
      // A watcher event or a queued rerun can outlive the inbox (pane closed,
      // folder cleaned). Inside Pi an unhandled rejection can end the process.
      const s = await setup()
      await s.close()
      await s.extension.consume()
    })

    it('reports uncertain on ack timeout without an automatic retry', async () => {
      // The ack timeout is also the message's lifetime: 40 ms expired before the
      // extension read it under load, which is a refusal, not "admission unknown".
      const s = await setup({ ackTimeoutMs: 1000 })
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

    it('takes the verdict the extension gives at the expiry when it lands late', async () => {
      // The extension gives it from a timer in Pi's process, then writes it in
      // three file operations: on a busy CI runner it landed after the channel
      // had stopped looking, 30 ms past the expiry, on macOS and on Windows.
      const s = await setup({ ackTimeoutMs: 200 })
      const inbox = join(s.root, 'unwatched-inbox')
      await mkdir(inbox)
      const target = s.targetFor()
      target.launch.channel.inbox = inbox
      try {
        const sending = send(target, 'followup answered late')
        const record = JSON.parse(await readFile(await waitForInboxFile(inbox), 'utf8'))
        await new Promise((resolve) => setTimeout(resolve, record.expiresAt + 200 - Date.now()))
        const verdict = join(s.ack, `${record.id}.json`)
        const late = { id: record.id, admitted: null, reason: 'admission-unknown' }
        await writeFile(`${verdict}.tmp`, `${JSON.stringify(late)}\n`)
        await rename(`${verdict}.tmp`, verdict)
        const result = await sending
        assert.equal(result.admitted, null)
        assert.equal(result.cause, 'admission-unknown')
        assert.deepEqual(result.ack, late)
      } finally {
        await s.close()
      }
    })
  })
}
