import { watch } from 'node:fs'
import { access, mkdir, readdir, readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const DELIVERY_ID = /^d-\d+$/
const MESSAGE_ID = /^m-[a-f0-9]{32}$/
const SAFE_PATH_SEGMENT = /^[A-Za-z0-9._-]+$/
const EDITOR_PROBE_ID = /^editor-[a-f0-9]{32}$/
const MESSAGE_FIELDS = new Set(['id', 'type', 'launchId', 'session', 'text', 'expiresAt'])

// Interactive Pi reads the expanded editor, including pasted attachment paths.
// RPC/headless also return an empty string, so the native TUI mode is required.
function nativeEditorState(ctx, session) {
  try {
    if (typeof session !== 'string' || sessionIdOf(ctx) !== session) {
      return { ready: false, reason: 'native session changed' }
    }
    if (ctx?.mode !== 'tui' || ctx.hasUI !== true || typeof ctx.ui?.getEditorText !== 'function') {
      return { ready: false, reason: 'native editor unavailable' }
    }
    const text = ctx.ui.getEditorText()
    if (typeof text !== 'string') return { ready: false, reason: 'native editor unavailable' }
    if (text !== '') return { ready: false, reason: 'draft open' }
    if (ctx.isIdle?.() !== true || ctx.hasPendingMessages?.() !== false)
      return { ready: false, reason: 'lead busy' }
    return { ready: true }
  } catch {
    return { ready: false, reason: 'native editor unavailable' }
  }
}

function messageText(message) {
  if (typeof message?.content === 'string') return message.content
  if (!Array.isArray(message?.content)) return null
  return message.content
    .filter((part) => part?.type === 'text')
    .map((part) => part.text)
    .join('')
}

/** Accept exactly the canonical envelope or the canonical cf-read pointer. */
function envelopeText(record) {
  if (typeof record?.text !== 'string') return null
  if (record.channel === 'cf-read') {
    try {
      if (record.text === pointer(record)) return record.text
    } catch {}
    return null
  }
  if (typeof record?.answer !== 'string') return null
  try {
    if (record.text === envelope(record)) return record.text
  } catch {}
  return null
}

function validDeliveryId(id) {
  return typeof id === 'string' && DELIVERY_ID.test(id)
}

function validMessageId(id) {
  return typeof id === 'string' && MESSAGE_ID.test(id)
}

/**
 * A raw worker followup is strictly bounded: exactly the adapter's six
 * fields, a non-empty text sent verbatim and no result envelope. Anything
 * wider is refused before send without touching the native editor.
 */
function invalidMessageShape(record) {
  if (record?.type !== 'message') return 'invalid-record'
  for (const key of Object.keys(record ?? {})) {
    if (!MESSAGE_FIELDS.has(key)) return 'invalid-record'
  }
  if (typeof record.launchId !== 'string' || record.launchId.length === 0) return 'invalid-record'
  if (typeof record.session !== 'string' || record.session.length === 0) return 'invalid-record'
  if (typeof record.text !== 'string' || record.text.length === 0) return 'missing-text'
  return null
}

function validExpiry(expiresAt) {
  return Number.isFinite(expiresAt) && expiresAt > 0
}

function sessionIdOf(ctx) {
  const value = ctx?.sessionManager?.getSessionId?.()
  return typeof value === 'string' && value.length > 0 ? value : null
}

function frontierOf(ctx) {
  const value = ctx?.sessionManager?.getLeafId?.()
  return typeof value === 'string' && value.length > 0 ? { id: value } : null
}

/**
 * Install the delivery extension against Pi's ExtensionAPI.
 *
 * The fake in tests follows Pi 0.85.1's installed types: `ExtensionAPI.on`
 * exposes `agent_start`, `agent_settled`, `turn_start`, and `message_start`
 * (dist/core/extensions/types.d.ts:924-931). `AgentSettledEvent` is the
 * post-retry/compaction/continuation boundary (line 561); `TurnStartEvent`
 * fires for each turn (line 580); and `MessageStartEvent` carries the message
 * entering the run (lines 593-595). The context's read-only session manager
 * exposes `getSessionId()` and `getLeafId()` (dist/core/session-manager.d.ts:
 * 140, 240-241), so the evidence is tied to Pi's native session frontier.
 *
 * `settled/<launchId>.json` is app-owned evidence with `{launchId, sessionId,
 * frontier: {id}, settledAt}`. It is written only at `agent_settled` and
 * removed at new work. A delivery record carries its own absolute `expiresAt`;
 * this extension never derives a second timeout from launch configuration.
 * Before a send, false means zero-byte refusal. After a send, absent
 * `message_start` means null/unknown, never false. Invalid ids are quarantined;
 * expired records are acknowledged false and moved to `expired/`.
 */
export function createDeliveryExtension(
  pi,
  { inbox, ack, quarantine, settled, expired, launchId, editorGuard, logger = console } = {},
) {
  let context
  let watcher
  let running = false
  let queued = false
  const pending = new Map()

  const logError = (...args) => logger?.error?.(...args)
  const evidenceFile =
    typeof settled === 'string' && typeof launchId === 'string' && SAFE_PATH_SEGMENT.test(launchId)
      ? join(settled, `${launchId}.json`)
      : null

  const uniquePath = async (directory, file) => {
    await mkdir(directory, { recursive: true })
    for (let suffix = 0; ; suffix += 1) {
      const name = suffix === 0 ? file : `${file}.${suffix}`
      const destination = join(directory, name)
      try {
        await access(destination)
      } catch (cause) {
        if (cause?.code === 'ENOENT') return destination
        throw cause
      }
    }
  }

  const invalidateSettlement = async () => {
    if (evidenceFile === null) return
    try {
      await unlink(evidenceFile)
    } catch (cause) {
      if (cause?.code !== 'ENOENT') {
        logError(`could not invalidate settlement evidence: ${cause?.message ?? String(cause)}`)
      }
    }
  }

  const writeSettlement = async () => {
    if (evidenceFile === null) return
    const sessionId = sessionIdOf(context)
    const frontier = frontierOf(context)
    if (sessionId === null || frontier === null) {
      logError('could not record settlement evidence: Pi session frontier is unavailable')
      return
    }
    try {
      await mkdir(settled, { recursive: true })
      const temporary = `${evidenceFile}.tmp`
      await writeFile(
        temporary,
        `${JSON.stringify({ launchId, sessionId, frontier, settledAt: Date.now() })}\n`,
        'utf8',
      )
      await rename(temporary, evidenceFile)
    } catch (cause) {
      logError(`could not record settlement evidence: ${cause?.message ?? String(cause)}`)
    }
  }

  const acknowledge = async (id, admitted, reason, archive = null) => {
    const entry = pending.get(id)
    if (entry === undefined || entry.done) return
    entry.done = true
    if (entry.timer !== null) clearTimeout(entry.timer)
    const response = { id, admitted }
    if (admitted === false && editorGuard === 1) response.bytesWritten = 0
    if (admitted === true) response.mode = 'tui'
    else response.reason = reason
    const ackPath = join(ack, `${id}.json`)
    const temporary = `${ackPath}.tmp`
    try {
      await mkdir(ack, { recursive: true })
      await writeFile(temporary, `${JSON.stringify(response)}\n`, 'utf8')
      await rename(temporary, ackPath)
      if (archive === null) await unlink(entry.path)
      else await rename(entry.path, await uniquePath(archive, entry.file))
      pending.delete(id)
    } catch (cause) {
      logError(`could not acknowledge delivery ${id}: ${cause?.message ?? String(cause)}`)
    }
  }

  const refuseBeforeSend = async (path, file, id, reason, archive = null) => {
    if (pending.has(id)) return
    pending.set(id, { path, file, timer: null, done: false })
    await acknowledge(id, false, reason, archive)
  }

  const observeMessage = async (event, ctx) => {
    context = ctx ?? context
    await invalidateSettlement()
    if (event?.message?.role !== 'user') return
    const text = messageText(event.message)
    if (text === null) return
    for (const [id, entry] of pending) {
      if (entry.text === text) void acknowledge(id, true)
    }
  }

  const newWork = async (_event, ctx) => {
    context = ctx ?? context
    await invalidateSettlement()
  }

  const consume = async () => {
    if (running) {
      queued = true
      return
    }
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    running = true
    try {
      const files = (await readdir(inbox))
        .filter((name) => name.endsWith('.json'))
        .sort(
          (a, b) =>
            Number(b.startsWith('editor-')) - Number(a.startsWith('editor-')) || a.localeCompare(b),
        )
      for (const file of files) {
        const path = join(inbox, file)
        let record
        try {
          record = JSON.parse(await readFile(path, 'utf8'))
        } catch (cause) {
          logError(`could not read delivery ${file}: ${cause?.message ?? String(cause)}`)
          continue
        }
        const id = record?.id
        if (editorGuard === 1 && EDITOR_PROBE_ID.test(id) && file === `${id}.json`) {
          // A fresh launch/session-bound challenge, never cached editor text.
          if (
            record.launchId === launchId &&
            validExpiry(record.expiresAt) &&
            record.expiresAt > Date.now()
          ) {
            const response = {
              id,
              launchId,
              session: record.session,
              expiresAt: record.expiresAt,
              ...nativeEditorState(context, record.session),
            }
            await mkdir(ack, { recursive: true })
            const destination = join(ack, file)
            await writeFile(`${destination}.tmp`, `${JSON.stringify(response)}\n`, 'utf8')
            await rename(`${destination}.tmp`, destination)
          }
          await unlink(path).catch(() => {})
          continue
        }
        if (record?.type === 'message') {
          if (!validMessageId(id)) {
            if (typeof quarantine === 'string') {
              try {
                await rename(path, await uniquePath(quarantine, file))
                logError(`invalid message id quarantined from ${file}`)
              } catch (cause) {
                logError(
                  `could not quarantine invalid message ${file}: ${cause?.message ?? String(cause)}`,
                )
              }
            } else {
              logError(`invalid message id in ${file}: quarantine is not configured`)
            }
            continue
          }
          if (pending.has(id)) continue
          const shapeError = invalidMessageShape(record)
          if (shapeError !== null) {
            logError(`message ${id} is ${shapeError}`)
            await refuseBeforeSend(path, file, id, shapeError)
            continue
          }
          if (record.launchId !== launchId) {
            await refuseBeforeSend(path, file, id, 'wrong-launch')
            continue
          }
          if (!validExpiry(record.expiresAt)) {
            await refuseBeforeSend(path, file, id, 'missing-expiry')
            continue
          }
          if (record.expiresAt <= Date.now()) {
            if (typeof expired === 'string') {
              await refuseBeforeSend(path, file, id, 'expired-before-send', expired)
            } else {
              logError(`expired message ${id}: expired directory is not configured`)
            }
            continue
          }
          if (context?.isIdle?.() !== true) break
          if (editorGuard === 1) {
            const editor = nativeEditorState(context, record.session)
            if (editor.ready !== true) {
              await refuseBeforeSend(path, file, id, editor.reason)
              continue
            }
          } else if (sessionIdOf(context) !== record.session) {
            await refuseBeforeSend(path, file, id, 'native session changed')
            continue
          }
          const message = record.text
          const messageEntry = { path, file, text: message, timer: null, done: false }
          pending.set(id, messageEntry)
          messageEntry.timer = setTimeout(
            () => void acknowledge(id, null, 'admission-unknown'),
            Math.max(0, record.expiresAt - Date.now()),
          )
          try {
            // The raw text goes verbatim. Only message_start proves entry;
            // the draft is only read, never cleared or overwritten.
            pi.sendUserMessage(message)
          } catch (cause) {
            void acknowledge(id, false, 'send-user-message-threw')
            logError(`could not submit message ${id}: ${cause?.message ?? String(cause)}`)
          }
          break
        }
        if (!validDeliveryId(id)) {
          if (typeof quarantine === 'string') {
            try {
              await rename(path, await uniquePath(quarantine, file))
              logError(`invalid delivery id quarantined from ${file}`)
            } catch (cause) {
              logError(
                `could not quarantine invalid delivery ${file}: ${cause?.message ?? String(cause)}`,
              )
            }
          } else {
            logError(`invalid delivery id in ${file}: quarantine is not configured`)
          }
          continue
        }
        if (pending.has(id)) continue
        if (!validExpiry(record.expiresAt)) {
          await refuseBeforeSend(path, file, id, 'missing-expiry')
          continue
        }
        if (record.expiresAt <= Date.now()) {
          if (typeof expired === 'string') {
            await refuseBeforeSend(path, file, id, 'expired-before-send', expired)
          } else {
            logError(`expired delivery ${id}: expired directory is not configured`)
          }
          continue
        }
        const text = envelopeText(record)
        if (text === null) {
          logError(`delivery ${id} is missing envelope or pointer`)
          await refuseBeforeSend(path, file, id, 'missing-envelope')
          continue
        }
        if (context?.isIdle?.() !== true) break

        if (editorGuard === 1) {
          const editor = nativeEditorState(context, record.target?.session)
          if (editor.ready !== true) {
            await refuseBeforeSend(path, file, id, editor.reason)
            continue
          }
        }

        const entry = { path, file, text, timer: null, done: false }
        pending.set(id, entry)
        entry.timer = setTimeout(
          () => void acknowledge(id, null, 'admission-unknown'),
          Math.max(0, record.expiresAt - Date.now()),
        )
        try {
          // Pi's sendUserMessage returns void. Only message_start proves entry
          // into the session; a settled promise would merely mean turn end.
          pi.sendUserMessage(text)
        } catch (cause) {
          void acknowledge(id, false, 'send-user-message-threw')
          logError(`could not submit delivery ${id}: ${cause?.message ?? String(cause)}`)
        }
        break
      }
    } finally {
      running = false
      if (queued) {
        queued = false
        void consume()
      }
    }
  }

  pi.on('session_start', async (_event, ctx) => {
    context = ctx
    await invalidateSettlement()
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    await mkdir(inbox, { recursive: true })
    watcher = watch(inbox, { persistent: false }, () => void consume())
    await consume()
  })
  pi.on('agent_start', newWork)
  pi.on('turn_start', newWork)
  pi.on('message_start', observeMessage)
  pi.on('agent_settled', async (_event, ctx) => {
    context = ctx ?? context
    await writeSettlement()
    await consume()
  })
  pi.on('session_shutdown', () => {
    watcher?.close()
    watcher = undefined
    for (const entry of pending.values()) {
      if (entry.timer !== null) clearTimeout(entry.timer)
    }
    pending.clear()
    context = undefined
  })

  return { consume }
}

// This is the only environment-reading entry point. The launch producer
// supplies these names in the enabled Pi process environment.
export default function consensflowDelivery(pi) {
  return createDeliveryExtension(pi, {
    inbox: process.env.CF_DELIVERY_INBOX,
    ack: process.env.CF_DELIVERY_ACK,
    quarantine: process.env.CF_DELIVERY_QUARANTINE,
    settled: process.env.CF_DELIVERY_SETTLED,
    expired: process.env.CF_DELIVERY_EXPIRED,
    launchId: process.env.CF_DELIVERY_LAUNCH_ID,
    editorGuard: process.env.CF_DELIVERY_EDITOR_GUARD === '1' ? 1 : undefined,
  })
}
