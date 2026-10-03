/**
 * Pi's record of a session: its JSONL session file, and what ConsensFlow's
 * extension writes beside it.
 */
import fs from 'node:fs/promises'
import path from 'node:path'
import { piSessionDir } from '../../../src/harnesses.js'
import { exhaustedQuota, refusedForQuota } from '../quota.js'
import { emit, findFile, followedTranscript, nativeId, resultBase, unreadable } from './shared.js'

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

/** Where Pi keeps one session's file, or null. */
export async function piTranscript(sessionId, env) {
  return findFile(piSessionDir(env), (name) => name.includes(sessionId))
}

/**
 * Pi's reader. Its answer also rests on the extension's evidence beside the
 * session and on how long the session file has been quiet, both of which
 * change while the transcript does not, so every look works it out anew.
 */
export function piReader(sessionId, env) {
  const transcript = followedTranscript(
    () => piTranscript(sessionId, env),
    () => piParser(sessionId),
  )
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
  let failed = false
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
          at,
        })
      return
    }
    if (record.type !== 'message') return

    const message = record.message ?? {}
    const id = nativeId(record.id, 'pi message', seq)
    if (message.role === 'user') {
      const text = piText(message.content)
      if (!text.trim()) return
      openTools.clear()
      failed = false
      list.push({
        id,
        role: 'user',
        text,
        complete: true,
        at,
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
        at,
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
      at,
    }
    list.push(item)

    quota = null
    if (message.stopReason === 'stop') {
      turnOpen = true
      failed = false
      terminal = { complete: true, item }
    } else if (message.stopReason === 'aborted') {
      // Stopped by an Escape (a pause, a tell, the human): the turn is over,
      // not failed, and the extension's settled evidence names this message.
      turnOpen = true
      failed = false
      terminal = { complete: false, aborted: true, item }
    } else if (message.stopReason === 'error') {
      turnOpen = true
      failed = true
      const failure = String(message.errorMessage ?? 'provider error')
      if (refusedForQuota(failure)) {
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

    const answer = resultBase()
    answer.items = list.map(emit)
    answer.inFlight = open || (terminal ? !(quiet || nativeSettled) : turnOpen)
    answer.failed = failed
    answer.quota = quota
    answer.settlement = {
      state: nativeSettled || canSettle ? 'settled' : answer.inFlight ? 'in-flight' : 'unknown',
    }
    return answer
  }

  return { visit, result }
}
