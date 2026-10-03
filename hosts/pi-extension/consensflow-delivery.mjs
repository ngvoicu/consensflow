import { watch } from 'node:fs'
import { access, mkdir, readdir, readFile, rename, unlink, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

const MESSAGE_ID = /^m-[a-f0-9]{32}$/
const SAFE_PATH_SEGMENT = /^[A-Za-z0-9._-]+$/
const MESSAGE_FIELDS = new Set(['id', 'type', 'launchId', 'session', 'text', 'expiresAt'])

// A message goes to the interactive TUI (never a headless or RPC run) on the
// expected conversation, once Pi is idle with nothing pending. Text the human
// left in the editor holds nothing and stays there: Pi's own send never
// touches the editor (the owner's choice, 2026-10-01).
function nativeTuiState(ctx, session) {
  try {
    if (typeof session !== 'string' || sessionIdOf(ctx) !== session) {
      return { ready: false, reason: 'native session changed' }
    }
    if (ctx?.mode !== 'tui' || ctx.hasUI !== true) {
      return { ready: false, reason: 'native TUI unavailable' }
    }
    if (ctx.isIdle?.() !== true || ctx.hasPendingMessages?.() !== false)
      return { ready: false, reason: 'chief busy' }
    return { ready: true }
  } catch {
    return { ready: false, reason: 'native TUI unavailable' }
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
 * frontier: {id}, settledAt}`. It is written at `agent_settled` or when an idle
 * TUI restores a completed assistant leaf, and removed at new work.
 * `settled/<launchId>.shown.json` says `{launchId, sessionId}`: the
 * conversation the window shows, written whenever this extension starts on
 * one (Pi starts it anew at every /new, /resume or /fork), so ConsensFlow
 * follows the window there.
 * A delivery record carries its own absolute `expiresAt`;
 * this extension never derives a second timeout from launch configuration.
 * Before a send, false means zero-byte refusal. After a send, absent
 * `message_start` means null/unknown, never false. Invalid ids are quarantined;
 * expired records are acknowledged false and moved to `expired/`.
 */
export function createDeliveryExtension(
  pi,
  {
    inbox,
    ack,
    quarantine,
    settled,
    expired,
    launchId,
    logger = console,
    // How the inbox is watched, and how often it is read anyway: on macOS a
    // watch event can be lost under load, and a message nobody reads never
    // lands (a loaded test machine lost one again and again, 2026-09-28).
    watchInbox = (path, onChange) => watch(path, { persistent: false }, onChange),
    pollMs = 1000,
  } = {},
) {
  let context
  let watcher
  let poller
  let running = false
  let queued = false
  const pending = new Map()
  // Ids settled in this process, sent or refused. A scan that listed a file
  // before its acknowledgement removed it, or a second copy with the same id,
  // must never reach the model again; a new process is a new launch, whose
  // inbox refuses the old launch's records anyway.
  const settledIds = new Set()

  const logError = (...args) => logger?.error?.(...args)
  const evidenceFile =
    typeof settled === 'string' && typeof launchId === 'string' && SAFE_PATH_SEGMENT.test(launchId)
      ? join(settled, `${launchId}.json`)
      : null
  // A turn on: Pi saves nothing until an answer is complete, so a first
  // request that hangs would leave ConsensFlow nothing to read but this.
  const workingFile = evidenceFile === null ? null : join(settled, `${launchId}.working.json`)
  const shownFile = evidenceFile === null ? null : join(settled, `${launchId}.shown.json`)

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

  const writeWorking = async () => {
    if (workingFile === null) return
    const sessionId = sessionIdOf(context)
    if (sessionId === null) return
    try {
      await mkdir(settled, { recursive: true })
      await writeFile(
        `${workingFile}.tmp`,
        `${JSON.stringify({ launchId, sessionId, startedAt: Date.now() })}\n`,
        'utf8',
      )
      await rename(`${workingFile}.tmp`, workingFile)
    } catch (cause) {
      logError(`could not record a working turn: ${cause?.message ?? String(cause)}`)
    }
  }

  const writeShown = async () => {
    if (shownFile === null) return
    const sessionId = sessionIdOf(context)
    if (sessionId === null) return
    try {
      await mkdir(settled, { recursive: true })
      await writeFile(`${shownFile}.tmp`, `${JSON.stringify({ launchId, sessionId })}\n`, 'utf8')
      await rename(`${shownFile}.tmp`, shownFile)
    } catch (cause) {
      logError(`could not record the conversation shown: ${cause?.message ?? String(cause)}`)
    }
  }

  const writeSettlement = async () => {
    if (evidenceFile === null) return
    await unlink(workingFile).catch((cause) => {
      if (cause?.code !== 'ENOENT')
        logError(`could not clear a working turn: ${cause?.message ?? String(cause)}`)
    })
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
    settledIds.add(id)
    if (entry.timer !== null) clearTimeout(entry.timer)
    const response = { id, admitted }
    // Every refusal comes before the send: nothing went in.
    if (admitted === false) response.bytesWritten = 0
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
    await writeWorking()
  }

  const consume = async () => {
    if (running) {
      queued = true
      return
    }
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    running = true
    try {
      // A watcher event or queued rerun can outlive the inbox; nothing to read.
      const names = await readdir(inbox).catch((cause) => {
        if (cause?.code === 'ENOENT') return []
        throw cause
      })
      const files = names.filter((name) => name.endsWith('.json')).sort()
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
          if (settledIds.has(id)) {
            await unlink(path).catch(() => {})
            continue
          }
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
          const tui = nativeTuiState(context, record.session)
          if (tui.ready !== true) {
            await refuseBeforeSend(path, file, id, tui.reason)
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
            // the editor is never touched.
            pi.sendUserMessage(message)
          } catch (cause) {
            void acknowledge(id, null, 'send-user-message-threw')
            logError(`could not submit message ${id}: ${cause?.message ?? String(cause)}`)
          }
          break
        }
        // ConsensFlow writes messages alone: a record of any other type is set aside.
        if (typeof quarantine === 'string') await rename(path, await uniquePath(quarantine, file))
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
    watcher?.close()
    clearInterval(poller)
    await writeShown()
    await invalidateSettlement()
    const leaf = ctx.sessionManager?.getLeafEntry?.()
    if (
      ctx.mode === 'tui' &&
      ctx.hasUI === true &&
      ctx.isIdle?.() === true &&
      ctx.hasPendingMessages?.() === false &&
      leaf?.type === 'message' &&
      leaf.id === frontierOf(ctx)?.id &&
      leaf.message?.role === 'assistant' &&
      leaf.message.stopReason === 'stop'
    )
      await writeSettlement()
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    await mkdir(inbox, { recursive: true })
    watcher = watchInbox(inbox, () => void consume())
    poller = setInterval(() => void consume(), pollMs)
    poller.unref?.()
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
  pi.on('session_shutdown', async () => {
    watcher?.close()
    watcher = undefined
    clearInterval(poller)
    poller = undefined
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
  })
}
