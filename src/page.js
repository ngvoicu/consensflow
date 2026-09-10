import { basename } from 'node:path'
import { answers as harnessAnswers } from '../hosts/lib/completion.js'
import { cancel, plan, resend } from '../hosts/lib/deliveries.js'
import { effectivePolicy } from '../hosts/lib/policy.js'
import { PaneError } from './panes.js'
import { LEAD_HARNESSES, leadIdentity } from './tabs.js'

/**
 * What the page reads, and the three things it asks of a delivery.
 *
 * `src/panes.js` owns opening and closing windows; this module owns the
 * other half of the bridge surface — one picture of everything the page
 * draws (`state.list`), the answers behind one conversation
 * (`answers.list`), and the human's three verbs over deliveries
 * (`deliver.now`, `deliver.cancel`, `held.send`).
 *
 * Two rules shape it:
 *
 * - **It reads; the watcher writes.** `src/delivery-watch.js` owns delivery
 *   submission, and nothing here submits. `deliver.now` sets the one flag
 *   the watcher already honours — `record.manual`, its own override for a
 *   manual policy — and asks it to run. Two modules writing one state
 *   machine is the bug this avoids.
 * - **The page recomputes policy itself.** `app/ui/panes.js` runs the same
 *   `hosts/lib/policy.js` over the raw `policy` fields, so those are sent
 *   as they are stored. `effectivePolicy` goes along for the fallback path
 *   the page keeps for a state that has no raw fields.
 */
export class Page {
  #store
  #tabs
  #agents
  #env
  #watcher = null

  constructor({ store, tabs, agents, env }) {
    this.#store = store
    this.#tabs = tabs
    this.#agents = agents
    this.#env = env
  }

  /** The delivery watcher, once the app has started it. */
  attachWatcher(watcher) {
    this.#watcher = watcher
    return this
  }

  /**
   * `state.list {}` — everything the page draws, in one answer.
   *
   * Rust wraps this with what only it knows (the live pane table, the
   * roster it was given, whether Node answered at all) in `compose_state`,
   * so nothing here guesses at either.
   *
   * `answers` is deliberately empty: reading a harness's own store is a
   * disk walk per conversation, and the page has `answers.list` to ask for
   * the one it is showing. A state list that did it for every conversation
   * would make opening the app as slow as the slowest transcript on disk.
   */
  async state() {
    const tabs = await this.#tabs.list()
    const drawn = []
    for (const tab of tabs) {
      const threads = await this.#store.readThreads(tab.directory)
      drawn.push({
        id: tab.id,
        role: tab.role ?? 'lead',
        roleName: tab.roleName ?? tab.lead.harness,
        parentTabId: tab.parentTabId ?? null,
        // Derived for display, never stored: the store knows a tab by its
        // directory, and two tabs may share one.
        name: basename(tab.directory),
        directory: tab.directory,
        closed: tab.closed === true,
        deleting: tab.deleting === true,
        ...(tab.policy === undefined ? {} : { policy: tab.policy }),
        lead: {
          harness: tab.lead.harness,
          generation: tab.lead.generation,
          launching: isRecord(tab.lead.reserved) && tab.lead.reserved.resolvedAt === undefined,
          // Whether this tab's own conversation survives a suspend. An
          // unbound lead resumes cold, and the page can say so before the
          // human finds out by losing it.
          nativeSession: tab.lead.nativeSession ?? null,
          bound: typeof tab.lead.nativeSession === 'string' && tab.lead.nativeSession.length > 0,
        },
        panes: tab.panes.map((pane) => this.#pane(tab, pane, threads)),
      })
    }
    return {
      ok: true,
      available: true,
      // The page's harness picker reads this rather than keeping a list of
      // its own: what is offered and what `tab.open` accepts are the same
      // array, so neither can drift into offering a tab that will not open.
      leadHarnesses: [...LEAD_HARNESSES],
      agents: this.#agents.names().map((name) => {
        const row = this.#agents.row(name)
        return { name, harness: row?.kind ?? row?.harness ?? null }
      }),
      tabs: drawn,
      deliveries: await this.#deliveries(tabs),
      held: await this.#held(tabs),
      answers: {},
    }
  }

  /**
   * `answers.list {tab, pane, conversation}` — what this agent has said.
   *
   * Read from the harness's OWN store, which is the only lossless record of
   * it, and only for a conversation that is bound to a native session: an
   * unbound conversation has not started talking yet, and that is an empty
   * list rather than an error.
   */
  async answersList(request) {
    const conversation = requireText(request?.conversation, 'conversation')
    const tab = await this.#tab(requireText(request?.tab, 'tab'))
    const threads = await this.#store.readThreads(tab.directory)
    const row = threads[conversation]
    if (!isRecord(row)) {
      throw new PaneError(`no conversation named ${conversation} here`, {
        status: 404,
        code: 'no-conversation',
      })
    }
    this.#requireConversationTab(tab, row, conversation)
    if (typeof row.sessionId !== 'string' || row.sessionId.length === 0) {
      return { ok: true, conversation, answers: [], unknown: false, reason: 'not bound yet' }
    }
    const found = await harnessAnswers(row.kind, row.sessionId, this.#env)
    if (found?.unknown === true) {
      return { ok: true, conversation, answers: [], unknown: true, reason: found.reason }
    }
    const outcomes = await this.#deliveryOutcomes(tab.directory, conversation)
    return {
      ok: true,
      conversation,
      unknown: false,
      // The adapter returns the whole transcript as `items`, each with a
      // `role` of user, assistant or tool. Only the assistant's are
      // answers: listing the lead's own prompts back to it as things to
      // deliver is a list nobody can use. An item the adapter could not
      // confirm was finished is offered with that said, not hidden.
      answers: (found?.items ?? [])
        .filter((item) => item?.role === 'assistant')
        .map((item) => ({
          id: item.id,
          preview: preview(item.text ?? ''),
          ready: item.complete === true && item.settled === true,
          delivered: outcomes.accepted.has(item.id),
          // Two different unknowns, and the human needs both: an answer the
          // harness has not finished writing, and one whose delivery may or
          // may not have reached the lead. Reporting the second as settled
          // says plainly "this did not arrive" about something that might
          // have — and a second copy is worse than a missing one.
          uncertain: item.complete !== true || outcomes.uncertain.has(item.id),
          ...(!outcomes.accepted.has(item.id) && outcomes.parts.has(item.id)
            ? { partProgress: outcomes.parts.get(item.id) }
            : {}),
        })),
    }
  }

  /**
   * `deliver.now {delivery, tab, conversation, answerId, resend}` — the
   * human overriding a manual policy for one answer.
   *
   * The watcher already has this override: a record carrying `manual: true`
   * is not cancelled by a manual policy (`src/delivery-watch.js:696`). So
   * this finds the record the watcher made, marks it, and asks the watcher
   * to run. `resend` builds a fresh record for an answer already delivered.
   */
  async deliverNow(request) {
    const watcher = this.#requireWatcher()
    const found = await this.#find(request)
    if (found.record === null) return await this.#planManual(found)

    const { workspace, record } = found
    // An id for the resend, allocated BEFORE the decision: the allocator is
    // itself a queued mutation and cannot run inside one. An id decided
    // against and never written is simply spent — they are never reused.
    const fresh = await this.#store.allocateDeliveryId()
    const decided = await this.#store.deliveryDecide(workspace, {
      id: record.id,
      // Chosen against the record as it IS. A record that reached a
      // terminal state while this click was in flight keeps it, and the
      // human's send becomes a new record rather than a rewritten one.
      decide: (current) => {
        if (current === undefined) return undefined
        if (current.state === 'pending' && request?.resend !== true) {
          return current.manual === true ? current : { ...current, manual: true }
        }
        return { ...resend(current, { id: fresh, now: Date.now(), workspace }), manual: true }
      },
    })
    if (decided.record === null) {
      throw new PaneError(`no delivery ${record.id}`, { status: 404, code: 'no-delivery' })
    }
    await watcher.reconcile('deliver.now')
    return {
      ok: true,
      delivery: decided.record.id,
      state: decided.record.state,
      resent: decided.record.id !== record.id,
    }
  }

  /**
   * The human sending an answer the watcher never planned.
   *
   * Under a manual policy the watcher plans NOTHING — that is what manual
   * means — so there is no record to mark and "send this one now" had
   * nothing to act on. The plan is made here with `manual: true`, from the
   * same module the watcher plans with, and the watcher still decides
   * whether it may go: this chooses WHICH answer, never whether.
   */
  async #planManual({ workspace, tab, conversation, answerId, row }) {
    const watcher = this.#requireWatcher()
    const found = await harnessAnswers(row.kind, row.sessionId, this.#env)
    const item = (found?.items ?? []).find((candidate) => candidate?.id === answerId)
    if (item === undefined) {
      throw new PaneError(`no answer ${answerId} in ${conversation}`, {
        status: 404,
        code: 'no-answer',
      })
    }
    if (item.role !== 'assistant') {
      throw new PaneError(`${answerId} in ${conversation} is not an assistant answer`, {
        status: 409,
        code: 'answer-not-assistant',
      })
    }
    if (item.complete !== true || item.settled !== true) {
      throw new PaneError(`${answerId} in ${conversation} is not settled`, {
        status: 409,
        code: 'answer-incomplete',
      })
    }
    const session = tab.lead?.nativeSession
    if (typeof session !== 'string' || session.length === 0) {
      // Nothing can be delivered to a lead nobody has bound: the receipt
      // that proves it arrived is read from the lead's own transcript.
      throw new PaneError(`the lead of ${tab.id} has no bound session to deliver to`, {
        status: 409,
        code: 'lead-unbound',
      })
    }
    const leadPane = tab.panes.find((pane) => pane.kind === 'lead')
    const minted = await this.#store.allocateDeliveryId()
    const records = plan({
      row,
      items: [item],
      policy: { mode: 'manual' },
      tab,
      pane: tab.panes.find((pane) => pane.conversation === conversation) ?? null,
      kind: row.kind,
      conversation,
      agent: row.agent,
      target: {
        leadId: leadIdentity(tab),
        session,
        tab: tab.id,
        pane: leadPane.id,
        generation: tab.lead.generation,
      },
      newId: () => minted,
      now: Date.now(),
      workspace,
      manual: true,
      deliveries: Object.values(await this.#store.readDeliveries(workspace)).filter(isRecord),
    })
    if (records.length === 0) {
      throw new PaneError(`${answerId} is already covered by a delivery`, {
        status: 409,
        code: 'already-planned',
      })
    }
    const record = records[0]
    await this.#store.deliveryDecide(workspace, {
      id: record.id,
      decide: (current) => (current === undefined ? record : current),
    })
    await watcher.reconcile('deliver.now')
    return { ok: true, delivery: record.id, state: record.state, resent: false, planned: true }
  }

  /** `deliver.cancel {delivery}` — the human stopping one. */
  async deliverCancel(request) {
    const delivery = requireText(request?.delivery, 'delivery')
    const found = await this.#findById(delivery)
    // Decided inside the queue, against the record as it is: `cancel` is a
    // transition and an illegal one changes nothing, so an acceptance that
    // landed a moment ago survives the click that raced it.
    const decided = await this.#store.deliveryDecide(found.workspace, {
      id: delivery,
      decide: (current) =>
        current === undefined ? undefined : cancel(current, { reason: 'cancelled by the human' }),
    })
    if (decided.record === null) {
      throw new PaneError(`no delivery ${delivery}`, { status: 404, code: 'no-delivery' })
    }
    return { ok: true, delivery, state: decided.record.state, changed: decided.changed }
  }

  /** `held.send {tab}` — the answers a previous lead never received. */
  async heldSend(request) {
    const tab = requireText(request?.tab, 'tab')
    const watcher = this.#requireWatcher()
    let sent
    try {
      sent = await watcher.sendHeld(tab)
    } catch (cause) {
      // The watcher refuses in prose — no live lead to receive them, most
      // often. That is the person's business, not a fault. A `TypeError`
      // out of the same module IS a fault and goes up as one: the same line
      // `src/panes.js` draws for the store's refusals.
      if (!isRefusal(cause)) throw cause
      throw new PaneError(cause.message, { status: 409, code: 'held-refused' })
    }
    return { ok: true, tab, ...(isRecord(sent) ? sent : { sent }) }
  }

  // --- the reading behind them ---------------------------------------------

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

  async #deliveries(tabs) {
    const latest = new Map()
    const isLater = (record, previous) => {
      const createdAt = Number(record.createdAt)
      const previousCreatedAt = Number(previous.createdAt)
      if (
        Number.isFinite(createdAt) &&
        Number.isFinite(previousCreatedAt) &&
        createdAt !== previousCreatedAt
      ) {
        return createdAt > previousCreatedAt
      }
      const numericId = Number(String(record.id ?? '').replace(/^d-/, ''))
      const previousNumericId = Number(String(previous.id ?? '').replace(/^d-/, ''))
      if (Number.isFinite(numericId) && Number.isFinite(previousNumericId)) {
        return numericId > previousNumericId
      }
      return true
    }
    for (const workspace of workspaces(tabs)) {
      for (const record of Object.values(await this.#store.readDeliveries(workspace))) {
        if (!isRecord(record)) continue
        // A delivery is aimed at a LEAD, so `plan()` puts the tab and the
        // pane on `target` — the pane that produced the answer is not where
        // it is going. Reading them off the record itself finds nothing,
        // and the page's pane filter then hides a perfectly valid badge.
        const target = record.target ?? {}
        const tab = target.tab ?? record.tab ?? null
        const pane = target.pane ?? record.pane ?? null
        const generation = target.generation ?? record.generation ?? null
        const answerKey =
          record.answerId === undefined || record.answerId === null
            ? `delivery:${record.id}`
            : `${tab}\u0000${pane}\u0000${generation}\u0000${record.conversation ?? ''}\u0000${record.answerId}`
        const previous = latest.get(answerKey)
        if (previous !== undefined && !isLater(record, previous)) continue
        latest.set(answerKey, {
          id: record.id,
          tab,
          pane,
          generation,
          conversation: record.conversation ?? null,
          agent: record.agent ?? null,
          answerId: record.answerId ?? null,
          state: record.suspended === true ? 'suspended' : (record.state ?? 'unknown'),
          ...(record.reason === undefined ? {} : { reason: record.reason }),
        })
      }
    }
    return [...latest.values()]
  }

  async #held(tabs) {
    if (this.#watcher === null) return []
    const listed = []
    for (const tab of tabs) {
      const held = await this.#watcher.held(tab.id)
      for (const record of held?.records ?? []) {
        listed.push({ id: record.id, tab: tab.id, answerId: record.answerId ?? null })
      }
    }
    return listed
  }

  /** What this conversation's deliveries say about each answer. */
  async #deliveryOutcomes(workspace, conversation) {
    const accepted = new Set()
    const uncertain = new Set()
    const parts = new Map()
    for (const record of Object.values(await this.#store.readDeliveries(workspace))) {
      if (!isRecord(record) || record.conversation !== conversation) continue
      if (record.answerId === undefined || record.answerId === null) continue
      if (record.state === 'accepted') accepted.add(record.answerId)
      if (record.state === 'uncertain') uncertain.add(record.answerId)
      if (record.channel === 'cf-read' && Array.isArray(record.parts) && record.parts.length > 0) {
        const previous = parts.get(record.answerId)
        if (
          previous === undefined ||
          Number(record.id.slice(2)) > Number(previous.delivery.slice(2))
        ) {
          parts.set(record.answerId, {
            delivery: record.id,
            total: record.parts.length,
            uncovered: record.parts.flatMap((part, index) =>
              Array.isArray(record.partCoverage?.[index]) && record.partCoverage[index].length > 0
                ? []
                : [part.k],
            ),
          })
        }
      }
    }
    // An answer accepted by a later attempt is delivered, whatever an
    // earlier uncertain one said.
    for (const id of accepted) uncertain.delete(id)
    return { accepted, uncertain, parts }
  }

  /**
   * The delivery this request is about, and everything needed to make one
   * if it does not exist. A missing record is NOT a refusal: under a manual
   * policy the watcher plans nothing, so "no record" is the ordinary state
   * of every answer the human might want to send.
   */
  async #find(request) {
    if (typeof request?.delivery === 'string' && request.delivery.length > 0) {
      const byId = await this.#findById(request.delivery, { required: false })
      if (byId === null) {
        throw new PaneError(`no delivery ${request.delivery}`, {
          status: 404,
          code: 'no-delivery',
        })
      }
      const { workspace, record } = byId
      const expected = {
        tab: record.target?.tab ?? record.tab,
        conversation: record.conversation,
        answerId: record.answerId,
      }
      for (const field of ['tab', 'conversation', 'answerId']) {
        if (request[field] !== undefined && request[field] !== expected[field]) {
          throw new PaneError(
            `delivery ${request.delivery} does not match ${field} ${JSON.stringify(request[field])}`,
            { status: 400, code: 'delivery-mismatch' },
          )
        }
      }
      const tab = await this.#tab(expected.tab)
      if (tab.directory !== workspace) {
        throw new PaneError(`delivery ${request.delivery} does not match tab ${tab.id}`, {
          status: 400,
          code: 'delivery-mismatch',
        })
      }
      const threads = await this.#store.readThreads(workspace)
      const row = threads[expected.conversation]
      if (!isRecord(row)) {
        throw new PaneError(`no conversation named ${expected.conversation} here`, {
          status: 404,
          code: 'no-conversation',
        })
      }
      this.#requireConversationTab(tab, row, expected.conversation)
      return {
        workspace,
        record,
        tab,
        conversation: expected.conversation,
        answerId: expected.answerId,
        row: { ...row, name: expected.conversation },
      }
    }
    const tab = await this.#tab(requireText(request?.tab, 'tab'))
    const answerId = requireText(request?.answerId, 'answerId')
    const conversation = requireText(request?.conversation, 'conversation')
    const threads = await this.#store.readThreads(tab.directory)
    const row = threads[conversation]
    if (!isRecord(row)) {
      throw new PaneError(`no conversation named ${conversation} here`, {
        status: 404,
        code: 'no-conversation',
      })
    }
    this.#requireConversationTab(tab, row, conversation)
    const records = await this.#store.readDeliveries(tab.directory)
    const record = Object.values(records).find(
      (candidate) =>
        isRecord(candidate) &&
        candidate.answerId === answerId &&
        candidate.conversation === conversation &&
        (candidate.target?.tab ?? candidate.tab) === tab.id &&
        candidate.state !== 'cancelled',
    )
    return {
      workspace: tab.directory,
      record: record ?? null,
      tab,
      conversation,
      answerId,
      row: { ...row, name: conversation },
    }
  }

  #requireConversationTab(tab, row, conversation) {
    const owned =
      (typeof row.lead === 'string' && row.lead.startsWith(`tab:${tab.id}:`)) ||
      (isRecord(row.reserved) && row.reserved.tab === tab.id) ||
      (tab.panes ?? []).some(
        (pane) => isRecord(pane) && pane.kind === 'worker' && pane.conversation === conversation,
      )
    if (owned) return
    throw new PaneError(`conversation ${conversation} does not belong to tab ${tab.id}`, {
      status: 409,
      code: 'conversation-mismatch',
    })
  }

  async #findById(delivery, { required = true } = {}) {
    for (const workspace of workspaces(await this.#tabs.list())) {
      const record = (await this.#store.readDeliveries(workspace))[delivery]
      if (isRecord(record)) return { workspace, record }
    }
    if (!required) return null
    throw new PaneError(`no delivery ${delivery}`, { status: 404, code: 'no-delivery' })
  }

  async #tab(tabId) {
    const tab = await this.#tabs.get(tabId)
    if (tab === null) throw new PaneError(`no tab ${tabId}`, { status: 404, code: 'no-tab' })
    return tab
  }

  #requireWatcher() {
    if (this.#watcher === null) {
      throw new PaneError('the delivery watcher is not running', {
        status: 503,
        code: 'no-watcher',
      })
    }
    return this.#watcher
  }
}

const isRecord = (value) => value !== null && typeof value === 'object' && !Array.isArray(value)

/** A refusal another module made about the request, not a fault of its own. */
const isRefusal = (cause) =>
  cause instanceof Error &&
  cause.name === 'Error' &&
  cause.errno === undefined &&
  cause.syscall === undefined

/** One directory per workspace, however many tabs share it. */
const workspaces = (tabs) => new Set(tabs.map((tab) => tab.directory))

function preview(text) {
  const flat = String(text).replace(/\s+/g, ' ').trim()
  return flat.length > 160 ? `${flat.slice(0, 159)}…` : flat
}

function requireText(value, what) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new PaneError(`${what} is required`, { status: 400, code: 'bad-request' })
  }
  return value
}
