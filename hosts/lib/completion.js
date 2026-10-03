/**
 * Lossless completion extraction from each harness's native, read-only store.
 *
 * A record is read on from where the previous look at it stopped: a JSONL
 * transcript from the byte after its last whole line, OpenCode's store from
 * the conversation's last event, Devin's from its last message row and each
 * wire log from where it was left. What a look returns is what a reading of
 * the whole record would: a record that shrank or was replaced is read again
 * from its start. A final unterminated JSONL append may be incomplete;
 * malformed newline-terminated records fail closed. SQLite is read under one
 * transaction. Nothing in this module uses the bounded display normaliser.
 */
import fs from 'node:fs/promises'
import path from 'node:path'
import { devinFolders, opencodeStores, piSessionDir } from '../../src/harnesses.js'
import { codexQuota, exhaustedQuota } from './quota.js'

/** What a harness's own record of a conversation says, read whole. */
export async function answers(kind, sessionId, env, options = {}) {
  if (!sessionId) return { unknown: true, reason: 'missing session id' }
  if (env === null || typeof env !== 'object') {
    return { unknown: true, reason: 'missing explicit env argument' }
  }
  const read = recordReader(kind, sessionId, env)
  return read === null ? { unknown: true, reason: `unknown kind: ${kind}` } : read(options)
}

/**
 * `answers` for a caller that looks at the same conversations every second
 * (the delivery watcher; the live chief's transcript reached 135 MB). Each
 * conversation keeps its reader, so a look reads only what its harness wrote
 * since the last one, and a record that did not change returns the previous
 * result. Looks at one conversation take turns: each reads on from where the
 * one before stopped. Results are shared: callers must not mutate them.
 * A conversation nobody has asked about for `idleMs` (its window closed) is
 * forgotten, so a daemon that runs for weeks keeps only what it still reads.
 */
export function cachedAnswers({ idleMs = 10 * 60_000, now = Date.now } = {}) {
  const known = new Map()
  let swept = now()
  return (kind, sessionId, env, options = {}) => {
    const at = now()
    if (at - swept >= idleMs) {
      swept = at
      for (const [key, entry] of known) if (at - entry.readAt >= idleMs) known.delete(key)
    }
    const key = `${kind}\n${sessionId}`
    let entry = known.get(key)
    if (entry === undefined) {
      const read =
        sessionId && env !== null && typeof env === 'object'
          ? recordReader(kind, sessionId, env)
          : null
      if (read === null) return answers(kind, sessionId, env, options)
      entry = { read, looked: Promise.resolve() }
      known.set(key, entry)
    }
    entry.readAt = at
    const look = entry.looked.then(() => entry.read(options))
    // A look that failed must not hold up the ones after it.
    entry.looked = look.catch(() => {})
    return look
  }
}

/** Whether the harness has kept a record of the conversation at all. */
export async function hasTranscript(kind, sessionId, env) {
  return (await locateTranscript(kind, sessionId, env)) !== null
}

/**
 * A reader of one conversation's record: each call reads on from where the
 * last stopped, and never throws (an unreadable record is an unknown answer).
 * Null for a harness this module does not read.
 */
function recordReader(kind, sessionId, env) {
  switch (kind) {
    case 'codex':
      return transcriptReader(kind, sessionId, env, codexParser, `codex rollout for ${sessionId}`)
    case 'claude-code':
      return transcriptReader(kind, sessionId, env, claudeParser, `claude session ${sessionId}`)
    case 'pi':
      return piReader(sessionId, env)
    case 'opencode':
      return opencodeReader(sessionId, env)
    case 'devin':
      return devinReader(sessionId, env)
    default:
      return null
  }
}

/** Where a JSONL harness keeps one session's transcript, or null. */
async function locateTranscript(kind, sessionId, env) {
  switch (kind) {
    case 'claude-code':
      return findFile(
        path.join(env.CLAUDE_CONFIG_DIR ?? path.join(home(env), '.claude'), 'projects'),
        (name) => name === `${sessionId}.jsonl`,
      )
    case 'codex':
      return findFile(
        path.join(env.CODEX_HOME ?? path.join(home(env), '.codex'), 'sessions'),
        (name) => name.includes(sessionId),
      )
    case 'pi':
      return findFile(piSessionDir(env), (name) => name.includes(sessionId))
    default:
      return null
  }
}

const describeError = (error) => (error instanceof Error ? error.message : String(error))
const unreadable = (error) => ({ unknown: true, reason: `unreadable: ${describeError(error)}` })
const home = (env) => {
  const value = env.HOME ?? env.USERPROFILE
  if (typeof value !== 'string' || value.length === 0) throw new Error('missing home in env')
  return value
}

const ITEM_ROLES = new Set(['user', 'assistant', 'tool', 'custom'])

/**
 * What a reading says. An item's `seq` is its native position in the record
 * (a line's place, Codex's ordinal, OpenCode's event seq, Devin's row id).
 */
function resultBase() {
  return {
    items: [],
    inFlight: false,
    // Its own question dialog is open: the window waits for the human's answer.
    asking: false,
    cancelled: false,
    failed: false,
    failure: null,
    quota: null,
    settlement: { state: 'unknown' },
  }
}

/**
 * An item as a reading returns it: a copy, so that a later look, which may
 * still settle or grow the item it was read from, never changes an earlier
 * answer.
 */
function emit(item, settled = item.settled) {
  const { id, role, text, complete, at, seq } = item
  return item.commentary
    ? { id, role, text, complete, settled, at, seq, commentary: true }
    : { id, role, text, complete, settled, at, seq }
}

function nativeId(value, kind, seq) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`missing native ${kind} id at record ${seq}`)
  }
  return value
}

function visibleText(value) {
  if (typeof value === 'string') return value
  if (value === undefined || value === null) return ''
  if (Array.isArray(value)) {
    const text = value
      .map((part) => {
        if (typeof part === 'string') return part
        if (part && typeof part.text === 'string') return part.text
        return JSON.stringify(part)
      })
      .join('\n')
    return text
  }
  return JSON.stringify(value)
}

function updateNativeFragment(item, identity, text, separator) {
  if (!text) return
  if (!item._fragmentText.has(identity)) item._fragmentOrder.push(identity)
  item._fragmentText.set(identity, text)
  item.text = item._fragmentOrder.map((id) => item._fragmentText.get(id)).join(separator)
}

/** Depth-first lookup; all harness stores remain read-only. */
async function findFile(root, matches, depth = 6) {
  if (depth < 0) return null
  let entries
  try {
    entries = await fs.readdir(root, { withFileTypes: true })
  } catch {
    return null
  }
  for (const entry of entries) {
    const full = path.join(root, entry.name)
    if (entry.isFile() && matches(entry.name)) return full
    if (entry.isDirectory()) {
      const found = await findFile(full, matches, depth - 1)
      if (found !== null) return found
    }
  }
  return null
}

function isJsonWhitespace(character) {
  return character === ' ' || character === '\t' || character === '\r' || character === '\n'
}

function isJsonBlank(source) {
  for (const character of source) {
    if (!isJsonWhitespace(character)) return false
  }
  return true
}

function jsonPrefixState(source) {
  const INCOMPLETE = Symbol('incomplete')
  const INVALID = Symbol('invalid')
  let at = 0

  const skipSpace = () => {
    while (at < source.length && isJsonWhitespace(source[at])) at += 1
  }
  const need = () => {
    if (at >= source.length) throw INCOMPLETE
  }
  const parseString = () => {
    need()
    if (source[at] !== '"') throw INVALID
    at += 1
    while (at < source.length) {
      const character = source[at]
      at += 1
      if (character === '"') return
      if (character.charCodeAt(0) < 0x20) throw INVALID
      if (character !== '\\') continue
      need()
      const escapeCode = source[at]
      at += 1
      if ('"\\/bfnrt'.includes(escapeCode)) continue
      if (escapeCode !== 'u') throw INVALID
      for (let digit = 0; digit < 4; digit += 1) {
        need()
        if (!/[0-9a-f]/i.test(source[at])) throw INVALID
        at += 1
      }
    }
    throw INCOMPLETE
  }
  const parseNumber = () => {
    if (source[at] === '-') {
      at += 1
      need()
    }
    if (source[at] === '0') {
      at += 1
      if (/[0-9]/.test(source[at] ?? '')) throw INVALID
    } else if (/[1-9]/.test(source[at] ?? '')) {
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    } else {
      throw INVALID
    }
    if (source[at] === '.') {
      at += 1
      need()
      if (!/[0-9]/.test(source[at])) throw INVALID
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    }
    if (source[at] === 'e' || source[at] === 'E') {
      at += 1
      need()
      if (source[at] === '+' || source[at] === '-') {
        at += 1
        need()
      }
      if (!/[0-9]/.test(source[at])) throw INVALID
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    }
  }
  const parseLiteral = (literal) => {
    for (const expected of literal) {
      need()
      if (source[at] !== expected) throw INVALID
      at += 1
    }
  }
  const parseValue = () => {
    skipSpace()
    need()
    const character = source[at]
    if (character === '"') return parseString()
    if (character === '{') return parseObject()
    if (character === '[') return parseArray()
    if (character === 't') return parseLiteral('true')
    if (character === 'f') return parseLiteral('false')
    if (character === 'n') return parseLiteral('null')
    if (character === '-' || /[0-9]/.test(character)) return parseNumber()
    throw INVALID
  }
  const parseObject = () => {
    at += 1
    skipSpace()
    need()
    if (source[at] === '}') {
      at += 1
      return
    }
    while (true) {
      parseString()
      skipSpace()
      need()
      if (source[at] !== ':') throw INVALID
      at += 1
      parseValue()
      skipSpace()
      need()
      if (source[at] === '}') {
        at += 1
        return
      }
      if (source[at] !== ',') throw INVALID
      at += 1
      skipSpace()
      need()
    }
  }
  const parseArray = () => {
    at += 1
    skipSpace()
    need()
    if (source[at] === ']') {
      at += 1
      return
    }
    while (true) {
      parseValue()
      skipSpace()
      need()
      if (source[at] === ']') {
        at += 1
        return
      }
      if (source[at] !== ',') throw INVALID
      at += 1
      skipSpace()
      need()
    }
  }

  try {
    parseValue()
    skipSpace()
    return at === source.length ? 'complete' : 'invalid'
  } catch (error) {
    return error === INCOMPLETE ? 'incomplete' : 'invalid'
  }
}

// ================================================================= JSONL

const EMPTY = Buffer.alloc(0)
/** How many bytes before where a look stopped the next look checks are unchanged. */
const EDGE_BYTES = 1024
/** Thrown by a parser that must have its transcript's records again, from the first. */
const REREAD = Symbol('read the transcript again')

/**
 * Reads on in a JSONL file from where an earlier look stopped (`seen`; null
 * reads from the start), handing each record found to `visit` with its place
 * among the file's records, without holding the file. A line is a record
 * once its newline is written. An unterminated last line that is already
 * whole JSON is visited too, and remembered, so that the newline which ends
 * it later adds nothing; one that is not whole yet waits for a later look,
 * and one that never can be fails, as a malformed whole line does. Returns
 * where the next look starts (`seen` itself when the file did not change),
 * or null when the file is not the one read so far: another file took its
 * place, or it no longer holds the bytes just before where the last look
 * stopped (it shrank, or was written over).
 */
async function readOn(file, seen, visit) {
  const handle = await fs.open(file, 'r')
  try {
    const { ino, size, mtimeMs } = await handle.stat()
    if (seen !== null) {
      if (ino !== seen.ino) return null
      if (size === seen.size && mtimeMs === seen.mtimeMs) return seen
      if (!(await holds(handle, seen.offset - seen.edge.length, seen.edge))) return null
      if (!(await holds(handle, seen.offset, seen.tail))) return null
    }
    let offset = seen?.offset ?? 0
    let tail = seen?.tail ?? EMPTY
    let records = seen?.records ?? 0
    // The line being read, in pieces; `visited` while it is the rest of `tail`'s line.
    let pieces = []
    let visited = tail.length > 0
    let at = offset + tail.length
    if (at < size) {
      const stream = handle.createReadStream({ start: at, end: size - 1, autoClose: false })
      for await (const chunk of stream) {
        let start = 0
        for (let newline = chunk.indexOf(10); newline !== -1; newline = chunk.indexOf(10, start)) {
          pieces.push(chunk.subarray(start, newline))
          const line = pieces.length === 1 ? pieces[0] : Buffer.concat(pieces)
          pieces = []
          if (visited) {
            // A record visited whole may be followed by nothing but whitespace.
            if (!isBlankBytes(line)) return null
            visited = false
            tail = EMPTY
          } else records = consumeLine(line.toString('utf8'), records, visit)
          offset = at + newline + 1
          start = newline + 1
        }
        pieces.push(chunk.subarray(start))
        at += chunk.length
      }
    }
    const rest = Buffer.concat(pieces)
    if (visited) {
      if (!isBlankBytes(rest)) return null
    } else if (!isBlankBytes(rest)) {
      const text = rest.toString('utf8')
      let record
      try {
        record = JSON.parse(text)
      } catch {
        // A live writer may have left only the final, unterminated append.
        if (jsonPrefixState(text) !== 'incomplete') {
          throw new Error(`malformed JSONL at record ${records}`)
        }
      }
      if (record !== undefined) {
        visit(record, records)
        records += 1
        tail = rest
      }
    }
    const edge =
      offset === seen?.offset
        ? seen.edge
        : await bytesAt(handle, Math.max(0, offset - EDGE_BYTES), offset)
    return { ino, size: at, mtimeMs, offset, edge, tail, records }
  } finally {
    await handle.close()
  }
}

/** One whole line: a record for `visit` at `index`, nothing when blank, a failure when malformed. */
function consumeLine(raw, index, visit) {
  const text = raw.endsWith('\r') ? raw.slice(0, -1) : raw
  if (isJsonBlank(text)) return index
  let record
  try {
    record = JSON.parse(text)
  } catch {
    throw new Error(`malformed JSONL at record ${index}`)
  }
  visit(record, index)
  return index + 1
}

function isBlankBytes(bytes) {
  for (const byte of bytes) {
    if (byte !== 0x20 && byte !== 0x09 && byte !== 0x0d && byte !== 0x0a) return false
  }
  return true
}

async function bytesAt(handle, from, to) {
  const bytes = Buffer.alloc(to - from)
  const { bytesRead } = await handle.read(bytes, 0, bytes.length, from)
  return bytes.subarray(0, bytesRead)
}

/** Whether the file still holds `bytes` at `at`. */
async function holds(handle, at, bytes) {
  if (bytes.length === 0) return true
  return (await bytesAt(handle, at, at + bytes.length)).equals(bytes)
}

const sameFile = (left, right) =>
  left.ino === right.ino && left.size === right.size && left.mtimeMs === right.mtimeMs

/**
 * A conversation's JSONL transcript, followed from look to look. `read()`
 * locates it (again, once its file is gone), reads on from where the last
 * look stopped into the state `parser` makes, and says whether it read
 * anything; null when the harness keeps no transcript of the conversation.
 * A transcript that is not the one read so far is read again from its
 * start, into a fresh state. One that could not be read is not read again
 * until it changes.
 */
function followedTranscript(kind, sessionId, env, parser) {
  let file = null
  let seen = null
  let state = null
  let broken = null
  return {
    async read() {
      let stat = file === null ? null : await fs.stat(file).catch(() => null)
      if (stat === null) {
        const located = await locateTranscript(kind, sessionId, env)
        if (located !== file) {
          seen = null
          state = null
          broken = null
        }
        file = located
        if (file === null) return null
        stat = await fs.stat(file)
      }
      if (broken !== null && sameFile(broken.stat, stat)) throw broken.error
      broken = null
      for (;;) {
        state ??= parser(sessionId)
        let next = null
        try {
          next = await readOn(file, seen, state.visit)
          if (next !== null) state.flush?.()
        } catch (error) {
          if (error !== REREAD) {
            seen = null
            state = null
            broken = { stat, error }
            throw error
          }
        }
        if (next !== null) {
          const changed = next !== seen
          seen = next
          return { file, state, changed }
        }
        seen = null
        state = null
      }
    },
  }
}

/**
 * The reader of a harness whose answer is its transcript's alone (Codex,
 * Claude Code): a look that read nothing new returns the previous answer.
 */
function transcriptReader(kind, sessionId, env, parser, missing) {
  const transcript = followedTranscript(kind, sessionId, env, parser)
  let answer = null
  return async () => {
    try {
      const read = await transcript.read()
      if (read === null) {
        answer = null
        // A thread with no transcript yet is unknown: missing history alone proves nothing.
        return { unknown: true, reason: `unreadable: no ${missing}` }
      }
      if (read.changed || answer === null) answer = read.state.result()
      return answer
    } catch (error) {
      answer = null
      return unreadable(error)
    }
  }
}

function jsonlSeq(record, recordIndex) {
  return Number.isInteger(record.ordinal) ? record.ordinal : recordIndex
}

function contentText(content) {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .map((part) => {
      if (!part || typeof part !== 'object') return ''
      const value = part.text ?? part.Text
      return typeof value === 'string' ? value : ''
    })
    .filter(Boolean)
    .join('\n')
}

// ================================================================ codex

function codexTurn(turns, turnId) {
  if (!turnId) return null
  let turn = turns.get(turnId)
  if (!turn) {
    turn = {
      started: false,
      terminal: null,
      assistantIds: [],
      openTools: new Set(),
      openSubagents: new Set(),
    }
    turns.set(turnId, turn)
  }
  return turn
}

function codexParser(sessionId) {
  const list = []
  const items = new Map()
  const turns = new Map()
  const calls = new Map()
  const subagents = new Map()
  let currentTurnId = null
  let latestTurnId = null
  let quota = null
  let count = 0

  const addItem = (id, role, text, complete, settled, at, seq) => {
    const stableId = nativeId(id, 'codex item', seq)
    const existing = items.get(stableId)
    if (existing) {
      if (text && existing.text !== text && !existing._nativeFinalText) existing.text = text
      existing.complete ||= complete
      existing.settled ||= settled
      existing.at = at
      existing.seq = seq
      return existing
    }
    const item = { id: stableId, role, text, complete, settled, at, seq }
    items.set(stableId, item)
    list.push(item)
    return item
  }

  const visit = (record, recordIndex) => {
    count += 1
    const seq = jsonlSeq(record, recordIndex)
    const at = record.timestamp ?? seq

    if (record.type === 'response_item') {
      const payload = record.payload ?? {}
      const turnId = payload.internal_chat_message_metadata_passthrough?.turn_id ?? currentTurnId
      const turn = codexTurn(turns, turnId)
      if (turnId) {
        latestTurnId = turnId
        turn.started = true
      }

      if (payload.type === 'message' && (payload.role === 'user' || payload.role === 'assistant')) {
        const text = contentText(payload.content)
        if (!text.trim()) return
        const item = addItem(
          payload.id,
          payload.role,
          text,
          payload.role === 'user',
          payload.role === 'user',
          at,
          seq,
        )
        if (payload.role === 'assistant' && turn && !turn.assistantIds.includes(item.id)) {
          turn.assistantIds.push(item.id)
        }
        return
      }

      if (payload.type === 'function_call' || payload.type === 'custom_tool_call') {
        const callId = payload.call_id ?? payload.id
        if (callId && turn) {
          turn.openTools.add(callId)
          calls.set(callId, turnId)
        }
        return
      }

      if (payload.type === 'function_call_output' || payload.type === 'custom_tool_call_output') {
        const callId = payload.call_id
        const ownerId = calls.get(callId) ?? turnId
        codexTurn(turns, ownerId)?.openTools.delete(callId)
        addItem(payload.id, 'tool', visibleText(payload.output), true, true, at, seq)
      }
      return
    }

    if (record.type !== 'event_msg') return
    const payload = record.payload ?? {}
    const turnId = payload.turn_id ?? currentTurnId
    const turn = codexTurn(turns, turnId)

    if (payload.type === 'token_count') {
      if (payload.rate_limits) quota = codexQuota(payload.rate_limits)
      return
    }

    if (payload.type === 'task_started') {
      currentTurnId = payload.turn_id
      latestTurnId = payload.turn_id
      codexTurn(turns, payload.turn_id).started = true
      return
    }

    if (payload.type === 'task_complete') {
      latestTurnId = payload.turn_id
      codexTurn(turns, payload.turn_id).terminal = {
        kind: payload.error ? 'error' : 'complete',
        error: payload.error ?? null,
        lastAgentMessage: payload.last_agent_message,
      }
      return
    }

    if (payload.type === 'turn_aborted') {
      latestTurnId = payload.turn_id
      codexTurn(turns, payload.turn_id).terminal = { kind: 'cancelled' }
      return
    }

    if (payload.type !== 'item_completed' && payload.type !== 'item_started') return
    const native = payload.item ?? {}
    if (turnId) {
      latestTurnId = turnId
      turn.started = true
    }

    if (native.type === 'UserMessage' && payload.type === 'item_completed') {
      const text = contentText(native.content)
      if (text.trim()) addItem(native.id, 'user', text, true, true, at, seq)
      return
    }

    if (native.type === 'AgentMessage' && payload.type === 'item_completed') {
      const text = contentText(native.content)
      if (!text.trim()) return
      const item = addItem(
        native.id,
        'assistant',
        text,
        native.phase === 'final_answer',
        false,
        at,
        seq,
      )
      if (native.phase === 'final_answer') {
        item.text = text
        item._nativeFinalText = text
      }
      // Codex's progress notes ("I'll read the diff…"), marked by Codex itself.
      if (native.phase === 'commentary') item.commentary = true
      if (turn && !turn.assistantIds.includes(item.id)) turn.assistantIds.push(item.id)
      return
    }

    if (native.type === 'CommandExecution') {
      const id = native.id
      if (payload.type === 'item_started') {
        if (id && turn) turn.openTools.add(id)
        return
      }
      turn?.openTools.delete(id)
      const output =
        native.aggregated_output ??
        native.formatted_output ??
        `${native.stdout ?? ''}${native.stderr ?? ''}`
      addItem(id, 'tool', String(output), true, true, at, seq)
      return
    }

    if (native.type === 'SubAgentActivity') {
      const agentId = native.agent_thread_id ?? native.id
      if (!agentId) return
      if (native.kind === 'started') {
        turn?.openSubagents.add(agentId)
        subagents.set(agentId, turnId)
      } else if (native.kind === 'completed') {
        const ownerId = subagents.get(agentId) ?? turnId
        codexTurn(turns, ownerId)?.openSubagents.delete(agentId)
        subagents.delete(agentId)
      }
    }
  }

  const result = () => {
    if (count === 0) throw new Error(`empty codex rollout for ${sessionId}`)
    // A turn's task_complete proves the answer it names, which settles once
    // the turn's tools and sub-agents are done.
    const proven = new Set()
    const settled = new Set()
    for (const turn of turns.values()) {
      if (turn.terminal?.kind !== 'complete') continue
      const final = [...turn.assistantIds]
        .reverse()
        .map((id) => items.get(id))
        .find((item) => item?.complete && item._nativeFinalText === turn.terminal.lastAgentMessage)
      if (!final) continue
      proven.add(turn)
      if (turn.openTools.size === 0 && turn.openSubagents.size === 0) settled.add(final)
    }

    const answer = resultBase()
    answer.items = list.map((item) => emit(item, item.settled || settled.has(item)))
    const latest = latestTurnId ? turns.get(latestTurnId) : null
    const activeTurn = Boolean(latest?.started && !latest.terminal)
    answer.inFlight =
      activeTurn || (latest ? latest.openTools.size + latest.openSubagents.size > 0 : false)
    answer.cancelled = latest?.terminal?.kind === 'cancelled'
    answer.failed = latest?.terminal?.kind === 'error'
    answer.failure = answer.failed
      ? (latest.terminal.error?.message ?? visibleText(latest.terminal.error))
      : null
    answer.quota = quota
    // A task_complete whose final answer cannot be matched proves nothing.
    if (answer.inFlight) answer.settlement = { state: 'in-flight' }
    else if (latest?.terminal && (latest.terminal.kind !== 'complete' || proven.has(latest)))
      answer.settlement = { state: 'settled' }
    return answer
  }

  return { visit, result }
}

// =========================================================== claude-code

function claudeText(content) {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .filter((block) => block?.type === 'text' && typeof block.text === 'string')
    .map((block) => block.text)
    .join('\n')
}

function claudeToolText(content) {
  if (typeof content === 'string') return content
  if (Array.isArray(content) && content.every((block) => block?.type === 'text')) {
    return content.map((block) => String(block.text ?? '')).join('\n')
  }
  return visibleText(content)
}

const CLAUDE_INTERRUPT_MARKERS = new Set([
  '[Request interrupted by user]',
  '[Request interrupted by user for tool use]',
])

/**
 * Whether a queue removal names the same queued message. Claude Code wraps
 * cross-session messages in an envelope tag whose attributes differ between
 * the enqueue and the remove record (2.1.275 adds `hop-chain` to one only), so
 * the envelope's attributes are set aside after an exact match fails.
 */
function sameQueuedContent(queued, removed) {
  if (queued === removed) return true
  const envelope = (content) => content.replace(/^<([A-Za-z][\w-]*)(?:\s[^>]*)?>/, '<$1>')
  return envelope(queued) === envelope(removed)
}

function isClaudeInterrupt(record) {
  if (
    record.type !== 'user' ||
    record.message?.role !== 'user' ||
    typeof record.interruptedMessageId !== 'string' ||
    !record.interruptedMessageId
  ) {
    return false
  }
  const content = record.message?.content
  return (
    Array.isArray(content) &&
    content.length === 1 &&
    content[0]?.type === 'text' &&
    CLAUDE_INTERRUPT_MARKERS.has(content[0].text)
  )
}

function claudeParser(sessionId) {
  const list = []
  const assistants = new Map()
  const toolItems = new Map()
  const openTools = new Set()
  const queued = []
  const dequeued = []
  const popped = []
  const hooks = new Set()
  let turnOpen = false
  let candidate = null
  let terminal = null
  let activeAssistantId = null
  let cancelled = false
  let failed = false
  let failure = null
  let quota = null
  let count = 0

  const addAssistant = (id, at, seq) => {
    const stableId = nativeId(id, 'claude message', seq)
    let item = assistants.get(stableId)
    if (!item) {
      item = {
        id: stableId,
        role: 'assistant',
        text: '',
        complete: false,
        settled: false,
        at,
        seq,
        _fragmentOrder: [],
        _fragmentText: new Map(),
      }
      assistants.set(stableId, item)
      list.push(item)
    }
    item.at = at
    item.seq = seq
    return item
  }

  const addTool = (id, text, at, seq) => {
    const stableId = nativeId(id, 'claude tool result', seq)
    const existing = toolItems.get(stableId)
    if (existing) {
      existing.text = text
      existing.at = at
      existing.seq = seq
      return
    }
    const item = {
      id: stableId,
      role: 'tool',
      text,
      complete: true,
      settled: true,
      at,
      seq,
    }
    toolItems.set(stableId, item)
    list.push(item)
  }

  const queuedTurns = () => queued.length + dequeued.length + popped.length

  const candidateCanSettle = () =>
    terminal?.provenance === 'derived' &&
    openTools.size === 0 &&
    queuedTurns() === 0 &&
    hooks.size === 0

  const settleCandidate = () => {
    if (!candidateCanSettle()) return
    const item = assistants.get(terminal.itemId)
    if (item) item.settled = true
  }

  // Claude can flush a turn's user record and its ancestors after the answer
  // they started, so whether a user record is such a late ancestor is decided
  // from every record read, not only those before it. `parents` holds each
  // record's place in the conversation's tree by uuid (null when two records
  // claim one uuid); `watched` holds every uuid a decision looked up, since a
  // record read later under one of them may decide it otherwise, and then the
  // transcript is replayed from its start. Records of the latest read wait in
  // `pending` until all of them are in `parents`.
  const parents = new Map()
  const watched = new Set()
  let pending = []
  const lookup = (uuid) => {
    if (typeof uuid === 'string' && uuid) watched.add(uuid)
    return parents.get(uuid)
  }
  const lateAncestor = (user) => {
    if (terminal?.provenance !== 'derived' || terminal.itemId !== candidate?.itemId) return false
    const end = lookup(terminal.uuid)
    if (!end?.own || !end.main || end.parentUuid !== candidate.uuid) return false
    let record = lookup(candidate.uuid)
    const seen = new Set()
    while (record && !seen.has(record.uuid)) {
      if (!record.own || !record.main) return false
      if (record === user) return true
      if (record.uuid !== candidate.uuid && !record.attachment) return false
      seen.add(record.uuid)
      record = lookup(record.parentUuid)
    }
    return false
  }

  const visit = (record, index) => {
    count += 1
    let place = null
    if (typeof record.uuid === 'string' && record.uuid) {
      if (watched.has(record.uuid)) throw REREAD
      place = {
        uuid: record.uuid,
        parentUuid: record.parentUuid,
        own: record.sessionId === sessionId,
        main: record.isSidechain === false,
        attachment: record.type === 'attachment',
      }
      parents.set(record.uuid, parents.has(record.uuid) ? null : place)
    }
    pending.push({ record, place, seq: index })
  }

  const replay = (record, place, seq) => {
    const at = record.timestamp ?? seq
    if (
      record.type === 'attachment' &&
      record.sessionId === sessionId &&
      record.isSidechain === false &&
      record.attachment?.type === 'hook_additional_context' &&
      record.attachment.hookEvent === 'UserPromptSubmit' &&
      Array.isArray(record.attachment.content)
    ) {
      const text = record.attachment.content.filter((part) => typeof part === 'string').join('\n')
      if (text)
        list.push({
          id: nativeId(record.uuid, 'claude hook context', seq),
          role: 'custom',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
    }

    if (record.type === 'queue-operation') {
      if (record.operation === 'enqueue') {
        queued.push({ content: String(record.content ?? '') })
      } else if (record.operation === 'dequeue') {
        dequeued.push(queued.shift() ?? { content: '' })
      } else if (record.operation === 'popAll') {
        const content = String(record.content ?? '')
        const queuedIndex = queued.findIndex((entry) => sameQueuedContent(entry.content, content))
        popped.push(queuedIndex === -1 ? { content } : queued.splice(queuedIndex, 1)[0])
      } else if (record.operation === 'remove') {
        const content = String(record.content ?? '')
        const queuedIndex = queued.findIndex((entry) => sameQueuedContent(entry.content, content))
        if (queuedIndex !== -1) queued.splice(queuedIndex, 1)
        const dequeuedIndex = dequeued.findIndex((entry) =>
          sameQueuedContent(entry.content, content),
        )
        if (dequeuedIndex !== -1) dequeued.splice(dequeuedIndex, 1)
      }
      return
    }

    if (record.type === 'assistant') {
      const message = record.message ?? {}
      if (popped.length > 0) popped.length = 0
      if (activeAssistantId !== null && activeAssistantId !== message.id) {
        openTools.clear()
        if (candidate?.itemId !== message.id) {
          hooks.delete(candidate?.itemId)
          candidate = null
        }
      }
      activeAssistantId = message.id
      turnOpen = true
      terminal = null
      if (record.isApiErrorMessage !== true) {
        failed = false
        failure = null
      }
      // The latest assistant record has the last word on quota.
      quota = null
      const item = addAssistant(message.id, at, seq)
      const text = claudeText(message.content)
      updateNativeFragment(item, nativeId(record.uuid, 'claude record', seq), text, '\n')

      for (const block of Array.isArray(message.content) ? message.content : []) {
        if ((block?.type === 'tool_use' || block?.type === 'server_tool_use') && block.id) {
          openTools.add(block.id)
        }
        if (block?.type === 'advisor_tool_result' || block?.type === 'tool_result') {
          const toolId = block.tool_use_id
          if (!toolId) continue
          openTools.delete(toolId)
          addTool(toolId, claudeToolText(block.content), at, seq)
        }
      }

      if (record.isApiErrorMessage === true) {
        hooks.clear()
        candidate = null
        turnOpen = false
        failed = true
        failure = String(record.errorDetails ?? record.error ?? text)
        if (record.apiErrorStatus === 429 || record.error === 'rate_limit') {
          quota = exhaustedQuota(text, Date.parse(record.timestamp))
        }
        terminal = { provenance: 'native', itemId: item.id }
        return
      }

      if (message.stop_reason === 'end_turn' || message.stop_reason === 'stop_sequence') {
        candidate = { itemId: item.id, uuid: record.uuid }
        hooks.add(item.id)
      }
      return
    }

    if (record.type === 'user') {
      const content = record.message?.content
      for (const block of Array.isArray(content) ? content : []) {
        if (block?.type !== 'tool_result' || !block.tool_use_id) continue
        openTools.delete(block.tool_use_id)
        addTool(block.tool_use_id, claudeToolText(block.content), at, seq)
      }

      const text = claudeText(content)
      if (isClaudeInterrupt(record)) {
        list.push({
          id: nativeId(record.uuid, 'claude user', seq),
          role: 'user',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
        cancelled = true
        turnOpen = false
        hooks.clear()
        terminal = { provenance: 'native' }
        candidate = null
        return
      }

      if (!text.trim()) return
      list.push({
        id: nativeId(record.uuid, 'claude user', seq),
        role: 'user',
        text,
        complete: true,
        settled: true,
        at,
        seq,
      })
      if (lateAncestor(place)) return
      const poppedIndex = popped.findIndex((entry) => entry.content === text)
      if (poppedIndex !== -1) popped.splice(poppedIndex, 1)
      if (record.promptSource === 'queued' || dequeued.length > 0) {
        dequeued.shift()
      } else {
        const queuedIndex = queued.findIndex((entry) => entry.content === text)
        if (queuedIndex !== -1) queued.splice(queuedIndex, 1)
      }
      settleCandidate()
      openTools.clear()
      hooks.clear()
      activeAssistantId = null
      cancelled = false
      failed = false
      failure = null
      turnOpen = true
      candidate = null
      terminal = null
      return
    }

    const command = list.at(-1)
    if (
      record.type === 'system' &&
      record.subtype === 'local_command' &&
      record.sessionId === sessionId &&
      record.isSidechain === false &&
      record.isMeta === false &&
      record.level === 'info' &&
      record.content === '<local-command-stdout></local-command-stdout>' &&
      command?.role === 'user' &&
      record.parentUuid === command.id &&
      /^<command-name>\/clear<\/command-name>\s*<command-message>clear<\/command-message>\s*<command-args><\/command-args>$/.test(
        command.text,
      ) &&
      openTools.size === 0 &&
      hooks.size === 0
    ) {
      turnOpen = false
      candidate = null
      terminal = { provenance: 'native' }
      return
    }

    // 2.1.263/265/266's root query finalizer emits this only after query completion,
    // after stop hooks, and when not aborted. The transcript omits optional
    // background counts; candidate/tool/queue/hook guards establish readiness.
    // The exact installed call sites and native fixture are documented beside
    // tests/engine/fixtures/completion/claude-code/v263-tool-loop.jsonl.
    const durationBoundary =
      record.type === 'system' &&
      record.subtype === 'turn_duration' &&
      record.isSidechain === false &&
      Number.isFinite(record.durationMs) &&
      record.durationMs >= 0 &&
      Number.isSafeInteger(record.messageCount) &&
      record.messageCount >= 0 &&
      [record.pendingBackgroundAgentCount, record.pendingWorkflowCount].every(
        (count) => count === undefined || count === 0,
      )
    if (durationBoundary || (record.type === 'system' && record.subtype === 'stop_hook_summary')) {
      if (!candidate) return
      if (!durationBoundary && record.preventedContinuation !== false) return
      hooks.delete(candidate.itemId)
      const item = assistants.get(candidate.itemId)
      if (item) item.complete = true
      turnOpen = false
      terminal = { provenance: 'derived', itemId: candidate.itemId, uuid: record.uuid }
      settleCandidate()
    }
  }

  const flush = () => {
    const records = pending
    pending = []
    for (const { record, place, seq } of records) replay(record, place, seq)
  }

  const result = () => {
    if (count === 0) throw new Error(`empty claude session ${sessionId}`)
    let state = 'unknown'
    if (terminal?.provenance === 'native' && openTools.size === 0 && queuedTurns() === 0) {
      state = 'settled'
    } else if (candidateCanSettle()) {
      state = 'settled'
    } else if (turnOpen || openTools.size > 0 || queuedTurns() > 0 || hooks.size > 0 || terminal) {
      state = 'in-flight'
    }

    const final = state === 'settled' && terminal?.itemId ? assistants.get(terminal.itemId) : null
    const answer = resultBase()
    answer.items = list
      .map((item) => emit(item, item.settled || item === final))
      .sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
    answer.inFlight = state === 'in-flight'
    answer.cancelled = cancelled
    answer.failed = failed
    answer.failure = failure
    answer.quota = quota
    answer.settlement = { state }
    return answer
  }

  return { visit, flush, result }
}

// =================================================================== pi

function piText(content) {
  if (!Array.isArray(content)) return ''
  return content
    .filter((block) => block?.type === 'text' && typeof block.text === 'string')
    .map((block) => block.text)
    .join('\n')
}

const SAFE_PATH_SEGMENT = /^[A-Za-z0-9._-]+$/

// Pi 0.85.1 emits agent_settled only in memory after retries, compaction, and
// queued continuations (agent-session.js:772-810). The bundled extension
// persists that boundary in app-owned settled/<launchId>.json, tied to Pi's
// session id and leaf entry. A matching file is native evidence; otherwise the
// provider backoff is capped at 60 seconds (settings-manager.js:610-615), so a
// derived settlement requires twice that period without a file append.
const PI_SETTLEMENT_QUIET_MS = 120_000

async function piSettlementEvidence(sessionId, env, options) {
  const config = options?.piSettlement ?? options?.pi ?? {}
  const directory = config.directory ?? env.CF_DELIVERY_SETTLED
  const launchId = config.launchId ?? env.CF_DELIVERY_LAUNCH_ID
  if (
    typeof directory !== 'string' ||
    typeof launchId !== 'string' ||
    !SAFE_PATH_SEGMENT.test(launchId)
  ) {
    return null
  }
  try {
    const evidence = JSON.parse(await fs.readFile(path.join(directory, `${launchId}.json`), 'utf8'))
    if (
      evidence?.launchId !== launchId ||
      evidence?.sessionId !== sessionId ||
      typeof evidence?.frontier?.id !== 'string' ||
      evidence.frontier.id.length === 0
    ) {
      return null
    }
    return evidence
  } catch (cause) {
    if (cause?.code === 'ENOENT' || cause instanceof SyntaxError) return null
    throw cause
  }
}

/**
 * Pi writes no session file until an assistant message is complete
 * (session-manager.js `_persist`), so a first request that hangs leaves
 * nothing to read. ConsensFlow's extension writes `<launchId>.working.json`
 * when a turn starts and removes it when Pi settles: with it, that turn is in
 * flight, not unknown, and a window's watchdog sees it.
 */
async function piWorkingEvidence(sessionId, env, options) {
  const config = options?.piSettlement ?? options?.pi ?? {}
  const directory = config.directory ?? env.CF_DELIVERY_SETTLED
  const launchId = config.launchId ?? env.CF_DELIVERY_LAUNCH_ID
  if (
    typeof directory !== 'string' ||
    typeof launchId !== 'string' ||
    !SAFE_PATH_SEGMENT.test(launchId)
  )
    return false
  try {
    const marker = JSON.parse(
      await fs.readFile(path.join(directory, `${launchId}.working.json`), 'utf8'),
    )
    return marker?.launchId === launchId && marker?.sessionId === sessionId
  } catch (cause) {
    if (cause?.code === 'ENOENT' || cause instanceof SyntaxError) return false
    throw cause
  }
}

/**
 * Pi's reader. Its answer also rests on the extension's evidence beside the
 * session and on how long the session file has been quiet, both of which
 * change while the transcript does not, so every look works it out anew.
 */
function piReader(sessionId, env) {
  const transcript = followedTranscript('pi', sessionId, env, piParser)
  return async (options = {}) => {
    try {
      const read = await transcript.read()
      if (read === null) {
        if (!(await piWorkingEvidence(sessionId, env, options)))
          return { unknown: true, reason: `unreadable: no pi session ${sessionId}` }
        const working = resultBase()
        working.inFlight = true
        working.settlement = { state: 'in-flight' }
        return working
      }
      return await read.state.result(env, options, read.file)
    } catch (error) {
      return unreadable(error)
    }
  }
}

function piParser(sessionId) {
  const list = []
  const openTools = new Set()
  let turnOpen = false
  let terminal = null
  let cancelled = false
  let failed = false
  let failure = null
  let quota = null
  let count = 0

  const visit = (record, seq) => {
    count += 1
    const at = record.timestamp ?? record.message?.timestamp ?? seq

    if (record.type === 'custom_message') {
      const text = typeof record.content === 'string' ? record.content : piText(record.content)
      if (text)
        list.push({
          id: nativeId(record.id, 'pi custom message', seq),
          role: 'custom',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
      return
    }
    if (record.type !== 'message') return

    const message = record.message ?? {}
    const id = nativeId(record.id, 'pi message', seq)
    if (message.role === 'user') {
      const text = piText(message.content)
      if (!text.trim()) return
      // A later user turn preserves the preceding final as history. This
      // does not settle the new turn or promote unfinished tool work.
      if (terminal?.complete && openTools.size === 0) terminal.item.settled = true
      openTools.clear()
      failed = false
      failure = null
      list.push({
        id,
        role: 'user',
        text,
        complete: true,
        settled: true,
        at,
        seq,
      })
      turnOpen = true
      terminal = null
      return
    }

    if (message.role === 'toolResult') {
      if (message.toolCallId) openTools.delete(message.toolCallId)
      list.push({
        id,
        role: 'tool',
        text: piText(message.content),
        complete: true,
        settled: true,
        at,
        seq,
      })
      return
    }

    if (message.role !== 'assistant') return
    const calls = (Array.isArray(message.content) ? message.content : []).filter(
      (block) => block?.type === 'toolCall' && block.id,
    )
    for (const call of calls) openTools.add(call.id)

    const item = {
      id,
      role: 'assistant',
      text: piText(message.content),
      complete: message.stopReason === 'stop',
      settled: false,
      at,
      seq,
    }
    list.push(item)

    quota = null
    if (message.stopReason === 'stop') {
      turnOpen = true
      failed = false
      failure = null
      cancelled = false
      terminal = { complete: true, item }
    } else if (message.stopReason === 'aborted') {
      // Stopped by an Escape (a pause, a tell, the human): the turn is over,
      // not failed, and the extension's settled evidence names this message.
      turnOpen = true
      failed = false
      failure = null
      cancelled = true
      terminal = { complete: false, aborted: true, item }
    } else if (message.stopReason === 'error') {
      turnOpen = true
      failed = true
      failure = String(message.errorMessage ?? 'provider error')
      if (/^429\b/.test(failure)) {
        quota = exhaustedQuota(failure, Number(message.timestamp))
      }
      terminal = { complete: false, item }
    } else {
      turnOpen = true
      terminal = null
    }
  }

  /** The answer, with the extension's evidence and the session file's quiet as they are now. */
  const result = async (env, options, file) => {
    if (count === 0) throw new Error(`empty pi session ${sessionId}`)
    const nativeEvidence = await piSettlementEvidence(sessionId, env, options)
    const hasNativeBoundary = Boolean(
      nativeEvidence && terminal?.item?.id === nativeEvidence.frontier.id,
    )
    const { mtimeMs } = await fs.stat(file)
    const quiet = Date.now() - mtimeMs >= PI_SETTLEMENT_QUIET_MS
    const open = openTools.size > 0
    const canSettle = Boolean((terminal?.complete || terminal?.aborted) && quiet && !open)
    const nativeSettled = hasNativeBoundary && !open
    const final = canSettle || nativeSettled ? terminal.item : null

    const answer = resultBase()
    answer.items = list.map((item) => emit(item, item.settled || item === final))
    answer.inFlight = open || (terminal ? !(quiet || nativeSettled) : turnOpen)
    answer.cancelled = cancelled
    answer.failed = failed
    answer.failure = failure
    answer.quota = quota
    answer.settlement = {
      state: nativeSettled || canSettle ? 'settled' : answer.inFlight ? 'in-flight' : 'unknown',
    }
    return answer
  }

  return { visit, result }
}

// ============================================================== opencode

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
function opencodeReader(sessionId, env) {
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
      result.cancelled = false
      result.failed = false
      result.failure = null
      if (text.trim()) {
        result.items.push({
          id: row.id,
          role: 'user',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
      }
      turnOpen = true
      terminal = null
      continue
    }
    if (message.role !== 'assistant') continue

    result.cancelled = false
    result.failed = false
    result.failure = null
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
      settled: closesTurn && openTools.size === 0,
      at,
      seq,
    }
    result.items.push(item)

    for (const { row: partRow, data: part } of messageParts) {
      if (part.type !== 'tool') continue
      const status = part.state?.status
      if (status !== 'completed' && status !== 'error') continue
      const toolTime = part.state?.time?.end ?? partRow.time_updated
      result.items.push({
        id: partRow.id,
        role: 'tool',
        text: opencodeToolText(part),
        complete: true,
        settled: true,
        at: toolTime,
        seq: position(read.partPositions, partRow.id, 'part'),
      })
    }

    if (completed) result.quota = null
    if (isFailure) {
      result.failed = true
      result.failure = String(message.error?.data?.message ?? visibleText(message.error))
      if (message.error?.data?.statusCode === 429) {
        result.quota = exhaustedQuota(result.failure, Number(message.time?.completed ?? at))
      }
    }

    // The answer that closed the turn.
    terminal = closesTurn ? item : null
    turnOpen = !closesTurn
  }

  const canSettle = terminal !== null && openTools.size === 0
  if (canSettle) terminal.settled = true
  result.inFlight = turnOpen || openTools.size > 0
  result.settlement = {
    state: canSettle ? 'settled' : result.inFlight ? 'in-flight' : 'unknown',
  }

  result.items.sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
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

// ================================================================= devin

// Devin persists revisions, including cancelled assistant text. Only its main
// chain plus a matching native request/complete boundary proves a reply.
/**
 * The text a Devin message is compared by: its wire streams a file link as
 * `[name](file:///path)` and its store keeps `<ref_file file="/path" />`
 * (Devin 3000.11, 2026-09-26), so both are read as the path; nothing else is
 * loosened, since the comparison is what tells a final message from a half
 * one.
 */
function devinComparable(text) {
  const path = (value) => {
    try {
      return decodeURI(value)
    } catch {
      return value
    }
  }
  return text
    .replace(/<ref_file\s+file="([^"]*)"\s*\/>/g, (_, file) => path(file))
    .replace(/\[[^\]]*\]\(file:\/\/([^)\s]*)\)/g, (_, file) => path(file))
}

/** The wire updates that mean Devin is on a turn; settings and mode updates are not work. */
const DEVIN_WORK = new Set([
  'agent_thought_chunk',
  'agent_message_chunk',
  'tool_call',
  'tool_call_update',
])

/**
 * Devin's reader. Its store only gains message rows (a revision is a new
 * node), so a look reads the rows after the last one it saw, and the store is
 * read whole when the conversation's row count disagrees. Each launch's wire
 * log is read on from where it was left; one that shrank, was replaced or is
 * gone takes what it said with it, and every wire log is read again.
 */
function devinReader(sessionId, env) {
  let store = null
  let wires = new Map()
  let outcomes = new Map()
  let launches = []
  let answer = null
  // Each row's message, parsed once.
  const parsed = new WeakMap()

  const readStore = async () => {
    const { DatabaseSync } = await import('node:sqlite')
    const file = path.join(devinFolders(env).data, 'cli', 'sessions.db')
    const db = new DatabaseSync(file, { readOnly: true })
    try {
      db.exec('BEGIN')
      const session = db.prepare('select main_chain_id from sessions where id = ?').get(sessionId)
      if (!session) throw new Error('missing Devin session')
      const columns = 'select row_id, node_id, parent_node_id, chat_message, created_at'
      const all = () =>
        db
          .prepare(`${columns} from message_nodes where session_id = ? order by row_id`)
          .all(sessionId)
      let rows
      if (store === null) rows = all()
      else {
        rows =
          store.last === null
            ? all()
            : db
                .prepare(
                  `${columns} from message_nodes where session_id = ? and row_id > ? order by row_id`,
                )
                .all(sessionId, store.last)
        const { count } = db
          .prepare('select count(*) as count from message_nodes where session_id = ?')
          .get(sessionId)
        if (count !== store.count + rows.length) {
          store = null
          rows = all()
        }
      }
      const fresh = store === null
      store ??= { nodes: new Map(), count: 0, last: null, head: undefined, chain: null }
      for (const row of rows) {
        store.nodes.set(row.node_id, row)
        store.count += 1
        store.last = row.row_id
      }
      const changed = fresh || rows.length > 0 || session.main_chain_id !== store.head
      if (changed) {
        store.head = session.main_chain_id
        store.chain = devinChain(store.nodes, store.head, parsed)
      }
      db.exec('COMMIT')
      return changed
    } finally {
      db.close()
    }
  }

  // A turn Devin is still on shows only on the wire: thoughts, messages and
  // tool calls after the last end; its store holds the finished steps.
  const visitWire = (wire, event) => {
    if (event.sessionId !== sessionId) return
    const update = event.update
    if (DEVIN_WORK.has(update?.sessionUpdate)) wire.busy = true
    if (update?.sessionUpdate === 'agent_message_chunk') {
      const id = update._meta?.['cognition.ai/streamingMessageId']
      // History replay has timestamps but no streaming UUID.
      if (typeof id !== 'string' || update.content?.type !== 'text') return
      if (wire.active?.id !== id) wire.active = { id, text: '', request: null }
      wire.active.text += update.content.text
    }
    if (wire.active && typeof event.turnClientMessageId === 'string')
      wire.active.request = event.turnClientMessageId
    if (['complete', 'cancelled', 'error'].includes(event.cause)) {
      if (wire.active?.request) {
        const outcome = { ...wire.active, cause: event.cause }
        const previous = outcomes.get(wire.active.request)
        if (previous && (previous.text !== outcome.text || previous.cause !== outcome.cause))
          throw new Error('conflicting Devin completion evidence')
        outcomes.set(wire.active.request, outcome)
      }
      wire.active = null
      wire.busy = false
    }
  }

  const readWires = async () => {
    const root = path.join(
      env.CONSENSFLOW_HOME ?? path.join(home(env), '.consensflow'),
      'integrations',
      'devin',
    )
    launches = []
    try {
      launches = (await fs.readdir(root, { withFileTypes: true }))
        .filter((entry) => entry.isDirectory())
        .map((entry) => entry.name)
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
    for (;;) {
      let changed = false
      let whole = true
      const present = new Set()
      for (const launch of launches) {
        const file = path.join(root, launch, 'wire.jsonl')
        const wire = wires.get(launch) ?? { seen: null, active: null, busy: false }
        let next
        try {
          next = await readOn(file, wire.seen, (event) => visitWire(wire, event))
        } catch (error) {
          if (error.code !== 'ENOENT') throw error
          continue
        }
        if (next === null) {
          whole = false
          break
        }
        changed ||= next !== wire.seen
        wire.seen = next
        wires.set(launch, wire)
        present.add(launch)
      }
      if (whole && [...wires.keys()].every((launch) => present.has(launch))) return changed
      wires = new Map()
      outcomes = new Map()
    }
  }

  return async () => {
    try {
      const stored = await readStore()
      const wired = await readWires()
      if (!stored && !wired && answer !== null) return answer
      answer = devinAnswer(store.chain, outcomes, launches, wires)
      return answer
    } catch (error) {
      store = null
      wires = new Map()
      outcomes = new Map()
      answer = null
      return unreadable(error)
    }
  }
}

/**
 * The messages on Devin's main chain, the one ending at `head`, oldest first,
 * and whether its question tool waits for an answer.
 */
function devinChain(nodes, head, parsed) {
  const chain = []
  const visited = new Set()
  let node = head
  while (node !== null) {
    if (visited.has(node)) throw new Error('cyclic Devin main chain')
    visited.add(node)
    const row = nodes.get(node)
    if (!row) throw new Error('missing Devin main chain ancestor')
    chain.push(row)
    node = row.parent_node_id
  }
  const items = []
  const ids = new Set()
  let request = null
  // Its question tool's call, until a tool message answers it.
  let asking = null
  for (const row of chain.reverse()) {
    if (!parsed.has(row)) parsed.set(row, JSON.parse(row.chat_message))
    const message = parsed.get(row)
    const call = message.tool_calls?.find((c) => c.name === 'ask_user_question')
    if (message.role === 'assistant' && call) asking = call.id
    else if (message.role === 'tool' && message.tool_call_id === asking) asking = null
    if (typeof message.message_id !== 'string' || ids.has(message.message_id))
      throw new Error('invalid Devin message identity')
    ids.add(message.message_id)
    const role = message.role === 'system' ? 'custom' : message.role
    if (role === 'user')
      request = message.metadata?.extensions?.['chisel/client-message-id'] ?? message.message_id
    if (!ITEM_ROLES.has(role)) throw new Error('unknown Devin message role')
    const text =
      typeof message.content === 'string'
        ? message.content
        : Array.isArray(message.content)
          ? message.content
              .filter((part) => part.type === 'text')
              .map((part) => part.text)
              .join('')
          : ''
    items.push({
      id: message.message_id,
      role,
      text,
      complete: role !== 'assistant',
      settled: role !== 'assistant',
      at: message.metadata?.created_at ?? row.created_at,
      seq: Number(row.row_id),
      request,
    })
  }
  return { items, asking: asking !== null }
}

/**
 * What Devin's chain says, by the wire's outcomes: a reply is complete only
 * when it is its request's last, the wire saw that request complete, and the
 * streamed text is the stored one. Whether Devin is still on a turn is judged
 * by the launch whose wire was written last (a resume opens a new one).
 */
function devinAnswer(chain, outcomes, launches, wires) {
  let working = false
  let latestWire = -1
  for (const launch of launches) {
    const wire = wires.get(launch)
    if (wire !== undefined && wire.seen.mtimeMs >= latestWire) {
      latestWire = wire.seen.mtimeMs
      working = wire.busy
    }
  }
  const result = resultBase()
  result.asking = chain.asking
  const finalByRequest = new Map(
    chain.items.filter((item) => item.role === 'assistant').map((item) => [item.request, item.id]),
  )
  result.items = chain.items.map(({ request, ...item }) => {
    if (item.role !== 'assistant') return item
    const outcome = outcomes.get(request)
    // The same text needs no comparing, which every look would do again.
    const complete =
      finalByRequest.get(request) === item.id &&
      outcome?.cause === 'complete' &&
      (outcome.text === item.text || devinComparable(outcome.text) === devinComparable(item.text))
    return { ...item, complete, settled: complete }
  })
  const lastIndex = chain.items.findLastIndex((item) => item.role !== 'custom')
  const last = lastIndex === -1 ? undefined : result.items[lastIndex]
  const outcome = outcomes.get(chain.items[lastIndex]?.request)
  result.cancelled = outcome?.cause === 'cancelled'
  result.failed = outcome?.cause === 'error'
  result.inFlight =
    working || (last?.role === 'assistant' && !last.complete && !result.cancelled && !result.failed)
  result.settlement = {
    state: working
      ? 'in-flight'
      : last?.complete || result.cancelled || result.failed
        ? 'settled'
        : 'unknown',
  }
  return result
}
