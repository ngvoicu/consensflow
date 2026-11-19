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
 * `settled/<launchId>.compacting.json` is the extension's word that it asked Pi to compact the
 * conversation at a rest for the project's size: `{launchId, sessionId, startedAt}`. It is written
 * before the ask (and before the settlement evidence the engine takes the result by) and taken back
 * when that compaction ends, completed, failed or aborted, and at shutdown: only for the compaction
 * this extension asked for, never for Pi's own or another extension's. The engine keeps the close of
 * a done task's window open while it stands, up to its own bound from `startedAt` (T-1475: the
 * close, ten seconds after the result, aborted the summary, which takes 39 to 80 seconds).
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
    // The size the project chose for its members to compact at, in tokens, or none:
    // Pi's own point.
    compaction,
    // The longest a compaction this extension asked for may go on before it is ended: past it
    // Pi is told to abort it, its word is taken back and the window is free of it. The engine
    // holds a done task's window by the word for the same length (`COMPACTION_HOLD_MS`), so
    // nothing, a reopen's words included, waits past it (T-1475).
    compactionLimitMs = 300_000,
  } = {},
) {
  let context
  // Whether a compaction this extension asked for is at work: a delivery waits for it.
  let compacting = false
  // Whether Pi compacts, whoever began it (this extension, or Pi itself at its own point or
  // to recover an overflow): Pi tells its extensions `session_before_compact`, then
  // `session_compact` or `session_compact_failed`, and its `isIdle()` is false all along
  // (`!this._isAgentRunActive && !this.isCompacting`, Pi 1.4.0 and 1.5.0).
  let piCompacting = false
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
  const compactingFile = evidenceFile === null ? null : join(settled, `${launchId}.compacting.json`)

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

  // The word that this extension's own compaction is at work: written before the ask, and
  // the ask is made only once it is in place. Says whether it is: where no folder is told (no
  // engine to read it) there is nothing to write and the ask goes; a word that cannot be written
  // asks for no compaction at this rest (the engine would find the window free with nothing
  // to hold it by, and its close would cut the summary), said once in the log. It is written
  // under another name and moved into place, as the settlement evidence is: a reader never
  // finds half of it, and a rename replaces a file a reader has open (Rust opens a file
  // sharing delete) on Windows too, with a few tries where it is refused for a moment.
  let wordFailure
  const writeCompactionWord = async () => {
    if (compactingFile === null) return true
    const sessionId = sessionIdOf(context)
    try {
      if (sessionId === null) throw new Error('Pi has no conversation to name')
      await mkdir(settled, { recursive: true })
      await writeFile(
        `${compactingFile}.tmp`,
        `${JSON.stringify({ launchId, sessionId, startedAt: Date.now() })}\n`,
        'utf8',
      )
      for (let attempt = 1; ; attempt += 1) {
        try {
          await rename(`${compactingFile}.tmp`, compactingFile)
          break
        } catch (cause) {
          if (attempt >= 4 || !['EPERM', 'EBUSY', 'EACCES'].includes(cause?.code)) throw cause
          await new Promise((resolve) => setTimeout(resolve, 25 * attempt))
        }
      }
      wordFailure = undefined
      return true
    } catch (cause) {
      const said = `could not record a compaction at work, so none is asked for at this rest: ${cause?.message ?? String(cause)}`
      if (said !== wordFailure) logError(said)
      wordFailure = said
      return false
    }
  }

  const takeCompactionWordBack = async () => {
    if (compactingFile === null) return
    try {
      await unlink(compactingFile)
    } catch (cause) {
      if (cause?.code !== 'ENOENT') {
        logError(`could not take back the word of a compaction: ${cause?.message ?? String(cause)}`)
      }
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
          // A window that compacts is at work, though no turn runs: the message is refused
          // before it is sent, in Pi's own word for a window at work, which spends no attempt
          // of it (T-1272: left for later, each expired, and three spent the delivery).
          if (compactingNow()) {
            const reason =
              sessionIdOf(context) === record.session ? 'chief busy' : 'native session changed'
            await refuseBeforeSend(path, file, id, reason)
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
  /**
   * Whether the window compacts now: a compaction this extension asked for and whose callback
   * has not come, or one Pi said it began and has not said it ended. Pi tells no extension of
   * an end where it finds no entry it saved, so an idle Pi is taken for one that compacts
   * nothing.
   */
  const compactingNow = () => {
    if (compacting) return true
    if (piCompacting && context?.isIdle?.() === true) piCompacting = false
    return piCompacting
  }

  /**
   * Asks Pi to compact the window's conversation at this rest, once its context is past
   * the size the project chose (smaller-contexts, SCX-28 to SCX-30): Pi has no flag or
   * variable for it, and its settings are the owner's. `ctx.compact()` aborts a turn at
   * work, so it is asked only at a rest (idle, nothing pending, the turn not aborted by the
   * human), and Pi's own compaction does the rest; a long turn grows meanwhile, to Pi's own
   * point at most. A size at or above the model's window, or a context Pi cannot say yet,
   * asks for none. A compaction that fails is said, and the next rest asks again. The word
   * (`compacting/<launch>.json`) is written before the ask and taken back as it ends, for the
   * engine to keep the window open meanwhile (T-1475).
   */
  // Whether this rest asks for a compaction, and if so the compaction is marked as ours
  // (`compacting`) at once, so no second ask comes before the first has begun.
  const wantsCompaction = (event, ctx) => {
    if (!Number.isInteger(compaction) || compaction <= 0 || compacting) return false
    if (event?.aborted === true) return false
    if (ctx?.isIdle?.() !== true || ctx.hasPendingMessages?.() !== false) return false
    let usage
    try {
      usage = ctx.getContextUsage?.()
    } catch {
      return false
    }
    const tokens = usage?.tokens
    if (!Number.isFinite(tokens) || tokens <= compaction) return false
    if (!(compaction < usage.contextWindow)) return false
    compacting = true
    return true
  }

  // The ask now at work, counted: the end of an earlier one, late, is not this one's.
  let asks = 0
  let limit
  const askCompaction = (ctx) => {
    asks += 1
    const mine = asks
    const over = () => {
      if (mine !== asks) return
      clearTimeout(limit)
      compacting = false
      void takeCompactionWordBack()
      void consume()
    }
    try {
      ctx.compact({
        onComplete: over,
        onError: (cause) => {
          logError(`compaction at ${compaction} tokens failed: ${cause?.message ?? String(cause)}`)
          over()
        },
      })
    } catch (cause) {
      logError(`compaction at ${compaction} tokens failed: ${cause?.message ?? String(cause)}`)
      over()
      return
    }
    // A compaction that does not end is ended: Pi is told to abort it, and the window is free of
    // it, its word taken back, whether or not Pi ever calls back.
    limit = setTimeout(() => {
      if (mine !== asks || !compacting) return
      logError(
        `compaction at ${compaction} tokens did not end in ${Math.round(compactionLimitMs / 1000)} s: aborted`,
      )
      try {
        ctx.abort?.()
      } catch (cause) {
        logError(`could not abort it: ${cause?.message ?? String(cause)}`)
      }
      over()
    }, compactionLimitMs)
    limit.unref?.()
  }

  pi.on('session_before_compact', (_event, ctx) => {
    context = ctx ?? context
    piCompacting = true
  })
  // Ended: a turn the compaction came in the middle of goes on as any turn does.
  const compactionEnded = async (_event, ctx) => {
    context = ctx ?? context
    // The word is taken back as the compaction this extension asked for ends, completed,
    // failed or aborted; one Pi began itself, or another extension did, never had one.
    if (compacting) await takeCompactionWordBack()
    piCompacting = false
    await consume()
  }
  pi.on('session_compact', compactionEnded)
  pi.on('session_compact_failed', compactionEnded)
  pi.on('agent_settled', async (event, ctx) => {
    context = ctx ?? context
    // The word comes before the settlement evidence the engine takes the result by: a window
    // found free is never found free before the word that holds its close.
    let asking = wantsCompaction(event, context)
    if (asking && !(await writeCompactionWord())) {
      compacting = false
      asking = false
    }
    await writeSettlement()
    if (asking) askCompaction(context)
    await consume()
  })
  pi.on('session_shutdown', async () => {
    clearTimeout(limit)
    await takeCompactionWordBack()
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
    compaction: compactionOf(process.env),
  })
}

/**
 * The size the launch gave the window to compact at, from its environment
 * (`CF_COMPACTION_TOKENS`): a whole number of tokens above nought, or none, which is Pi's
 * own point.
 */
export function compactionOf(env) {
  const text = env?.CF_COMPACTION_TOKENS
  if (typeof text !== 'string' || !/^[1-9][0-9]*$/.test(text)) return undefined
  const tokens = Number(text)
  return Number.isSafeInteger(tokens) ? tokens : undefined
}
