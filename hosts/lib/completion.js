/**
 * Lossless completion extraction from each harness's native, read-only store.
 *
 * JSONL is consumed a record at a time. A final unterminated append may be
 * incomplete; malformed newline-terminated records fail closed. SQLite is
 * read under one transaction. Nothing in this module uses the bounded display
 * normaliser.
 */
import { createReadStream } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { piSessionDir } from '../../src/harnesses.js'

const SUPPORTED = {
  kimi: new Set(['1.5']),
}

class UnsupportedVersionError extends Error {}

export async function answers(kind, sessionId, env, options = {}) {
  if (!sessionId) return { unknown: true, reason: 'missing session id' }
  if (env === null || typeof env !== 'object') {
    return { unknown: true, reason: 'missing explicit env argument' }
  }
  try {
    switch (kind) {
      case 'codex':
        return await codexAnswers(sessionId, env, options)
      case 'claude-code':
        return await claudeAnswers(sessionId, env)
      case 'pi':
        return await piAnswers(sessionId, env, options)
      case 'kimi':
        return await kimiAnswers(sessionId, env)
      case 'opencode':
        return await opencodeAnswers(sessionId, env, options)
      default:
        return { unknown: true, reason: `unknown kind: ${kind}` }
    }
  } catch (error) {
    if (error instanceof UnsupportedVersionError) {
      return { unknown: true, reason: error.message }
    }
    return { unknown: true, reason: `unreadable: ${describeError(error)}` }
  }
}

const describeError = (error) => (error instanceof Error ? error.message : String(error))
const home = (env) => {
  const value = env.HOME ?? env.USERPROFILE
  if (typeof value !== 'string' || value.length === 0) throw new Error('missing home in env')
  return value
}

const CURSOR_NAMESPACE = 4_000_000_000_000_000
const CURSOR_KIND_SPAN = 100_000_000_000
const cursorKindCodes = Object.freeze(
  Object.assign(Object.create(null), {
    codex: 1,
    'claude-code': 2,
    pi: 3,
    kimi: 4,
    opencode: 5,
  }),
)
const ITEM_ROLES = new Set(['user', 'assistant', 'tool', 'custom'])

/**
 * Return items strictly after an adapter-minted cursor. An empty array is a
 * verified empty suffix; null means the items or cursor could not be parsed by
 * that adapter. Cursors are opaque to consumers even though their current wire
 * representation is a safe integer; only this adapter decodes or compares it.
 */
export function itemsAfterCursor(kind, items, cursor) {
  if (!Array.isArray(items)) return null
  for (const item of items) {
    if (!isNormalisedItem(kind, item)) return null
  }
  if (cursorPosition(kind, cursor) === null) return null
  const after = []
  for (const item of items) {
    if (compareCursors(kind, item.seq, cursor) > 0) after.push(item)
  }
  return after
}

function isNormalisedItem(kind, item) {
  if (item === null || typeof item !== 'object' || Array.isArray(item)) return false
  for (const field of ['id', 'role', 'text', 'complete', 'settled', 'at', 'seq']) {
    if (!Object.hasOwn(item, field)) return false
  }
  return (
    typeof item.id === 'string' &&
    item.id.length > 0 &&
    ITEM_ROLES.has(item.role) &&
    typeof item.text === 'string' &&
    typeof item.complete === 'boolean' &&
    typeof item.settled === 'boolean' &&
    item.at !== null &&
    item.at !== undefined &&
    cursorPosition(kind, item.seq) !== null
  )
}

function cursorKindCode(kind) {
  if (typeof kind !== 'string' || !Object.hasOwn(cursorKindCodes, kind)) return null
  return cursorKindCodes[kind]
}

function mintCursor(kind, position) {
  const code = cursorKindCode(kind)
  if (
    code === null ||
    !Number.isSafeInteger(position) ||
    position < 0 ||
    position >= CURSOR_KIND_SPAN
  ) {
    throw new Error(`invalid ${kind} native cursor position`)
  }
  return CURSOR_NAMESPACE + code * CURSOR_KIND_SPAN + position
}

function cursorPosition(kind, cursor) {
  const code = cursorKindCode(kind)
  if (code === null || !Number.isSafeInteger(cursor)) return null
  const start = CURSOR_NAMESPACE + code * CURSOR_KIND_SPAN
  if (cursor < start || cursor >= start + CURSOR_KIND_SPAN) return null
  return cursor - start
}

function compareCursors(kind, left, right) {
  const leftPosition = cursorPosition(kind, left)
  const rightPosition = cursorPosition(kind, right)
  if (leftPosition === null || rightPosition === null) throw new Error('unrecognised cursor')
  return leftPosition - rightPosition
}

function checkedVersion(kind, value) {
  const version = value === undefined || value === null ? 'missing' : String(value)
  if (!SUPPORTED[kind].has(version)) {
    throw new UnsupportedVersionError(`unsupported version ${version} for ${kind}`)
  }
  return version
}

function resultBase() {
  return {
    items: [],
    inFlight: false,
    cancelled: false,
    replaced: false,
    failed: false,
    failure: null,
    cursor: null,
    settlement: {
      state: 'unknown',
      provenance: 'unknown',
      cursor: null,
      boundary: null,
      evidence: {
        complete: false,
        openTools: [],
        queuedTurns: [],
        hooksInFlight: [],
      },
    },
  }
}

function boundary(record, seq, at, extra = {}) {
  return { record, cursor: seq, at, ...extra }
}

function setSettlement(result, state, provenance, nativeBoundary, cursor, evidence) {
  result.settlement = {
    state,
    provenance,
    cursor,
    boundary: nativeBoundary?.record ?? null,
    evidence: {
      complete: Boolean(evidence.complete),
      openTools: uniqueSorted(evidence.openTools),
      queuedTurns: uniqueSorted(evidence.queuedTurns),
      hooksInFlight: uniqueSorted(evidence.hooksInFlight),
    },
  }
}

function uniqueSorted(values) {
  return [...new Set(values)].sort()
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

async function findDir(root, name, depth = 4) {
  if (depth < 0) return null
  let entries
  try {
    entries = await fs.readdir(root, { withFileTypes: true })
  } catch {
    return null
  }
  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    if (entry.name === name) return path.join(root, entry.name)
    const found = await findDir(path.join(root, entry.name), name, depth - 1)
    if (found !== null) return found
  }
  return null
}

/**
 * Stream JSONL without retaining the file. A syntactically incomplete tail
 * without a newline is the only malformed record tolerated.
 */
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

async function readJsonl(file, visit) {
  const stream = createReadStream(file, { encoding: 'utf8' })
  let buffer = ''
  let recordIndex = 0
  let count = 0

  const consume = (raw) => {
    const text = raw.endsWith('\r') ? raw.slice(0, -1) : raw
    if (isJsonBlank(text)) return
    let record
    try {
      record = JSON.parse(text)
    } catch {
      throw new Error(`malformed JSONL at record ${recordIndex}`)
    }
    visit(record, recordIndex)
    recordIndex += 1
    count += 1
  }

  for await (const chunk of stream) {
    buffer += chunk
    let newline = buffer.indexOf('\n')
    while (newline !== -1) {
      const line = buffer.slice(0, newline)
      buffer = buffer.slice(newline + 1)
      consume(line)
      newline = buffer.indexOf('\n')
    }
  }

  if (!isJsonBlank(buffer)) {
    let record
    try {
      record = JSON.parse(buffer)
    } catch {
      if (jsonPrefixState(buffer) === 'incomplete') {
        // A live writer may have left only the final, unterminated append.
        return count
      }
      throw new Error(`malformed JSONL at record ${recordIndex}`)
    }
    visit(record, recordIndex)
    count += 1
  }
  return count
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

async function codexAnswers(sessionId, env, options) {
  const root = path.join(env.CODEX_HOME ?? path.join(home(env), '.codex'), 'sessions')
  const file = await findFile(root, (name) => name.includes(sessionId))
  if (file === null) {
    // Only a current authenticated native observation proves an empty thread.
    // The adapter owns its initial cursor; missing history alone proves nothing.
    if (options.codexSession?.sessionId === sessionId && options.codexSession.empty === true) {
      const result = resultBase()
      result.cursor = mintCursor('codex', 0)
      result.settlement = {
        ...result.settlement,
        state: 'settled',
        provenance: 'native',
        cursor: result.cursor,
        boundary: 'thread/started-empty',
      }
      return result
    }
    return { unknown: true, reason: `unreadable: no codex rollout for ${sessionId}` }
  }

  const result = resultBase()
  const items = new Map()
  const turns = new Map()
  const calls = new Map()
  const subagents = new Map()
  let currentTurnId = null
  let latestTurnId = null

  const addItem = (id, role, text, complete, settled, at, seq, turnId) => {
    const stableId = nativeId(id, 'codex item', seq)
    const existing = items.get(stableId)
    if (existing) {
      if (text && existing.text !== text && !existing._nativeFinalText) existing.text = text
      existing.complete ||= complete
      existing.settled ||= settled
      existing.at = at
      existing.seq = seq
      if (turnId) existing._turnId = turnId
      return existing
    }
    const item = { id: stableId, role, text, complete, settled, at, seq, _turnId: turnId }
    items.set(stableId, item)
    result.items.push(item)
    return item
  }

  const count = await readJsonl(file, (record, recordIndex) => {
    const nativeSeq = jsonlSeq(record, recordIndex)
    const seq = mintCursor('codex', nativeSeq)
    const at = record.timestamp ?? nativeSeq
    result.cursor = seq

    if (record.type === 'session_meta') {
      const meta = record.payload ?? {}
      const own = meta.id ?? meta.session_id
      if ((own && own !== sessionId) || meta.forked_from_id) result.replaced = true
      return
    }

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
          turnId,
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
        addItem(payload.id, 'tool', visibleText(payload.output), true, true, at, seq, ownerId)
      }
      return
    }

    if (record.type !== 'event_msg') return
    const payload = record.payload ?? {}
    const turnId = payload.turn_id ?? currentTurnId
    const turn = codexTurn(turns, turnId)

    if (payload.type === 'task_started') {
      currentTurnId = payload.turn_id
      latestTurnId = payload.turn_id
      codexTurn(turns, payload.turn_id).started = true
      return
    }

    if (payload.type === 'task_complete') {
      latestTurnId = payload.turn_id
      const owner = codexTurn(turns, payload.turn_id)
      owner.terminal = {
        kind: payload.error ? 'error' : 'complete',
        error: payload.error ?? null,
        lastAgentMessage: payload.last_agent_message,
        boundary: boundary('task_complete', seq, at, { turnId: payload.turn_id }),
      }
      if (payload.error) {
        result.failed = true
        result.failure = payload.error.message ?? visibleText(payload.error)
      }
      return
    }

    if (payload.type === 'turn_aborted') {
      latestTurnId = payload.turn_id
      const owner = codexTurn(turns, payload.turn_id)
      owner.terminal = {
        kind: 'cancelled',
        boundary: boundary('turn_aborted', seq, at, {
          turnId: payload.turn_id,
          reason: payload.reason ?? null,
        }),
      }
      result.cancelled = true
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
      if (text.trim()) addItem(native.id, 'user', text, true, true, at, seq, turnId)
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
        turnId,
      )
      if (native.phase === 'final_answer') {
        item.text = text
        item._nativeFinalText = text
      }
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
      addItem(id, 'tool', String(output), true, true, at, seq, turnId)
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
  })

  if (count === 0) throw new Error(`empty codex rollout for ${sessionId}`)

  for (const turn of turns.values()) {
    if (turn.terminal?.kind !== 'complete') continue
    const final = [...turn.assistantIds]
      .reverse()
      .map((id) => items.get(id))
      .find((item) => item?.complete && item._nativeFinalText === turn.terminal.lastAgentMessage)
    turn.final = final ?? null
    turn.validComplete = Boolean(final)
    if (turn.validComplete && turn.openTools.size === 0 && turn.openSubagents.size === 0) {
      final.settled = true
    }
  }

  const latest = latestTurnId ? turns.get(latestTurnId) : null
  const openTools = latest
    ? [...latest.openTools, ...[...latest.openSubagents].map((id) => `subagent:${id}`)]
    : []
  const activeTurn = Boolean(latest?.started && !latest.terminal)
  result.inFlight = activeTurn || openTools.length > 0

  result.cancelled = latest?.terminal?.kind === 'cancelled'
  result.failed = latest?.terminal?.kind === 'error'
  result.failure = result.failed
    ? (latest.terminal.error?.message ?? visibleText(latest.terminal.error))
    : null
  const complete = Boolean(latest?.validComplete)
  const nativeBoundary = latest?.terminal?.boundary ?? null
  if (latest?.terminal && !result.inFlight) {
    if (latest.terminal.kind === 'complete' && !latest.validComplete) {
      setSettlement(result, 'unknown', 'native', nativeBoundary, null, {
        complete: false,
        openTools,
        queuedTurns: [],
        hooksInFlight: [],
      })
    } else {
      setSettlement(result, 'settled', 'native', nativeBoundary, nativeBoundary.cursor, {
        complete,
        openTools,
        queuedTurns: [],
        hooksInFlight: [],
      })
    }
  } else if (result.inFlight) {
    setSettlement(result, 'in-flight', 'native', nativeBoundary, null, {
      complete,
      openTools,
      queuedTurns: [],
      hooksInFlight: [],
    })
  }

  for (const item of result.items) {
    delete item._nativeFinalText
    delete item._turnId
  }
  return result
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

async function claudeAnswers(sessionId, env) {
  const root = path.join(env.CLAUDE_CONFIG_DIR ?? path.join(home(env), '.claude'), 'projects')
  const file = await findFile(root, (name) => name === `${sessionId}.jsonl`)
  if (file === null) {
    return { unknown: true, reason: `unreadable: no claude session ${sessionId}` }
  }

  const result = resultBase()
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
      result.items.push(item)
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
    result.items.push(item)
  }

  const queueEvidence = () => [
    ...queued.map((entry) => entry.id),
    ...dequeued.map((entry) => entry.id),
    ...popped.map((entry) => entry.id),
  ]

  const candidateCanSettle = () =>
    terminal?.provenance === 'derived' &&
    terminal.complete &&
    openTools.size === 0 &&
    queueEvidence().length === 0 &&
    hooks.size === 0

  const settleCandidate = () => {
    if (!candidateCanSettle()) return false
    const item = assistants.get(terminal.itemId)
    if (item) item.settled = true
    return true
  }

  // A single snapshot resolves ancestors flushed after their completed answer.
  // Replay still uses physical positions: ancestry must never mint fresh delivery cursors.
  const records = []
  const parents = new Map()
  const count = await readJsonl(file, (record) => {
    records.push(record)
    if (typeof record.uuid === 'string' && record.uuid) {
      parents.set(record.uuid, parents.has(record.uuid) ? null : record)
    }
  })
  const lateAncestor = (user) => {
    if (terminal?.provenance !== 'derived' || terminal.itemId !== candidate?.itemId) return false
    const end = parents.get(terminal.boundary.uuid)
    if (
      end?.sessionId !== sessionId ||
      end.isSidechain !== false ||
      end.parentUuid !== candidate.uuid
    )
      return false
    let record = parents.get(candidate.uuid)
    const seen = new Set()
    while (record && !seen.has(record.uuid)) {
      if (record.sessionId !== sessionId || record.isSidechain !== false) return false
      if (record === user) return true
      if (record.uuid !== candidate.uuid && record.type !== 'attachment') return false
      seen.add(record.uuid)
      record = parents.get(record.parentUuid)
    }
    return false
  }

  records.forEach((record, recordIndex) => {
    const seq = mintCursor('claude-code', recordIndex)
    const at = record.timestamp ?? recordIndex
    result.cursor = seq
    const own = record.sessionId
    if (own && own !== sessionId) result.replaced = true
    if (
      record.type === 'attachment' &&
      own === sessionId &&
      record.isSidechain === false &&
      record.attachment?.type === 'hook_additional_context' &&
      record.attachment.hookEvent === 'UserPromptSubmit' &&
      Array.isArray(record.attachment.content)
    ) {
      const text = record.attachment.content.filter((part) => typeof part === 'string').join('\n')
      if (text)
        result.items.push({
          id: nativeId(record.uuid, 'claude hook context', seq),
          role: 'custom',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
    }
    if (record.type === 'continued-in' && own === sessionId && record.isSidechain !== true) {
      const successor = record.continuedInSessionId
      if (
        typeof successor !== 'string' ||
        !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
          successor,
        ) ||
        successor === sessionId ||
        (result.continuedInSessionId && result.continuedInSessionId !== successor)
      ) {
        throw new Error('invalid or ambiguous native continuation')
      }
      result.continuedInSessionId = successor
      result.continuedAt = record.timestamp ?? null
    }

    if (record.type === 'queue-operation') {
      if (record.operation === 'enqueue') {
        queued.push({ id: `queue:${recordIndex}`, content: String(record.content ?? '') })
      } else if (record.operation === 'dequeue') {
        dequeued.push(queued.shift() ?? { id: `queue:${recordIndex}`, content: '' })
      } else if (record.operation === 'popAll') {
        const content = String(record.content ?? '')
        const queuedIndex = queued.findIndex((entry) => entry.content === content)
        popped.push(
          queuedIndex === -1
            ? { id: `queue:${recordIndex}`, content }
            : queued.splice(queuedIndex, 1)[0],
        )
      } else if (record.operation === 'remove') {
        const content = String(record.content ?? '')
        const queuedIndex = queued.findIndex((entry) => entry.content === content)
        if (queuedIndex !== -1) queued.splice(queuedIndex, 1)
        const dequeuedIndex = dequeued.findIndex((entry) => entry.content === content)
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
        result.failed = false
        result.failure = null
      }
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
        result.failed = true
        result.failure = String(record.errorDetails ?? record.error ?? text)
        terminal = {
          provenance: 'native',
          complete: false,
          itemId: item.id,
          boundary: boundary('assistant.api_error', seq, at, {
            uuid: record.uuid,
            status: record.apiErrorStatus ?? null,
          }),
        }
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
        result.items.push({
          id: nativeId(record.uuid, 'claude user', seq),
          role: 'user',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        })
        result.cancelled = true
        turnOpen = false
        hooks.clear()
        terminal = {
          provenance: 'native',
          complete: false,
          boundary: boundary('user.request_interrupted', seq, at, {
            uuid: record.uuid,
          }),
        }
        candidate = null
        return
      }

      if (!text.trim()) return
      result.items.push({
        id: nativeId(record.uuid, 'claude user', seq),
        role: 'user',
        text,
        complete: true,
        settled: true,
        at,
        seq,
      })
      if (lateAncestor(record)) return
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
      result.cancelled = false
      result.failed = false
      result.failure = null
      turnOpen = true
      candidate = null
      terminal = null
      return
    }

    const command = result.items.at(-1)
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
      terminal = {
        provenance: 'native',
        complete: true,
        boundary: boundary('system.local_command', seq, at, { uuid: record.uuid }),
      }
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
      terminal = {
        provenance: 'derived',
        complete: true,
        itemId: candidate.itemId,
        boundary: boundary(`system.${record.subtype}`, seq, at, {
          uuid: record.uuid,
          hookCount: record.hookCount ?? null,
        }),
      }
      settleCandidate()
    }
  })

  if (count === 0) throw new Error(`empty claude session ${sessionId}`)
  const queuedTurns = queueEvidence()
  const hookEvidence = [...hooks].map((id) => `stop-hook:${id}`)
  const evidence = {
    complete: terminal?.complete ?? false,
    openTools: [...openTools],
    queuedTurns,
    hooksInFlight: hookEvidence,
  }

  let state = 'unknown'
  const provenance = terminal?.provenance ?? (turnOpen ? 'derived' : 'unknown')
  let settlementCursor = null
  if (terminal?.provenance === 'native' && openTools.size === 0 && queuedTurns.length === 0) {
    state = 'settled'
    settlementCursor = terminal.boundary.cursor
  } else if (terminal?.provenance === 'derived' && candidateCanSettle()) {
    state = 'settled'
    settlementCursor = terminal.boundary.cursor
  } else if (
    turnOpen ||
    openTools.size > 0 ||
    queuedTurns.length > 0 ||
    hooks.size > 0 ||
    terminal
  ) {
    state = 'in-flight'
  }

  result.inFlight = state === 'in-flight'
  if (state === 'settled' && terminal?.itemId) {
    const item = assistants.get(terminal.itemId)
    if (item) item.settled = true
  }
  setSettlement(result, state, provenance, terminal?.boundary ?? null, settlementCursor, evidence)
  for (const item of result.items) {
    delete item._fragmentOrder
    delete item._fragmentText
  }
  result.items.sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
  return result
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
// This describes source completion; the native receiver owns destination readiness.
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

async function piAnswers(sessionId, env, options = {}) {
  const root = piSessionDir(env)
  const file = await findFile(root, (name) => name.includes(sessionId))
  if (file === null) return { unknown: true, reason: `unreadable: no pi session ${sessionId}` }

  const result = resultBase()
  const openTools = new Set()
  let turnOpen = false
  let terminal = null

  const count = await readJsonl(file, (record, recordIndex) => {
    const seq = mintCursor('pi', recordIndex)
    const at = record.timestamp ?? record.message?.timestamp ?? recordIndex
    result.cursor = seq

    if (record.type === 'session') {
      if (record.id && record.id !== sessionId) result.replaced = true
      return
    }
    if (record.type === 'custom_message') {
      const text = typeof record.content === 'string' ? record.content : piText(record.content)
      if (text)
        result.items.push({
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
      result.failed = false
      result.failure = null
      result.items.push({
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
      result.items.push({
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
    result.items.push(item)

    if (message.stopReason === 'stop') {
      turnOpen = true
      result.failed = false
      result.failure = null
      terminal = {
        provenance: 'derived',
        complete: true,
        item,
        boundary: boundary('message.assistant.stop', seq, at, { id }),
      }
    } else if (message.stopReason === 'error') {
      turnOpen = true
      result.failed = true
      result.failure = String(message.errorMessage ?? 'provider error')
      terminal = {
        provenance: 'derived',
        complete: false,
        item,
        boundary: boundary('message.assistant.error', seq, at, { id }),
      }
    } else {
      turnOpen = true
      terminal = null
    }
  })

  if (count === 0) throw new Error(`empty pi session ${sessionId}`)
  const nativeEvidence = await piSettlementEvidence(sessionId, env, options)
  const hasNativeBoundary = Boolean(
    nativeEvidence && terminal?.item?.id === nativeEvidence.frontier.id,
  )
  const { mtimeMs } = await fs.stat(file)
  const quiet = Date.now() - mtimeMs >= PI_SETTLEMENT_QUIET_MS
  const open = [...openTools]
  const canSettle = Boolean(terminal?.complete && quiet && open.length === 0)
  const nativeSettled = Boolean(hasNativeBoundary && open.length === 0)
  if (canSettle || nativeSettled) terminal.item.settled = true

  result.inFlight = open.length > 0 || (terminal ? !(quiet || nativeSettled) : turnOpen)
  const state = nativeSettled || canSettle ? 'settled' : result.inFlight ? 'in-flight' : 'unknown'
  const quietBoundary = nativeSettled
    ? boundary('agent_settled', terminal.boundary.cursor, terminal.boundary.at, {
        launchId: nativeEvidence.launchId,
        sessionId: nativeEvidence.sessionId,
        frontier: nativeEvidence.frontier,
      })
    : canSettle
      ? boundary('session.quiet_window', terminal.boundary.cursor, terminal.boundary.at, {
          quietMs: PI_SETTLEMENT_QUIET_MS,
        })
      : null
  setSettlement(
    result,
    state,
    nativeSettled ? 'native' : 'derived',
    quietBoundary,
    quietBoundary?.cursor ?? null,
    {
      complete: terminal?.complete ?? false,
      openTools: open,
      queuedTurns: [],
      hooksInFlight: [],
    },
  )
  return result
}

// ================================================================= kimi

function kimiInput(input) {
  if (typeof input === 'string') return input
  if (!Array.isArray(input)) return ''
  return input.map((part) => (typeof part === 'string' ? part : String(part?.text ?? ''))).join('')
}

function kimiTurn(turns, turnId) {
  const key = String(turnId)
  let turn = turns.get(key)
  if (!turn) {
    turn = {
      id: key,
      openTools: new Set(),
      ended: null,
      finalStepId: null,
      lastStepId: null,
    }
    turns.set(key, turn)
  }
  return turn
}

async function kimiAnswers(sessionId, env) {
  const root = path.join(env.KIMI_CODE_HOME ?? path.join(home(env), '.kimi-code'), 'sessions')
  const dir = await findDir(root, sessionId)
  if (dir === null) return { unknown: true, reason: `unreadable: no kimi session ${sessionId}` }

  const result = resultBase()
  try {
    const state = JSON.parse(await fs.readFile(path.join(dir, 'state.json'), 'utf8'))
    if (typeof state.id === 'string' && state.id !== sessionId) result.replaced = true
  } catch {
    // state.json is optional; wire.jsonl is authoritative for completion.
  }

  const file = path.join(dir, 'agents', 'main', 'wire.jsonl')
  const turns = new Map()
  const steps = new Map()
  const toolItems = new Map()
  const userItems = new Map()
  const calls = new Map()
  const admissions = []
  const pendingPrompts = []
  let version
  let currentTurnId = null
  let latestTurnId = null
  let turnOpen = false

  const addStep = (stepId, at, seq, turnId) => {
    const id = nativeId(stepId, 'kimi step', seq)
    let item = steps.get(id)
    if (!item) {
      item = {
        id,
        role: 'assistant',
        text: '',
        complete: false,
        settled: false,
        at,
        seq,
        _fragmentOrder: [],
        _fragmentText: new Map(),
        _turnId: String(turnId),
      }
      steps.set(id, item)
      result.items.push(item)
    }
    item.at = at
    item.seq = seq
    return item
  }

  const addUser = (id, text, at, seq) => {
    const native = nativeId(id, 'kimi user message', seq)
    let item = userItems.get(native)
    if (!item) {
      item = {
        id: native,
        role: 'user',
        text,
        complete: true,
        settled: true,
        at,
        seq,
      }
      userItems.set(native, item)
      result.items.push(item)
    } else {
      item.text = text
      item.at = at
      item.seq = seq
    }
    return item
  }

  const count = await readJsonl(file, (record, recordIndex) => {
    const seq = mintCursor('kimi', recordIndex)
    const at = record.time ?? recordIndex
    result.cursor = seq

    if (record.type === 'metadata') {
      version = checkedVersion('kimi', record.protocol_version)
      return
    }

    if (record.type === 'prompt.accepted') {
      const id = nativeId(record.promptId, 'kimi accepted prompt', seq)
      if (!admissions.some((entry) => entry.id === id)) admissions.push({ id, seq })
      turnOpen = true
      latestTurnId = null
      return
    }

    if (record.type === 'turn.prompt' && record.origin?.kind === 'user') {
      const text = kimiInput(record.input)
      if (!text.trim()) return
      const acceptedIndex =
        typeof record.promptId === 'string'
          ? admissions.findIndex((entry) => entry.id === record.promptId)
          : 0
      const accepted = acceptedIndex >= 0 ? admissions.splice(acceptedIndex, 1)[0] : undefined
      const id =
        typeof record.promptId === 'string' && record.promptId
          ? record.promptId
          : (accepted?.id ?? null)
      if (id) addUser(id, text, at, seq)
      pendingPrompts.push({ id, text })
      turnOpen = true
      latestTurnId = null
      return
    }

    if (
      record.type === 'context.append_message' &&
      record.message?.role === 'user' &&
      record.message.origin?.kind === 'user'
    ) {
      const message = record.message
      const id = nativeId(message.id, 'kimi user message', seq)
      const prompt = pendingPrompts.shift()
      if (prompt?.id && prompt.id !== id) {
        throw new Error(`kimi prompt admission ${prompt.id} does not match message ${id}`)
      }
      const text = kimiInput(message.content) || prompt?.text || ''
      if (text.trim()) addUser(id, text, at, seq)
      turnOpen = true
      latestTurnId = null
      return
    }

    if (record.type === 'turn.ended') {
      const turn = kimiTurn(turns, record.turnId)
      turn.ended = {
        reason: record.reason,
        error: record.error ?? null,
        boundary: boundary('turn.ended', seq, at, {
          turnId: record.turnId,
          reason: record.reason ?? null,
        }),
      }
      latestTurnId = String(record.turnId)
      turnOpen = false
      return
    }

    if (record.type !== 'context.append_loop_event') return
    const event = record.event ?? {}

    if (event.type === 'step.begin') {
      currentTurnId = String(event.turnId)
      latestTurnId = currentTurnId
      kimiTurn(turns, currentTurnId)
      turnOpen = true
      return
    }

    if (event.type === 'content.part' && event.part?.type === 'text') {
      const turnId = String(event.turnId ?? currentTurnId)
      latestTurnId = turnId
      const item = addStep(event.stepUuid, at, seq, turnId)
      const text = String(event.part.text ?? '')
      updateNativeFragment(item, nativeId(event.uuid, 'kimi content part', seq), text, '')
      return
    }

    if (event.type === 'tool.call') {
      const turnId = String(event.turnId ?? currentTurnId)
      const turn = kimiTurn(turns, turnId)
      const callId = event.toolCallId ?? event.uuid
      if (callId) turn.openTools.add(callId)
      const owner = { turnId, callId }
      if (event.uuid) calls.set(event.uuid, owner)
      if (event.toolCallId) calls.set(event.toolCallId, owner)
      latestTurnId = turnId
      turnOpen = true
      return
    }

    if (event.type === 'tool.result') {
      const owner = calls.get(event.parentUuid) ?? calls.get(event.toolCallId)
      if (owner) kimiTurn(turns, owner.turnId).openTools.delete(owner.callId)
      const id = nativeId(event.parentUuid ?? event.toolCallId, 'kimi tool result', seq)
      const text = visibleText(event.result?.output ?? event.result)
      const existing = toolItems.get(id)
      if (existing) {
        existing.text = text
        existing.at = at
        existing.seq = seq
      } else {
        const item = {
          id,
          role: 'tool',
          text,
          complete: true,
          settled: true,
          at,
          seq,
        }
        toolItems.set(id, item)
        result.items.push(item)
      }
      return
    }

    if (event.type === 'step.end') {
      const turnId = String(event.turnId ?? currentTurnId)
      const turn = kimiTurn(turns, turnId)
      const item = addStep(event.uuid, at, seq, turnId)
      turn.lastStepId = item.id
      if (event.finishReason === 'end_turn') {
        item.complete = true
        turn.finalStepId = item.id
      }
      latestTurnId = turnId
    }
  })

  if (count === 0) throw new Error(`empty kimi wire for ${sessionId}`)
  result.version = version ?? checkedVersion('kimi', undefined)
  for (const turn of turns.values()) {
    if (turn.id === latestTurnId) continue
    const final = turn.finalStepId ? steps.get(turn.finalStepId) : null
    if (turn.ended && final?.complete && turn.openTools.size === 0) final.settled = true
  }
  const latest = latestTurnId === null ? null : turns.get(String(latestTurnId))
  const openTools = latest ? [...latest.openTools] : []
  const final = latest?.finalStepId ? steps.get(latest.finalStepId) : null
  const complete = Boolean(final?.complete)
  const failed = latest?.ended?.reason === 'failed'
  const canSettle = Boolean(
    latest?.ended &&
      (complete || failed) &&
      openTools.length === 0 &&
      admissions.length === 0 &&
      !turnOpen,
  )
  const terminalItem = final ?? (latest?.lastStepId ? steps.get(latest.lastStepId) : null)
  if (canSettle && terminalItem) terminalItem.settled = true

  result.failed = failed
  result.failure = failed
    ? String(latest.ended.error?.message ?? visibleText(latest.ended.error))
    : null
  result.inFlight = turnOpen || openTools.length > 0 || admissions.length > 0
  const state = canSettle ? 'settled' : result.inFlight ? 'in-flight' : 'unknown'
  setSettlement(
    result,
    state,
    latest?.ended ? 'native' : state === 'unknown' ? 'unknown' : 'native',
    latest?.ended?.boundary ?? null,
    canSettle ? latest.ended.boundary.cursor : null,
    {
      complete,
      openTools,
      queuedTurns: admissions.map((entry) => entry.id),
      hooksInFlight: [],
    },
  )

  for (const item of result.items) {
    delete item._fragmentOrder
    delete item._fragmentText
    delete item._turnId
  }
  result.items.sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
  return result
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

async function opencodeAnswers(sessionId, env, options) {
  const db = await openOpencodeDb(env)
  if (db === null) {
    return { unknown: true, reason: `unreadable: no opencode store for ${sessionId}` }
  }

  let transaction = false
  try {
    db.exec('BEGIN')
    transaction = true
    const session = db.prepare('select * from session where id = ?').get(sessionId)
    if (!session) {
      return { unknown: true, reason: `unreadable: no opencode session ${sessionId}` }
    }
    const messages = db
      .prepare('select * from message where session_id = ? order by time_created, id')
      .all(sessionId)
    await options.betweenOpenCodeSnapshotReads?.()
    const parts = db
      .prepare('select * from part where session_id = ? order by time_created, id')
      .all(sessionId)
    const events = db
      .prepare('select * from event where aggregate_id = ? order by seq')
      .all(sessionId)

    const result = resultBase()
    const messagePositions = new Map()
    const messageCompletionPositions = new Map()
    const partPositions = new Map()
    let previousPosition = -1
    for (const row of events) {
      const nativeSeq = Number(row.seq)
      if (!Number.isInteger(nativeSeq) || nativeSeq < 0 || nativeSeq <= previousPosition) {
        throw new Error(`malformed OpenCode event sequence at ${String(row.seq)}`)
      }
      previousPosition = nativeSeq
      const seq = mintCursor('opencode', nativeSeq)
      const data = parseStoredJson(row.data, `event ${row.id}`)
      if (row.type === 'message.updated.1' && data.info?.id) {
        if (!messagePositions.has(data.info.id)) messagePositions.set(data.info.id, seq)
        if (
          data.info.time?.completed !== undefined &&
          data.info.time?.completed !== null &&
          !messageCompletionPositions.has(data.info.id)
        ) {
          messageCompletionPositions.set(data.info.id, seq)
        }
      } else if (row.type === 'message.part.updated.1' && data.part?.id) {
        partPositions.set(data.part.id, seq)
      }
    }
    if (previousPosition < 0) throw new Error(`missing OpenCode event sequence for ${sessionId}`)
    result.cursor = mintCursor('opencode', previousPosition)

    const position = (positions, id, description) => {
      const seq = positions.get(id)
      if (!Number.isInteger(seq)) throw new Error(`missing OpenCode event for ${description} ${id}`)
      return seq
    }

    const partsByMessage = new Map()
    for (const row of parts) {
      const parsed = parseStoredJson(row.data, `part ${row.id}`)
      const entry = { row, data: parsed }
      const list = partsByMessage.get(row.message_id) ?? []
      list.push(entry)
      partsByMessage.set(row.message_id, list)
    }
    for (const list of partsByMessage.values()) {
      list.sort(
        (left, right) =>
          position(partPositions, left.row.id, 'part') -
            position(partPositions, right.row.id, 'part') ||
          left.row.id.localeCompare(right.row.id),
      )
    }
    messages.sort(
      (left, right) =>
        position(messagePositions, left.id, 'message') -
          position(messagePositions, right.id, 'message') || left.id.localeCompare(right.id),
    )

    const openTools = new Set()
    let turnOpen = false
    let terminal = null
    for (const row of messages) {
      const data = parseStoredJson(row.data, `message ${row.id}`)
      const messageParts = partsByMessage.get(row.id) ?? []
      const text = messageParts
        .filter(({ data: part }) => part.type === 'text' && typeof part.text === 'string')
        .map(({ data: part }) => part.text)
        .join('\n')
      const messageSeq = position(messagePositions, row.id, 'message')
      const textPositions = messageParts
        .filter(({ data: part }) => part.type === 'text')
        .map(({ row: partRow }) => position(partPositions, partRow.id, 'part'))
      const at = data.time?.completed ?? data.time?.created ?? row.time_created
      const completed = data.time?.completed !== undefined && data.time?.completed !== null
      const completionSeq = completed
        ? position(messageCompletionPositions, row.id, 'message completion')
        : null
      const seq =
        textPositions.length > 0
          ? Math.max(messageSeq, ...textPositions)
          : (completionSeq ?? messageSeq)

      if (data.role === 'user') {
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
      if (data.role !== 'assistant') continue

      result.cancelled = false
      result.failed = false
      result.failure = null
      for (const { row: partRow, data: part } of messageParts) {
        if (part.type !== 'tool') continue
        const status = part.state?.status
        const toolId = part.callID ?? partRow.id
        if (status === 'completed' || status === 'error') openTools.delete(toolId)
        else openTools.add(toolId)
      }

      const errorName = data.error?.name
      const isFailure = completed && Boolean(errorName)
      // No supported-version native fixture establishes OpenCode cancellation.
      // In particular, MessageAbortedError remains a failure, never cancellation.
      const closesTurn =
        completed && (data.finish === 'stop' || data.finish === 'length' || isFailure)
      const complete = completed && data.finish === 'stop' && !errorName
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
          seq: position(partPositions, partRow.id, 'part'),
        })
      }

      if (isFailure) {
        result.failed = true
        result.failure = String(data.error?.data?.message ?? visibleText(data.error))
      }

      if (closesTurn) {
        turnOpen = false
        terminal = {
          complete,
          item,
          boundary: boundary('message.time.completed', completionSeq, at, {
            id: row.id,
            finish: data.finish ?? null,
            error: errorName ?? null,
          }),
        }
      } else {
        turnOpen = true
        terminal = null
      }
    }

    const open = [...openTools]
    const canSettle = Boolean(terminal && open.length === 0)
    if (canSettle) terminal.item.settled = true
    result.inFlight = turnOpen || open.length > 0
    const state = canSettle ? 'settled' : result.inFlight ? 'in-flight' : 'unknown'
    setSettlement(
      result,
      state,
      state === 'unknown' ? 'unknown' : 'native',
      terminal?.boundary ?? null,
      canSettle ? terminal.boundary.cursor : null,
      {
        complete: terminal?.complete ?? false,
        openTools: open,
        queuedTurns: [],
        hooksInFlight: [],
      },
    )

    result.items.sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
    return result
  } finally {
    if (transaction) {
      try {
        db.exec('ROLLBACK')
      } catch {
        // The read transaction may already have been closed by SQLite.
      }
    }
    try {
      db.close()
    } catch {
      // Closing a failed read-only open must not mask the original result.
    }
  }
}

async function openOpencodeDb(env) {
  const file = path.join(
    env.XDG_DATA_HOME ?? path.join(home(env), '.local', 'share'),
    'opencode',
    'opencode.db',
  )
  try {
    await fs.access(file)
  } catch {
    return null
  }
  let sqlite
  try {
    sqlite = await import('node:sqlite')
  } catch {
    return null
  }
  try {
    return new sqlite.DatabaseSync(file, { readOnly: true })
  } catch {
    return null
  }
}
