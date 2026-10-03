/**
 * Devin's record of a session: its store, and each launch's wire log. Devin
 * persists revisions, including cancelled assistant text. Only its main
 * chain plus a matching native request/complete boundary proves a reply.
 */
import fs from 'node:fs/promises'
import path from 'node:path'
import { devinFolders } from '../../../src/harnesses.js'
import { home, readOn, resultBase, unreadable } from './shared.js'

const ITEM_ROLES = new Set(['user', 'assistant', 'tool', 'custom'])

/**
 * The text a Devin message is compared by: its wire streams a file link as
 * `[name](file:///path)`, a quoted range as `[name:1-3](file:///path)`, and
 * its store keeps `<ref_file file="/path" />` and `<ref_snippet file="/path"
 * lines="1-3" />` (Devin 3000.11, 2026-09-26 and 10-03), so each is read as
 * its path; on Windows the wire's `file:///C:/Users/…` and the store's
 * `C:\Users\…` are one path. Nothing else is loosened, since the comparison
 * is what tells a final message from a half one.
 */
function devinComparable(text) {
  const path = (value) => {
    let decoded
    try {
      decoded = decodeURI(value)
    } catch {
      decoded = value
    }
    const slashed = decoded.replaceAll('\\', '/')
    return /^\/[A-Za-z]:\//.test(slashed) ? slashed.slice(1) : slashed
  }
  return text
    .replace(/<ref_\w+\s+file="([^"]*)"[^>]*\/>/g, (_, file) => path(file))
    .replace(/\[[^\]]*\]\(file:\/\/([^)\s]*)\)/g, (_, file) => path(file))
}

/** The wire updates that mean Devin is on a turn; settings and mode updates are not work. */
const DEVIN_WORK = new Set([
  'agent_thought_chunk',
  'agent_message_chunk',
  'tool_call',
  'tool_call_update',
])

/** The last rows of a Devin conversation each look reads again, in case one was rewritten. */
const DEVIN_RECHECKED = 8

/**
 * Devin's reader. Its store gains message rows (a revision is a new node),
 * so a look reads the rows after the last one it saw, and the store is read
 * whole when the conversation's row count disagrees. Its last few rows are
 * read again too: a message Devin rewrote in place would otherwise stay as
 * first seen, and a reply never read whole never settles. Each launch's wire
 * log is read on from where it was left; one that shrank, was replaced or is
 * gone takes what it said with it, and every wire log is read again.
 */
export function devinReader(sessionId, env) {
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
      let rewritten = false
      if (store === null) rows = all()
      else {
        for (const row of db
          .prepare(
            `${columns} from message_nodes where session_id = ? and row_id <= ?
             order by row_id desc limit ${DEVIN_RECHECKED}`,
          )
          .all(sessionId, store.last ?? -1)) {
          if (store.nodes.get(row.node_id)?.chat_message === row.chat_message) continue
          store.nodes.set(row.node_id, row)
          rewritten = true
        }
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
      const changed = fresh || rewritten || rows.length > 0 || session.main_chain_id !== store.head
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
  // tool calls after the last end; its store holds the finished steps. A
  // tool's last word is not new work: a shell an Escape left running ends
  // after its turn did, and its window would read as working for good. Nor
  // is the history a window reopened on the conversation replays, which
  // carries its timestamps and ends in no turn end (poker-lab's T-4 read as
  // working for good after its window was opened again, 2026-10-03).
  const visitWire = (wire, event) => {
    if (event.sessionId !== sessionId) return
    wire.mine = true
    const update = event.update
    const toolEnded =
      update?.sessionUpdate === 'tool_call_update' &&
      ['completed', 'failed', 'cancelled'].includes(update.status)
    const replayed = update?._meta?.['cognition.ai/timestamp'] !== undefined
    if (DEVIN_WORK.has(update?.sessionUpdate) && !toolEnded && !replayed) wire.busy = true
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
        const wire = wires.get(launch) ?? { seen: null, active: null, busy: false, mine: false }
        let next
        try {
          next = await readOn(file, wire.seen, (event) => visitWire(wire, event), {
            only: JSON.stringify(sessionId),
          })
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
      at: message.metadata?.created_at ?? row.created_at,
      request,
      stopped: message.metadata?.finish_reason === 'stop',
    })
  }
  return { items, asking: asking !== null }
}

/**
 * What Devin's chain says: a reply is complete only when it is its request's
 * last, and either Devin stored it as the turn's end (`finish_reason` stop)
 * or the wire saw that request complete with the stored text streamed. A
 * window reopened on the conversation replays its history with no turn end,
 * so only the store tells that turn ended. Whether Devin is still on a turn
 * is judged by the launch whose wire was written last (a resume opens a new
 * one).
 */
function devinAnswer(chain, outcomes, launches, wires) {
  // The newest window that carried this session says whether it is at work:
  // another session's, written later, says nothing of this one.
  let working = false
  let latestWire = -1
  for (const launch of launches) {
    const wire = wires.get(launch)
    if (wire?.mine && wire.seen.mtimeMs >= latestWire) {
      latestWire = wire.seen.mtimeMs
      working = wire.busy
    }
  }
  const result = resultBase()
  result.asking = chain.asking
  const finalByRequest = new Map(
    chain.items.filter((item) => item.role === 'assistant').map((item) => [item.request, item.id]),
  )
  result.items = chain.items.map(({ request, stopped, ...item }) => {
    if (item.role !== 'assistant') return item
    const outcome = outcomes.get(request)
    // The same text needs no comparing, which every look would do again.
    const complete =
      finalByRequest.get(request) === item.id &&
      (stopped ||
        (outcome?.cause === 'complete' &&
          (outcome.text === item.text ||
            devinComparable(outcome.text) === devinComparable(item.text))))
    return { ...item, complete }
  })
  const lastIndex = chain.items.findLastIndex((item) => item.role !== 'custom')
  const last = lastIndex === -1 ? undefined : result.items[lastIndex]
  const outcome = outcomes.get(chain.items[lastIndex]?.request)
  const cancelled = outcome?.cause === 'cancelled'
  result.failed = outcome?.cause === 'error'
  result.inFlight =
    working || (last?.role === 'assistant' && !last.complete && !cancelled && !result.failed)
  result.settlement = {
    state: working
      ? 'in-flight'
      : last?.complete || cancelled || result.failed
        ? 'settled'
        : 'unknown',
  }
  return result
}
