import fs from 'node:fs/promises'
import path from 'node:path'
import { answers, itemsAfterCursor } from '../hosts/lib/completion.js'
import {
  cancel,
  fail,
  plan,
  receipt,
  recover,
  seenAfter,
  submit,
  writeFailed,
} from '../hosts/lib/deliveries.js'
import { effectivePolicy } from '../hosts/lib/policy.js'
import { leadReady } from '../hosts/lib/readiness.js'
import { deliver, enabledChannels } from './channels.js'
import { leadIdentity } from './tabs.js'

export const HELD_ACTION = 'Send held answers to this lead'

/**
 * The worker transcript has one bookkeeping projection everywhere it is
 * walked. Tool results are evidence for lead-side receipts, but they are not
 * worker answers, are never printed, and can never be delivery-covered.
 */
export function bookkeepingItems(items) {
  return Array.isArray(items) ? items.filter((item) => item?.role !== 'tool') : []
}

const sameTarget = (left, right) =>
  left?.leadId === right?.leadId &&
  left?.session === right?.session &&
  left?.tab === right?.tab &&
  left?.pane === right?.pane &&
  left?.generation === right?.generation

const isRecord = (value) => value !== null && typeof value === 'object' && !Array.isArray(value)

const describe = (cause) => (cause instanceof Error ? cause.message : String(cause))

function targetFor(tab) {
  const pane = tab?.panes?.find((candidate) => candidate?.kind === 'lead')
  const session = tab?.lead?.nativeSession
  if (tab?.lead?.replaced?.generation === tab?.lead?.generation) return null
  if (!pane || typeof session !== 'string' || session.length === 0) return null
  return {
    leadId: leadIdentity(tab),
    session,
    tab: tab.id,
    pane: pane.id,
    generation: tab.lead.generation,
  }
}

function generationForLead(row, tab) {
  const prefix = `tab:${tab?.id}:`
  if (typeof row?.lead !== 'string' || !row.lead.startsWith(prefix)) return null
  const generation = Number(row.lead.slice(prefix.length))
  return Number.isInteger(generation) && generation > 0 ? generation : null
}

function targetForRow(tab, row, records) {
  const current = targetFor(tab)
  if (current === null || row?.lead === current.leadId) return current
  const generation = generationForLead(row, tab)
  const pane = tab?.panes?.find((candidate) => candidate?.kind === 'lead')
  if (generation === null || !pane) return null
  const known = records.find((record) => record?.target?.leadId === row.lead)?.target
  return (
    known ?? {
      leadId: row.lead,
      session: `held:${row.lead}`,
      tab: tab.id,
      pane: pane.id,
      generation,
    }
  )
}

function deliveryRoute(route, target, record, bridge, epoch) {
  const stored = route?.channel
  if (isRecord(stored)) {
    return {
      channel: stored.kind,
      target: {
        ...target,
        bridge,
        epoch,
        enabledChannels: enabledChannels(route.harness),
        launch: stored,
        channel: stored,
      },
    }
  }
  return {
    channel: record.channel,
    target: { ...target, bridge, epoch },
  }
}

function belongsToTab(row, tab) {
  return typeof row?.lead === 'string' && row.lead.startsWith(`tab:${tab.id}:`)
}

function bound(row) {
  return (
    isRecord(row) &&
    isRecord(row.binding) &&
    typeof row.sessionId === 'string' &&
    row.sessionId.length > 0
  )
}

function workerPane(tab, conversation) {
  return (
    tab?.panes?.find((pane) => pane?.kind === 'worker' && pane.conversation === conversation) ?? {}
  )
}

function latestSubmittedCursor(records, target, excludingId = null) {
  let latest = null
  let latestIndex = -1
  for (const [index, record] of records.entries()) {
    if (record?.id === excludingId || !sameTarget(record?.target, target)) continue
    if (record?.state === 'failed') continue
    if (!Number.isFinite(record?.submittedAt) || record?.snapshot?.cursor == null) continue
    const order = Number.isSafeInteger(record.submissionOrder) ? record.submissionOrder : null
    const latestOrder = Number.isSafeInteger(latest?.submissionOrder)
      ? latest.submissionOrder
      : null
    if (
      latest === null ||
      (order !== null && latestOrder === null) ||
      (order !== null && latestOrder !== null && order > latestOrder) ||
      (order === latestOrder && record.submittedAt > latest.submittedAt) ||
      (order === latestOrder && record.submittedAt === latest.submittedAt && index > latestIndex)
    ) {
      latest = record
      latestIndex = index
    }
  }
  return latest?.snapshot?.cursor
}

function nextSubmissionOrder(records) {
  let latest = 0
  for (const record of records) {
    if (Number.isSafeInteger(record?.submissionOrder)) {
      latest = Math.max(latest, record.submissionOrder)
    }
  }
  if (latest >= Number.MAX_SAFE_INTEGER) throw new Error('the submission order is exhausted')
  return latest + 1
}

function changed(before, after) {
  return JSON.stringify(before) !== JSON.stringify(after)
}

function timestamp(value) {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value !== 'string') return null
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) ? parsed : null
}

function waitGraceMs(env) {
  const parsed = Number.parseInt(env.CONSENSFLOW_WAIT_GRACE_MS ?? '4000', 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0
}

function waitsForSentQuestion(row, items, now, graceMs) {
  const sentAt = timestamp(Array.isArray(row?.sent) ? row.sent.at(-1)?.at : null)
  if (sentAt === null || now - sentAt >= graceMs) return false
  return !items.some((item) => item?.role === 'user' && timestamp(item.at) >= sentAt)
}

/**
 * Reconciles durable completion and delivery state for the life of the app.
 * It owns no process globals: the app supplies its Store, Tabs, environment,
 * clock and bridge, and starts it once during app launch.
 */
export class Watcher {
  constructor({
    store,
    tabs,
    env,
    now = Date.now,
    floorMs = 1_000,
    receiptMs,
    inlineBudget,
    partBudget,
    onError = null,
  } = {}) {
    if (!store || typeof store.readThreads !== 'function') {
      throw new Error('Watcher needs the app Store')
    }
    if (!tabs || typeof tabs.list !== 'function') throw new Error('Watcher needs Tabs')
    if (!isRecord(env)) throw new Error('Watcher needs an explicit env object')
    if (typeof now !== 'function') throw new Error('Watcher needs a clock function')
    if (!Number.isFinite(floorMs) || floorMs <= 0) {
      throw new Error('Watcher floorMs must be a positive number')
    }
    this.store = store
    this.tabs = tabs
    this.env = env
    this.now = now
    this.floorMs = floorMs
    this.receiptMs = receiptMs
    this.inlineBudget = inlineBudget
    this.partBudget = partBudget
    this.onError = typeof onError === 'function' ? onError : () => {}
    this.bridge = null
    this.started = false
    this.closed = false
    this.timer = null
    this.unsubscribe = []
    this.tail = Promise.resolve()
  }

  attachBridge(bridge) {
    if (this.closed) throw new Error('Watcher is closed')
    if (!bridge || typeof bridge.request !== 'function' || typeof bridge.onEvent !== 'function') {
      throw new Error('Watcher needs a bridge with request and onEvent')
    }
    for (const unsubscribe of this.unsubscribe.splice(0)) unsubscribe()
    this.bridge = bridge
    const hint = (reason) => {
      if (!this.started || this.closed) return
      void this.reconcile(reason).catch((cause) => this.onError(cause))
    }
    this.unsubscribe.push(bridge.onEvent('pane.idle', () => hint('pane.idle')))
    this.unsubscribe.push(bridge.onEvent('pane.exit', () => hint('pane.exit')))
    return this
  }

  async start() {
    if (this.closed) throw new Error('Watcher is closed')
    if (this.started) return await this.tail
    this.started = true
    this.timer = setInterval(() => {
      void this.reconcile('floor').catch((cause) => this.onError(cause))
    }, this.floorMs)
    this.timer.unref?.()
    return await this.#enqueue(true)
  }

  async reconcile(_reason = 'reconcile') {
    if (this.closed) return
    return await this.#enqueue(false)
  }

  async close() {
    if (this.closed) return await this.tail
    this.closed = true
    this.started = false
    if (this.timer !== null) clearInterval(this.timer)
    this.timer = null
    for (const unsubscribe of this.unsubscribe.splice(0)) unsubscribe()
    return await this.tail
  }

  async held(tabId) {
    const tab = await this.tabs.get(tabId)
    if (!tab) throw new Error(`unknown tab ${tabId}`)
    const target = targetFor(tab)
    const all = Object.values(await this.store.readDeliveries(tab.directory))
    const copiedToCurrent = new Set(
      all
        .filter((record) => target !== null && sameTarget(record?.target, target))
        .map((record) => `${record.conversation}\u0000${record.answerId}`),
    )
    const candidates = all.filter(
      (record) =>
        record?.state === 'pending' &&
        record?.target?.tab === tab.id &&
        target !== null &&
        !sameTarget(record.target, target) &&
        !copiedToCurrent.has(`${record.conversation}\u0000${record.answerId}`),
    )
    const byAnswer = new Map()
    for (const record of candidates) {
      const key = `${record.conversation}\u0000${record.answerId}`
      const previous = byAnswer.get(key)
      if (
        previous === undefined ||
        record.target.generation > previous.target.generation ||
        (record.target.generation === previous.target.generation &&
          record.createdAt >= previous.createdAt)
      ) {
        byAnswer.set(key, record)
      }
    }
    return { action: HELD_ACTION, records: [...byAnswer.values()] }
  }

  async sendHeld(tabId) {
    if (this.closed) throw new Error('Watcher is closed')
    return await this.#serial(async () => {
      if (this.closed) throw new Error('Watcher is closed')
      const tab = await this.tabs.get(tabId)
      if (!tab) throw new Error(`unknown tab ${tabId}`)
      const target = targetFor(tab)
      if (target === null || tab.closed === true) {
        throw new Error(`tab ${tabId} has no live lead to receive held answers`)
      }
      const held = (await this.held(tabId)).records
      const created = []
      for (const old of held) {
        if (this.closed) throw new Error('Watcher is closed')
        const id = await this.store.allocateDeliveryId()
        if (this.closed) throw new Error('Watcher is closed')
        const [fresh] = plan({
          items: [
            {
              id: old.answerId,
              role: 'assistant',
              text: old.answer,
              complete: true,
            },
          ],
          policy: { mode: 'manual' },
          kind: tab.lead.harness,
          conversation: old.conversation,
          agent: old.agent,
          target,
          newId: () => id,
          now: this.now(),
          workspace: tab.directory,
          manual: true,
          deliveries: [],
          inlineBudget: this.inlineBudget,
          partBudget: this.partBudget,
        })
        const record = { ...fresh, heldOf: old.id }
        await this.#persist(tab.directory, record)
        created.push(record)
      }
      return created
    })
  }

  #enqueue(restart) {
    return this.#serial(() => this.#run(restart))
  }

  #serial(operation) {
    const run = this.tail.then(operation)
    this.tail = run.then(
      () => undefined,
      () => undefined,
    )
    return run
  }

  async #run(restart) {
    if (this.closed) return
    const tabs = await this.tabs.list()
    if (restart) await this.#recoverSubmissions(tabs)
    for (const tab of tabs) {
      if (this.closed) return
      try {
        await this.#reconcileTab(tab)
      } catch (cause) {
        this.onError(cause)
      }
    }
  }

  async #recoverSubmissions(tabs) {
    const workspaces = new Set(tabs.map((tab) => tab?.directory).filter(Boolean))
    for (const workspace of workspaces) {
      if (this.closed) return
      const records = Object.values(await this.store.readDeliveries(workspace))
      for (const record of records) {
        if (this.closed) return
        if (record?.state !== 'submitting') continue
        const recovered = recover(record)
        await this.#persist(workspace, recovered)
      }
    }
  }

  async #reconcileTab(tabAtStart) {
    let tab = tabAtStart
    if (!tab?.directory) return
    let threads = await this.store.readThreads(tab.directory)
    const records = Object.values(await this.store.readDeliveries(tab.directory))
    const completions = new Map()

    const repairedWorkers = new Set()
    for (const record of records) {
      const invalidated = record?.workerInvalidated
      if (!isRecord(invalidated) || repairedWorkers.has(record.conversation)) continue
      const row = threads[record.conversation]
      if (!bound(row) || row.sessionId !== invalidated.previousSession) continue
      repairedWorkers.add(record.conversation)
      await this.#replace(tab, record.conversation, row, invalidated, records)
    }
    if (repairedWorkers.size > 0) threads = await this.store.readThreads(tab.directory)

    const currentTarget = targetFor(tab)
    const interruptedInvalidation = records.find(
      (record) =>
        isRecord(record?.leadInvalidated) &&
        currentTarget !== null &&
        sameTarget(record.target, currentTarget),
    )
    if (interruptedInvalidation) {
      await this.#invalidateLead(
        tab,
        currentTarget,
        interruptedInvalidation.leadInvalidated.reason,
        records,
      )
      tab = await this.tabs.get(tab.id)
      if (!tab) return
    }

    for (const [conversation, row] of Object.entries(threads)) {
      if (!belongsToTab(row, tab) || !bound(row)) continue
      const completion = await answers(row.kind, row.sessionId, this.env)
      completions.set(conversation, completion)
      if (completion?.replaced === true) {
        await this.#replace(tab, conversation, row, completion, records)
        continue
      }
      const target = targetForRow(tab, row, records)
      if (target === null) continue
      const pane = workerPane(tab, conversation)
      const policy = effectivePolicy(tab, pane, row)
      if (policy.mode !== 'auto') continue
      const items = bookkeepingItems(completion?.items)
      if (waitsForSentQuestion(row, items, this.now(), waitGraceMs(this.env))) continue
      const planItems = items.filter(
        (item) =>
          item?.role !== 'assistant' ||
          item.complete !== true ||
          item.settled === true ||
          (row.kind === 'pi' && completion?.settlement?.state === 'settled'),
      )
      const candidates = planItems.filter(
        (item) =>
          item?.role === 'assistant' &&
          item.complete === true &&
          !records.some(
            (record) =>
              record?.conversation === conversation &&
              record?.answerId === item.id &&
              record?.state !== 'failed',
          ) &&
          !(row.seen?.[target.leadId] ?? []).includes(item.id),
      )
      if (candidates.length === 0) continue
      const ids = []
      for (let index = 0; index < candidates.length; index++) {
        ids.push(await this.store.allocateDeliveryId())
      }
      const planned = plan({
        row,
        items: planItems,
        policy,
        pane,
        kind: tab.lead.harness,
        conversation,
        agent: row.agent,
        target,
        newId: () => ids.shift(),
        now: this.now(),
        workspace: tab.directory,
        coveredIds: row.seen?.[target.leadId] ?? [],
        deliveries: records,
        inlineBudget: this.inlineBudget,
        partBudget: this.partBudget,
      })
      for (const record of planned) {
        await this.#persist(tab.directory, record)
        records.push(record)
      }
    }

    for (let index = 0; index < records.length; index++) {
      const record = records[index]
      if (record?.target?.tab !== tab.id || record.state !== 'submitting') continue
      const next = await this.#receipt(tab, record, records)
      if (changed(record, next)) {
        await this.#persist(tab.directory, next)
        records[index] = next
      }
    }

    threads = await this.store.readThreads(tab.directory)
    for (const record of records) {
      if (record?.target?.tab !== tab.id || record.state !== 'accepted') continue
      await this.#markSeen(tab, record, threads, records, completions)
    }

    for (let index = 0; index < records.length; index++) {
      const record = records[index]
      if (record?.target?.tab !== tab.id || record.state !== 'pending') continue
      const outcome = await this.#pending(tab, record, records)
      const next = outcome.record
      if (changed(record, next)) {
        if (outcome.persisted !== true) {
          await this.#persist(tab.directory, next)
        }
        records[index] = next
      }
    }
  }

  async #replace(tab, conversation, row, completion, records) {
    const replacementReason =
      completion.reason ?? 'replaced: the bound worker session was replaced in place'
    const invalidated = {
      at: new Date(this.now()).toISOString(),
      reason: replacementReason,
      previousSession: row.sessionId,
    }
    const updates = await this.store.mutate(
      tab.directory,
      'delivery.session-replaced',
      async (io) => {
        const deliveries = await io.readDeliveries()
        const suspended = []
        for (const record of Object.values(deliveries)) {
          if (
            record?.conversation !== conversation ||
            record?.target?.tab !== tab.id ||
            !['pending', 'submitting'].includes(record?.state)
          ) {
            continue
          }
          const next = {
            ...record,
            suspended: true,
            reason: replacementReason,
            workerInvalidated: invalidated,
          }
          if (record.state === 'pending') {
            next.snapshot = null
            next.submittedAt = null
          }
          deliveries[next.id] = next
          suspended.push(next)
        }
        if (suspended.length > 0) await io.writeDeliveries(deliveries)

        const threads = await io.readThreads()
        const current = threads[conversation]
        if (current && current.sessionId === row.sessionId) {
          current.replaced = invalidated
          current.updatedAt = current.replaced.at
          current.sessionId = null
          delete current.binding
          await io.writeThreads(threads)
        }
        return suspended
      },
    )
    for (const next of updates) {
      const index = records.findIndex((record) => record?.id === next.id)
      if (index !== -1) records[index] = next
    }
  }

  async #invalidateLead(tab, target, reason, records) {
    const outcome = await this.store.mutate(tab.directory, 'delivery.lead-replaced', async (io) => {
      const invalidated = {
        at: new Date(this.now()).toISOString(),
        reason,
        previousSession: target.session,
        generation: target.generation,
      }
      const deliveries = await io.readDeliveries()
      const suspended = []
      for (const record of Object.values(deliveries)) {
        if (
          !['pending', 'submitting'].includes(record?.state) ||
          !sameTarget(record?.target, target)
        ) {
          continue
        }
        const next = {
          ...record,
          suspended: true,
          reason,
          leadInvalidated: invalidated,
        }
        if (record.state === 'pending') {
          next.snapshot = null
          next.submittedAt = null
        }
        deliveries[next.id] = next
        suspended.push(next)
      }
      if (suspended.length > 0) await io.writeDeliveries(deliveries)

      const tabs = await io.readTabs()
      const current = tabs.find((candidate) => candidate?.id === tab.id)
      if (sameTarget(targetFor(current), target)) {
        current.lead.replaced = invalidated
        current.lead.nativeSession = null
        await io.writeTabs(tabs)
      }
      return suspended
    })
    for (const next of outcome) {
      const index = records.findIndex((record) => record?.id === next.id)
      if (index !== -1) records[index] = next
    }
    return outcome
  }

  async #receipt(tab, record, records) {
    const leadAnswers = await answers(tab.lead.harness, record.target.session, this.env)
    if (leadAnswers?.replaced === true) {
      await this.#invalidateLead(
        tab,
        record.target,
        'replaced: the lead native session changed in place',
        records,
      )
      return fail(record, {
        reason: 'lead session replaced after submission; write outcome is uncertain',
      })
    }
    return receipt(record, record.snapshot, {
      session: record.target.session,
      generation: record.target.generation,
      itemsAfter: (cursor) => itemsAfterCursor(tab.lead.harness, leadAnswers?.items, cursor),
      now: this.now(),
      receiptMs: this.receiptMs,
    })
  }

  async #markSeen(tab, record, threads, records, completions) {
    const row = threads[record.conversation]
    if (!bound(row)) return
    let completion = completions.get(record.conversation)
    if (!completion) {
      completion = await answers(row.kind, row.sessionId, this.env)
      completions.set(record.conversation, completion)
    }
    if (completion?.replaced === true) return
    const items = bookkeepingItems(completion?.items)
    const seen = seenAfter({
      row,
      leadId: record.target.leadId,
      conversation: record.conversation,
      deliveries: records,
      items,
    })
    const current = Array.isArray(row.seen?.[record.target.leadId])
      ? row.seen[record.target.leadId]
      : []
    if (JSON.stringify(current) === JSON.stringify(seen)) return
    await this.store.seenSet(tab.directory, {
      name: record.conversation,
      lead: record.target.leadId,
      items: seen,
    })
  }

  async #pending(tabAtStart, record, records) {
    const tab = await this.tabs.get(tabAtStart.id)
    if (!tab) return { record: { ...record, reason: 'lead tab no longer exists' } }
    const threads = await this.store.readThreads(tab.directory)
    const row = threads[record.conversation]
    if (!bound(row) || isRecord(row?.replaced)) {
      return {
        record: {
          ...record,
          suspended: true,
          reason: row?.replaced?.reason ?? 'the worker binding is no longer valid',
          snapshot: null,
          submittedAt: null,
        },
      }
    }
    const pane = workerPane(tab, record.conversation)
    const policy = effectivePolicy(tab, pane, row)
    if (policy.mode === 'manual' && record.manual !== true) {
      return {
        record: cancel(record, {
          reason: `automatic delivery cancelled by manual policy (${policy.source})`,
        }),
      }
    }
    if (record.suspended === true) return { record }
    const target = targetFor(tab)
    if (target === null || tab.closed === true) {
      return { record: { ...record, reason: 'lead is closed' } }
    }
    if (!sameTarget(record.target, target)) {
      return { record: { ...record, reason: 'held for previous lead generation' } }
    }
    if (!this.bridge || this.bridge.closed === true) {
      return { record: { ...record, reason: 'bridge unavailable' } }
    }
    let snapshot
    try {
      snapshot = await this.bridge.request('pane.snapshot', {
        id: target.pane,
        generation: target.generation,
      })
    } catch (cause) {
      return { record: { ...record, reason: `snapshot unavailable: ${describe(cause)}` } }
    }
    if (snapshot?.ok !== true) {
      return {
        record: {
          ...record,
          reason: `snapshot unavailable: ${snapshot?.error ?? 'unknown refusal'}`,
        },
      }
    }

    const currentTab = await this.tabs.get(tab.id)
    const currentThreads = await this.store.readThreads(tab.directory)
    const currentRow = currentThreads[record.conversation]
    const currentTarget = targetFor(currentTab)
    const currentPolicy = effectivePolicy(
      currentTab,
      workerPane(currentTab, record.conversation),
      currentRow,
    )
    if (currentPolicy.mode === 'manual' && record.manual !== true) {
      return {
        record: cancel(record, {
          reason: `automatic delivery cancelled by manual policy (${currentPolicy.source})`,
        }),
      }
    }
    if (
      currentTarget === null ||
      currentTab?.closed === true ||
      !sameTarget(target, currentTarget)
    ) {
      return { record: { ...record, reason: 'held because the lead changed before submission' } }
    }

    const leadAnswers = await answers(currentTab.lead.harness, target.session, this.env)
    if (leadAnswers?.replaced === true) {
      const suspended = await this.#invalidateLead(
        currentTab,
        target,
        'replaced: the lead native session changed in place',
        records,
      )
      return {
        record: suspended.find((candidate) => candidate.id === record.id) ?? {
          ...record,
          suspended: true,
          reason: 'replaced: the lead native session changed in place',
        },
        persisted: true,
      }
    }
    const sinceCursor = latestSubmittedCursor(records, target, record.id)
    const decision = leadReady({
      answers: leadAnswers,
      kind: currentTab.lead.harness,
      draftLatched: snapshot.draftLatched,
      epoch: snapshot.inputEpoch,
      sinceCursor,
    })
    if (decision.state !== 'ready') return { record: { ...record, reason: decision.reason } }
    if (
      records.some(
        (candidate) =>
          candidate?.id !== record.id &&
          candidate?.state === 'submitting' &&
          sameTarget(candidate.target, target),
      )
    ) {
      return {
        record: { ...record, reason: 'lead busy: another delivery is awaiting receipt' },
      }
    }

    if (record.channel === 'cf-read') {
      try {
        await this.#writeImmutable(record)
      } catch (cause) {
        return {
          record: writeFailed(record, `delivery file write failed: ${describe(cause)}`),
        }
      }
    }
    const admitted = await this.#admitSubmission(
      tab.directory,
      record,
      target,
      decision,
      sinceCursor,
    )
    if (admitted.admitted !== true) return admitted
    const readyRecord = admitted.record
    const at = records.findIndex((candidate) => candidate?.id === record.id)
    if (at !== -1) records[at] = readyRecord

    const route = deliveryRoute(admitted.route, target, readyRecord, this.bridge, decision.epoch)
    try {
      const response = await deliver(route.channel, route.target, readyRecord)
      if (response?.admitted === false) {
        return {
          record: fail(readyRecord, {
            bytesWritten: 0,
            reason:
              response?.error ??
              response?.ack?.error ??
              response?.ack?.reason ??
              'delivery was not admitted by the lead channel',
          }),
        }
      }
      if (response?.ok === true) return admitted
      const refusal = response?.error ?? 'unknown bridge refusal'
      if (/stale|draft/i.test(refusal)) {
        return {
          record: {
            ...readyRecord,
            state: 'pending',
            reason: refusal,
            snapshot: null,
            submittedAt: null,
          },
        }
      }
      if (refusal === 'uncertain') {
        return {
          record: fail(readyRecord, {
            reason: `delivery outcome uncertain: ${response?.cause ?? refusal}`,
          }),
        }
      }
      return {
        record: fail(readyRecord, {
          bytesWritten: response?.admitted === false ? 0 : response?.bytesWritten,
          reason: refusal,
        }),
      }
    } catch (cause) {
      return {
        record: fail(readyRecord, {
          bytesWritten: cause?.bytesWritten,
          reason: `bridge EOF or write outcome unknown: ${describe(cause)}`,
        }),
      }
    }
  }

  async #admitSubmission(workspace, candidate, target, decision, observedSinceCursor) {
    const outcome = await this.store.mutate(workspace, 'delivery.submit', async (io) => {
      const deliveries = await io.readDeliveries()
      const current = deliveries[candidate.id]
      if (current?.state !== 'pending' || this.closed) {
        return { record: current ?? candidate, admitted: false }
      }
      const tabs = await io.readTabs()
      const tab = tabs.find((record) => record?.id === target.tab)
      const threads = await io.readThreads()
      const row = threads[current.conversation]
      const all = Object.values(deliveries)
      const workerInvalidated = all.find(
        (record) =>
          record?.conversation === current.conversation &&
          isRecord(record?.workerInvalidated) &&
          record.workerInvalidated.previousSession === row?.sessionId,
      )
      const route = {
        harness: tab?.lead?.harness,
        channel: tab?.lead?.reserved?.channel ?? null,
      }
      let next = current
      let wroteDeliveries = false

      if (!bound(row) || isRecord(row?.replaced) || workerInvalidated) {
        next = {
          ...current,
          suspended: true,
          reason:
            row?.replaced?.reason ??
            workerInvalidated?.workerInvalidated?.reason ??
            'the worker binding is no longer valid',
          snapshot: null,
          submittedAt: null,
        }
      } else {
        const policy = effectivePolicy(tab, workerPane(tab, current.conversation), row)
        if (policy.mode === 'manual' && current.manual !== true) {
          next = cancel(current, {
            reason: `automatic delivery cancelled by manual policy (${policy.source})`,
          })
        } else {
          const completion = await answers(row.kind, row.sessionId, this.env)
          if (completion?.replaced === true) {
            const reason =
              completion.reason ?? 'replaced: the bound worker session was replaced in place'
            const invalidated = {
              at: new Date(this.now()).toISOString(),
              reason,
              previousSession: row.sessionId,
            }
            for (const record of Object.values(deliveries)) {
              if (
                record?.conversation !== current.conversation ||
                record?.target?.tab !== tab.id ||
                !['pending', 'submitting'].includes(record?.state)
              ) {
                continue
              }
              const suspended = {
                ...record,
                suspended: true,
                reason,
                workerInvalidated: invalidated,
              }
              if (record.state === 'pending') {
                suspended.snapshot = null
                suspended.submittedAt = null
              }
              deliveries[suspended.id] = suspended
            }
            next = deliveries[current.id]
            await io.writeDeliveries(deliveries)
            wroteDeliveries = true
            row.replaced = invalidated
            row.updatedAt = invalidated.at
            row.sessionId = null
            delete row.binding
            await io.writeThreads(threads)
          } else if (
            current.manual !== true &&
            waitsForSentQuestion(
              row,
              bookkeepingItems(completion?.items),
              this.now(),
              waitGraceMs(this.env),
            )
          ) {
            next = {
              ...current,
              reason: 'worker question is still reaching the transcript during wait grace',
            }
          } else if (tab?.closed === true || !sameTarget(targetFor(tab), target)) {
            next = { ...current, reason: 'held because the lead changed before submission' }
          } else {
            const invalidated = all.find(
              (record) => isRecord(record?.leadInvalidated) && sameTarget(record?.target, target),
            )
            const currentSinceCursor = latestSubmittedCursor(all, target, current.id)
            if (invalidated) {
              next = {
                ...current,
                suspended: true,
                reason: invalidated.leadInvalidated.reason,
                snapshot: null,
                submittedAt: null,
              }
            } else if (!Object.is(currentSinceCursor, observedSinceCursor)) {
              next = { ...current, reason: 'lead readiness changed before submission' }
            } else if (
              all.some(
                (record) =>
                  record?.id !== current.id &&
                  record?.state === 'submitting' &&
                  sameTarget(record?.target, target),
              )
            ) {
              next = { ...current, reason: 'lead busy: another delivery is awaiting receipt' }
            } else {
              next = {
                ...submit(current, {
                  target,
                  cursor: decision.cursor,
                  now: this.now(),
                }),
                submissionOrder: nextSubmissionOrder(all),
              }
            }
          }
        }
      }

      const didChange = changed(current, next)
      if (didChange && !wroteDeliveries) {
        deliveries[next.id] = next
        await io.writeDeliveries(deliveries)
      }
      return {
        record: next,
        admitted: next.state === 'submitting',
        route,
      }
    })
    return {
      record: outcome.record,
      persisted: true,
      admitted: outcome.admitted,
      route: outcome.route,
    }
  }

  async #writeImmutable(record) {
    await fs.mkdir(path.dirname(record.file), { recursive: true })
    try {
      await fs.writeFile(record.file, record.answer, { encoding: 'utf8', flag: 'wx' })
    } catch (cause) {
      if (cause?.code !== 'EEXIST') throw cause
      const existing = await fs.readFile(record.file, 'utf8')
      if (existing !== record.answer) throw new Error('immutable delivery file already differs')
    }
  }

  async #persist(workspace, record) {
    return this.store.deliveryUpsert(workspace, record)
  }
}
