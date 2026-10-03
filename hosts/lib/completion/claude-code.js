/** Claude Code's record of a session: its transcript, a JSONL file in its `projects` folder. */
import path from 'node:path'
import { exhaustedQuota } from '../quota.js'
import {
  emit,
  findFile,
  home,
  nativeId,
  REREAD,
  resultBase,
  transcriptReader,
  visibleText,
} from './shared.js'

/** Where Claude Code keeps one session's transcript, or null. */
export async function claudeTranscript(sessionId, env) {
  return findFile(
    path.join(env.CLAUDE_CONFIG_DIR ?? path.join(home(env), '.claude'), 'projects'),
    (name) => name === `${sessionId}.jsonl`,
  )
}

/** Claude Code's reader: its answer is its transcript's alone. */
export function claudeReader(sessionId, env) {
  return transcriptReader(
    () => claudeTranscript(sessionId, env),
    () => claudeParser(sessionId),
    `claude session ${sessionId}`,
  )
}

function updateNativeFragment(item, identity, text, separator) {
  if (!text) return
  if (!item._fragmentText.has(identity)) item._fragmentOrder.push(identity)
  item._fragmentText.set(identity, text)
  item.text = item._fragmentOrder.map((id) => item._fragmentText.get(id)).join(separator)
}

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
  let failed = false
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
      if (record.isApiErrorMessage !== true) failed = false
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
        if (record.apiErrorStatus === 429 || record.error === 'rate_limit') {
          quota = exhaustedQuota(text, Date.parse(record.timestamp))
        }
        terminal = { provenance: 'native' }
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
          at,
          seq,
        })
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
      openTools.clear()
      hooks.clear()
      activeAssistantId = null
      failed = false
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

    const answer = resultBase()
    // In the record's order: an item's `seq` is its line's place.
    answer.items = [...list]
      .sort((left, right) => left.seq - right.seq || left.id.localeCompare(right.id))
      .map(emit)
    answer.inFlight = state === 'in-flight'
    answer.failed = failed
    answer.quota = quota
    answer.settlement = { state }
    return answer
  }

  return { visit, flush, result }
}
