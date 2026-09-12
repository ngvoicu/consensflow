import { basename } from 'node:path'
import { resultStatus } from '../hosts/lib/inbox.js'
import { effectivePolicy } from '../hosts/lib/policy.js'
import { PaneError } from './panes.js'
import { LEAD_HARNESSES } from './tabs.js'

/** Shared page projection. Human viewing never creates a model receipt. */
export class Page {
  #store
  #tabs
  #agents
  constructor({ store, tabs, agents }) {
    this.#store = store
    this.#tabs = tabs
    this.#agents = agents
  }
  async state() {
    const tabs = await this.#tabs.list()
    const drawn = [],
      results = []
    const visited = new Set()
    for (const tab of tabs) {
      const threads = await this.#store.readThreads(tab.directory)
      drawn.push({
        id: tab.id,
        role: tab.role ?? 'lead',
        roleName: tab.roleName ?? tab.lead.harness,
        parentTabId: tab.parentTabId ?? null,
        name: tab.name ?? basename(tab.directory),
        directory: tab.directory,
        closed: tab.closed === true,
        deleting: tab.deleting === true,
        ...(tab.policy === undefined ? {} : { policy: tab.policy }),
        lead: {
          harness: tab.lead.harness,
          generation: tab.lead.generation,
          launching: isRecord(tab.lead.reserved) && tab.lead.reserved.resolvedAt === undefined,
          nativeSession: tab.lead.nativeSession ?? null,
          bound: Boolean(tab.lead.nativeSession),
        },
        panes: tab.panes.map((pane) => this.#pane(tab, pane, threads)),
      })
      if (visited.has(tab.directory)) continue
      visited.add(tab.directory)
      for (const result of Object.values((await this.#store.readInbox(tab.directory)).results)) {
        if (!tabs.some((owner) => owner.id === result.owner)) continue
        results.push(project(result))
      }
    }
    return {
      ok: true,
      available: true,
      leadHarnesses: [...LEAD_HARNESSES],
      agents: this.#agents.names().map((name) => ({
        name,
        harness: this.#agents.row(name)?.kind ?? this.#agents.row(name)?.harness ?? null,
      })),
      tabs: drawn,
      results: results.sort((a, b) => a.createdAt - b.createdAt),
      answers: {},
    }
  }
  async answersList(request) {
    const tab = await this.#tab(request.tab)
    const conversation = requireText(request.conversation, 'conversation')
    const state = await this.#store.readInbox(tab.directory)
    const rows = await this.#store.readThreads(tab.directory)
    const results = Object.values(state.results).filter(
      (r) => r.owner === tab.id && r.conversation === conversation,
    )
    if (!results.length && !rows[conversation]?.lead?.startsWith(`tab:${tab.id}:`))
      throw new PaneError('conversation does not belong to this coordinator', { status: 403 })
    return {
      ok: true,
      conversation,
      unknown: false,
      answers: results.map((r) => ({
        ...project(r),
        id: r.answerId,
        result: r.id,
        ready: true,
        delivered: resultStatus(r) === 'received',
        uncertain: resultStatus(r) === 'uncertain',
      })),
    }
  }
  async resultBody(request) {
    const { result } = await this.#result(request)
    const offset = request.offset ?? 0
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > result.answer.length)
      throw new PaneError('invalid result offset')
    const end = Math.min(offset + 8000, result.answer.length)
    return {
      ok: true,
      result: result.id,
      text: result.answer.slice(offset, end),
      next: end < result.answer.length ? end : null,
    }
  }
  async collectResult(request) {
    return this.#change(request, (result) => {
      result.requested = true
    })
  }
  async cancelResult(request) {
    return this.#change(request, (result) => {
      result.cancelledAt = Date.now()
      delete result.requested
    })
  }
  async #change(request, change) {
    const { tab, result } = await this.#result(request)
    return this.#store.mutate(tab.directory, 'inbox.request', async (io) => {
      const state = await io.readInbox()
      const current = state.results[result.id]
      if (current?.owner !== tab.id)
        throw new PaneError('result does not belong to this coordinator')
      if (resultStatus(current) !== 'waiting')
        throw new PaneError('only a waiting result can be requested or cancelled', { status: 409 })
      change(current)
      await io.writeInbox(state)
      return { ok: true, result: current.id, state: resultStatus(current) }
    })
  }
  async #result(request) {
    const tab = await this.#tab(request.tab)
    const id = requireText(request.result, 'result')
    const result = (await this.#store.readInbox(tab.directory)).results[id]
    if (result?.owner !== tab.id)
      throw new PaneError('result does not belong to this coordinator', { status: 404 })
    return { tab, result }
  }
  async #tab(id) {
    const tab = await this.#tabs.get(requireText(id, 'tab'))
    if (!tab) throw new PaneError('coordinator no longer exists', { status: 404 })
    return tab
  }
  #pane(tab, pane, threads) {
    const row = pane.conversation === null ? undefined : threads[pane.conversation]
    const reserved = isRecord(row?.reserved) ? row.reserved : null
    const progress =
      isRecord(row?.progress) &&
      row.progress.pane === pane.id &&
      row.progress.generation === pane.generation
        ? row.progress
        : undefined
    const alive =
      tab.closed !== true &&
      pane.closed !== true &&
      !pane.failure &&
      (pane.kind === 'lead'
        ? isRecord(tab.lead.reserved) && tab.lead.reserved.resolvedAt !== undefined
        : pane.kind === 'shell' || reserved === null || reserved.resolvedAt !== undefined)
    return {
      id: pane.id,
      generation: pane.generation,
      kind: pane.kind,
      order: pane.order,
      alive,
      // An open lead is published before its native launch resolves. A dead
      // lead closes its tab; an explicit worker failure has its own record.
      ...(!alive && tab.closed !== true && pane.closed !== true && !pane.failure
        ? { starting: true }
        : {}),
      ...(pane.failure ? { failure: pane.failure } : {}),
      ...(progress === undefined ? {} : { progress }),
      ...(pane.conversation === null
        ? {}
        : { conversation: pane.conversation, name: pane.conversation }),
      ...(row === undefined ? {} : { agent: row.agent, harness: row.kind }),
      ...(pane.policy === undefined ? {} : { policy: pane.policy }),
      effectivePolicy: effectivePolicy(tab, pane, row),
    }
  }
}

const isRecord = (value) => value !== null && typeof value === 'object' && !Array.isArray(value)
function requireText(value, name) {
  if (typeof value !== 'string' || !value) throw new PaneError(`${name} is required`)
  return value
}
function project(result) {
  const state = resultStatus(result)
  return {
    id: result.id,
    tab: result.owner,
    conversation: result.conversation,
    agent: result.agent,
    answerId: result.answerId,
    kind: result.kind,
    createdAt: result.createdAt,
    state,
    requested: result.requested === true,
    bytes: Buffer.byteLength(result.answer),
    preview: result.answer.replace(/\s+/g, ' ').slice(0, 160),
    parts: result.parts.length,
    receivedParts: result.legacyReceived
      ? result.parts.length
      : Math.max(
          0,
          ...result.claims.map(
            (claim) =>
              new Set(
                result.claims
                  .filter(
                    (c) => c.receiver.lease === claim.receiver.lease && c.state === 'received',
                  )
                  .map((c) => c.part),
              ).size,
          ),
        ),
  }
}
