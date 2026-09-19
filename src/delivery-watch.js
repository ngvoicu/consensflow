import { randomUUID } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'
import { cachedAnswers, itemsAfterCursor } from '../hosts/lib/completion.js'
import { digest, envelope, legacyReceipt } from '../hosts/lib/deliveries.js'
import {
  claimForRead,
  indexResult,
  observeReceipt,
  resultStatus,
  retireReceiver,
} from '../hosts/lib/inbox.js'
import { launchConfiguration } from './channels.js'
import { receiverSignal } from './claude-install.js'

export function bookkeepingItems(items) {
  return Array.isArray(items)
    ? items.filter((item) => ['user', 'assistant'].includes(item?.role))
    : []
}
const belongs = (row, tab) =>
  typeof row?.lead === 'string' &&
  row.lead.startsWith(`tab:${tab.id}:`) &&
  (tab.role !== 'pm' || row.role === 'advisor')
const complete = (native) =>
  native?.unknown || native?.replaced
    ? []
    : (native.items ?? []).filter(
        (item) => item.role === 'assistant' && item.complete === true && item.settled === true,
      )
const timestamp = (value, fallback) =>
  typeof value === 'number' && Number.isFinite(value) ? value : Date.parse(value) || fallback

/** Stores every completed reply and reconciles evidence. It never submits to a harness. */
export class Watcher {
  constructor({ store, tabs, env, now = Date.now, floorMs = 1000, onError = () => {} } = {}) {
    if (!store || !tabs || !env || typeof now !== 'function' || !(floorMs > 0))
      throw new Error('Watcher needs Store, Tabs, environment and a positive interval')
    Object.assign(this, { store, tabs, env, now, floorMs, onError })
    this.tail = Promise.resolve()
    this.closed = false
    this.started = false
    this.unsubscribe = []
    this.signals = new Map()
    this.activities = new Map()
    // Every bound session is re-read each scan; unchanged transcripts are not re-parsed.
    this.answers = cachedAnswers()
  }
  activity(tab, pane, row, receiver) {
    const key = activityKey(tab, pane, row, receiver)
    const observed = key && this.activities.get(key)
    return observed?.expiresAt > this.now() ? observed : { state: 'unknown' }
  }
  #observeActivity(key, native, claude) {
    if (!key) return
    // Claude records its own live status; it outranks what a transcript implies.
    if (claude) {
      this.activities.set(key, { ...claude, expiresAt: this.now() + 10_000 })
      return
    }
    const state =
      native?.unknown || native?.replaced || native?.continuedInSessionId
        ? 'unknown'
        : native?.inFlight || native?.settlement?.state === 'in-flight'
          ? 'working'
          : native?.failed
            ? 'failed'
            : native?.settlement?.state === 'settled'
              ? 'idle'
              : 'unknown'
    this.activities.set(key, { state, expiresAt: this.now() + 10_000 })
  }
  attachBridge(bridge) {
    for (const unsubscribe of this.unsubscribe.splice(0)) unsubscribe()
    // Rust announces a pane exit; there is no idle event (activity comes from
    // native status and transcripts).
    this.unsubscribe.push(
      bridge.onEvent('pane.exit', () => {
        if (this.started && !this.closed) this.reconcile().catch(this.onError)
      }),
    )
    return this
  }
  async start() {
    if (this.closed) throw new Error('Watcher is closed')
    if (this.started) return this.tail
    this.started = true
    this.timer = setInterval(() => this.reconcile().catch(this.onError), this.floorMs)
    this.timer.unref?.()
    return this.reconcile()
  }
  reconcile() {
    if (this.closed) return this.tail
    if (this.scanning) return this.scanning
    const next = this.tail
      .then(() => this.#scan())
      .finally(() => {
        this.scanning = null
      })
    this.scanning = next
    this.tail = next.catch(this.onError)
    return next
  }
  async close() {
    this.closed = true
    clearInterval(this.timer)
    for (const unsubscribe of this.unsubscribe.splice(0)) unsubscribe()
    await this.tail
  }
  async #native(row, directory, session = row.sessionId) {
    let options = {}
    if (row.kind === 'pi' && row.binding?.launchId) {
      const { channel } = await launchConfiguration('pi', {
        launchId: row.binding.launchId,
        workspace: directory,
        env: this.env,
      })
      options = { piSettlement: { directory: channel.settled, launchId: channel.launchId } }
    }
    return this.answers(row.kind, session, this.env, options)
  }
  async #scan() {
    const tabs = await this.tabs.list()
    for (const [key, value] of this.activities)
      if (value.expiresAt <= this.now()) this.activities.delete(key)
    for (const directory of new Set(tabs.map((tab) => tab.directory))) {
      if (this.closed) return
      try {
        await this.#workspace(
          directory,
          tabs.filter((tab) => tab.directory === directory),
        )
      } catch (error) {
        this.onError(error)
      }
    }
  }
  async #workspace(directory, tabs) {
    const threads = await this.store.readThreads(directory)
    const claude = await claudeStatuses(this.env)
    const observed = []
    const current = await this.store.readInbox(directory)
    const nativeReceipts = new Map()
    for (const tab of tabs) {
      const receiver = current.receivers[tab.id]
      const pane = tab.panes.find((pane) => pane.kind === 'lead')
      const key = pane && activityKey(tab, pane, null, receiver)
      if (key) {
        const native = await this.#native(
          { kind: receiver.kind, binding: { launchId: receiver.launch } },
          directory,
          receiver.session,
        )
        this.#observeActivity(
          key,
          native,
          receiver.kind === 'claude-code' ? claude.get(receiver.session) : undefined,
        )
        nativeReceipts.set(JSON.stringify([receiver.kind, receiver.session]), native)
      }
      for (const [conversation, row] of Object.entries(threads)) {
        if (!belongs(row, tab) || !row.sessionId || !row.binding) continue
        if (tab.deletedConversations?.includes(conversation)) continue
        let session = row.sessionId
        let latest
        const pane = tab.panes.find((pane) => pane.conversation === conversation)
        const key = pane && activityKey(tab, pane, row)
        const visited = new Set()
        let current = session
        // Native continuation is explicit source ancestry, never newest-file or cwd matching.
        while (session && !visited.has(session) && visited.size < 16) {
          visited.add(session)
          current = session
          const native = await this.#native(row, directory, session)
          latest = native
          for (const item of complete(native))
            observed.push({
              owner: tab.id,
              conversation,
              agent: row.agent,
              kind: row.kind,
              session,
              answerId: item.id,
              answer: item.text,
              now: timestamp(item.at, this.now()),
            })
          if (native.replaced || native.unknown || row.kind !== 'claude-code') break
          session = native.continuedInSessionId
        }
        this.#observeActivity(
          key,
          latest,
          row.kind === 'claude-code' ? claude.get(current) : undefined,
        )
      }
    }
    // Allocate outside Store's queue; gaps are harmless, nested mutations would deadlock.
    const legacy = Object.values(await this.store.readDeliveries(directory))
    const additions = observed.filter(
      (item) =>
        !Object.values(current.results).some(
          (r) =>
            r.key ===
              JSON.stringify([
                item.owner,
                item.conversation,
                item.kind,
                item.session,
                item.answerId,
              ]) ||
            (r.owner === item.owner &&
              r.conversation === item.conversation &&
              r.answerId === item.answerId &&
              r.answer === item.answer &&
              r.nativeSources?.some(
                (source) => source.kind === item.kind && source.session === item.session,
              )),
        ),
    )
    for (const item of additions) item.id = await this.store.allocateDeliveryId()
    const oldIds = new Set(
      Object.values(current.results).flatMap((r) => (r.legacy ?? []).map((old) => old.id)),
    )
    const histories = []
    for (const old of legacy) {
      if (
        oldIds.has(old.id) ||
        !tabs.some((tab) => tab.id === old.target?.tab) ||
        typeof old.answer !== 'string'
      )
        continue
      if (digest(envelope(old)) !== old.digest) continue
      histories.push({ old, id: await this.store.allocateDeliveryId() })
    }
    for (const result of Object.values(current.results))
      for (const claim of result.claims) {
        if (!['submitting', 'uncertain'].includes(claim.state)) continue
        const key = JSON.stringify([claim.receiver.kind, claim.receiver.session])
        if (!nativeReceipts.has(key))
          nativeReceipts.set(
            key,
            await this.answers(claim.receiver.kind, claim.receiver.session, this.env),
          )
      }
    const historicalReceipts = []
    for (const result of Object.values(current.results))
      for (const old of result.legacy ?? []) {
        if (
          !['submitting', 'uncertain'].includes(old.state) ||
          result.legacyEvidence?.some((e) => e.delivery === old.id) ||
          !old.snapshot ||
          !['claude-code', 'codex', 'pi', 'opencode'].includes(old.kind)
        )
          continue
        let session = old.snapshot.targetSession
        const native = await this.answers(old.kind, session, this.env)
        if (native.unknown || native.replaced) continue
        let proof = legacyReceipt(old, {
          session,
          items: itemsAfterCursor(old.kind, native.items, old.snapshot.cursor),
        })
        let continuation
        if (!proof && old.kind === 'claude-code' && old.manualRead && native.continuedInSessionId) {
          continuation = {
            from: session,
            to: native.continuedInSessionId,
            at: Date.parse(native.continuedAt),
          }
          session = continuation.to
          const successor = await this.answers(old.kind, session, this.env)
          if (!successor.unknown && !successor.replaced)
            proof = legacyReceipt(old, { session, items: successor.items, continuation })
        }
        if (proof)
          historicalReceipts.push({
            result: result.id,
            delivery: old.id,
            session,
            ...proof,
            ...(continuation ? { continuation } : {}),
          })
      }
    await this.store.mutate(directory, 'inbox.scan', async (io) => {
      const state = await io.readInbox()
      const before = JSON.stringify(state)
      for (const item of additions) {
        // A previously migrated answer with unknown native origin is not silently reassigned.
        const migrated = Object.values(state.results).find(
          (r) =>
            r.owner === item.owner &&
            r.conversation === item.conversation &&
            r.answerId === item.answerId &&
            r.answer === item.answer &&
            r.session.startsWith('legacy:'),
        )
        if (migrated) {
          migrated.nativeSources ??= []
          if (
            !migrated.nativeSources.some(
              (source) => source.kind === item.kind && source.session === item.session,
            )
          )
            migrated.nativeSources.push({ kind: item.kind, session: item.session })
        } else indexResult(state, item)
      }
      for (const { old, id } of histories) {
        if (
          Object.values(state.results).some((r) => r.legacy?.some((entry) => entry.id === old.id))
        )
          continue
        const matches = Object.values(state.results).filter(
          (r) =>
            r.owner === old.target.tab &&
            r.conversation === old.conversation &&
            r.answerId === old.answerId &&
            r.answer === old.answer,
        )
        const result =
          matches.length === 1
            ? matches[0]
            : indexResult(state, {
                id,
                owner: old.target.tab,
                conversation: old.conversation,
                agent: old.agent,
                kind: 'legacy',
                session: `legacy:${old.answerId}`,
                answerId: old.answerId,
                answer: old.answer,
                now: timestamp(old.createdAt, this.now()),
              })
        result.legacy ??= []
        result.legacy.push(structuredClone(old))
        if (old.sourceParent) {
          result.sourceParent = old.sourceParent
          result.manualOnly = true
        }
        if (old.state === 'cancelled') result.cancelledAt ??= timestamp(old.cancelledAt, this.now())
        if (old.state === 'failed') result.manualOnly = true
        if (old.state === 'accepted') result.legacyReceived = true
        if (['uncertain', 'submitting'].includes(old.state)) result.legacyUncertain = true
      }
      for (const proof of historicalReceipts) {
        const result = state.results[proof.result]
        result.legacyEvidence ??= []
        if (!result.legacyEvidence.some((e) => e.delivery === proof.delivery))
          result.legacyEvidence.push(proof)
        result.legacyReceived = true
      }
      const liveTabs = await io.readTabs()
      for (const receiver of Object.values(state.receivers)) {
        if (receiver.retiredAt !== undefined) continue
        const tab = liveTabs.find((tab) => tab.id === receiver.owner)
        if (tab?.closed || !tab || tab.lead.reserved?.launchId !== receiver.launch)
          retireReceiver(state, { owner: receiver.owner, lease: receiver.lease, now: this.now() })
      }
      for (const result of Object.values(state.results))
        for (const claim of result.claims) {
          const native = nativeReceipts.get(
            JSON.stringify([claim.receiver.kind, claim.receiver.session]),
          )
          if (native && !native.unknown && !native.replaced)
            observeReceipt(state, {
              result: result.id,
              claim: claim.id,
              session: claim.receiver.session,
              kind: claim.receiver.kind,
              items: native.items,
              now: this.now(),
            })
          if (claim.state === 'submitting' && this.now() - claim.startedAt >= 60_000) {
            claim.state = 'uncertain'
            claim.reason = 'native receipt has not been confirmed'
          }
          if (claim.state === 'claimed' && this.now() >= claim.expiresAt) claim.state = 'released'
        }
      if (JSON.stringify(state) !== before) await io.writeInbox(state)
    })
    const state = await this.store.readInbox(directory)
    for (const receiver of Object.values(state.receivers)) {
      if (
        receiver.kind !== 'claude-code' ||
        receiver.retiredAt !== undefined ||
        !Object.values(state.results).some(
          (r) => r.owner === receiver.owner && resultStatus(r) === 'waiting',
        ) ||
        this.now() - (this.signals.get(receiver.launch) ?? 0) < 3000
      )
        continue
      // Native FileChanged observes this private signal and decides whether to wake.
      const signal = receiverSignal(this.env, receiver.launch)
      await fs.mkdir(path.dirname(signal), { recursive: true, mode: 0o700 })
      await fs.writeFile(signal, String(this.now()), { mode: 0o600 })
      this.signals.set(receiver.launch, this.now())
    }
  }
  async results(tabId) {
    this.reconcile().catch(this.onError)
    const tab = await this.tabs.get(tabId)
    if (!tab) throw new Error(`unknown session ${tabId}`)
    const state = await this.store.readInbox(tab.directory)
    const rows = await this.store.readThreads(tab.directory)
    const groups = new Map(
      Object.entries(rows)
        .filter(([, row]) => belongs(row, tab))
        .map(([conversation, row]) => [
          conversation,
          { conversation, agent: row.agent, results: [] },
        ]),
    )
    for (const result of Object.values(state.results)) {
      if (result.owner !== tabId) continue
      if (!groups.has(result.conversation))
        groups.set(result.conversation, {
          conversation: result.conversation,
          agent: result.agent,
          results: [],
        })
      const status = resultStatus(result)
      groups.get(result.conversation).results.push({
        id: result.answerId,
        deliveryId: result.id,
        bytes: Buffer.byteLength(result.answer),
        preview: result.answer.replace(/\s+/g, ' ').slice(0, 120),
        status: status === 'received' ? 'read' : status === 'collecting' ? 'reading' : 'unread',
        state: status,
        parts: result.parts.length,
      })
    }
    return [...groups.values()]
  }
  async readResult(tabId, conversation, answerId) {
    // A durable report must not wait for unrelated harnesses or native histories.
    this.reconcile().catch(this.onError)
    const tab = await this.tabs.get(tabId)
    if (!tab) throw new Error(`unknown session ${tabId}`)
    const state = await this.store.readInbox(tab.directory)
    const result = Object.values(state.results).find(
      (r) =>
        r.owner === tabId &&
        r.conversation === conversation &&
        (answerId === undefined ? resultStatus(r) !== 'received' : r.answerId === answerId),
    )
    if (!result) throw new Error('no unread completed result; inspect cf results')
    return result
  }
  async readPart(tabId, id, part) {
    const tab = await this.tabs.get(tabId)
    if (!tab || tab.closed) throw new Error('result reading needs an open coordinator')
    return this.store.mutate(tab.directory, 'inbox.read', async (io) => {
      const state = await io.readInbox()
      const result = state.results[id]
      if (result?.owner !== tabId) throw new Error('result does not belong to this coordinator')
      const selected = state.receivers[tabId]
      const claim = claimForRead(state, {
        owner: tabId,
        lease: selected?.lease,
        result: id,
        part,
        id: randomUUID(),
        now: this.now(),
      })
      await io.writeInbox(state)
      return {
        outcome: 'read',
        deliveryId: id,
        conversation: result.conversation,
        agent: result.agent,
        k: part,
        of: result.parts.length,
        text: claim.text,
        bytes: Buffer.byteLength(claim.text),
      }
    })
  }
}

// Cached activity belongs to a live launch and selected native conversation.
// A /new, resume, or pane replacement invalidates it before the next scan finishes.
/**
 * Claude Code's own live status for each running session, from the
 * `sessions/<pid>.json` files it keeps (the same files peer delivery reads):
 * busy, idle, or waiting with the reason (a permission prompt, input needed, a
 * dialog). A file whose process is gone is ignored.
 */
async function claudeStatuses(env) {
  const directory = path.join(
    env.CLAUDE_CONFIG_DIR ?? path.join(env.HOME ?? '', '.claude'),
    'sessions',
  )
  const statuses = new Map()
  const names = await fs.readdir(directory).catch(() => [])
  for (const name of names) {
    if (!/^\d+\.json$/.test(name)) continue
    const row = await fs
      .readFile(path.join(directory, name), 'utf8')
      .then(JSON.parse)
      .catch(() => null)
    if (typeof row?.sessionId !== 'string' || !Number.isSafeInteger(row.pid) || !alive(row.pid))
      continue
    const state = { busy: 'working', waiting: 'waiting', idle: 'idle', shell: 'idle' }[row.status]
    if (!state) continue
    statuses.set(row.sessionId, {
      state,
      ...(state === 'waiting' && typeof row.waitingFor === 'string'
        ? { reason: row.waitingFor }
        : {}),
    })
  }
  return statuses
}

function alive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return error.code === 'EPERM'
  }
}

function activityKey(tab, pane, row, receiver) {
  if (tab.closed || tab.deleting || pane.closed || pane.deleting || pane.failure) return null
  if (pane.kind === 'lead') {
    if (
      !tab.lead.reserved?.resolvedAt ||
      !receiver ||
      receiver.retiredAt !== undefined ||
      receiver.owner !== tab.id ||
      receiver.pane !== pane.id ||
      receiver.generation !== pane.generation ||
      receiver.kind !== tab.lead.harness ||
      receiver.launch !== tab.lead.reserved.launchId
    )
      return null
    return JSON.stringify([
      tab.id,
      pane.id,
      pane.generation,
      receiver.launch,
      receiver.session,
      receiver.lease,
    ])
  }
  if (
    pane.kind !== 'worker' ||
    !belongs(row, tab) ||
    !row.sessionId ||
    !row.reserved?.resolvedAt ||
    row.reserved.tab !== tab.id ||
    row.reserved.pane !== pane.id ||
    row.reserved.generation !== pane.generation ||
    row.binding?.generation !== pane.generation ||
    row.binding?.launchId !== row.reserved.launchId
  )
    return null
  return JSON.stringify([
    tab.id,
    pane.id,
    pane.generation,
    row.binding.launchId,
    row.kind,
    row.sessionId,
  ])
}
