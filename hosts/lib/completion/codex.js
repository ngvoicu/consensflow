/** Codex's record of a thread: its rollout, a JSONL file in its `sessions` folder. */
import path from 'node:path'
import { codexQuota } from '../quota.js'
import {
  emit,
  findFile,
  home,
  nativeId,
  resultBase,
  transcriptReader,
  visibleText,
} from './shared.js'

/** Where Codex keeps one thread's rollout, or null. */
export async function codexTranscript(sessionId, env) {
  return findFile(path.join(env.CODEX_HOME ?? path.join(home(env), '.codex'), 'sessions'), (name) =>
    name.includes(sessionId),
  )
}

/** Codex's reader: its answer is its rollout's alone. */
export function codexReader(sessionId, env) {
  return transcriptReader(
    () => codexTranscript(sessionId, env),
    () => codexParser(sessionId),
    `codex rollout for ${sessionId}`,
  )
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

  const addItem = (id, role, text, complete, at, seq) => {
    const stableId = nativeId(id, 'codex item', seq)
    const existing = items.get(stableId)
    if (existing) {
      if (text && existing.text !== text && !existing._nativeFinalText) existing.text = text
      existing.complete ||= complete
      existing.at = at
      return existing
    }
    const item = { id: stableId, role, text, complete, at }
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
        const item = addItem(payload.id, payload.role, text, payload.role === 'user', at, seq)
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
        addItem(payload.id, 'tool', visibleText(payload.output), true, at, seq)
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
      if (text.trim()) addItem(native.id, 'user', text, true, at, seq)
      return
    }

    if (native.type === 'AgentMessage' && payload.type === 'item_completed') {
      const text = contentText(native.content)
      if (!text.trim()) return
      const item = addItem(native.id, 'assistant', text, native.phase === 'final_answer', at, seq)
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
      addItem(id, 'tool', String(output), true, at, seq)
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

  // A turn's task_complete proves the answer it names.
  const proven = (turn) =>
    turn.assistantIds.some((id) => {
      const item = items.get(id)
      return item?.complete && item._nativeFinalText === turn.terminal.lastAgentMessage
    })

  const result = () => {
    if (count === 0) throw new Error(`empty codex rollout for ${sessionId}`)
    const answer = resultBase()
    answer.items = list.map(emit)
    const latest = latestTurnId ? turns.get(latestTurnId) : null
    const activeTurn = Boolean(latest?.started && !latest.terminal)
    answer.inFlight =
      activeTurn || (latest ? latest.openTools.size + latest.openSubagents.size > 0 : false)
    answer.failed = latest?.terminal?.kind === 'error'
    answer.quota = quota
    // A task_complete whose final answer cannot be matched proves nothing.
    if (answer.inFlight) answer.settlement = { state: 'in-flight' }
    else if (latest?.terminal && (latest.terminal.kind !== 'complete' || proven(latest)))
      answer.settlement = { state: 'settled' }
    return answer
  }

  return { visit, result }
}
