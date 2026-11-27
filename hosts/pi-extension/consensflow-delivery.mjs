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

/** How many ended runs the word keeps for a reader that has not taken them yet. */
const ENDS_KEPT = 8
/**
 * How long a prompt submitted with no run begun is held in hand: Pi tells no extension that
 * another extension's `input` handler handled it, or that it was refused for its model or login.
 */
const IN_HAND_MS = 30_000
/**
 * On a Pi that marks a run only at its first request, how many passes of the timer in a row must
 * find Pi idle, and how long since the run began, for it to be taken as dropped.
 */
const LATE_IDLE_POLLS = 3
const LATE_IDLE_MS = 3_000

/**
 * Install the delivery extension against Pi's ExtensionAPI.
 *
 * **The word** (`settled/<launchId>.state.json`, written under another name and moved into place):
 * `{launchId, sessionId, state, leaf, begun, ended, ends, n, at}`, Pi's own run state as this
 * extension follows it (the brain's `architecture/pi-turn-state.md`). `state` is `working` (a run
 * is on), `busy` (no run, and Pi is not free: it compacts, sums up a branch, or holds a prompt
 * someone submitted) or `rest`. `leaf` is `getLeafId()` at the write. `begun` and `ended` count
 * this launch's runs; `ends` keeps the last {@link ENDS_KEPT} ended runs, each `{n, leaf,
 * aborted}`: its count, its leaf and Pi's flag at its `agent_settled` (null where Pi gave none,
 * or the run was dropped). `n` counts the writes, `at` is the time of this one.
 *
 * Three notes, changed only by hooks: a run is on while `begun > ended`; `begun` goes up at the
 * first of `before_agent_start`, `agent_start` or `turn_start` since the last end, and `ended`
 * at `agent_settled`, the handler writing the end at once. A prompt is in hand from the `input`
 * hook, or from this extension's own send, while no run is on, until the next begin (or
 * {@link IN_HAND_MS}). A run Pi dropped (begun, and Pi idle with no settle) is ended by a pass of
 * the 1 s timer that finds Pi idle, never by an inbox event's pass: on a Pi that marks a run only
 * at its first request (before 1.2.0, told by finding Pi idle at this extension's
 * `before_agent_start`), by the third in a row and three seconds after it began. A run is ended
 * once: a late hook of a run taken as dropped takes it up again, uncounted.
 *
 * The counts are the launch's. The word stays at `session_shutdown`, and the instance Pi starts
 * next in the launch (`/reload`, a new session, a switch) goes on from its counts, and from its
 * ends where the session is the same; a new session's ends are its own.
 *
 * The state is `working` while a run is on; else `busy` while a prompt is in hand or Pi is not
 * idle; else `rest`. It is written when it changes, at every hook and every pass of the poll;
 * **`rest` is written only by a pass of the poll** (and at `session_start`), never inside a
 * handler, which keeps the state it had. The inbox consumer sends only in a pass of the poll or of
 * an inbox event whose state is `rest`: never from a handler, `session_start` included. This extension's own compaction for the project's size is asked by the pass that writes
 * `rest` after a run's end: its word (`<launchId>.compacting.json`, `{launchId, sessionId,
 * startedAt}`) is written before `rest` and the ask made after it, so the engine never finds the
 * window free before the word that holds a done task's close (T-1475).
 *
 * `settled/<launchId>.shown.json` says `{launchId, sessionId}`: the conversation the window shows,
 * written whenever this extension starts on one (Pi starts it anew at every /new, /resume or
 * /fork), so ConsensFlow follows the window there. A delivery record carries its own absolute
 * `expiresAt`; this extension never derives a second timeout from launch configuration. Before a
 * send, false means zero-byte refusal. After a send, absent `message_start` means null/unknown,
 * never false. Invalid ids are quarantined; expired records are acknowledged false and moved to
 * `expired/`.
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
    // How long a prompt submitted with no run begun is held in hand (tests shorten it).
    inHandMs = IN_HAND_MS,
    now = () => Date.now(),
  } = {},
) {
  let context
  // Whether a compaction this extension asked for is at work.
  let compacting = false
  let watcher
  let poller
  const pending = new Map()
  // Ids settled in this process, sent or refused. A scan that listed a file
  // before its acknowledgement removed it, or a second copy with the same id,
  // must never reach the model again; a new process is a new launch, whose
  // inbox refuses the old launch's records anyway.
  const settledIds = new Set()

  // The notes, and what was last written of them.
  let begun = 0
  let ended = 0
  let ends = []
  let inHand = null
  let idlePolls = 0
  // When the run now on began, and whether the last end was a run a poll took as dropped: a
  // late hook of that run then takes it up again, and it is not ended twice.
  let startedAt = 0
  let dropped = false
  let revived = false
  // This Pi marks a run only at its first request (Pi before 1.2.0).
  let late = false
  let state = null
  let writes = 0
  let lastWord = null
  // The count of ended runs the last rest looked at for a compaction.
  let restAfter = 0
  // The window shut down: nothing more is written, until a session starts again.
  let shut = false

  const logError = (...args) => logger?.error?.(...args)
  const named =
    typeof settled === 'string' && typeof launchId === 'string' && SAFE_PATH_SEGMENT.test(launchId)
  const stateFile = named ? join(settled, `${launchId}.state.json`) : null
  const shownFile = named ? join(settled, `${launchId}.shown.json`) : null
  const compactingFile = named ? join(settled, `${launchId}.compacting.json`) : null

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

  // A file written under another name and moved into place: a reader never finds half of it,
  // and a rename replaces a file a reader has open (Rust opens a file sharing delete) on
  // Windows too, with a few tries where it is refused for a moment.
  const put = async (file, text) => {
    await mkdir(settled, { recursive: true })
    await writeFile(`${file}.tmp`, text, 'utf8')
    for (let attempt = 1; ; attempt += 1) {
      try {
        await rename(`${file}.tmp`, file)
        return
      } catch (cause) {
        if (attempt >= 4 || !['EPERM', 'EBUSY', 'EACCES'].includes(cause?.code)) throw cause
        await new Promise((resolve) => setTimeout(resolve, 25 * attempt))
      }
    }
  }

  const runOn = () => begun > ended || revived
  const idle = () => {
    try {
      return context?.isIdle?.() === true
    } catch {
      return false
    }
  }
  // Busy also from this extension's own ask of a compaction until it ends: Pi reads idle for a
  // moment after the ask, before it sets its compaction state.
  const sampled = () => {
    if (runOn()) return 'working'
    if (inHand !== null || compacting || !idle()) return 'busy'
    return 'rest'
  }
  const end = (aborted, drop = false) => {
    ended += 1
    ends.push({ n: ended, leaf: frontierOf(context)?.id ?? null, aborted })
    if (ends.length > ENDS_KEPT) ends = ends.slice(-ENDS_KEPT)
    idlePolls = 0
    dropped = drop
  }

  // The word's writes, one after another, in the order they were asked.
  let writing = Promise.resolve()
  const say = (inPoll) => {
    let next = sampled()
    if (next === 'rest' && !inPoll) next = state ?? 'busy'
    state = next
    const sessionId = sessionIdOf(context)
    if (shut || stateFile === null || sessionId === null) return writing
    const word = {
      launchId,
      sessionId,
      state: next,
      leaf: frontierOf(context)?.id ?? null,
      begun,
      ended,
      ends,
    }
    const key = JSON.stringify(word)
    if (key === lastWord) return writing
    lastWord = key
    writes += 1
    const text = `${JSON.stringify({ ...word, n: writes, at: now() })}\n`
    writing = writing
      .then(() => put(stateFile, text))
      .catch((cause) => {
        lastWord = null
        logError(`could not record Pi's run state: ${cause?.message ?? String(cause)}`)
      })
    return writing
  }

  const writeShown = async () => {
    if (shownFile === null) return
    const sessionId = sessionIdOf(context)
    if (sessionId === null) return
    try {
      await put(shownFile, `${JSON.stringify({ launchId, sessionId })}\n`)
    } catch (cause) {
      logError(`could not record the conversation shown: ${cause?.message ?? String(cause)}`)
    }
  }

  // The word that this extension's own compaction is at work: written before the ask, and
  // the ask is made only once it is in place. Says whether it is: where no folder is told (no
  // engine to read it) there is nothing to write and the ask goes; a word that cannot be written
  // asks for no compaction at this rest (the engine would find the window free with nothing
  // to hold it by, and its close would cut the summary), said once in the log.
  let wordFailure
  const writeCompactionWord = async () => {
    if (shut) return false
    if (compactingFile === null) return true
    const sessionId = sessionIdOf(context)
    try {
      if (sessionId === null) throw new Error('Pi has no conversation to name')
      await put(compactingFile, `${JSON.stringify({ launchId, sessionId, startedAt: now() })}\n`)
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
    if (event?.message?.role !== 'user') return
    const text = messageText(event.message)
    if (text === null) return
    for (const [id, entry] of pending) {
      if (entry.text === text) void acknowledge(id, true)
    }
  }

  // A run begins: the first of its beginning hooks since the last end. Written first thing,
  // and awaited, so the word says `working` before any later extension's handler runs.
  const begin = (hook, ctx) => {
    context = ctx ?? context
    if (!runOn()) {
      // A late hook of a run a poll took as dropped takes that run up again, uncounted: a run
      // is ended once. Only `before_agent_start` begins a run of its own for certain.
      if (dropped && hook !== 'before_agent_start') revived = true
      else {
        begun += 1
        startedAt = now()
      }
      dropped = false
      inHand = null
      idlePolls = 0
    }
    return say(false)
  }

  // The inbox, read in a pass whose state is `rest`: one message at most is sent.
  const consume = async () => {
    if (typeof inbox !== 'string' || typeof ack !== 'string') return
    // A watcher event can outlive the inbox; nothing to read.
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
        if (record.expiresAt <= now()) {
          if (typeof expired === 'string') {
            await refuseBeforeSend(path, file, id, 'expired-before-send', expired)
          } else {
            logError(`expired message ${id}: expired directory is not configured`)
          }
          continue
        }
        // The one gate: a message goes in only where the state this pass would write is `rest`.
        // A window that is busy (it compacts, or holds a prompt someone submitted) refuses it
        // before it is sent, in Pi's own word for a window at work, which spends no attempt of it
        // (T-1272); one at work leaves it waiting.
        const now_ = sampled()
        if (now_ === 'busy') {
          const reason =
            sessionIdOf(context) === record.session ? 'chief busy' : 'native session changed'
          await refuseBeforeSend(path, file, id, reason)
          continue
        }
        if (now_ !== 'rest' || state !== 'rest') break
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
          Math.max(0, record.expiresAt - now()),
        )
        // The prompt is in hand from the moment it is sent, until its run begins.
        inHand = now()
        try {
          // The raw text goes verbatim. Only message_start proves entry;
          // the editor is never touched.
          pi.sendUserMessage(message)
        } catch (cause) {
          void acknowledge(id, null, 'send-user-message-threw')
          logError(`could not submit message ${id}: ${cause?.message ?? String(cause)}`)
        }
        await say(false)
        break
      }
      // ConsensFlow writes messages alone: a record of any other type is set aside.
      if (typeof quarantine === 'string') await rename(path, await uniquePath(quarantine, file))
    }
  }

  // One pass of the poll (also on an inbox event): the notes a poll may change, the word, the
  // compaction a rest asks for, and the inbox.
  const onePass = async (fromTimer) => {
    if (shut || context === undefined) return
    try {
      if (inHand !== null && now() - inHand >= inHandMs) inHand = null
      // A run Pi dropped is told by the timer's passes alone, never by an inbox event's: on a Pi
      // that marks a run only at its first request, by three of them and three seconds since it
      // began.
      if (!fromTimer) {
        // nothing of a dropped run
      } else if (runOn() && idle()) {
        idlePolls += 1
        const gone = late
          ? idlePolls >= LATE_IDLE_POLLS && now() - startedAt >= LATE_IDLE_MS
          : idlePolls >= 1
        if (gone && revived) {
          revived = false
          idlePolls = 0
        } else if (gone) end(null, true)
      } else {
        idlePolls = 0
      }
      let asking = false
      if (sampled() === 'rest' && ended > restAfter) {
        restAfter = ended
        asking =
          wantsCompaction(ends.at(-1)?.aborted === true, context) && (await writeCompactionWord())
      }
      await say(true)
      if (asking && state === 'rest') {
        // Ours from the ask on, so that no second ask and no message comes before Pi has begun.
        compacting = true
        askCompaction(context)
      } else {
        if (asking) await takeCompactionWordBack()
        await consume()
      }
    } catch (cause) {
      logError(`could not read the inbox: ${cause?.message ?? String(cause)}`)
    }
  }

  // Passes go one after another; one asked while another runs is the next, shared by every
  // ask until it begins, and its promise is its end.
  let passes = Promise.resolve()
  let next = null
  let timerAsked = false
  const pass = (fromTimer = false) => {
    if (fromTimer) timerAsked = true
    if (next !== null) return next
    next = passes.then(() => {
      next = null
      const timer = timerAsked
      timerAsked = false
      return onePass(timer)
    })
    passes = next
    return next
  }

  // The word this launch wrote before, by an instance of this extension that Pi replaced
  // (`/reload`, a new session, a switch): its counts go on, and in the same session its ends.
  const carried = async () => {
    if (stateFile === null) return null
    try {
      const word = JSON.parse(await readFile(stateFile, 'utf8'))
      return word?.launchId === launchId && Number.isSafeInteger(word.ended) ? word : null
    } catch {
      return null
    }
  }

  pi.on('session_start', async (_event, ctx) => {
    context = ctx
    shut = false
    watcher?.close()
    clearInterval(poller)
    await writeShown()
    // The notes cleared: no run of this conversation is on, no prompt in hand. The counts go on:
    // they are the launch's, from the word an instance before this one wrote; its ends too where
    // the session is the same (a `/reload`), so that a reader still takes them.
    const before = await carried()
    if (before !== null && before.ended > ended) ended = before.ended
    begun = ended
    ends =
      before !== null && before.sessionId === sessionIdOf(ctx) && Array.isArray(before.ends)
        ? before.ends.filter((end) => Number.isSafeInteger(end?.n)).slice(-ENDS_KEPT)
        : []
    inHand = null
    idlePolls = 0
    dropped = false
    revived = false
    restAfter = ended
    lastWord = null
    // The word at once; the timer's first pass reads the inbox: nothing is sent from a handler.
    await say(true)
    if (typeof inbox === 'string' && typeof ack === 'string') {
      await mkdir(inbox, { recursive: true })
      watcher = watchInbox(inbox, () => void pass())
    }
    poller = setInterval(() => void pass(true), pollMs)
    poller.unref?.()
  })
  pi.on('input', (_event, ctx) => {
    context = ctx ?? context
    if (runOn()) return undefined
    inHand = now()
    return say(false).then(() => undefined)
  })
  pi.on('before_agent_start', (_event, ctx) => {
    context = ctx ?? context
    // Idle here: this Pi marks a run only at its first request (before 1.2.0).
    if (!runOn() && idle()) late = true
    return begin('before_agent_start', ctx).then(() => undefined)
  })
  pi.on('agent_start', (_event, ctx) => begin('agent_start', ctx))
  pi.on('turn_start', (_event, ctx) => begin('turn_start', ctx))
  pi.on('message_start', observeMessage)
  pi.on('agent_settled', async (event, ctx) => {
    context = ctx ?? context
    if (revived) {
      // The run a poll took as dropped, and ended then: not ended again.
      revived = false
    } else {
      // A run whose beginning this extension did not see is still a run that ended.
      if (!runOn()) begun += 1
      end(typeof event?.aborted === 'boolean' ? event.aborted : null)
    }
    await say(false)
  })

  /**
   * Asks Pi to compact the window's conversation at this rest, once its context is past
   * the size the project chose (smaller-contexts, SCX-28 to SCX-30): Pi has no flag or
   * variable for it, and its settings are the owner's. `ctx.compact()` aborts a turn at
   * work, so it is asked only at a rest (idle, nothing pending, the run not aborted by the
   * human), and Pi's own compaction does the rest; a long turn grows meanwhile, to Pi's own
   * point at most. A size at or above the model's window, or a context Pi cannot say yet,
   * asks for none. A compaction that fails is said, and the next rest asks again.
   */
  // Whether this rest asks for a compaction.
  const wantsCompaction = (aborted, ctx) => {
    if (!Number.isInteger(compaction) || compaction <= 0 || compacting) return false
    if (aborted) return false
    if (ctx?.isIdle?.() !== true || ctx.hasPendingMessages?.() !== false) return false
    let usage
    try {
      usage = ctx.getContextUsage?.()
    } catch {
      return false
    }
    const tokens = usage?.tokens
    if (!Number.isFinite(tokens) || tokens <= compaction) return false
    return compaction < usage.contextWindow
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
      // A compaction that failed is asked for again at the next rest after a run.
      void takeCompactionWordBack().then(() => pass())
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

  // The word is taken back as the compaction this extension asked for ends, completed,
  // failed or aborted; one Pi began itself, or another extension did, never had one.
  const compactionEnded = async (_event, ctx) => {
    context = ctx ?? context
    if (compacting) await takeCompactionWordBack()
  }
  pi.on('session_compact', compactionEnded)
  pi.on('session_compact_failed', compactionEnded)
  pi.on('session_shutdown', async () => {
    shut = true
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
    // The word stays: the instance Pi starts next in this launch (`/reload`, a new session, a
    // switch) goes on from it.
    lastWord = null
    await writing
    context = undefined
  })

  return { pass }
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
