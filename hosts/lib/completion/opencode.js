/** OpenCode's record of a session: its SQLite store, read on from the session's last event. */
import fs from 'node:fs/promises'
import { opencodeStores } from '../../../src/harnesses.js'
import { exhaustedQuota, quotaStatus } from '../quota.js'
import { emit, resultBase, unreadable, visibleText } from './shared.js'

function parseStoredJson(raw, description) {
  try {
    return JSON.parse(raw)
  } catch {
    throw new Error(`malformed OpenCode ${description}`)
  }
}

function opencodeToolText(part) {
  if (part.state?.output !== undefined) return visibleText(part.state.output)
  if (part.state?.error !== undefined) return visibleText(part.state.error)
  return ''
}

/** The event types a look reads on past; any other has the store read whole. */
const OPENCODE_EVENTS = new Set([
  'message.updated.1',
  'message.part.updated.1',
  'session.created.1',
  'session.updated.1',
])

/**
 * OpenCode's reader. OpenCode writes each change to a message or a part as
 * an event of the conversation, numbered in order, in the same transaction
 * as the row (a 1.1 GB store of 1.18.33 and 1.18.34, checked on 2026-10-03:
 * every message and part row is its latest event's data). So a look reads
 * the events after the last one it saw, and again only the rows they name.
 * The conversation's message, part or event count disagreeing with what
 * was read (a row removed, an event written out of order), or an event of a
 * type not followed here, has the store read whole.
 */
export function opencodeReader(sessionId, env) {
  let store = null
  let read = null
  let answer = null
  // Each row's data, parsed once; a row read again is a new object.
  const parsed = new WeakMap()
  const data = (row, description) => {
    if (!parsed.has(row)) parsed.set(row, parseStoredJson(row.data, description))
    return parsed.get(row)
  }

  /**
   * Reads on through the conversation's events in `rows`: the messages and
   * parts they name, and whether each is of a type followed here.
   */
  const readEvents = (rows) => {
    const named = { messages: new Set(), parts: new Set(), followed: true }
    for (const row of rows) {
      const seq = Number(row.seq)
      if (!Number.isInteger(seq) || seq < 0 || seq <= read.last) {
        throw new Error(`malformed OpenCode event sequence at ${String(row.seq)}`)
      }
      read.last = seq
      read.events += 1
      const event = parseStoredJson(row.data, `event ${row.id}`)
      if (row.type === 'message.updated.1' && event.info?.id) {
        named.messages.add(event.info.id)
        if (!read.messagePositions.has(event.info.id)) read.messagePositions.set(event.info.id, seq)
        if (
          event.info.time?.completed !== undefined &&
          event.info.time?.completed !== null &&
          !read.completionPositions.has(event.info.id)
        ) {
          read.completionPositions.set(event.info.id, seq)
        }
      } else if (row.type === 'message.part.updated.1' && event.part?.id) {
        named.parts.add(event.part.id)
        read.partPositions.set(event.part.id, seq)
      } else if (!OPENCODE_EVENTS.has(row.type)) named.followed = false
    }
    if (read.last < 0) throw new Error(`missing OpenCode event sequence for ${sessionId}`)
    return named
  }

  const readWhole = async (db, options) => {
    read = {
      last: -1,
      events: 0,
      messages: new Map(),
      parts: new Map(),
      messagePositions: new Map(),
      completionPositions: new Map(),
      partPositions: new Map(),
    }
    for (const row of db
      .prepare('select * from message where session_id = ? order by time_created, id')
      .all(sessionId))
      read.messages.set(row.id, row)
    await options.betweenOpenCodeSnapshotReads?.()
    for (const row of db
      .prepare('select * from part where session_id = ? order by time_created, id')
      .all(sessionId))
      read.parts.set(row.id, row)
    readEvents(db.prepare('select * from event where aggregate_id = ? order by seq').all(sessionId))
  }

  /**
   * Reads on from the last event seen, and the rows the events after it
   * name: whether the store must be read whole instead, and whether anything
   * changed.
   */
  const readOnward = (db) => {
    const before = read.events
    const named = readEvents(
      db
        .prepare('select * from event where aggregate_id = ? and seq > ? order by seq')
        .all(sessionId, read.last),
    )
    if (!named.followed) return { whole: true }
    const message = db.prepare('select * from message where id = ? and session_id = ?')
    for (const id of named.messages) {
      const row = message.get(id, sessionId)
      if (row === undefined) read.messages.delete(id)
      else read.messages.set(id, row)
    }
    const part = db.prepare('select * from part where id = ? and session_id = ?')
    for (const id of named.parts) {
      const row = part.get(id, sessionId)
      if (row === undefined) read.parts.delete(id)
      else read.parts.set(id, row)
    }
    const counts = db
      .prepare(
        `select (select count(*) from message where session_id = ?) as messages,
          (select count(*) from part where session_id = ?) as parts,
          (select count(*) from event where aggregate_id = ?) as events`,
      )
      .get(sessionId, sessionId, sessionId)
    const whole =
      counts.messages !== read.messages.size ||
      counts.parts !== read.parts.size ||
      counts.events !== read.events
    return { whole, changed: read.events > before }
  }

  return async (options = {}) => {
    let db = null
    let transaction = false
    try {
      const opened = await openOpencodeDb(env, sessionId)
      if (opened === null) {
        return { unknown: true, reason: `unreadable: no opencode store for ${sessionId}` }
      }
      db = opened.db
      if (opened.store !== store) {
        store = opened.store
        read = null
      }
      db.exec('BEGIN')
      transaction = true
      if (db.prepare('select 1 from session where id = ?').get(sessionId) === undefined) {
        read = null
        return { unknown: true, reason: `unreadable: no opencode session ${sessionId}` }
      }
      let changed = true
      if (read === null) await readWhole(db, options)
      else {
        const onward = readOnward(db)
        if (onward.whole) await readWhole(db, options)
        else changed = onward.changed
      }
      if (!changed && answer !== null) return answer
      answer = opencodeAnswer(read, data)
      return answer
    } catch (error) {
      read = null
      answer = null
      return unreadable(error)
    } finally {
      if (transaction) {
        try {
          db.exec('ROLLBACK')
        } catch {
          // The read transaction may already have been closed by SQLite.
        }
      }
      try {
        db?.close()
      } catch {
        // Closing a failed read-only open must not mask the original result.
      }
    }
  }
}

const byCreation = (left, right) =>
  left.time_created - right.time_created || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0)

/** What OpenCode's rows and events, as read so far, say. */
function opencodeAnswer(read, data) {
  const result = resultBase()
  const position = (positions, id, description) => {
    const seq = positions.get(id)
    if (!Number.isInteger(seq)) throw new Error(`missing OpenCode event for ${description} ${id}`)
    return seq
  }

  const partsByMessage = new Map()
  for (const row of [...read.parts.values()].sort(byCreation)) {
    const entry = { row, data: data(row, `part ${row.id}`) }
    const list = partsByMessage.get(row.message_id) ?? []
    list.push(entry)
    partsByMessage.set(row.message_id, list)
  }
  for (const list of partsByMessage.values()) {
    list.sort(
      (left, right) =>
        position(read.partPositions, left.row.id, 'part') -
          position(read.partPositions, right.row.id, 'part') ||
        left.row.id.localeCompare(right.row.id),
    )
  }
  const messages = [...read.messages.values()].sort(byCreation)
  messages.sort(
    (left, right) =>
      position(read.messagePositions, left.id, 'message') -
        position(read.messagePositions, right.id, 'message') || left.id.localeCompare(right.id),
  )

  const items = []
  const openTools = new Set()
  let turnOpen = false
  let terminal = null
  for (const row of messages) {
    const message = data(row, `message ${row.id}`)
    const messageParts = partsByMessage.get(row.id) ?? []
    const text = messageParts
      .filter(({ data: part }) => part.type === 'text' && typeof part.text === 'string')
      .map(({ data: part }) => part.text)
      .join('\n')
    const messageSeq = position(read.messagePositions, row.id, 'message')
    const textPositions = messageParts
      .filter(({ data: part }) => part.type === 'text')
      .map(({ row: partRow }) => position(read.partPositions, partRow.id, 'part'))
    const at = message.time?.completed ?? message.time?.created ?? row.time_created
    const completed = message.time?.completed !== undefined && message.time?.completed !== null
    const completionSeq = completed
      ? position(read.completionPositions, row.id, 'message completion')
      : null
    const seq =
      textPositions.length > 0
        ? Math.max(messageSeq, ...textPositions)
        : (completionSeq ?? messageSeq)

    if (message.role === 'user') {
      openTools.clear()
      result.failed = false
      if (text.trim()) {
        items.push({
          id: row.id,
          role: 'user',
          text,
          complete: true,
          at,
          seq,
        })
      }
      turnOpen = true
      terminal = null
      continue
    }
    if (message.role !== 'assistant') continue

    result.failed = false
    for (const { row: partRow, data: part } of messageParts) {
      if (part.type !== 'tool') continue
      const status = part.state?.status
      const toolId = part.callID ?? partRow.id
      const done = status === 'completed' || status === 'error'
      if (done) openTools.delete(toolId)
      else openTools.add(toolId)
      if (part.tool === 'question') result.asking = !done
    }

    const errorName = message.error?.name
    const isFailure = completed && Boolean(errorName)
    // No supported-version native fixture establishes OpenCode cancellation.
    // In particular, MessageAbortedError remains a failure, never cancellation.
    const closesTurn =
      completed && (message.finish === 'stop' || message.finish === 'length' || isFailure)
    const complete = completed && message.finish === 'stop' && !errorName
    const item = {
      id: row.id,
      role: 'assistant',
      text,
      complete,
      at,
      seq,
    }
    items.push(item)

    for (const { row: partRow, data: part } of messageParts) {
      if (part.type !== 'tool') continue
      const status = part.state?.status
      if (status !== 'completed' && status !== 'error') continue
      const toolTime = part.state?.time?.end ?? partRow.time_updated
      items.push({
        id: partRow.id,
        role: 'tool',
        text: opencodeToolText(part),
        complete: true,
        at: toolTime,
        seq: position(read.partPositions, partRow.id, 'part'),
      })
    }

    if (completed) result.quota = null
    if (isFailure) {
      result.failed = true
      const failure = String(message.error?.data?.message ?? visibleText(message.error))
      if (quotaStatus(message.error?.data?.statusCode)) {
        result.quota = exhaustedQuota(failure, Number(message.time?.completed ?? at))
      }
    }

    // The answer that closed the turn.
    terminal = closesTurn ? item : null
    turnOpen = !closesTurn
  }

  const canSettle = terminal !== null && openTools.size === 0
  result.inFlight = turnOpen || openTools.size > 0
  result.settlement = {
    state: canSettle ? 'settled' : result.inFlight ? 'in-flight' : 'unknown',
  }

  // In the record's order: an item's `seq` is its event's seq.
  result.items = items
    .sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
    .map(emit)
  return result
}

/**
 * OpenCode's store, read-only: the one of its places that holds `sessionId`,
 * else the first there is (whose answer is then that the session is not in
 * it yet), else null. `store` tells the file read from one that later takes
 * its place.
 */
async function openOpencodeDb(env, sessionId) {
  let sqlite
  try {
    sqlite = await import('node:sqlite')
  } catch {
    return null
  }
  let first = null
  for (const file of opencodeStores(env)) {
    let db
    let store
    try {
      store = `${file}\n${(await fs.stat(file)).ino}`
      db = new sqlite.DatabaseSync(file, { readOnly: true })
    } catch {
      continue
    }
    let holds = false
    try {
      holds = db.prepare('select 1 from session where id = ?').get(sessionId) !== undefined
    } catch {
      // A store that cannot be read as OpenCode's holds nothing of ours.
    }
    if (holds) {
      first?.db.close()
      return { db, store }
    }
    if (first === null) first = { db, store }
    else db.close()
  }
  return first
}
