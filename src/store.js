import { randomUUID } from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'
import { emptyInbox } from '../hosts/lib/inbox.js'
import { recordLeadPreference } from '../hosts/lib/policy.js'
import { bindEvidence } from '../hosts/lib/session-binding.js'
import { workspaceKey, writeJsonAtomic } from '../hosts/lib/state.js'
import { newSessionName } from '../hosts/lib/threads.js'
import { nowIso } from '../hosts/lib/utils.js'

/**
 * The app-wide serialised store (Phase 2, IMPL-PANE-12).
 *
 * One app instance owns one state root, and every mutation of the app's own
 * state — `threads.json`, `tabs.json`, delivery records — goes through ONE
 * queue held by this module. Not one queue per workspace: the files this
 * queue guards are small and few, and two queues over one file is how a
 * write gets lost. `mutate(cwd, name, fn)` is the primitive; the named ops
 * (`conversation.create`, `session.bind`, `sent.record`, `policy.set`,
 * `lead.preference`, `progress.set`, `seen.set`, `delivery.allocate`,
 * `delivery.upsert`, `delivery.readAttempt`,
 * `reservation.create|resolve|release`, `tab.*`) are thin methods over it, and
 * `src/tabs.js` builds the tab semantics on the same queue.
 *
 * Rules, each with a test in `tests/store.test.mjs`:
 * - `open()` takes `<root>/app/instance.lock` with a KERNEL lock — opened
 *   `O_EXLOCK | O_NONBLOCK` — and holds the descriptor for the Store's
 *   whole life. A second `Store`, in this process or any other, fails with
 *   `EAGAIN`; the kernel releases the lock when the descriptor closes,
 *   which includes the owning process being killed. `close()` releases it
 *   last, after the queue has drained.
 *
 *   Ownership is therefore never inferred from the file's CONTENTS. There
 *   is no pid to trust, no start time to compare, no staleness rule, no
 *   reclaim and no tombstone — so there is no window between deciding and
 *   acting for a second instance to arrive in, and no bytes a contender can
 *   forge to let itself in. The file records `{pid, startedAt}` as a note
 *   for whoever is debugging, and nothing reads it back. The lock file is
 *   never unlinked: a successor could otherwise create and lock a DIFFERENT
 *   inode while someone still held this one.
 *
 *   `O_EXLOCK` is BSD and macOS. Node does not expose the flag, so its
 *   value is written out here, and a wrong value would not fail the open —
 *   it would succeed with no lock at all. So Linux and Windows do not run
 *   this store at all in this spec: they are refused at `open()`, and the
 *   packaging spec gives them their own lock (`flock`, `LockFileEx`).
 *   Running unlocked is the one outcome worse than not starting, and a
 *   teammate on Linux meeting that refusal is the signal working.
 * - The queue is a single promise chain. A rejected mutation rejects for
 *   its caller and never poisons the chain — except a failed compensation,
 *   which leaves the store `degraded`: every mutation, queued or new,
 *   refuses until `recover()` reconciles the state.
 * - Every write goes through `writeJsonAtomic` and a containment check: the
 *   store opens nothing outside its own root for writing — harness stores
 *   are read-only to it.
 * - `conversation.create` with a pane, and `reserve` on an existing
 *   conversation, validate everything (tab, directory, pane generation and
 *   ownership) before the first write, then write the reservation into
 *   `threads.json` FIRST, then the pane into `tabs.json`. A failed second
 *   write restores the first file inside the queue. A crash between the
 *   two writes is the documented recovery case: on `open()`, a reservation
 *   whose pane no longer serves its conversation at the reserved generation
 *   is released, and a restart reads every tab `closed` (the previous
 *   process's panes are gone even though their records remain). Every
 *   reservation carries a durable `launchId`, recorded on the binding too,
 *   and `release` names the launch it releases — a launch's delayed cleanup
 *   never takes the next launch's reservation. A reservation is `resolve`d
 *   when its pane is really open: until then the launch is in flight, and
 *   an unresolved reservation is what refuses a second launch for one
 *   conversation. It keeps the `opId` that asked for it too, so a retry
 *   arriving after a restart can still tell its own launch from a new one.
 * - A delivery identity is issued by ONE allocator too, `allocateDeliveryId`,
 *   from a counter persisted beside the pane one and incremented on the
 *   queue. `d-<n>`, digits only: the id becomes a filename. The delivery
 *   module is pure and mints nothing, so ids minted there were unique only
 *   within one plan.
 * - A pane identity is issued by ONE allocator (`allocatePaneId`), whichever
 *   path creates the pane — `tab.create`, `tab.addPane`, or either
 *   reservation writer. It is app-wide: the minted `p-<n>` namespace is the
 *   allocator's alone, an id live in any tab is a collision, and an id
 *   issued before comes back only at a newer generation.
 * - `session.bind` never takes an evidence word from the caller. The store
 *   holds the launch record; `bindEvidence(kind, candidate, launch)` from
 *   `hosts/lib/session-binding.js` decides against it, and the bind
 *   happens only on `bound: true`, recording the evidence and the
 *   generation it was made at — a binding made at one pane generation must
 *   never authorise a write for another. A `replaced` verdict is refused
 *   AND persisted: the old binding dies on disk with the session it named.
 * - A controller write (`session.bind`, `progress.set`, `sent.record`)
 *   names the launch it is for, and the store compares that against the
 *   reservation of the moment inside its queue — required in effect, not in
 *   shape: a RESERVED row refuses a write that cannot name its launch, and
 *   an unreserved row has no launch to compare, so the generic queue writes
 *   keep working.
 * - `seen.set` scopes marks to the app-owned lead identity
 *   (`tab:<id>:<generation>`): generations keep their own marks.
 * - Unreadable state is never proof of absence: only a missing file reads
 *   as empty. Corrupt or otherwise unreadable state refuses, preserves its
 *   bytes, and aborts recovery without releasing reservations.
 *
 * This module never reads `process.env`: the state root is an explicit
 * argument, so tests hand it a throwaway directory and the app hands it the
 * config home it resolved itself.
 */

const TAB_POLICY_VALUES = ['auto', 'manual']
const PANE_POLICY_VALUES = ['auto', 'manual', 'inherit']
const PANE_KINDS = ['lead', 'worker', 'shell']
/**
 * `O_EXLOCK` from BSD's `<sys/fcntl.h>`. Node does not put it on
 * `fs.constants`, so the value is written out here — and a wrong value does
 * not fail the open, it succeeds WITHOUT a lock, which is why the platform
 * is checked before the flag is used.
 */
/**
 * A refusal admission makes about the state of a conversation, carrying the
 * word the HTTP layer answers with. `reserved` is a launch of ours that has
 * not come back; `elsewhere` is a pane up in another session.
 */
/**
 * A refusal the store makes ABOUT THE REQUEST, with a code.
 *
 * The type is the whole point. Every other error out of this module — a
 * corrupt file, a full disk, a bug — is a fault on this side, and a caller
 * that guesses from the shape of a bare `Error` will eventually tell someone
 * their perfectly good request was wrong because a JSON file was truncated.
 * So a refusal says so by TYPE, and nothing else may be read as one.
 */
export class StoreRefusal extends Error {
  constructor(message, code, detail = null) {
    super(message)
    this.name = 'StoreRefusal'
    this.code = code
    this.detail = detail
  }
}

export class AdmissionError extends StoreRefusal {
  constructor(message, code, detail = null) {
    super(message, code, detail)
    this.name = 'AdmissionError'
  }
}

const O_EXLOCK = 0x20
const O_EXLOCK_PLATFORMS = new Set(['darwin', 'freebsd', 'openbsd', 'netbsd'])
const LOCK_FILE_NOTE = 'ownership is the kernel lock; this file is diagnostics only'

export class Store {
  #root
  #lockPath
  #tail = Promise.resolve()
  #opened = false
  #closing = false
  #closePromise = null
  #lockHandle = null
  #degraded = null

  constructor(home) {
    if (typeof home !== 'string' || home.trim().length === 0) {
      throw new Error('Store needs the state root as an explicit path')
    }
    this.#root = path.resolve(home)
    this.#lockPath = path.join(this.#root, 'app', 'instance.lock')
  }

  get root() {
    return this.#root
  }

  /** Claims the instance lock, then runs restart recovery. Idempotent. */
  #watchers = new Set()

  async open() {
    // A reopen waits for the outstanding close: the successor of this root
    // is this instance, and it may not race its own release.
    if (this.#closing) await this.#closePromise
    if (this.#opened) return this
    await this.#acquireLock()
    // Every piece of lifecycle state is reset BEFORE recovery, because
    // recovery is a queued mutation like any other: a store still marked
    // closing has no queue to run it on.
    this.#opened = true
    this.#closing = false
    this.#closePromise = null
    this.#tail = Promise.resolve()
    this.#degraded = null
    try {
      await this.#restartRecovery()
    } catch (cause) {
      this.#opened = false
      await this.#releaseLock()
      throw cause
    }
    return this
  }

  /**
   * The lead's twin of `admit`: one launch at a time for a tab's own window.
   *
   * A worker's reservation lives on its conversation row; a lead has no
   * conversation, so its reservation lives on the tab record, at
   * `tab.lead.reserved`. That is where the delivery watcher reads the
   * `channel` the launch was configured with — the endpoint and password,
   * or the inbox and ack directories — so a delivery never has to
   * rediscover them.
   *
   * The refusals are the worker rules, said about a tab: a closed tab is
   * resumed before it launches, and a reservation nobody has resolved means
   * a launch is in flight and a second one must not start. A reservation
   * left behind at an OLDER lead generation is not in flight — `resume`
   * minted a new generation past it — so it is replaced rather than obeyed.
   *
   * `launch` may be a function of the tab record, so the caller can decide
   * the launch id from what the store holds inside this same mutation.
   */
  async leadAdmit(cwd, { tab, launch = null, channel = null } = {}) {
    return this.mutate(cwd, 'tab.leadAdmit', async (io) => {
      requireText(tab, 'tab id')
      const tabs = await io.readTabs()
      const record = tabs.find((candidate) => candidate.id === tab)
      if (record === undefined) {
        throw new AdmissionError(`no tab ${tab} in this workspace`, 'no-tab')
      }
      if (record.closed === true) {
        throw new AdmissionError(`the tab ${tab} is closed — resume it first`, 'session-closed')
      }
      const generation = record.lead.generation
      const held = isRecord(record.lead.reserved) ? record.lead.reserved : null
      if (held !== null && held.resolvedAt === undefined && held.generation === generation) {
        throw new AdmissionError(`the lead of ${tab} is already launching`, 'reserved', {
          launchId: held.launchId,
        })
      }
      const leadPane = record.panes.find((candidate) => candidate.kind === 'lead')
      if (leadPane === undefined) {
        // Unreachable through `create` and `resume`, which both mint one.
        // Coded rather than bare so a corrupt record cannot read as a
        // routine bad request on its way out.
        throw new AdmissionError(`the tab ${tab} has no lead pane`, 'no-lead-pane')
      }
      if (record.lead.harness === 'opencode' && record.lead.nativeSelection === 'home') {
        record.lead.nativeSession = null
        delete record.lead.binding
        delete record.lead.nativeSelection
      }
      const decided = typeof launch === 'function' ? await launch(record) : launch
      const { launchId, ...evidence } = isRecord(decided) ? decided : { launchId: decided }
      requireText(launchId, 'lead launch id')
      // The evidence a lead binds by, decided INSIDE this mutation from the
      // record the store just read — the same rule admission follows for a
      // worker, and for the same reason: a lead that already has a native
      // session is RESUMED on it, never handed a newly minted identity.
      record.lead.reserved = {
        launchId,
        generation,
        pane: leadPane.id,
        ...evidence,
        ...(channel === null || channel === undefined ? {} : { channel }),
        at: nowIso(),
      }
      // A resumed lead keeps its session and takes a NEW launch, so the
      // binding is re-stamped for the launch that now holds it — inside
      // this mutation, against the record it was just written on. A binding
      // still naming a dead launch cannot be invalidated by generation,
      // which is the mechanism every later decision leans on.
      if (isRecord(record.lead.binding)) {
        record.lead.binding = { ...record.lead.binding, launchId, generation, at: nowIso() }
      }
      record.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return {
        launchId,
        tab: record.id,
        directory: record.directory,
        harness: record.lead.harness,
        pane: { id: leadPane.id, generation: leadPane.generation },
        generation,
        nativeSession: record.lead.nativeSession ?? null,
        evidence,
      }
    })
  }

  /**
   * The lead's twin of `session.bind`.
   *
   * A worker's binding lives on its conversation row; a lead has no
   * conversation, so its native session lives at `tab.lead.nativeSession` —
   * the field the record has always carried and nothing ever wrote. The
   * evidence rule is the SAME one, from the same module: an id we
   * preallocated, an id the harness reported on our own stream, or the
   * launch nonce in the opening line. A bare session id binds nothing.
   */
  async leadBind(cwd, { tab, candidate = {}, expect } = {}) {
    return this.mutate(cwd, 'tab.leadBind', async (io) => {
      requireText(tab, 'tab id')
      const tabs = await io.readTabs()
      const record = tabs.find((candidate2) => candidate2.id === tab)
      if (record === undefined) throw new Error(`no tab ${tab} in this workspace`)
      const reserved = isRecord(record.lead.reserved) ? record.lead.reserved : null
      if (reserved === null) {
        throw new StoreRefusal(
          `tab.leadBind refuses for ${tab}: the store holds no lead launch`,
          'no-launch',
        )
      }
      if (isRecord(expect) && reserved.launchId !== expect.launchId) {
        throw new StoreRefusal(
          `launch ${expect.launchId} is over for the lead of ${tab}: it is not the current one`,
          'stale-launch',
        )
      }
      const decision = bindEvidence(record.lead.harness, candidate, reserved)
      if (decision?.bound !== true) {
        throw new StoreRefusal(
          `tab.leadBind refuses for ${tab}: ${decision?.reason ?? 'no evidence'}`,
          'no-evidence',
        )
      }
      record.lead.nativeSession = candidate.sessionId
      record.lead.binding = {
        evidence: decision.evidence,
        generation: reserved.generation,
        launchId: reserved.launchId,
        at: nowIso(),
      }
      record.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return { bound: true, nativeSession: record.lead.nativeSession, evidence: decision.evidence }
    })
  }

  /** The lead launch came back: the reservation stands, resolved. */
  async leadResolve(cwd, { tab, launchId, outcome = 'opened' } = {}) {
    return this.#leadSettle(cwd, 'tab.leadResolve', tab, launchId, (record) => {
      record.lead.reserved.resolvedAt = nowIso()
      record.lead.reserved.outcome = outcome
    })
  }

  /**
   * The lead's window ended: release its launch AND suspend its tab, as one
   * decision inside one mutation.
   *
   * Two writes with the guard read before both is how a late exit closed the
   * generation that had already replaced it: the release found a reservation
   * that was no longer its own and settled nothing, and the suspend ran
   * anyway. So the match — pane, generation and launch together — is read in
   * the same queued operation that writes, and a stale exit changes nothing
   * at all rather than half of something.
   */
  async leadEnded(cwd, { tab, pane, generation, launchId } = {}) {
    return this.mutate(cwd, 'tab.leadEnded', async (io) => {
      requireText(tab, 'tab id')
      const tabs = await io.readTabs()
      const record = tabs.find((candidate) => candidate.id === tab)
      if (record === undefined) return { ended: false, reason: 'no such tab' }
      const held = isRecord(record.lead.reserved) ? record.lead.reserved : null
      if (held === null) return { ended: false, reason: 'no reservation' }
      const leadPane = record.panes.find((candidate) => candidate.kind === 'lead')
      if (
        held.launchId !== launchId ||
        held.generation !== generation ||
        leadPane?.id !== pane ||
        leadPane?.generation !== generation
      ) {
        return { ended: false, reason: 'not the current lead window' }
      }
      delete record.lead.reserved
      record.closed = true
      record.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return { ended: true, launchId, tab: record.id }
    })
  }

  /** The lead launch is over, or never happened: the reservation goes. */
  async leadRelease(cwd, { tab, launchId } = {}) {
    return this.#leadSettle(cwd, 'tab.leadRelease', tab, launchId, (record) => {
      delete record.lead.reserved
    })
  }

  /**
   * Settles a lead reservation, and only the one named. A launch that is no
   * longer the tab's own — a resume minted a newer one, a duplicate exit
   * arrived late — must not settle its successor's.
   */
  async #leadSettle(cwd, op, tab, launchId, apply) {
    return this.mutate(cwd, op, async (io) => {
      requireText(tab, 'tab id')
      requireText(launchId, 'lead launch id')
      const tabs = await io.readTabs()
      const record = tabs.find((candidate) => candidate.id === tab)
      if (record === undefined) return { settled: false, reason: 'no such tab' }
      const held = isRecord(record.lead.reserved) ? record.lead.reserved : null
      if (held === null) return { settled: false, reason: 'no reservation' }
      if (held.launchId !== launchId) {
        return { settled: false, reason: 'not the current lead launch' }
      }
      apply(record)
      record.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return { settled: true, launchId }
    })
  }

  /**
   * Shuts admission synchronously, drains every admitted mutation, then
   * releases the claim. Idempotent, and concurrent calls share the one
   * drain: nothing admitted runs after the lock is released, and no
   * successor acquires the root before admitted work has landed.
   */
  close() {
    // The closing check comes FIRST: from here to the released claim there
    // is one drain, and every concurrent caller is handed that one — during
    // the release `#opened` is already false, and a fresh resolved promise
    // there would tell its caller the claim is gone while it is still on
    // disk.
    if (this.#closing) return this.#closePromise
    if (!this.#opened) return Promise.resolve()
    this.#closing = true
    this.#closePromise = (async () => {
      await this.#tail
      this.#opened = false
      await this.#releaseLock()
    })()
    return this.#closePromise
  }

  /**
   * ONE queue for the whole app. `fn` runs only after everything enqueued
   * before it has settled; its return (or rejection) is the caller's.
   * `name` is the op name for the record (`conversation.create`, `tab.*`…).
   */
  async mutate(cwd, name, fn) {
    return this.#enqueue(cwd, name, fn, false)
  }

  /**
   * Reconciles the state a failed compensation left behind and lifts the
   * degraded refusal. It releases exactly what the restart rule releases —
   * a reservation whose pane no longer serves it — and it is the only op
   * that runs while the store is degraded.
   */
  async recover() {
    return this.#enqueue(
      null,
      'store.recover',
      async (io) => {
        const reservationsReleased = await this.#reconcileReservations(await io.readTabs())
        // Cleared INSIDE the queued job: a mutation waiting behind this one
        // must see the lifted refusal, not race the assignment. A failed
        // reconciliation leaves the store degraded, which is the point.
        this.#degraded = null
        return { reservationsReleased }
      },
      true,
    )
  }

  /** True while a failed compensation is refusing every mutation. */
  get degraded() {
    return this.#degraded !== null
  }

  #enqueue(cwd, name, fn, allowDegraded) {
    if (!this.#opened || this.#closing) {
      throw new Error('the store is not open — open() claims the root before anything mutates')
    }
    if (typeof name !== 'string' || name.length === 0) {
      throw new Error(`mutate needs an op name, got ${JSON.stringify(name)}`)
    }
    if (typeof fn !== 'function') throw new Error(`mutate('${name}') needs a function`)
    const io = this.#io(cwd ?? null)
    const run = this.#tail.then(async () => {
      // The degraded refusal is checked HERE and not at admission: a
      // mutation queued BEFORE the failure has not run yet either, and it
      // would run against state the store already knows is inconsistent.
      if (!allowDegraded && this.#degraded !== null) throw degradedRefusal(this.#degraded)
      return fn(io)
    })
    // The chain swallows the rejection: the failing job still rejects for
    // its own caller, and the queue keeps working for everyone after it.
    // A mutation that SUCCEEDED announces itself here, in the queue slot it
    // just finished: after its write, before the next mutation starts, once.
    this.#tail = run.then(
      () => {
        this.#announce(name, cwd ?? null)
        return undefined
      },
      () => undefined,
    )
    return run
  }

  /**
   * Watch every mutation this store completes. Returns the unsubscribe.
   *
   * ONE call per mutation, after its write and before the next mutation
   * runs, so a listener that reads the store sees exactly what the mutation
   * left behind. A failed mutation changed nothing and says nothing.
   *
   * Listeners run INSIDE the queue slot, so they must be synchronous and
   * quick — the app's is a single bridge frame. A listener that throws is
   * ignored: the queue is not the place to discover a broken observer.
   */
  onMutation(listener) {
    if (typeof listener !== 'function') throw new Error('onMutation needs a function')
    this.#watchers.add(listener)
    return () => this.#watchers.delete(listener)
  }

  #announce(name, cwd) {
    for (const listener of this.#watchers) {
      try {
        listener({ op: name, cwd })
      } catch {
        // An observer's fault is not the mutation's, and never the queue's.
      }
    }
  }

  // --- reads (safe outside the queue: every write is tmp+rename) ---------

  async readThreads(cwd) {
    requireText(cwd, 'workspace directory')
    return readJsonMap(threadsFile(this.#root, cwd))
  }

  async readTabs() {
    return (await readTabsFile(this.#root)).tabs
  }

  async readDeliveries(cwd) {
    requireText(cwd, 'workspace directory')
    return readJsonMap(deliveriesFile(this.#root, cwd))
  }

  async readInbox(cwd) {
    requireText(cwd, 'workspace directory')
    return readInboxFile(inboxFile(this.#root, cwd))
  }

  // --- named ops -----------------------------------------------------------

  /**
   * Creates a conversation row. With `pane: {tab, id, generation?}` the row
   * is reserved for that launch — the two-file write whose crash window the
   * recovery rule covers. `launch` carries the binding evidence minted
   * before the launch (`nonce`, `preallocatedId`, `reportedId`).
   *
   * Every check runs before the first write: an unknown tab, a directory
   * mismatch, a generation or ownership conflict never leaves a row behind.
   * If the second (pane-link) write fails, the first file is restored
   * inside the same queued mutation before the error reaches the caller.
   */
  async conversationCreate(
    cwd,
    { name, agent, kind, lead = null, pane = null, launch = null } = {},
  ) {
    return this.mutate(cwd, 'conversation.create', async (io) => {
      requireText(name, 'conversation name')
      requireText(agent, 'agent')
      requireText(kind, 'kind')
      if (launch !== null && launch !== undefined && pane === null) {
        throw new Error('a launch happens in a pane — pass pane with launch')
      }
      const threads = await io.readThreads()
      if (threads[name] !== undefined) {
        throw new Error(`a conversation named ${name} already exists in this workspace`)
      }
      // Validation stages the pane link — and any identity it issues — in
      // memory and throws before any write: a refusal commits nothing,
      // anywhere, and burns no pane identity.
      let envelope = null
      if (pane !== null) {
        envelope = await io.readTabsEnvelope()
        linkConversationPane(envelope, cwd, pane, name)
      }
      const at = nowIso()
      const row = {
        agent,
        kind,
        lead,
        sessionId: null,
        runs: 0,
        sent: [],
        seen: [],
        createdAt: at,
        updatedAt: at,
      }
      if (pane !== null) row.reserved = reservationFrom(pane, launch, at)
      const previousThreads = structuredClone(threads)
      threads[name] = row
      await this.#commitLinkedThreads(io, threads, previousThreads, envelope)
      return row
    })
  }

  /**
   * Admission: decide whether a conversation needs a launch, and take the
   * reservation, in ONE queued operation.
   *
   * This is one operation because the decision and the write cannot be
   * separated without a race. Reading the panes and the reservation, then
   * reserving in a later mutation, means two callers can both read "no
   * pane here", both decide to launch, and both write — two panes, two
   * processes, one conversation. So everything a launch decision rests on
   * is read here, inside the queue, and the reservation is taken before
   * anything else runs. Bridge I/O stays OUTSIDE: this holds the queue only
   * as long as reading and writing local state takes.
   *
   * Nothing is ever released on the strength of a snapshot taken earlier,
   * and nothing but a pane's own exit releases a reservation: a CLOSED tab
   * is refused before anything changes, because a closed tab says nothing
   * about whether its panes' processes ended.
   *
   * A reservation belonging to ANOTHER tab is never released here at all —
   * that tab's own pane exit releases it — and a stale reservation of this
   * tab is released and replaced in this same step, having just been read.
   *
   * `launch` may be a function of the row this mutation read, for the
   * evidence that depends on what the conversation already is.
   *
   * Answers `{outcome: 'live', pane}` when the conversation is already
   * running in a pane of this tab, or `{outcome: 'reserved', pane, row}`
   * with the pane it minted. Refuses `reserved` (a launch that has not come
   * back) and `elsewhere` (running under another tab).
   */
  async admit(cwd, { name, tab, agent, kind, lead = null, launch = null, notify } = {}) {
    return this.mutate(cwd, 'launch.admit', async (io) => {
      requireText(name, 'conversation name')
      requireText(tab, 'tab id')
      const threads = await io.readThreads()
      const envelope = await io.readTabsEnvelope()
      const tabRecord = findTab(envelope.tabs, tab)
      if (!Array.isArray(tabRecord.panes)) tabRecord.panes = []
      // A closed session is refused before anything changes — and it is
      // never evidence that its panes' processes ended. Treating a closed
      // tab as "the pane is gone" released a live reservation and minted a
      // second pane for a conversation that already had one running, with
      // no exit ever coming for the first. Only `pane.exit` releases.
      if (tabRecord.closed === true) {
        throw new AdmissionError(`the session ${tab} is closed — resume it first`, 'session-closed')
      }

      if (tabRecord.deletedConversations?.includes(name)) {
        throw new AdmissionError(
          'this conversation was deleted from this session',
          'conversation-deleted',
        )
      }
      const row = threads[name]
      if (row !== undefined && !isRecord(row)) throw new Error(`${name} is not a conversation`)
      // Identity, against the row THIS mutation read: a roster that changed
      // between the caller's look and this write cannot put a different
      // agent or harness onto an existing conversation.
      if (isRecord(row) && agent !== undefined) {
        if (row.agent !== agent || (kind !== undefined && row.kind !== kind)) {
          throw new AdmissionError(
            `${name} belongs to ${row.agent} (${row.kind}), not ${agent} (${kind})`,
            'agent-mismatch',
            { agent: row.agent, kind: row.kind },
          )
        }
      }
      if (isRecord(row) && (row.role === 'advisor' || tabRecord.role === 'pm')) {
        if (row.role !== 'advisor' || !row.lead?.startsWith(`tab:${tab}:`)) {
          throw new AdmissionError(`${name} belongs to another group`, 'elsewhere')
        }
      }
      const previousThreads = structuredClone(threads)

      if (isRecord(row) && isRecord(row.reserved)) {
        const reserved = row.reserved
        if (reserved.resolvedAt === undefined) {
          throw new AdmissionError(
            `${name} is reserved by a launch that has not come back — one conversation never opens two`,
            'reserved',
          )
        }
        const live = linkedPane(envelope.tabs, reserved)
        if (reserved.tab !== tab) {
          // Another session holds it. Never released from here, whether or
          // not its pane still answers: that session's exit releases it,
          // and taking it would put two windows on one native session.
          throw new AdmissionError(
            `${name} is running in another session — open it there`,
            'elsewhere',
            { session: reserved.tab },
          )
        }
        if (live !== null && live.conversation === name) {
          if (notify === undefined) return { outcome: 'live', pane: live, row }
          const applied = recordLeadPreference(row, notify)
          if (applied.ok !== true) {
            throw new Error(`the lead preference is refused for ${name}: ${applied.reason}`)
          }
          threads[name] = { ...applied.row, updatedAt: nowIso() }
          await io.writeThreads(threads)
          return { outcome: 'live', pane: live, row: threads[name] }
        }
        // This tab's own reservation, whose pane is gone: released here,
        // having just been read in this same mutation.
        delete row.reserved
      }

      if (row === undefined && (agent === undefined || kind === undefined)) {
        throw new Error(`no conversation named ${name} in this workspace`)
      }
      // The lead's `--notify` preference is applied HERE, with the
      // reservation, rather than in a write of its own afterwards: a
      // separate write is a window in which a failure leaves a conversation
      // reserved with nothing running and no exit ever coming for it.
      if (notify !== undefined && isRecord(row)) {
        const applied = recordLeadPreference(row, notify)
        if (applied.ok !== true) {
          throw new Error(`the lead preference is refused for ${name}: ${applied.reason}`)
        }
        threads[name] = applied.row
      }
      const pane = appendWorkerPane(envelope, tabRecord, name)
      const at = nowIso()
      // The launch record may be a function of the row, because what a
      // launch must bind by depends on whether this conversation already
      // has a native session — and that is only known here, from the row
      // this mutation read.
      const record = typeof launch === 'function' ? await launch(row) : launch
      const reserved = reservationFrom(
        { tab, id: pane.id, generation: pane.generation },
        record,
        at,
      )
      if (row === undefined) {
        const created = {
          agent,
          kind,
          lead,
          role: tabRecord.role === 'pm' ? 'advisor' : 'worker',
          ...(typeof tabRecord.lead.nativeSession === 'string' && tabRecord.lead.nativeSession
            ? {
                leadContext: {
                  kind: tabRecord.lead.harness,
                  session: tabRecord.lead.nativeSession,
                },
              }
            : {}),
          sessionId: null,
          runs: 0,
          sent: [],
          seen: [],
          createdAt: at,
          updatedAt: at,
          reserved,
        }
        if (notify !== undefined) {
          const applied = recordLeadPreference(created, notify)
          if (applied.ok !== true) {
            throw new Error(`the lead preference is refused for ${name}: ${applied.reason}`)
          }
          threads[name] = applied.row
        } else {
          threads[name] = created
        }
      } else {
        const current = threads[name]
        current.reserved = reserved
        current.updatedAt = at
        restampBinding(current, reserved, at)
      }
      await this.#commitLinkedThreads(io, threads, previousThreads, envelope)
      return { outcome: 'reserved', pane, row: threads[name] }
    })
  }

  /**
   * A pane's process ended: release the reservation, but ONLY when the
   * reservation is the one that names this pane, compared inside the queue.
   *
   * An exit event is not proof about the CURRENT reservation. A duplicate
   * exit, or one that arrives after the conversation reopened, names a pane
   * the reservation no longer holds — releasing on its word would free the
   * successor's launch and let a second window open on it.
   */
  async releaseExitedPane(cwd, { name, tab, pane, generation } = {}) {
    return this.mutate(cwd, 'reservation.releaseExited', async (io) => {
      requireText(name, 'conversation name')
      requireText(tab, 'tab id')
      requireText(pane, 'pane id')
      const threads = await io.readThreads()
      const row = threads[name]
      if (!isRecord(row) || !isRecord(row.reserved)) return { released: null, reason: 'none' }
      const reserved = row.reserved
      if (reserved.tab !== tab || reserved.pane !== pane || reserved.generation !== generation) {
        return { released: null, reason: 'stale', held: reserved.launchId }
      }
      const { launchId } = reserved
      delete row.reserved
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return { released: launchId, reason: 'released' }
    })
  }

  /**
   * Reserves an EXISTING conversation for a launch: the launch-ticket path,
   * where the conversation predates the pane. Same two-file commit as
   * creation — validated before the first write, restored on a second-file
   * failure — and the same generation/ownership rules for the pane.
   */
  async reserve(cwd, { name, pane, launch = null } = {}) {
    return this.mutate(cwd, 'reservation.create', async (io) => {
      requireText(name, 'conversation name')
      if (!isRecord(pane)) throw new Error('pane must be { tab, id, generation? }')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      if (isRecord(row.reserved)) {
        throw new Error(
          `${name} is already reserved for pane ${row.reserved.pane} — release it first`,
        )
      }
      const envelope = await io.readTabsEnvelope()
      linkConversationPane(envelope, cwd, pane, name)
      const previousThreads = structuredClone(threads)
      row.reserved = reservationFrom(pane, launch, nowIso())
      row.updatedAt = nowIso()
      await this.#commitLinkedThreads(io, threads, previousThreads, envelope)
      return row
    })
  }

  /**
   * Marks a reservation's launch resolved — the pane is open and serving.
   *
   * A reservation without this stamp is a launch still in flight, and that
   * is the whole point of the op: a `pane.open` whose answer never came
   * back leaves the reservation unresolved, and an unresolved reservation
   * is what refuses the next launch for that conversation. The caller names
   * the launch it is resolving, compared INSIDE the queued mutation, so a
   * late answer from an ended launch never resolves the one that replaced
   * it. Resolving twice is the same answer, not a second event: the stamp
   * is written once, so a retry that crossed the first answer cannot move
   * it.
   */
  async resolve(cwd, { name, launchId, outcome = 'opened' } = {}) {
    return this.mutate(cwd, 'reservation.resolve', async (io) => {
      requireText(name, 'conversation name')
      requireText(launchId, 'launch id')
      requireText(outcome, 'launch outcome')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      if (!isRecord(row.reserved)) {
        throw new Error(`${name} holds no reservation to resolve`)
      }
      if (row.reserved.launchId !== launchId) {
        throw new Error(
          `${name} is reserved for launch ${row.reserved.launchId}, not ${launchId} — a stale resolve never resolves another launch's reservation`,
        )
      }
      if (typeof row.reserved.resolvedAt === 'string') return row
      row.reserved.resolvedAt = nowIso()
      row.reserved.outcome = outcome
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /**
   * Releases a conversation's launch reservation. The pane link stays —
   * the pane still names the conversation — but nothing may bind against
   * the released launch afterwards.
   *
   * The caller names the launch it is releasing, and the comparison happens
   * INSIDE the queued mutation: a launch's delayed cleanup that arrives
   * after the next launch reserved the same conversation names a launch
   * that is over, and must take nothing with it.
   */
  async release(cwd, { name, launchId } = {}) {
    return this.mutate(cwd, 'reservation.release', async (io) => {
      requireText(name, 'conversation name')
      requireText(launchId, 'launch id')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      if (!isRecord(row.reserved)) {
        throw new Error(`${name} holds no reservation to release`)
      }
      if (row.reserved.launchId !== launchId) {
        throw new Error(
          `${name} is reserved for launch ${row.reserved.launchId}, not ${launchId} — a stale release never takes another launch's reservation`,
        )
      }
      delete row.reserved
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /**
   * Binds a native session to a conversation. The caller brings the
   * candidate discovery observed; the launch record — and the verdict —
   * are the store's. `bindEvidence(kind, candidate, launch)` decides; a
   * bind happens only on `bound: true`, and the row records the evidence
   * and the pane generation the binding was made at.
   */
  async sessionBind(cwd, { name, candidate = {}, expect } = {}) {
    return this.mutate(cwd, 'session.bind', async (io) => {
      requireText(name, 'conversation name')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      if (!isRecord(row.reserved)) {
        throw new StoreRefusal(
          `session.bind refuses for ${name}: the store holds no launch record`,
          'no-launch',
        )
      }
      requireCurrentLaunch(row, name, expect)
      // The binding is made against the pane as linked: a launch reserved
      // at one generation never authorises a write for another, and a pane
      // serving someone else never lends its reservation.
      requireLinkedPane(await io.readTabs(), row.reserved, name)
      const decision = bindEvidence(row.kind, candidate, row.reserved)
      if (decision?.bound !== true) {
        if (decision?.replaced === true) {
          // The native session changed in place: the old binding dies on
          // disk with it — kept rows would keep authorising the wrong
          // session. The refusal still reaches the caller.
          row.replaced = {
            at: nowIso(),
            reason: decision.reason,
            previousSession: row.sessionId,
          }
          row.sessionId = null
          delete row.binding
          row.updatedAt = nowIso()
          await io.writeThreads(threads)
        }
        const detail =
          decision?.replaced === true
            ? 'replaced: the native session changed in place — the binding and every decision leaning on it die with it'
            : decision?.reason
        throw new StoreRefusal(`session.bind refuses for ${name}: ${detail}`, 'no-evidence')
      }
      row.sessionId = candidate.sessionId
      row.binding = {
        evidence: decision.evidence,
        generation: row.reserved.generation,
        launchId: row.reserved.launchId,
        at: nowIso(),
      }
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /** Appends to the row's `sent` list — what was pasted into the pane. */
  async sentRecord(cwd, { name, entry = {}, expect } = {}) {
    return this.mutate(cwd, 'sent.record', async (io) => {
      requireText(name, 'conversation name')
      if (!isRecord(entry)) throw new Error('entry must be an object')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      requireCurrentLaunch(row, name, expect)
      if (!Array.isArray(row.sent)) row.sent = []
      row.sent.push({ ...entry, at: nowIso() })
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /**
   * Unions item ids into the conversation's marks for one app-owned lead
   * identity (`tab:<id>:<generation>`). Marks are scoped per lead AND per
   * generation: a resumed lead starts unmarked, and unseeing is not an op.
   */
  async seenSet(cwd, { name, items = [], lead } = {}) {
    return this.mutate(cwd, 'seen.set', async (io) => {
      requireText(name, 'conversation name')
      requireText(lead, 'lead identity')
      if (!Array.isArray(items)) throw new Error('items must be an array of transcript item ids')
      for (const item of items) requireText(item, 'seen item id')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      // Nested maps come back from JSON with an ordinary prototype, so the
      // one keyed by an identity is rebuilt without one here.
      row.seen = isRecord(row.seen)
        ? Object.assign(Object.create(null), row.seen)
        : Object.create(null)
      if (!Array.isArray(row.seen[lead])) row.seen[lead] = []
      const marks = row.seen[lead]
      const known = new Set(marks)
      for (const item of items) {
        if (known.has(item)) continue
        marks.push(item)
        known.add(item)
      }
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /**
   * Records what `cf run --notify` asked for, through `policy.js`'s own
   * writer so the rule and the write cannot drift apart. It reaches
   * exactly one field, `notifyPreference`: the human's scopes are the tab
   * and the pane, which `recordLeadPreference` is never handed.
   */
  async leadPreferenceSet(cwd, { name, value } = {}) {
    return this.mutate(cwd, 'lead.preference', async (io) => {
      requireText(name, 'conversation name')
      const threads = await io.readThreads()
      const applied = recordLeadPreference(requireRow(threads, name), value)
      if (applied.ok !== true) {
        throw new Error(`the lead preference is refused for ${name}: ${applied.reason}`)
      }
      threads[name] = { ...applied.row, updatedAt: nowIso() }
      await io.writeThreads(threads)
      return threads[name]
    })
  }

  /**
   * The launch's own account of where it has got to, written by the
   * controller under its capability. A state, not a log: the latest note
   * replaces the last one, so nothing here grows without bound.
   */
  async progressSet(cwd, { name, progress = {}, expect } = {}) {
    return this.mutate(cwd, 'progress.set', async (io) => {
      requireText(name, 'conversation name')
      if (!isRecord(progress)) throw new Error('progress must be an object')
      requireText(progress.state, 'progress state')
      const threads = await io.readThreads()
      const row = requireRow(threads, name)
      requireCurrentLaunch(row, name, expect)
      row.progress = { ...progress, at: nowIso() }
      row.updatedAt = nowIso()
      await io.writeThreads(threads)
      return row
    })
  }

  /**
   * Mints the next delivery identity — app-wide, durable, and one at a time.
   *
   * `hosts/lib/deliveries.js` is pure and mints nothing, so ids from two
   * plans were only ever unique within one call. Identity is the store's,
   * like pane identity: the counter is persisted in the same envelope as
   * the pane allocator's, incremented inside the mutation queue (so fifty
   * at once are fifty ids), and only ever climbing — a restart continues
   * the sequence rather than beginning it again.
   *
   * The namespace is `d-<n>`, digits only, because a delivery id becomes a
   * path: `<workspace>/deliveries/<id>.md`, and it is quoted back to the
   * lead inside the envelope's own markers. One path segment, nothing a
   * shell or a filesystem reads as structure.
   */
  async allocateDeliveryId() {
    return this.mutate(null, 'delivery.allocate', async (io) => {
      const envelope = await io.readTabsEnvelope()
      // Past this an increment stops being exact, and two deliveries would
      // share an id — and a delivery id names a file.
      if (envelope.nextDelivery >= Number.MAX_SAFE_INTEGER) {
        throw new Error('the delivery counter has reached the last id this build can count')
      }
      const id = `d-${envelope.nextDelivery}`
      envelope.nextDelivery += 1
      await io.writeTabsEnvelope(envelope)
      return id
    })
  }

  /**
   * The human policy, from the page only. Tab scope takes `auto|manual`
   * (a tab `manual` vetoes everything under it); pane scope also takes
   * `inherit`, which falls through to the tab's.
   */
  async policySet({ tab, pane = null, value } = {}) {
    return this.mutate(null, 'policy.set', async (io) => {
      requireText(tab, 'tab id')
      const tabs = await io.readTabs()
      const record = findTab(tabs, tab)
      if (pane === null) {
        requireOneOf(value, TAB_POLICY_VALUES, 'policy')
        record.policy = value
      } else {
        requireText(pane, 'pane id')
        const paneRecord = findPane(record, pane)
        requireOneOf(value, PANE_POLICY_VALUES, 'policy')
        paneRecord.policy = value
      }
      record.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return record
    })
  }

  // --- internals -----------------------------------------------------------

  /**
   * The two-file commit both reservation writers share. The reservation
   * lands in threads.json before the pane lands in tabs.json: a crash
   * between the two writes leaves a reservation with no pane, and the
   * recovery rule releases exactly that. A failed second write restores
   * the first file instead of leaving a partial row.
   */
  async #commitLinkedThreads(io, threads, previousThreads, envelope) {
    await io.writeThreads(threads)
    if (envelope === null) return
    try {
      await io.writeTabsEnvelope(envelope)
    } catch (error) {
      try {
        await io.writeThreads(previousThreads)
      } catch (compensation) {
        // Neither the second write nor the undo landed: the reservation is
        // committed without its pane, and no later mutation may build on a
        // row the store knows is half-written. Both failures are kept —
        // the second one explains why the first was not undone.
        this.#degraded = { at: nowIso(), error, compensation }
        throw degradedFailure(error, compensation)
      }
      throw error
    }
  }

  /** Every write funnels through here: atomic, and never outside the root. */
  async #writeAppJson(file, value) {
    const relative = path.relative(this.#root, file)
    if (relative.startsWith('..') || path.isAbsolute(relative)) {
      throw new Error(`the store writes only inside its own root: ${file}`)
    }
    await writeJsonAtomic(file, value)
  }

  #io(cwd) {
    const workspace = (at) => {
      const target = at ?? cwd
      if (target === null || target === undefined) {
        throw new Error('this mutation needs a workspace directory')
      }
      return target
    }
    return {
      readThreads: (at) => readJsonMap(threadsFile(this.root, workspace(at))),
      writeThreads: (threads, at) =>
        this.#writeAppJson(threadsFile(this.root, workspace(at)), asMap(threads, 'threads')),
      readDeliveries: (at) => readJsonMap(deliveriesFile(this.root, workspace(at))),
      readInbox: (at) => readInboxFile(inboxFile(this.root, workspace(at))),
      writeInbox: (inbox, at) =>
        this.#writeAppJson(inboxFile(this.root, workspace(at)), requireInbox(inbox)),
      writeDeliveries: (deliveries, at) =>
        this.#writeAppJson(
          deliveriesFile(this.root, workspace(at)),
          asMap(deliveries, 'deliveries'),
        ),
      readTabs: async () => (await readTabsFile(this.root)).tabs,
      writeTabs: async (tabs) => {
        asArray(tabs, 'tabs')
        const envelope = await readTabsFile(this.root)
        await this.#writeAppJson(tabsFile(this.root), {
          ...envelope,
          nextTab: Math.max(envelope.nextTab, tabCounter(undefined, tabs)),
          tabs,
        })
      },
      // The whole envelope, for the paths that issue a pane identity: the
      // allocator's state and the tabs it belongs to commit together.
      readTabsEnvelope: () => readTabsFile(this.root),
      writeTabsEnvelope: async (envelope) => {
        asArray(envelope?.tabs, 'tabs')
        asMap(envelope?.issued, 'issued pane ids')
        if (!Number.isInteger(envelope?.nextPane) || envelope.nextPane < 1) {
          throw new Error('the pane counter must stay a positive integer')
        }
        if (!Number.isSafeInteger(envelope?.nextDelivery) || envelope.nextDelivery < 1) {
          throw new Error('the delivery counter must stay a positive safe integer')
        }
        await this.#writeAppJson(tabsFile(this.root), {
          nextTab: tabCounter(envelope.nextTab, envelope.tabs),
          nextPane: envelope.nextPane,
          nextDelivery: envelope.nextDelivery,
          issued: envelope.issued,
          tabs: envelope.tabs,
        })
      },
    }
  }

  /**
   * Takes the root by locking it, and holds the descriptor for as long as
   * this Store lives.
   *
   * The lock is the kernel's, on the lock file's inode, taken at open time
   * with `O_EXLOCK | O_NONBLOCK`: a second open — in this process or any
   * other — fails with `EAGAIN`, and the kernel drops it when the
   * descriptor closes, which includes the owning process dying. There is
   * nothing to read, nothing to judge and nothing to reclaim, so there is
   * no moment between deciding and acting for a second instance to arrive
   * in.
   */
  async #acquireLock() {
    if (!O_EXLOCK_PLATFORMS.has(process.platform)) {
      // Refusing, not falling back. Where this flag means something else
      // the open SUCCEEDS and the store runs unlocked — two instances
      // owning one root, silently, which is worse than not starting.
      throw new Error(
        `the instance lock needs O_EXLOCK, which this build assumes only on BSD and macOS, not ${process.platform} — refusing the state root rather than run without a lock`,
      )
    }
    await fs.mkdir(path.dirname(this.#lockPath), { recursive: true })
    try {
      this.#lockHandle = await fs.open(
        this.#lockPath,
        fs.constants.O_RDWR | fs.constants.O_CREAT | O_EXLOCK | fs.constants.O_NONBLOCK,
        0o600,
      )
    } catch (error) {
      if (error?.code === 'EAGAIN' || error?.code === 'EWOULDBLOCK') {
        throw new Error(`another ConsensFlow instance holds ${this.#lockPath}`)
      }
      throw error
    }
    // Diagnostics, and only diagnostics: these bytes say who to go looking
    // for when something is stuck. Nothing reads them back, and no
    // contender is admitted or refused on their strength — so the file says
    // as much, in the file, where whoever finds it will be reading. Every
    // acquisition rewrites it, so it never names a process that is gone.
    try {
      const note = { pid: process.pid, startedAt: Math.floor(Date.now() / 1000) }
      await this.#lockHandle.truncate(0)
      await this.#lockHandle.write(`${JSON.stringify(note)}\n${LOCK_FILE_NOTE}\n`, 0)
    } catch (error) {
      // The DESCRIPTOR is the lock, so anything that fails after it is open
      // has to give it back here: this Store never becomes `opened`, so
      // `close()` will not do it, and the root would stay locked for the
      // life of the process over a note nobody reads.
      try {
        await this.#releaseLock()
      } catch {
        // The descriptor is already forgotten; what the caller needs is the
        // failure that stopped the open, not one from tidying up after it.
      }
      throw error
    }
  }

  async #releaseLock() {
    const handle = this.#lockHandle
    this.#lockHandle = null
    if (handle === null) return
    // Closing the descriptor releases the lock. The FILE stays: unlinking
    // it would let a successor create and lock a NEW inode while someone
    // still holds the old one — two owners, by tidiness.
    await handle.close()
  }
  /**
   * Restart recovery, under the queue like every other write: every tab
   * reads `closed`, and a reservation whose pane no longer serves it is
   * released.
   */
  async #restartRecovery() {
    return this.#enqueue(
      null,
      'store.recover',
      async (io) => {
        const envelope = await io.readTabsEnvelope()
        const tabs = envelope.tabs
        let closed = 0
        let named = false
        let restored = false
        for (const tab of tabs) {
          if (isRecord(tab) && !tab.roleName) {
            tab.role = tab.role === 'pm' ? 'pm' : 'lead'
            tab.roleName = newSessionName(
              tabs.map((item) => `${tab.role}-${item.roleName}`),
              [],
              tab.role,
            ).slice(tab.role.length + 1)
            named = true
          }
          if (isRecord(tab) && tab.closed !== true) {
            tab.closed = true
            closed += 1
          }
        }
        for (const tab of tabs) {
          if (
            !isRecord(tab) ||
            typeof tab.directory !== 'string' ||
            !Array.isArray(tab.panes) ||
            !Number.isSafeInteger(tab.lead?.generation)
          )
            continue
          const threads = await io.readThreads(tab.directory)
          for (const [name, row] of Object.entries(threads)) {
            if (tab.deletedConversations?.includes(name)) continue
            if (tab.role === 'pm' && row?.role !== 'advisor') continue
            const owner = typeof row?.lead === 'string' ? row.lead.split(':') : []
            if (
              owner.length !== 3 ||
              owner[0] !== 'tab' ||
              owner[1] !== tab.id ||
              !/^[1-9][0-9]*$/.test(owner[2]) ||
              Number(owner[2]) > tab.lead.generation ||
              tab.panes.some((pane) => pane.conversation === name)
            )
              continue
            // Older builds discarded pane rows on restart. Restore navigation,
            // never a process, and retain the conversation's original ownership.
            const pane = appendWorkerPane(envelope, tab, name)
            pane.closed = true
            pane.alive = false
            restored = true
          }
        }
        if (closed > 0 || named || restored) await io.writeTabsEnvelope(envelope)
        const released = await this.#reconcileReservations(tabs)
        return { tabsClosed: closed, reservationsReleased: released }
      },
      true,
    )
  }

  /**
   * The documented rule, in one place because both recoveries need it: a
   * reservation survives only while its pane still serves its conversation
   * at the reserved generation. Runs inside a queued mutation.
   */
  async #reconcileReservations(tabs) {
    const released = []
    const workspacesRoot = path.join(this.#root, 'workspaces')
    let entries = []
    try {
      entries = await fs.readdir(workspacesRoot, { withFileTypes: true })
    } catch (error) {
      // ENOENT alone means empty. Any other failure to read the directory
      // is not proof it holds no reservations, and recovery that skipped it
      // would silently keep every reservation it could not judge.
      if (error?.code !== 'ENOENT') {
        throw new Error(`cannot read ${workspacesRoot}: ${error?.message ?? error}`)
      }
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue
      const file = path.join(workspacesRoot, entry.name, 'threads.json')
      const threads = await readJsonMap(file)
      let changed = false
      for (const [name, row] of Object.entries(threads)) {
        if (!isRecord(row) || !isRecord(row.reserved)) continue
        if (!paneExists(tabs, row.reserved, name)) {
          delete row.reserved
          row.updatedAt = nowIso()
          changed = true
          released.push(`${entry.name}/${name}`)
        }
      }
      if (changed) await this.#writeAppJson(file, threads)
    }
    return released
  }
}

// --- file layout ------------------------------------------------------------

function workspaceRoot(root, cwd) {
  return path.join(root, 'workspaces', workspaceKey(cwd))
}

function threadsFile(root, cwd) {
  return path.join(workspaceRoot(root, cwd), 'threads.json')
}

function deliveriesFile(root, cwd) {
  return path.join(workspaceRoot(root, cwd), 'deliveries.json')
}

function inboxFile(root, cwd) {
  return path.join(workspaceRoot(root, cwd), 'inbox.json')
}

function requireInbox(value) {
  if (value?.version !== 1 || !isRecord(value.results) || !isRecord(value.receivers))
    throw new Error('cannot read inbox: unsupported version or invalid records')
  return value
}

async function readInboxFile(file) {
  const state = requireInbox(await readJsonMap(file, emptyInbox))
  state.results = Object.assign(Object.create(null), state.results)
  state.receivers = Object.assign(Object.create(null), state.receivers)
  return state
}

function tabsFile(root) {
  return path.join(root, 'app', 'tabs.json')
}

/**
 * Missing reads as empty — a workspace or tab list that was never written is
 * "nothing yet". Anything else unreadable (corrupt bytes, wrong shape, I/O
 * error) refuses: an unreadable file is never proof its rows do not exist,
 * and no mutation may adopt it as a fresh map. The bytes stay on disk for
 * whoever investigates.
 */
async function readJsonMap(file, missing = () => Object.create(null)) {
  let text
  try {
    text = await fs.readFile(file, 'utf8')
  } catch (error) {
    if (error?.code === 'ENOENT') return missing()
    throw new Error(`cannot read ${file}: ${error?.message ?? error}`)
  }
  let parsed
  try {
    parsed = JSON.parse(text)
  } catch {
    throw new Error(`cannot read ${file}: not JSON`)
  }
  if (!isRecord(parsed)) throw new Error(`cannot read ${file}: not an object`)
  // Every map this store keys by a name it did not choose — conversations,
  // deliveries, issued pane ids — is null-prototype. On an ordinary object
  // a key like `__proto__` reads the prototype instead of a row and writes
  // through a setter instead of storing, so a lookup would answer for a row
  // that does not exist and a write would vanish.
  return Object.assign(Object.create(null), parsed)
}

/**
 * `tabs.json` is an envelope: the tab list plus the pane allocator's own
 * state — the app-wide counter and the identities it has issued. Both live
 * beside the tabs so an identity is issued under the same queue that guards
 * the tabs: issuing and linking can never interleave, and one write commits
 * both. Absent reads as a fresh root; corrupt reads refuse (S5).
 */
async function readTabsFile(root) {
  const file = tabsFile(root)
  try {
    await fs.access(file)
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return { nextTab: 1, nextPane: 1, nextDelivery: 1, issued: Object.create(null), tabs: [] }
    }
    throw new Error(`cannot read ${file}: ${error?.message ?? error}`)
  }
  const parsed = await readJsonMap(file)
  if (!Array.isArray(parsed.tabs)) throw new Error(`cannot read ${file}: no tab list`)
  if (!Number.isInteger(parsed.nextPane) || parsed.nextPane < 1) {
    throw new Error(`cannot read ${file}: no pane counter`)
  }
  const issued = parsed.issued ?? Object.create(null)
  if (!isRecord(issued)) throw new Error(`cannot read ${file}: the issued pane ids are not a map`)
  return {
    nextTab: tabCounter(parsed.nextTab, parsed.tabs),
    nextPane: parsed.nextPane,
    // A store written before delivery ids were minted has no counter, and
    // has therefore issued none: starting at 1 repeats nothing.
    nextDelivery: deliveryCounter(parsed.nextDelivery, file),
    issued: Object.assign(Object.create(null), issued),
    tabs: parsed.tabs,
  }
}

/**
 * The delivery counter as read back. Absent means a store from before this
 * allocator existed — it has issued nothing, so 1 is safe. Present but not
 * a positive integer is corruption, and corruption never reads as empty:
 * starting over would hand out ids that name delivery files already on
 * disk.
 */
function tabCounter(value, tabs) {
  let minimum = 1
  for (const tab of tabs) {
    const match = /^t-(\d+)$/.exec(tab?.id ?? '')
    if (match) minimum = Math.max(minimum, Number(match[1]) + 1)
  }
  const counter = value === undefined ? minimum : value
  if (!Number.isSafeInteger(counter) || counter < minimum) {
    throw new Error('the tab counter must be a positive safe integer above existing ids')
  }
  return counter
}

function deliveryCounter(value, file) {
  // ABSENT means a store from before this allocator existed: it has issued
  // nothing, so 1 repeats nothing. `null` is a value somebody wrote, and
  // reading it as absent would start the sequence over on top of delivery
  // files already written and immutable.
  if (value === undefined) return 1
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new Error(
      `cannot read ${file}: the delivery counter must be a positive safe integer, not ${JSON.stringify(value)}`,
    )
  }
  return value
}

const MINTED_PANE_ID = /^p-\d+$/

/**
 * The ONE place a pane identity is issued, for every path that creates a
 * pane: `tab.create`, `tab.addPane`, and both reservation writers. Pane
 * identity is app-wide, so every check is app-wide too:
 * - `p-<n>` is the allocator's own namespace, minted from a counter that
 *   only ever climbs, so a minted id is never handed out twice and no
 *   caller may name one.
 * - an id that is live in ANY tab is a collision, not a second pane.
 * - an id that was issued before may come back only at a NEWER generation.
 *   A pane is `(id, generation)`: reissuing the pair would make every
 *   record that still names the old pane — a removal in flight, a binding —
 *   name the replacement instead.
 *
 * Mutates the envelope in memory. The caller writes it once, with the tabs,
 * so a refusal later in the same mutation burns no identity.
 */
export function allocatePaneId(envelope, { id = null, generation = 1 } = {}) {
  if (!Number.isInteger(generation) || generation < 1) {
    throw new Error('pane generation must be a positive integer')
  }
  if (id === null || id === undefined) {
    const minted = `p-${envelope.nextPane}`
    envelope.nextPane += 1
    return minted
  }
  requireText(id, 'pane id')
  if (id === '__proto__') {
    // Refused as input as well as handled as a key: an identity that means
    // something else to every object in the language is not an identity.
    throw new Error('pane id __proto__ is not an identity — pick a name')
  }
  if (MINTED_PANE_ID.test(id)) {
    throw new Error(`pane id ${id} is minted by the app allocator — pass no id to mint one`)
  }
  const live = findPaneAnywhere(envelope.tabs, id)
  if (live !== null) {
    throw new Error(
      `duplicate pane id ${id}: pane ids are app-wide unique, and ${live.tab.id} holds it`,
    )
  }
  const issued = Object.hasOwn(envelope.issued, id) ? envelope.issued[id] : undefined
  if (issued !== undefined && generation <= issued) {
    throw new Error(
      `pane id ${id} was issued at generation ${issued} — a reissued pane identity needs a newer generation`,
    )
  }
  envelope.issued[id] = generation
  return id
}

/**
 * Appends a worker pane to a tab, through the ONE allocator, inside the
 * caller's mutation. `src/tabs.js` does the same for the page's own paths;
 * admission needs it here because minting the pane and taking the
 * reservation for it have to be one step.
 */
function appendWorkerPane(envelope, tab, conversation) {
  // Readers resolve a conversation to one pane; a retry replaces its failed identity.
  tab.panes = tab.panes.filter(
    (pane) => !(pane.conversation === conversation && (pane.failure || pane.closed === true)),
  )
  const pane = {
    id: allocatePaneId(envelope, { generation: 1 }),
    kind: 'worker',
    conversation,
    generation: 1,
    order: nextPaneOrder(tab.panes),
  }
  tab.panes.push(pane)
  tab.updatedAt = nowIso()
  return pane
}

/**
 * The reservation a controller write claims to be for, compared against the
 * CURRENT one inside the mutation.
 *
 * A controller's authority was checked when its request was admitted, which
 * is earlier than when its write lands. In between, its launch can end and
 * another can take the conversation — so the ownership check has to be made
 * again here, against what the row says now, or an ended launch's write
 * overwrites its replacement's.
 */
function requireCurrentLaunch(row, name, expect) {
  const reserved = isRecord(row.reserved) ? row.reserved : null
  if (!isRecord(expect)) {
    // A row nobody has reserved is nobody's launch to claim, so a write
    // with no expectation is only ever admissible there. A RESERVED row
    // belongs to a launch, and a write that cannot name it is a write
    // whose authority we cannot check.
    if (reserved === null) return null
    throw new StoreRefusal(
      `${name} is reserved by launch ${reserved.launchId}: a write must name the launch it is for`,
      'launch-required',
    )
  }
  requireText(expect.launchId, 'expected launch id')
  requireText(expect.tab, 'expected tab')
  requireText(expect.pane, 'expected pane')
  if (!Number.isInteger(expect.generation) || expect.generation < 1) {
    throw new Error('expected generation must be a positive integer')
  }
  if (
    reserved === null ||
    reserved.launchId !== expect.launchId ||
    reserved.tab !== expect.tab ||
    reserved.pane !== expect.pane ||
    reserved.generation !== expect.generation
  ) {
    throw new StoreRefusal(
      `launch ${expect.launchId} is over for ${name}: it is not the current reservation`,
      'stale-launch',
    )
  }
  return reserved
}

/**
 * A conversation reopened on the session it already had: re-validate the
 * binding against the launch that is resuming it, and stamp it with the
 * pane now carrying it.
 *
 * A binding records the generation it was made at, because a binding made
 * for one pane must never authorise a decision for another. Reopening
 * without re-stamping leaves exactly that: a row naming a live session and
 * a generation belonging to a pane that is gone. So the launch record has
 * to vouch for the session again — `bindEvidence` decides, as it does
 * everywhere else — and a launch that cannot leaves no binding at all,
 * rather than a stale one. The session id itself is kept either way: it is
 * what the pane was told to resume, and forgetting it would lose the
 * conversation's own history.
 */
function restampBinding(row, reserved, at) {
  const sessionId = typeof row.sessionId === 'string' ? row.sessionId : ''
  if (sessionId.length === 0) return
  const decision = bindEvidence(row.kind, { sessionId }, reserved)
  if (decision?.bound !== true) {
    delete row.binding
    return
  }
  row.binding = {
    evidence: decision.evidence,
    generation: reserved.generation,
    launchId: reserved.launchId,
    at,
  }
}

/** The pane with this id, in whichever tab holds it. */
function findPaneAnywhere(tabs, paneId) {
  for (const tab of tabs) {
    if (!isRecord(tab) || !Array.isArray(tab.panes)) continue
    const pane = tab.panes.find((candidate) => isRecord(candidate) && candidate.id === paneId)
    if (pane !== undefined) return { tab, pane }
  }
  return null
}

// --- the reservation across two files --------------------------------------

function reservationFrom(pane, launch, at) {
  if (!isRecord(pane)) throw new Error('pane must be { tab, id, generation? }')
  requireText(pane.tab, 'pane tab')
  requireText(pane.id, 'pane id')
  const generation = pane.generation ?? 1
  if (!Number.isInteger(generation) || generation < 1) {
    throw new Error('pane generation must be a positive integer')
  }
  // The durable launch identity: minted when the launch is reserved, kept
  // on every reread, and recorded on the binding — the handle the next
  // units use to resolve or release this exact launch.
  let launchId = randomUUID()
  if (launch !== null && launch !== undefined) {
    if (!isRecord(launch))
      throw new Error('launch must be { nonce?, preallocatedId?, reportedId? }')
    if (launch.launchId !== undefined && launch.launchId !== null) {
      requireText(launch.launchId, 'launch id')
      launchId = launch.launchId
    }
  }
  const reserved = { tab: pane.tab, pane: pane.id, generation, launchId, at }
  // The operation that asked for this launch, kept beside it: a retry that
  // arrives after a restart can still tell its own launch from a new one.
  if (
    launch !== null &&
    launch !== undefined &&
    launch.opId !== undefined &&
    launch.opId !== null
  ) {
    requireText(launch.opId, 'launch opId')
    reserved.opId = launch.opId
  }
  if (isRecord(launch?.channel)) reserved.channel = structuredClone(launch.channel)
  const evidence = evidenceFields(launch)
  if (evidence !== null) Object.assign(reserved, evidence)
  return reserved
}

function evidenceFields(launch) {
  if (launch === null || launch === undefined) return null
  if (!isRecord(launch)) throw new Error('launch must be { nonce?, preallocatedId?, reportedId? }')
  const fields = {}
  for (const key of ['nonce', 'preallocatedId', 'reportedId']) {
    const value = launch[key]
    if (value !== undefined && value !== null) {
      requireText(value, `launch ${key}`)
      fields[key] = value
    }
  }
  if (Object.keys(fields).length === 0) {
    throw new Error('a launch record needs its evidence: nonce, preallocatedId, or reportedId')
  }
  return fields
}

/**
 * Links the launched pane to its conversation, adding it if it is new —
 * and validates everything first, so callers can run it before their first
 * write. A pane already serving another conversation, or linked at another
 * generation, is a conflict, never an overwrite; an unknown tab or a tab
 * from another directory is a refusal.
 */
function linkConversationPane(envelope, cwd, pane, name) {
  if (!isRecord(pane)) throw new Error('pane must be { tab, id, generation? }')
  requireText(pane.tab, 'pane tab')
  requireText(pane.id, 'pane id')
  const generation = pane.generation ?? 1
  if (!Number.isInteger(generation) || generation < 1) {
    throw new Error('pane generation must be a positive integer')
  }
  if (pane.kind !== undefined && pane.kind !== null) {
    requireOneOf(pane.kind, PANE_KINDS, 'pane kind')
  }
  const tab = findTab(envelope.tabs, pane.tab)
  if (
    typeof tab.directory === 'string' &&
    path.resolve(tab.directory) !== path.resolve(cwd ?? tab.directory)
  ) {
    throw new Error(`pane tab ${pane.tab} lives in ${tab.directory}, not this workspace`)
  }
  if (!Array.isArray(tab.panes)) tab.panes = []
  const held = findPaneAnywhere(envelope.tabs, pane.id)
  if (held !== null) {
    // A pane that exists is LINKED, never re-created — and it is linked
    // only where it lives: an identity held by another tab is a collision,
    // and taking it here would give one pane two homes.
    if (held.tab !== tab) {
      throw new Error(
        `pane ${pane.id} lives in tab ${held.tab.id}, not ${pane.tab} — pane identity is app-wide`,
      )
    }
    const existing = held.pane
    if (existing.generation !== generation) {
      throw new Error(
        `pane ${pane.id} in tab ${pane.tab} is at generation ${existing.generation}, not ${generation}`,
      )
    }
    if (
      existing.conversation !== null &&
      existing.conversation !== undefined &&
      existing.conversation !== name
    ) {
      throw new Error(`pane ${pane.id} in tab ${pane.tab} already serves ${existing.conversation}`)
    }
    existing.conversation = name
    return
  }
  // The pane does not exist yet: creating it here is a creation path like
  // any other, and it goes through the one allocator.
  tab.panes.push({
    id: allocatePaneId(envelope, { id: pane.id, generation }),
    kind: pane.kind ?? 'worker',
    conversation: name,
    generation,
    order: nextPaneOrder(tab.panes),
  })
}

/** The pane a reservation names, with the generation and owner it names. */
function linkedPane(tabs, reserved) {
  const tab = tabs.find((candidate) => isRecord(candidate) && candidate.id === reserved.tab)
  if (tab === undefined || !Array.isArray(tab.panes)) return null
  return (
    tab.panes.find(
      (pane) =>
        isRecord(pane) && pane.id === reserved.pane && pane.generation === reserved.generation,
    ) ?? null
  )
}

/**
 * Throws unless the reservation's pane still serves this conversation —
 * the SAME predicate recovery uses, because they answer the same question.
 * An unlinked pane (null, or no link at all) is not a weaker yes: it is a
 * pane that serves nobody, and binding a native session against it would
 * record ownership of a pane nothing points back to.
 */
function requireLinkedPane(tabs, reserved, name) {
  const pane = linkedPane(tabs, reserved)
  if (pane === null) {
    throw new Error(
      `no pane ${reserved.pane} at generation ${reserved.generation} in tab ${reserved.tab}`,
    )
  }
  if (pane.conversation !== name) {
    throw new Error(
      `pane ${reserved.pane} in tab ${reserved.tab} serves ${pane.conversation ?? 'nobody'}, not ${name}`,
    )
  }
}

function nextPaneOrder(panes) {
  let max = -1
  for (const pane of panes) {
    if (isRecord(pane) && Number.isInteger(pane.order) && pane.order > max) max = pane.order
  }
  return max + 1
}

/**
 * The recovery predicate: the reservation survives only while its pane
 * still serves its conversation at the reserved generation. A pane that is
 * gone, replaced, or rehomed releases the reservation — presence of a
 * matching pane key alone never keeps it.
 */
function paneExists(tabs, reserved, name) {
  const pane = linkedPane(tabs, reserved)
  if (pane === null) return false
  return pane.conversation === name
}

/**
 * The failure a caller sees when its own mutation could neither commit nor
 * be undone. Both errors ride along: the second explains why the first was
 * not rolled back.
 */
function degradedFailure(error, compensation) {
  const failure = new Error(
    `the store is degraded: the pane link failed (${error?.message ?? error}) and the rollback that would have undone the reservation failed too (${compensation?.message ?? compensation}) — every mutation refuses until recover() reconciles the state`,
  )
  failure.cause = error
  failure.compensation = compensation
  return failure
}

/** The refusal every other mutation gets while the store stays degraded. */
function degradedRefusal(degraded) {
  const refusal = new Error(
    `the store is degraded since ${degraded.at} (${degraded.error?.message ?? degraded.error}) and refuses every mutation until recover() reconciles the state`,
  )
  refusal.cause = degraded.error
  refusal.compensation = degraded.compensation
  return refusal
}

// --- small shared validation ------------------------------------------------

function findTab(tabs, tabId) {
  const tab = tabs.find((candidate) => isRecord(candidate) && candidate.id === tabId)
  if (tab === undefined) throw new Error(`no tab ${tabId}`)
  if (!Array.isArray(tab.panes)) tab.panes = []
  return tab
}

function findPane(tab, paneId) {
  const pane = tab.panes.find((candidate) => isRecord(candidate) && candidate.id === paneId)
  if (pane === undefined) throw new Error(`no pane ${paneId} in tab ${tab.id}`)
  return pane
}

function requireRow(threads, name) {
  const row = threads[name]
  // Naming a conversation that is not here is the caller's mistake, and
  // they can act on it; it is not this machine failing.
  if (!isRecord(row)) {
    throw new StoreRefusal(`no conversation named ${name} in this workspace`, 'no-conversation')
  }
  return row
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function requireText(value, label) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    // Argument validation is always about the request that arrived.
    throw new StoreRefusal(`${label} is required`, 'missing-field')
  }
}

function requireOneOf(value, allowed, label) {
  if (!allowed.includes(value)) {
    // A value outside the vocabulary is the CALLER's, not this machine's.
    throw new StoreRefusal(`${label} must be one of: ${allowed.join(', ')}`, 'not-allowed')
  }
}

function asMap(value, label) {
  if (!isRecord(value)) throw new Error(`${label} must be an object`)
  return value
}

function asArray(value, label) {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`)
  return value
}
