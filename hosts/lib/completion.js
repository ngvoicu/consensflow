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
import { codexQuota, exhaustedQuota } from './quota.js'

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
        return await claudeAnswers(sessionId, env, options)
      case 'pi':
        return await piAnswers(sessionId, env, options)
      case 'opencode':
        return await opencodeAnswers(sessionId, env, options)
      case 'devin':
        return await devinAnswers(sessionId, env)
      default:
        return { unknown: true, reason: `unknown kind: ${kind}` }
    }
  } catch (error) {
    return { unknown: true, reason: `unreadable: ${describeError(error)}` }
  }
}

/**
 * `answers` for a caller that re-reads the same sessions every second (the
 * delivery watcher; the live chief's transcript reached 135 MB). Each JSONL
 * transcript is located once, and a file whose size and modification time
 * have not changed returns the previous result instead of being searched for
 * and parsed again. Calls with options, and harnesses read through a
 * database query, always read. Results are shared: callers must not mutate them.
 */
export function cachedAnswers() {
  const known = new Map()
  return async (kind, sessionId, env, options = {}) => {
    if (
      Object.keys(options).length > 0 ||
      !['claude-code', 'codex', 'pi'].includes(kind) ||
      !sessionId ||
      env === null ||
      typeof env !== 'object'
    )
      return answers(kind, sessionId, env, options)
    const key = `${kind}\n${sessionId}`
    const previous = known.get(key)
    let file = previous?.file ?? null
    let stat = file === null ? null : await fs.stat(file).catch(() => null)
    if (stat === null) {
      file = await locateTranscript(kind, sessionId, env).catch(() => null)
      stat = file === null ? null : await fs.stat(file).catch(() => null)
    }
    if (stat === null) {
      known.delete(key)
      return answers(kind, sessionId, env)
    }
    const stamp = `${stat.size}:${stat.mtimeMs}`
    if (previous?.file === file && previous.stamp === stamp) return previous.result
    const result = await answers(kind, sessionId, env, { file })
    known.set(key, { file, stamp, result })
    return result
  }
}

/** Where a JSONL harness keeps one session's transcript, or null. */
/** Whether the harness has kept a record of the conversation at all. */
export async function hasTranscript(kind, sessionId, env) {
  return (await locateTranscript(kind, sessionId, env)) !== null
}

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
    opencode: 5,
    devin: 6,
  }),
)
const ITEM_ROLES = new Set(['user', 'assistant', 'tool', 'custom'])

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

function resultBase() {
  return {
    items: [],
    inFlight: false,
    // Its own question dialog is open: the window waits for the human's answer.
    asking: false,
    cancelled: false,
    replaced: false,
    failed: false,
    failure: null,
    quota: null,
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
  const file = options.file ?? (await locateTranscript('codex', sessionId, env))
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

    if (payload.type === 'token_count') {
      if (payload.rate_limits) result.quota = codexQuota(payload.rate_limits)
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

async function claudeAnswers(sessionId, env, options = {}) {
  const file = options.file ?? (await locateTranscript('claude-code', sessionId, env))
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
        const queuedIndex = queued.findIndex((entry) => sameQueuedContent(entry.content, content))
        popped.push(
          queuedIndex === -1
            ? { id: `queue:${recordIndex}`, content }
            : queued.splice(queuedIndex, 1)[0],
        )
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
        result.failed = false
        result.failure = null
      }
      // The latest assistant record has the last word on quota.
      result.quota = null
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
        if (record.apiErrorStatus === 429 || record.error === 'rate_limit') {
          result.quota = exhaustedQuota(text, Date.parse(record.timestamp))
        }
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

async function piAnswers(sessionId, env, options = {}) {
  const file = options.file ?? (await locateTranscript('pi', sessionId, env))
  if (file === null) {
    if (!(await piWorkingEvidence(sessionId, env, options)))
      return { unknown: true, reason: `unreadable: no pi session ${sessionId}` }
    const working = resultBase()
    working.inFlight = true
    working.cursor = mintCursor('pi', 0)
    setSettlement(working, 'in-flight', 'native', null, working.cursor, {})
    return working
  }

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

    result.quota = null
    if (message.stopReason === 'stop') {
      turnOpen = true
      result.failed = false
      result.failure = null
      result.cancelled = false
      terminal = {
        provenance: 'derived',
        complete: true,
        item,
        boundary: boundary('message.assistant.stop', seq, at, { id }),
      }
    } else if (message.stopReason === 'aborted') {
      // Stopped by an Escape (a pause, a tell, the human): the turn is over,
      // not failed, and the extension's settled evidence names this message.
      turnOpen = true
      result.failed = false
      result.failure = null
      result.cancelled = true
      terminal = {
        provenance: 'derived',
        complete: false,
        aborted: true,
        item,
        boundary: boundary('message.assistant.aborted', seq, at, { id }),
      }
    } else if (message.stopReason === 'error') {
      turnOpen = true
      result.failed = true
      result.failure = String(message.errorMessage ?? 'provider error')
      if (/^429\b/.test(result.failure)) {
        result.quota = exhaustedQuota(result.failure, Number(message.timestamp))
      }
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
  const canSettle = Boolean((terminal?.complete || terminal?.aborted) && quiet && open.length === 0)
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
        const done = status === 'completed' || status === 'error'
        if (done) openTools.delete(toolId)
        else openTools.add(toolId)
        if (part.tool === 'question') result.asking = !done
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

      if (completed) result.quota = null
      if (isFailure) {
        result.failed = true
        result.failure = String(data.error?.data?.message ?? visibleText(data.error))
        if (data.error?.data?.statusCode === 429) {
          result.quota = exhaustedQuota(result.failure, Number(data.time?.completed ?? at))
        }
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

async function devinAnswers(sessionId, env) {
  const { DatabaseSync } = await import('node:sqlite')
  const file = path.join(
    env.XDG_DATA_HOME ?? path.join(home(env), '.local', 'share'),
    'devin',
    'cli',
    'sessions.db',
  )
  const db = new DatabaseSync(file, { readOnly: true })
  const result = resultBase()
  try {
    db.exec('BEGIN')
    const session = db.prepare('select main_chain_id from sessions where id = ?').get(sessionId)
    if (!session) throw new Error('missing Devin session')
    const rows = db
      .prepare(
        'select row_id, node_id, parent_node_id, chat_message, created_at from message_nodes where session_id = ? order by row_id',
      )
      .all(sessionId)
    const nodes = new Map(rows.map((row) => [row.node_id, row]))
    const chain = [],
      visited = new Set()
    let node = session.main_chain_id
    while (node !== null) {
      if (visited.has(node)) throw new Error('cyclic Devin main chain')
      visited.add(node)
      const row = nodes.get(node)
      if (!row) throw new Error('missing Devin main chain ancestor')
      chain.push(row)
      node = row.parent_node_id
    }
    const ids = new Set()
    let request = null
    // Its question tool's call, until a tool message answers it.
    let asking = null
    for (const row of chain.reverse()) {
      const message = JSON.parse(row.chat_message)
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
      const seq = mintCursor('devin', Number(row.row_id))
      result.items.push({
        id: message.message_id,
        role,
        text,
        complete: role !== 'assistant',
        settled: role !== 'assistant',
        at: message.metadata?.created_at ?? row.created_at,
        seq,
        _request: request,
      })
      result.cursor = Math.max(result.cursor ?? seq, seq)
    }
    result.asking = asking !== null
    db.exec('COMMIT')
  } finally {
    db.close()
  }
  const root = path.join(
    env.CONSENSFLOW_HOME ?? path.join(home(env), '.consensflow'),
    'integrations',
    'devin',
  )
  let launches = []
  try {
    launches = await fs.readdir(root, { withFileTypes: true })
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
  const outcomes = new Map()
  // A turn Devin is still on shows only on the wire: thoughts, messages and
  // tool calls after the last end; its store holds the finished steps. Judged
  // by the launch whose wire was written last (a resume opens a new one).
  let working = false
  let latestWire = -1
  for (const launch of launches) {
    if (!launch.isDirectory()) continue
    let active = null
    let busy = false
    const wire = path.join(root, launch.name, 'wire.jsonl')
    try {
      await readJsonl(wire, (event) => {
        if (event.sessionId !== sessionId) return
        const update = event.update
        if (DEVIN_WORK.has(update?.sessionUpdate)) busy = true
        if (update?.sessionUpdate === 'agent_message_chunk') {
          const id = update._meta?.['cognition.ai/streamingMessageId']
          // History replay has timestamps but no streaming UUID.
          if (typeof id !== 'string' || update.content?.type !== 'text') return
          if (active?.id !== id) active = { id, text: '', request: null }
          active.text += update.content.text
        }
        if (active && typeof event.turnClientMessageId === 'string')
          active.request = event.turnClientMessageId
        if (['complete', 'cancelled', 'error'].includes(event.cause)) {
          if (active?.request) {
            const outcome = { ...active, cause: event.cause }
            const previous = outcomes.get(active.request)
            if (previous && (previous.text !== outcome.text || previous.cause !== outcome.cause))
              throw new Error('conflicting Devin completion evidence')
            outcomes.set(active.request, outcome)
          }
          active = null
          busy = false
        }
      })
      const { mtimeMs } = await fs.stat(wire)
      if (mtimeMs >= latestWire) {
        latestWire = mtimeMs
        working = busy
      }
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
  }
  const finalByRequest = new Map(
    result.items
      .filter((item) => item.role === 'assistant')
      .map((item) => [item._request, item.id]),
  )
  for (const item of result.items) {
    if (item.role !== 'assistant') continue
    const outcome = outcomes.get(item._request)
    item.complete =
      finalByRequest.get(item._request) === item.id &&
      outcome?.cause === 'complete' &&
      devinComparable(outcome.text) === devinComparable(item.text)
    item.settled = item.complete
  }
  const last = result.items.findLast((item) => item.role !== 'custom')
  const outcome = outcomes.get(last?._request)
  result.cancelled = outcome?.cause === 'cancelled'
  result.failed = outcome?.cause === 'error'
  result.inFlight =
    working || (last?.role === 'assistant' && !last.complete && !result.cancelled && !result.failed)
  setSettlement(
    result,
    working
      ? 'in-flight'
      : last?.complete || result.cancelled || result.failed
        ? 'settled'
        : 'unknown',
    'native',
    null,
    result.cursor,
    { complete: last?.complete === true, openTools: [], queuedTurns: [], hooksInFlight: [] },
  )
  for (const item of result.items) delete item._request
  return result
}
