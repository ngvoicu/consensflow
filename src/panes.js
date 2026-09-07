import { randomUUID } from 'node:crypto'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { DEFAULT_PART_BUDGETS, partsFor, seenAfter } from '../hosts/lib/deliveries.js'
import { discoverSessionWithEvidence } from '../hosts/lib/harness-transcript.js'
import { formatLaunchMarker } from '../hosts/lib/packets.js'
import { effectivePolicy } from '../hosts/lib/policy.js'
import { interactiveResume, interactiveStart } from '../hosts/lib/runners.js'
import { newSessionName } from '../hosts/lib/threads.js'
import { enabledChannels, launchConfiguration } from './channels.js'
import { controllerEnv, endLaunch, issueTicket, leadEnv, ownerOf } from './launch.js'
import { StoreRefusal } from './store.js'
import { leadIdentity, paneIdentity } from './tabs.js'

/**
 * The pane operations (Phase 2, IMPL-PANE-18) — what `/api/panes/*` means.
 *
 * `src/ui.js` owns the credentials and the routing; this module owns the
 * decisions. A lead asks for `consult`, `say`, `attach` or its tab's panes;
 * a controller, under the capability its launch redemption gave it,
 * records `session.bind`, `progress.set` and `sent.record`. Every op names
 * an `opId`, and the answer to an opId is the same answer every time.
 *
 * Rules, each with a test in `tests/ui-panes.test.mjs`:
 *
 * - **The continuation rule.** `fresh` mints a name and launches; a named
 *   or resolved conversation whose pane is live receives the task as a
 *   follow-up through the same paste `say` uses; one whose pane has ended
 *   resumes under the same name in a new pane. A conversation is resolved
 *   the way `cf run` has always resolved one: this lead's own most recent
 *   conversation with that agent, and a fresh name when there is none — a
 *   lead never joins a conversation somebody else started.
 *
 * - **One launch per conversation.** The store's reservation is the lock.
 *   It is taken before `pane.open` is sent and resolved only when Rust
 *   answers, so a launch whose answer never came back leaves the
 *   conversation reserved and unresolved — and every later launch for it,
 *   under any new `opId`, is refused (`reserved`) until the pane ends or
 *   the launch resolves. That is what makes the timeout answer honest:
 *   `{outcome:'unknown', launch}` means the pane may well be running.
 *
 * - **Idempotency.** A completed op's answer is kept under its `opId` and
 *   replayed verbatim; a replay that arrives while the first is still in
 *   flight waits on the same promise. The ledger is in memory on purpose:
 *   the credential that authorises these calls is minted in
 *   `src/launch.js` and lives exactly as long as this process, so a
 *   replay can never outlive the token that would carry it. What IS
 *   durable is the thing that must be — the reservation, with its launch
 *   id and the `opId` that asked for it.
 *
 * - **A controller never names its conversation.** Ownership comes from
 *   the launch: `ownerOf(launch)` in `src/launch.js` answers what this
 *   launch is for, and every controller op reads the conversation from
 *   there. A body that tries to name one is refused rather than obeyed.
 *
 * - **One identity for one launch.** `issueTicket` mints the launch id
 *   with the ticket and hands both back: the same id goes on the store's
 *   reservation, into `pane.open` for Rust to deduplicate by, into the
 *   lead's answer, and into `endLaunch`. Only the ticket is a secret, and
 *   it is never echoed to a lead. `ownerOf(launch)` is how a controller op
 *   learns which conversation it may write to — from `src/launch.js`,
 *   where the authority lives, never from its own request body.
 *
 * - **Rust decides what Rust owns.** The draft latch, the input epoch and
 *   the arbiter's refusals are the pane's, not Node's: `say` reads the
 *   epoch from `pane.snapshot` and lets a `Draft` or `Stale` refusal come
 *   back as it is.
 *
 * This module reads no environment: the roster, the store, the tabs and
 * the bridge all arrive as arguments.
 */

const CF_CLI = join(dirname(fileURLToPath(import.meta.url)), '..', 'bin', 'cf.mjs')
const DEFAULT_PANE_OPEN_DEADLINE_MS = 30_000
/**
 * The evidence a LEAD launch binds by, in the three shapes a worker has.
 *
 * claude-code and pi take an id from us, so we mint one and it IS the
 * binding the moment the window opens. Everything else mints its own, so
 * the launch nonce travels in the seed's opening line and binds when
 * discovery reads it back. A lead already bound offers no new identity —
 * it is resumed on the one it has.
 */
function leadEvidence(kind, tabId, launchId, nativeSession) {
  if (typeof nativeSession === 'string' && nativeSession.length > 0) return {}
  if (kind === 'claude-code') return { preallocatedId: randomUUID() }
  // pi creates the session when the name is new, so a name scoped to the
  // tab is both the identity and a thing a human can recognise in pi's own
  // list. It is stable across resumes for the same reason.
  if (kind === 'pi') return { preallocatedId: `${tabId}-lead` }
  return { nonce: launchId }
}

/** Has this conversation a native session anyone could resume? */
function boundSession(record) {
  return typeof record?.sessionId === 'string' && record.sessionId.length > 0
}

/** The store allocator's own namespace, and the only shape a delivery id has. */
const MINTED_DELIVERY_ID = /^d-\d+$/
/** The harnesses whose native session id is ours to name before they start. */
const PREALLOCATED_KINDS = new Set(['claude-code', 'pi'])

export class PaneError extends Error {
  constructor(message, { status = 400, code = null, detail = null, release = false } = {}) {
    super(message)
    this.name = 'PaneError'
    this.status = status
    this.code = code
    this.detail = detail
    // Asks the caller to give an admitted launch back even though the frame
    // was transmitted: the pane host answered that it opened nothing.
    this.release = release
  }
}

/**
 * A refusal another module made ABOUT THE REQUEST — by type, never by shape.
 *
 * Guessing from a bare `Error` reads a corrupt `threads.json` as a bad
 * request and tells the controller to change something that was fine. Only
 * a `StoreRefusal` (or anything carrying an explicit refusal `code`) is the
 * caller's business; everything else is ours and reaches the 500 handler
 * with its reason intact.
 */
function isStoreRefusal(cause) {
  return (
    cause instanceof StoreRefusal ||
    (cause instanceof Error && typeof cause.code === 'string' && cause.errno === undefined)
  )
}

/** The store's coded admission refusals, in its own words. */
function admissionRefusal(cause, conversation) {
  return new PaneError(cause.message, {
    status: 409,
    code: cause.code,
    detail: {
      ...(conversation === null ? {} : { conversation }),
      ...(cause.detail ?? {}),
    },
  })
}

/** The body a route sends back for a `PaneError`. */
export function paneErrorBody(error) {
  return {
    error: error.code ?? error.message,
    ...(error.code === null || error.code === undefined ? {} : { reason: error.message }),
    ...(error.detail === null || error.detail === undefined ? {} : error.detail),
  }
}

export class Panes {
  #store
  #tabs
  #agents
  #app
  #bridge = null
  #deadlineMs
  #node
  #ledger = new Map()
  #inFlight = new Map()
  // Every `pane.open` this process has sent and not yet had answered.
  #openings = new Map()
  #onNote = null
  #harnessPath
  #shell
  #env

  /**
   * @param {object} deps
   * @param {import('./store.js').Store} deps.store
   * @param {import('./tabs.js').Tabs} deps.tabs
   * @param {() => {row: (name: string) => object|undefined, names: () => string[]}} deps.agents
   *   the roster, as a lookup — this module never reads it from disk.
   * @param {() => {url: string}} deps.app — where this server is listening;
   *   a function because it is only known once the port is bound.
   * @param {string} deps.node — the absolute runtime a pane's process runs.
   * @param {number} [deps.paneOpenDeadlineMs] — how long `pane.open` has to
   *   answer before the launch becomes `unknown`.
   */
  constructor({
    store,
    tabs,
    agents,
    app,
    node,
    // Where a harness's CLI and the human's shell actually are on THIS
    // machine. Both are absolute paths the pane host insists on, and both
    // are answers only the environment has — which this module never reads.
    harnessPath = () => null,
    shell = '/bin/sh',
    // Discovery reads each harness's own store, and where those live is an
    // answer only the environment has — which this module never reads.
    env = {},
    onNote = null,
    paneOpenDeadlineMs = DEFAULT_PANE_OPEN_DEADLINE_MS,
  }) {
    this.#store = store
    this.#tabs = tabs
    this.#agents = agents
    this.#app = app
    this.#node = node
    this.#harnessPath = harnessPath
    this.#shell = shell
    this.#env = env
    this.#deadlineMs = paneOpenDeadlineMs
    this.#onNote = onNote
  }

  /**
   * The bridge to Rust, once there is one. A pane host arrives after the
   * server is listening (the handle line goes out first, then frames), so
   * this is set, never constructed here.
   */
  attachBridge(bridge) {
    this.#bridge = bridge
    if (bridge !== null && bridge !== undefined) {
      bridge.onEvent('pane.exit', (body) => this.#paneExited(body))
    }
    return this
  }

  // --- lead operations -----------------------------------------------------

  /**
   * `POST /api/panes/consult` — the continuation rule, and at most one
   * launch for one conversation.
   */
  async consult(tabId, request) {
    // Only the ledger key is read before the ledger: an operation's answer
    // must not depend on state that moved since it was decided, and the
    // roster is state like any other.
    const opId = requireText(request.opId, 'opId')
    return this.#once(tabId, opId, async () => {
      const { agent, task, fresh, session, notify } = readConsult(request)
      // Everything checkable before transmission is checked before
      // admission, so a known failure never leaves a conversation reserved
      // with nothing running and no exit ever coming for it.
      this.#requireBridge()
      const row = this.#agents.row(agent)
      if (row === undefined) throw new PaneError(`no agent named ${agent}`)
      const tab = await this.#openTab(tabId)
      const threads = await this.#store.readThreads(tab.directory)
      const name = this.#chooseConversation(tab, threads, { agent: row, fresh, session })
      requireSameAgent(name, threads[name], row)

      // The preference rides INSIDE admission: one write, so there is no
      // window between reserving and recording it.
      const admitted = await this.#admit(tab, { name, agent: row, opId, notify })
      if (admitted.outcome === 'live') {
        // A live conversation takes the task as its next turn: one pane,
        // one process, one session — that is the whole point of continuing.
        await this.#paste(tab, name, admitted.pane, consultText(request), {
          opId,
          kind: 'consult',
          expect: expectationOf(admitted.row),
        })
        return {
          outcome: 'said',
          conversation: name,
          agent: row.id,
          tab: tab.id,
          pane: paneRef(admitted.pane),
        }
      }
      const answer = await this.#openAdmitted(tab, admitted, {
        name,
        argv: (native) => [
          'run',
          `@${row.id}`,
          task,
          '--in-pane',
          '--session',
          name,
          ...(admitted.created ? ['--new'] : []),
          ...flag('--brief', request.brief),
          ...flag('--context', request.context),
          ...flag('--handoff-file', request.handoffFile),
          ...native,
        ],
      })
      return { ...answer, agent: row.id }
    })
  }

  /** `POST /api/panes/say` — words onto a live pane, recorded as sent. */
  async say(tabId, request) {
    const opId = requireText(request.opId, 'opId')
    const session = requireText(request.session, 'session')
    const text = requireText(request.text, 'text')
    return this.#once(tabId, opId, async () => {
      const tab = await this.#openTab(tabId)
      const threads = await this.#store.readThreads(tab.directory)
      const record = threads[session]
      if (!isRecord(record)) {
        throw new PaneError(`no conversation named ${session} here`, { status: 404 })
      }
      this.#refuseHeld(tab, session, record)
      const pane = this.#livePane(tab, record, session)
      if (pane === null) {
        throw new PaneError(`${session} has no live pane — attach to reopen it`, {
          status: 409,
          code: 'no-live-pane',
        })
      }
      await this.#paste(tab, session, pane, text, {
        opId,
        kind: 'say',
        expect: expectationOf(record),
      })
      return { outcome: 'said', conversation: session, tab: tab.id, pane: paneRef(pane) }
    })
  }

  /**
   * `POST /api/panes/attach` — the conversation's own window. A live one is
   * answered with its pane: a second window on one session is two processes
   * writing one store, which is the bug this refuses to commit.
   */
  async attach(tabId, request) {
    const opId = requireText(request.opId, 'opId')
    return this.#once(tabId, opId, async () => {
      const session = requireText(request.session, 'session')
      this.#requireBridge()
      const tab = await this.#openTab(tabId)
      const threads = await this.#store.readThreads(tab.directory)
      const record = threads[session]
      if (!isRecord(record)) {
        throw new PaneError(`no conversation named ${session} here`, { status: 404 })
      }
      const agent = this.#agents.row(record.agent)
      if (agent === undefined) {
        throw new PaneError(`${session} belongs to ${record.agent}, which is not in the roster`)
      }
      // A roster name recreated on another harness is not the agent this
      // conversation belongs to. Refused here as on consult, and again
      // inside admission against the row the store itself reads.
      requireSameAgent(session, record, agent)
      // Nothing to reopen. A conversation whose pane has ended and which
      // never bound a native session has no transcript to resume, so the
      // launch would carry nonce-only evidence, `cf attach` would refuse it
      // for having no session, and the human would be left with a pane
      // whose outer command exited 0 and which never ran a harness. Cold
      // recovery is not specified and is not built, so this says no here —
      // before admission, before a pane identity is spent, before a frame.
      // Only a CLOSED one — no launch anywhere and no session. A
      // conversation still launching, or running in another tab, has its
      // own refusal with its own code, and the store is the one that can
      // tell those apart without a race.
      const closed = !isRecord(record.reserved) && this.#livePane(tab, record, session) === null
      if (closed && !boundSession(record)) {
        throw new PaneError(
          `${session} never bound a session, so there is nothing to attach to — ` +
            'start a new consult with this agent instead',
          { status: 409, code: 'unbound-conversation', detail: { conversation: session } },
        )
      }
      const admitted = await this.#admit(tab, { name: session, agent, opId })
      if (admitted.outcome === 'live') {
        return {
          outcome: 'live',
          conversation: session,
          tab: tab.id,
          pane: paneRef(admitted.pane),
        }
      }
      return this.#openAdmitted(tab, admitted, {
        name: session,
        argv: (native) => ['attach', session, '--in-pane', ...native],
      })
    })
  }

  /**
   * `POST /api/panes/read` — one part of a delivery, printed whole.
   *
   * A delivery too big for the lead's pane is written immutable to
   * `<workspace>/deliveries/<id>.md` and the lead is pointed at it. This
   * hands back one numbered part at a time, each within the lead harness's
   * verified tool-output budget, because a receiver that keeps only the
   * tail of a long tool result keeps the end marker and drops the text —
   * so a single unbounded print would look read and not be.
   *
   * The answer's `text` is the part exactly as it must appear: the client
   * prints it and adds nothing. Printing a part records an ATTEMPT and
   * never coverage: coverage needs the part's framing AND a matching body
   * digest in the lead's own tool result, which only `receipt` can see.
   */
  async read(tabId, request) {
    const opId = requireText(request.opId, 'opId')
    return this.#once(tabId, opId, async () => {
      const deliveryId = requireText(request.deliveryId, 'deliveryId')
      if (!MINTED_DELIVERY_ID.test(deliveryId)) {
        throw new PaneError(`${deliveryId} is not a delivery id`, { code: 'no-such-delivery' })
      }
      const part = request.part === undefined ? 1 : request.part
      if (!Number.isInteger(part) || part < 1) {
        throw new PaneError(`a part number is a positive integer, not ${JSON.stringify(part)}`)
      }
      const tab = await this.#tab(tabId)
      const record = (await this.#store.readDeliveries(tab.directory))[deliveryId]
      // Two sessions can share a directory, so sharing a workspace is not
      // being the lead this delivery was addressed to.
      if (!isRecord(record) || record.target?.tab !== tab.id) {
        throw new PaneError(`no delivery ${deliveryId} for this session`, {
          status: 404,
          code: 'no-such-delivery',
        })
      }
      const parts = deliveryParts(record, tab)
      const chosen = parts[part - 1]
      if (chosen === undefined) {
        throw new PaneError(
          `delivery ${deliveryId} has ${parts.length} part${parts.length === 1 ? '' : 's'}, not ${part}`,
          { detail: { deliveryId, of: parts.length } },
        )
      }
      await this.#store.deliveryReadAttempt(tab.directory, {
        id: deliveryId,
        part,
        lead: leadIdentity(tab),
        opId,
      })
      return {
        outcome: 'read',
        deliveryId,
        conversation: record.conversation,
        agent: record.agent,
        ...chosen,
      }
    })
  }

  /**
   * `POST /api/panes/seen` — how far the lead has read one conversation.
   *
   * The frontier walks from the FIRST item while each one is already
   * marked, covered by an accepted delivery, or printed, and stops at the
   * first that is none of those: starting from a stored mark instead would
   * carry the frontier over unread items behind it.
   *
   * Tool items are dropped before the walk. A tool item is never
   * delivery-covered and never printed, so leaving one in stops the walk
   * at the worker's first tool call and everything the lead really read
   * after it stays unmarked. `bin/cf.mjs` filters the same way; this does
   * it too, because the frontier must not depend on which client asked.
   */
  async seen(tabId, request) {
    const opId = requireText(request.opId, 'opId')
    return this.#once(tabId, opId, async () => {
      const session = requireText(request.session, 'session')
      const items = readItems(request.items)
      const tab = await this.#tab(tabId)
      const row = (await this.#store.readThreads(tab.directory))[session]
      if (!isRecord(row)) {
        throw new PaneError(`no conversation named ${session} here`, { status: 404 })
      }
      const lead = leadIdentity(tab)
      const marks = seenAfter({
        row,
        leadId: lead,
        conversation: session,
        deliveries: Object.values(await this.#store.readDeliveries(tab.directory)),
        items,
      })
      await this.#store.seenSet(tab.directory, { name: session, items: marks, lead })
      return { outcome: 'seen', conversation: session, tab: tab.id, lead, seen: marks }
    })
  }

  /** `GET /api/panes` — the caller's tab, and nothing of anyone else's. */
  async list(tabId) {
    const tab = await this.#tab(tabId)
    const threads = await this.#store.readThreads(tab.directory)
    const panes = [...(tab.panes ?? [])]
      .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
      .map((pane) => {
        const record = isRecord(threads[pane.conversation]) ? threads[pane.conversation] : null
        return {
          id: pane.id,
          kind: pane.kind,
          generation: pane.generation,
          order: pane.order,
          conversation: pane.conversation ?? null,
          agent: record?.agent ?? null,
          ...paneLiveness(tab, pane, record, this.#livePane(tab, record, pane.conversation)),
          policy: effectivePolicy(tab, pane, record),
        }
      })
    return { tab: tab.id, directory: tab.directory, closed: tab.closed === true, panes }
  }

  // --- controller operations ----------------------------------------------

  /** `POST /api/panes/session.bind` — decided against the launch record. */
  async sessionBind(launch, request) {
    const owner = this.#owner(launch, request)
    const cwd = await this.#launchDirectory(owner)
    const bound = await this.#refusable('bind-refused', () =>
      this.#store.sessionBind(cwd, {
        name: owner.conversation,
        candidate: isRecord(request.candidate) ? request.candidate : {},
        expect: expectationFor(launch, owner),
      }),
    )
    return {
      outcome: 'bound',
      conversation: owner.conversation,
      sessionId: bound.sessionId,
      evidence: bound.binding.evidence,
      generation: bound.binding.generation,
    }
  }

  /** `POST /api/panes/progress.set` — where this launch has got to. */
  async progressSet(launch, request) {
    const owner = this.#owner(launch, request)
    const cwd = await this.#launchDirectory(owner)
    const row = await this.#refusable('progress-refused', () =>
      this.#store.progressSet(cwd, {
        name: owner.conversation,
        progress: isRecord(request.progress) ? request.progress : {},
        expect: expectationFor(launch, owner),
      }),
    )
    return { outcome: 'recorded', conversation: owner.conversation, progress: row.progress }
  }

  /**
   * `POST /api/panes/sent.record` — what the controller put in the pane.
   *
   * The one controller op that is not idempotent by nature. `session.bind`
   * and `progress.set` write a value, and writing it twice says the same
   * thing; this one APPENDS. So it names its `opId`, and a retry — a lost
   * answer, a reconnected bridge — gets the first answer back rather than
   * leaving a second line in the record.
   */
  async sentRecord(launch, request) {
    const owner = this.#owner(launch, request)
    const opId = requireText(request.opId, 'opId')
    return this.#once(launch, opId, async () => {
      const cwd = await this.#launchDirectory(owner)
      const row = await this.#refusable('record-refused', () =>
        this.#store.sentRecord(cwd, {
          name: owner.conversation,
          entry: { ...(isRecord(request.entry) ? request.entry : {}), opId },
          expect: expectationFor(launch, owner),
        }),
      )
      return { outcome: 'recorded', conversation: owner.conversation, sent: row.sent.length }
    })
  }

  // --- what the page asks for, over the bridge -----------------------------

  /**
   * `tab.open {dir, harness}` — a new tab, and its lead's own window.
   *
   * Rust's `open_lead` sends this; before it had a handler here, every
   * button on the page answered "not available yet". The tab is minted
   * first and the lead launched into it, because the launch is reserved ON
   * the tab record and there is nothing to reserve against until it exists.
   * A launch that never leaves gives its reservation back AND closes the
   * tab it just minted: a tab whose lead never opened is a window the page
   * would draw with nothing behind it. The record stays, so the id is never
   * handed out twice, and `tab.resume` is how the human tries again.
   */
  async tabOpen(request) {
    const dir = requireText(request?.dir, 'directory')
    const harness = requireText(request?.harness, 'harness')
    const created = await this.#refusable('tab-refused', () => this.#tabs.create(dir, harness))
    return this.#launchLead(created.id, { opened: true })
  }

  /**
   * `tab.resume {tab}` — a suspended tab's lead, opened again.
   *
   * `resume` mints the next lead generation before the window exists, so a
   * launch that fails leaves a resumed tab with no lead pane rather than a
   * tab still marked closed. That is the honest order: the human asked for
   * it back, and the tab IS back; only its window did not come.
   */
  async tabResume(request) {
    const tabId = requireText(request?.tab, 'tab')
    // Before the tab takes its next generation: a pane row is only Node's
    // memory of a window, and a pane host that restarted holds none of
    // them. Reconciling first means the resume mints a generation over a
    // tab whose rows all still exist — and that a host which cannot say
    // what it holds refuses the resume here, with the tab untouched,
    // rather than halfway through it.
    await this.#reconcilePanes(tabId)
    await this.#refusable('resume-refused', () => this.#tabs.resume(tabId))
    return this.#launchLead(tabId, { opened: false })
  }

  /**
   * The tab's non-lead panes, against the windows the pane host actually
   * has: `pane.list` is the only thing that knows.
   *
   * A worker row nobody is running reads as a live conversation to every
   * reader, and its controller capability still opens doors — so every row
   * the host cannot show at the SAME id and generation, alive, is given
   * exactly the bookkeeping its `pane.exit` would have done: the launch
   * released, its capability and ticket ended, the row dropped. The
   * conversation itself stays in the store, which is what lets `attach`
   * reopen it on the native session it bound.
   *
   * The lead is left alone. Its pane IS the tab, its exit is what suspended
   * this tab in the first place, and `#launchLead` opens it a line later.
   *
   * This FAILS CLOSED. A resume that cannot read the host's list has two
   * ways to be wrong, and they are not equally bad: reconciling against an
   * answer we do not understand ends launches that may be running, while
   * refusing leaves the tab exactly as the human left it — closed, at the
   * same generation, holding its reservations — and they can ask again. So
   * every doubt is spent BEFORE the first mutation: the request itself, the
   * answer's shape, and every entry in it are checked, and only a list we
   * fully understand is allowed to remove anything.
   */
  async #reconcilePanes(tabId) {
    const tab = await this.#tabs.get(tabId)
    // Only a SUSPENDED tab is being resumed. A tab that is still open has
    // its panes running, and `tab.resume` on it is a mistake `tabs.resume`
    // refuses a line later — reconciling first would answer that mistake
    // by ending the launches of a tab that is working perfectly well.
    // Same for a tab that is not there: its refusal is the store's to make.
    if (tab === null || tab.closed !== true) return
    // Drain what we admitted before reaping what the host has. A launch
    // whose `pane.open` is still in flight has no pane there YET — the
    // host has not been answered for it — so a `pane.list` that overtook
    // it would read "not there" about a window that is about to exist and
    // reap a live launch. Waiting costs the resume the time that open
    // already has: the request carries its own deadline, so this settles.
    await this.#drainOpens()
    // The tab is re-read because the drain is a real wait: an open that
    // settled in it may have taken its own row with it.
    const drained = await this.#tabs.get(tabId)
    if (drained === null || drained.closed !== true) return
    const rows = (drained.panes ?? []).filter((pane) => isRecord(pane) && pane.kind !== 'lead')
    if (rows.length === 0) return
    let answer
    try {
      // `#requireBridge` refuses with `no-pane-host` when there is none,
      // which is the same refusal one step earlier.
      answer = await this.#requireBridge().request(
        'pane.list',
        {},
        { deadlineMs: this.#deadlineMs },
      )
    } catch (cause) {
      if (cause instanceof PaneError) throw cause
      throw unlistablePanes(tabId, cause?.message ?? String(cause))
    }
    if (answer?.ok !== true) {
      throw unlistablePanes(tabId, answer?.reason ?? answer?.error ?? 'it refused')
    }
    if (!Array.isArray(answer.panes))
      throw unlistablePanes(tabId, 'the answer carried no pane list')
    // Every entry, before any of them is acted on: a list with one
    // unreadable row is not a list we can tell the dead from the living by.
    for (const entry of answer.panes) {
      if (!isListedPane(entry)) {
        throw unlistablePanes(tabId, `it listed a pane we cannot read: ${encodeFrame(entry)}`)
      }
    }
    const alive = new Set(
      answer.panes.filter((pane) => pane.alive === true).map((pane) => paneIdentity(pane)),
    )
    for (const pane of rows) {
      if (alive.has(paneIdentity(pane))) continue
      // A frame this process sent and has no ending for: the host may
      // still be opening that window, and "not in the list" is not the
      // same as "never coming". Its own exit is what settles it.
      if (this.#openings.has(paneIdentity(pane))) continue
      await this.#paneExited({ id: pane.id, generation: pane.generation })
    }
  }

  /**
   * `shell.open {tab}` — a plain shell beside the agents, in the tab's own
   * directory. No launch and no reservation: nothing here is an agent, so
   * there is no conversation to fence and nothing for a controller to
   * write. The pane row is the whole record.
   */
  async shellOpen(request) {
    const tabId = requireText(request?.tab, 'tab')
    const bridge = this.#requireBridge()
    const tab = await this.#openTab(tabId)
    const shell = this.#shell
    const pane = await this.#tabs.addPane(tabId, { kind: 'shell' })
    const body = {
      id: pane.id,
      generation: pane.generation,
      launch: `shell-${pane.id}-${pane.generation}`,
      cwd: tab.directory,
      argv: [shell, '-l'],
      env: {},
    }
    const sent = { transmitted: false }
    let answer
    try {
      answer = await this.#transmit(bridge, body, `the shell for ${tabId}`, sent)
    } catch (cause) {
      if (!sent.transmitted) {
        await this.#discardPane(tab, pane)
        // Given back, so nothing is left to protect — and nothing is left
        // for a drain to wait on. `#transmit` remembers the open before
        // `bridge.request`, which can throw the frame away synchronously,
        // so this is the same clearing the workers and the lead do.
        this.#openEnded(paneIdentity(pane))
      }
      throw cause
    }
    if (answer?.ok !== true) {
      const negative = definiteNegative(answer)
      if (negative === null) {
        // No answer we can act on. The shell may be running, and its row is
        // the only handle anyone has on it — dropping that loses a window
        // the human can see. Its exit is what removes it.
        return { outcome: 'unknown', tab: tabId, pane: paneRef(pane), kind: 'shell' }
      }
      await this.#discardPane(tab, pane)
      this.#openEnded(paneIdentity(pane))
      throw new PaneError(`the pane host did not open a shell: ${negative}`, {
        status: 409,
        code: 'pane-refused',
      })
    }
    this.#openEnded(paneIdentity(pane))
    return { outcome: 'opened', tab: tabId, pane: paneRef(pane), kind: 'shell' }
  }

  /**
   * `pane.close {id, generation}` — the human closed a window.
   *
   * Rust kills the process and THEN sends this
   * (`app/src-tauri/src/commands.rs:1329`), so it is bookkeeping and it
   * cannot refuse: the window is already gone whatever this answers. It is
   * the same bookkeeping a `pane.exit` event does, asked for rather than
   * observed — including for the lead, whose pane ending suspends its tab.
   */
  async paneClose(request) {
    const id = requireText(request?.id, 'pane id')
    const generation = request?.generation
    if (!Number.isInteger(generation) || generation < 1) {
      throw new PaneError('pane generation must be a positive integer', {
        status: 400,
        code: 'bad-generation',
      })
    }
    const tabs = await this.#tabs.list()
    const tab = tabs.find((candidate) =>
      candidate.panes.some((pane) => pane.id === id && pane.generation === generation),
    )
    if (tab === undefined) return { outcome: 'gone', pane: { id, generation } }
    const pane = tab.panes.find((candidate) => candidate.id === id)
    await this.#paneExited({ id, generation })
    return { outcome: 'closed', tab: tab.id, pane: { id, generation }, kind: pane.kind }
  }

  /**
   * `notify.set {scope, id, mode}` — the human's own delivery choice.
   *
   * The two scopes a human owns are the tab and the pane; the lead's
   * `--notify` is a third, lower one it can never reach from here
   * (`hosts/lib/policy.js`). `inherit` at pane scope is how a human takes
   * their hand off and lets the tab decide again.
   */
  async notifySet(request) {
    const scope = requireText(request?.scope, 'scope')
    const id = requireText(request?.id, 'id')
    const mode = requireText(request?.mode, 'mode')
    if (scope !== 'tab' && scope !== 'pane') {
      throw new PaneError(`a policy scope is 'tab' or 'pane', not ${JSON.stringify(scope)}`, {
        status: 400,
        code: 'bad-scope',
      })
    }
    if (scope === 'tab') {
      await this.#refusable('policy-refused', () => this.#store.policySet({ tab: id, value: mode }))
      return { outcome: 'set', scope, id, mode }
    }
    const tabs = await this.#tabs.list()
    const tab = tabs.find((candidate) => candidate.panes.some((pane) => pane.id === id))
    if (tab === undefined) {
      throw new PaneError(`no pane ${id} in any tab`, { status: 404, code: 'no-pane' })
    }
    await this.#refusable('policy-refused', () =>
      this.#store.policySet({ tab: tab.id, pane: id, value: mode }),
    )
    return { outcome: 'set', scope, id, mode, tab: tab.id }
  }

  /**
   * A store write whose refusal is the CALLER's business, not a fault.
   *
   * These writes carry an expectation — the launch they are for — and the
   * store refuses them in prose: the launch is over, the pane is not linked,
   * the candidate carries no evidence. The route answers 500 for anything it
   * cannot name, which is right for a bug and wrong for a refusal, so each
   * one is given a code here and stays a 4xx.
   *
   * A refusal is a bare `Error`, which is what the store deliberately
   * throws. A `TypeError`, an `InternalInvariantError`, or anything carrying
   * an `errno` (the disk, not the request) is a fault and goes up untouched.
   */
  async #refusable(code, work) {
    try {
      return await work()
    } catch (cause) {
      if (cause instanceof PaneError) throw cause
      if (cause?.name === 'AdmissionError') throw admissionRefusal(cause, null)
      if (isStoreRefusal(cause)) throw new PaneError(cause.message, { status: 400, code })
      throw cause
    }
  }

  // --- the decisions behind them ------------------------------------------

  /**
   * Today's continuation rule, over the app's own records: an explicit name
   * is the caller saying which one they mean, `fresh` mints one, and
   * otherwise this lead's own most recent conversation with that agent
   * continues — a lead we cannot name matches no row, so two leads never
   * share a conversation by accident.
   */
  #chooseConversation(tab, threads, { agent, fresh, session }) {
    if (session !== undefined) {
      if (!isRecord(threads[session])) {
        throw new PaneError(`no conversation named ${session} here`, { status: 404 })
      }
      return session
    }
    if (fresh === true) return this.#mint(threads, agent.id)
    const lead = leadIdentity(tab)
    const mine = Object.entries(threads)
      .filter(([, record]) => isRecord(record) && record.agent === agent.id && record.lead === lead)
      .sort((a, b) => String(b[1].updatedAt ?? '').localeCompare(String(a[1].updatedAt ?? '')))
    return mine.length === 0 ? this.#mint(threads, agent.id) : mine[0][0]
  }

  /**
   * Admission, in the store's own queue: what state is this conversation in
   * and, if it needs a launch, take the reservation now.
   *
   * Reading the state here and reserving in a later step is how two callers
   * both decide to launch one conversation, so there is no later step: the
   * store reads the panes and the reservation and writes the new one in a
   * single queued operation, and nothing is released on the strength of a
   * snapshot taken before it. The bridge I/O that follows is outside the
   * queue.
   *
   * The evidence the launch will be bound by is computed INSIDE that
   * mutation, from the row the store read, because it depends on whether
   * the conversation already has a native session: a bound conversation is
   * RESUMED on the session it already has. Minting a new identity for a
   * conversation that already had one is how a reopened Claude conversation
   * came to be told it was a session it was not.
   */
  async #admit(tab, { name, agent, opId, notify }) {
    const held = {}
    try {
      const admitted = await this.#store.admit(tab.directory, {
        name,
        tab: tab.id,
        agent: agent.id,
        kind: agent.kind,
        lead: leadIdentity(tab),
        ...(notify === undefined ? {} : { notify }),
        launch: (row) => {
          const launchId = randomUUID()
          const { evidence, nativeSession } = launchEvidence(agent.kind, name, launchId, row)
          Object.assign(held, { launchId, nativeSession, created: row === undefined })
          return { launchId, opId, ...evidence }
        },
      })
      return { ...admitted, ...held }
    } catch (cause) {
      // `reserved` and `elsewhere` are the store's words for two states
      // only it can see without a race; they reach the caller as they are.
      if (cause?.name === 'AdmissionError') throw admissionRefusal(cause, name)
      throw cause
    }
  }

  /**
   * The two ways a conversation is not this caller's to take, told apart
   * because they are different situations and want different answers:
   *
   * - `reserved` — a launch of ours that has not come back. We do not know
   *   whether a process is up in that pane, so nothing may launch a second
   *   one and nothing may paste into it.
   * - `elsewhere` — the pane IS up, in a session this caller does not
   *   hold. Taking it would put two harness windows on one native session:
   *   two processes writing one store, two screens showing halves of one
   *   conversation.
   *
   * Every operation that would touch the pane asks this — a consult, a
   * paste, a window — because they are the same mistake made three ways.
   */
  #refuseHeld(tab, name, record) {
    const reserved = isRecord(record?.reserved) ? record.reserved : null
    if (reserved === null) return
    if (reserved.resolvedAt === undefined) {
      throw new PaneError(
        `${name} is reserved by a launch that has not come back — one conversation never opens two`,
        { status: 409, code: 'reserved', detail: { conversation: name } },
      )
    }
    if (reserved.tab !== tab.id) {
      throw new PaneError(`${name} is running in another session — open it there`, {
        status: 409,
        code: 'elsewhere',
        detail: { conversation: name, session: reserved.tab },
      })
    }
  }

  /** A fresh name, never an agent's own and never one already here. */
  #mint(threads, agent) {
    return newSessionName(Object.keys(threads), this.#agents.names(), agent)
  }

  /**
   * Open the pane a reservation was just taken for: ticket, `pane.open`,
   * resolve.
   *
   * Admission already holds the conversation, so nothing else can launch
   * it while this answer is outstanding — and the reservation is resolved
   * only when the answer arrives. A deadline leaves it standing, which is
   * what makes `unknown` an honest word: the pane may well be running.
   */
  /**
   * Open the pane admission reserved, and give the launch back if anything
   * fails before the frame went out.
   *
   * The line that matters is TRANSMISSION. Before it, nothing is running
   * and no exit will ever come, so a stranded reservation would leave the
   * conversation refusing every later launch forever — it must be given
   * back. After it, the outcome is only ours to judge when Rust says
   * plainly that it did not open a pane; silence is not that, and a launch
   * that may be running keeps its reservation.
   */
  async #openAdmitted(tab, admitted, options) {
    const sent = { transmitted: false }
    try {
      return await this.#open(tab, admitted, { ...options, sent })
    } catch (cause) {
      // ONE place decides whether an admitted launch is given back, so
      // there is one rule to read and one to get wrong: nothing went out,
      // or the pane host said plainly that it opened nothing.
      const given = !sent.transmitted || cause?.release === true
      if (given) await this.#abandon(tab, options.name, admitted)
      // Only a launch that was GIVEN BACK stops protecting its identity,
      // and only after that cleanup has run — a reconcile in between would
      // race a row already being taken away. A frame that went out and
      // then failed for any other reason (a transport that died holding
      // it) leaves a host handler that may still be opening the window:
      // the reservation stands, and so does the protection.
      if (given) this.#openEnded(paneIdentity(admitted.pane))
      throw cause
    }
  }

  /** Gives back a launch that was admitted and never transmitted. */
  async #abandon(tab, name, admitted) {
    await this.#releasePane(tab, name, admitted.pane)
    endLaunch(admitted.launchId)
    await this.#discardPane(tab, admitted.pane)
  }

  async #open(tab, admitted, { name, argv, sent }) {
    const bridge = this.#requireBridge()
    const { pane, launchId, nativeSession } = admitted
    const { ticket } = issueTicket(
      {
        tab: tab.id,
        pane: pane.id,
        lead: leadIdentity(tab),
        requester: leadPaneId(tab),
        conversation: name,
        generation: pane.generation,
      },
      { launch: launchId },
    )
    const nonce = admitted.row?.reserved?.nonce
    const body = {
      id: pane.id,
      generation: pane.generation,
      launch: launchId,
      cwd: tab.directory,
      argv: [
        this.#node,
        CF_CLI,
        ...argv([
          ...(nonce === undefined ? [] : ['--launch', nonce]),
          ...(nativeSession === undefined ? [] : ['--native-session', nativeSession]),
        ]),
      ],
      env: controllerEnv({ pane: pane.id, app: this.#app(), ticket }),
    }
    // Everything the transport would refuse the frame for is settled BEFORE
    // the launch counts as transmitted: a body that cannot be encoded, and
    // one that cannot fit a frame, never reach it. Nothing is awaited
    // between these checks and the write, so nothing can change under them.
    const line = encodeFrame(body)
    if (line === null || byteLength(line) > bridge.maxFrameBytes) {
      throw new PaneError(
        line === null
          ? `the launch for ${name} could not be encoded for the pane host`
          : `the launch for ${name} does not fit one frame (${byteLength(line)} bytes)`,
        { status: 409, code: 'pane-refused' },
      )
    }

    // `request` writes the frame before it returns, so from here on the
    // bytes are the transport's and the outcome is not ours to assume.
    const opening = this.#beginOpen(pane)
    const pending = bridge.request('pane.open', body, { deadlineMs: this.#deadlineMs })
    sent.transmitted = true
    let answer
    try {
      answer = await pending
    } finally {
      // Waiting is over either way; the identity is released below, or
      // by `#openAdmitted` once a refusal's cleanup has run.
      this.#openSettled(opening)
    }

    if (answer?.ok !== true) {
      const negative = definiteNegative(answer)
      if (negative === null) {
        // Either the answer never came (`deadline`) or it says nothing we
        // can act on. The pane may well be running, so the reservation
        // stands and no later operation may open a second one for this
        // conversation until its pane exits or the launch resolves.
        return {
          outcome: 'unknown',
          conversation: name,
          tab: tab.id,
          pane: paneRef(pane),
          launch: launchId,
        }
      }
      // Rust said plainly that it did not open a pane. That is not
      // uncertainty, and the conversation must not be left holding a launch
      // nothing will ever end — so this refusal asks for the launch back
      // even though the frame did go out.
      throw new PaneError(`the pane host refused to open a pane: ${negative}`, {
        status: 409,
        code: 'pane-refused',
        release: true,
      })
    }
    await this.#store.resolve(tab.directory, { name, launchId, outcome: 'opened' })
    // Resolved, and only now: a reconcile that ran between the answer and
    // this write would have found a pane the host has and a launch the
    // store still called outstanding.
    this.#openEnded(opening)
    return {
      outcome: 'opened',
      conversation: name,
      tab: tab.id,
      pane: paneRef(pane),
      launch: launchId,
    }
  }

  /** Gives a refused launch's reservation back, and only its own. */
  async #releasePane(tab, name, pane) {
    try {
      await this.#store.releaseExitedPane(tab.directory, {
        name,
        tab: tab.id,
        pane: pane.id,
        generation: pane.generation,
      })
    } catch {
      // The caller's own failure is the one worth reporting.
    }
  }

  /**
   * Takes back a pane identity a launch never got to use. Best effort by
   * design: the failure the caller is already carrying is the one worth
   * reporting, and a pane row left behind is visible, not dangerous.
   */
  async #discardPane(tab, pane) {
    try {
      await this.#tabs.removePane(tab.id, pane.id, pane.generation)
    } catch {
      // Nothing to add: the caller's own error says what went wrong.
    }
  }

  /** Read the epoch, paste under it, record what was sent — in that order. */
  async #paste(tab, name, pane, text, { opId, kind, expect }) {
    const bridge = this.#requireBridge()
    const snapshot = await bridge.request('pane.snapshot', {
      id: pane.id,
      generation: pane.generation,
    })
    if (snapshot?.ok !== true) throw refusedBy(snapshot)
    const written = await bridge.request('pane.write_paste', {
      id: pane.id,
      generation: pane.generation,
      epoch: snapshot.inputEpoch,
      body: text,
    })
    if (written?.ok !== true) throw refusedBy(written)
    // The bytes are already in the pane. If the launch they were for is
    // over, recording them is refused — a refusal of THIS request, which
    // the caller must see as such and never retry.
    await this.#refusable('record-refused', () =>
      this.#store.sentRecord(tab.directory, {
        name,
        entry: { kind, opId, chars: text.length, pane: pane.id, generation: pane.generation },
        expect,
      }),
    )
  }

  /**
   * A pane's process ended: the launch is over. The reservation is
   * released and the pane row goes with it — the conversation itself, its
   * policy and its deliveries stay in the store, which is where the
   * sidebar reads a closed conversation from.
   */
  async #paneExited({ id, generation } = {}) {
    // The pane is gone, whatever its open ever answered — but the identity
    // keeps protecting until this bookkeeping is DONE. Released first, a
    // reconcile running beside it would find the protection gone, the row
    // still there and the host not listing it, and reap the same pane a
    // second time, halfway through the first.
    try {
      await this.#exitBookkeeping({ id, generation })
    } finally {
      this.#openEnded(paneIdentity({ id, generation }))
    }
  }

  /** Everything one exit settles: the launch, then the row it named. */
  async #exitBookkeeping({ id, generation }) {
    for (const tab of await this.#tabs.list()) {
      const pane = (tab.panes ?? []).find(
        (candidate) => candidate.id === id && candidate.generation === generation,
      )
      if (pane === undefined) continue
      const name = pane.conversation
      if (typeof name === 'string' && name.length > 0) {
        // The store compares the exit against the reservation of the
        // moment, inside its queue: a duplicate exit, or one that arrives
        // after the conversation reopened, names a pane the reservation no
        // longer holds and takes nothing.
        const { released, reason } = await this.#store.releaseExitedPane(tab.directory, {
          name,
          tab: tab.id,
          pane: id,
          generation,
        })
        if (released === null) {
          this.#report(`pane.exit for ${id}:${generation} of ${name} ignored (${reason})`)
        } else {
          // Everything that could still act for this launch goes with it —
          // the controller's capability, and the ticket if the process died
          // before it ever redeemed one.
          endLaunch(released)
        }
      }
      if (pane.kind === 'lead') {
        // The lead pane IS the tab: when its process ends the tab has no
        // window, and leaving it open would draw a live lead that is dead
        // and offer no way back. Suspending it makes `tab.resume` the
        // recovery, which is the one path that mints a new generation.
        const held = isRecord(tab.lead?.reserved) ? tab.lead.reserved : null
        if (held === null || held.generation !== pane.generation) return
        // Bind before the launch is let go: the evidence a lead binds by
        // lives ON the reservation, and the transcript it wrote is only
        // discoverable now. Release first and the session it just held
        // becomes unfindable, so every resume after this one opens cold.
        await this.bindLead(tab.id)
        // Release and suspend are ONE decision — see `store.leadEnded`.
        await this.#store.leadEnded(tab.directory, {
          tab: tab.id,
          pane: pane.id,
          generation: pane.generation,
          launchId: held.launchId,
        })
        return
      }
      await this.#tabs.removePane(tab.id, id, generation)
      return
    }
  }

  #report(message) {
    if (typeof this.#onNote === 'function') this.#onNote(message)
  }

  /**
   * A `pane.open` that has gone out, remembered by the identity it names.
   *
   * The KEY is protection and the entry is patience, and they end at
   * different moments on purpose. `Bridge.request`'s deadline stops US
   * waiting; it cancels nothing on the pane host, whose handler runs on and
   * whose window will exist. So the entry settles when this process stops
   * waiting — that is all a reconcile can be asked to wait for — while the
   * key stays until the launch ends for real: resolved, refused and cleaned
   * up, or ended by its own `pane.exit`. Everything here is in memory, so a
   * restarted app protects nothing and reaps what its predecessor left.
   */
  #beginOpen(pane) {
    const key = paneIdentity(pane)
    let done
    const finished = new Promise((resolve) => {
      done = resolve
    })
    this.#openings.set(key, { finished, done })
    return key
  }

  /** This process has stopped waiting; the identity stays protected. */
  #openSettled(key) {
    this.#openings.get(key)?.done()
  }

  /** The launch ended for real — with its bookkeeping already done. */
  #openEnded(key) {
    this.#openings.get(key)?.done()
    this.#openings.delete(key)
  }

  /** Waits out the opens still running — outcomes are their callers' business. */
  async #drainOpens() {
    if (this.#openings.size === 0) return
    await Promise.all([...this.#openings.values()].map((entry) => entry.finished))
  }

  /** The pane this conversation is running in, or null when none is. */
  #livePane(tab, record, name) {
    if (!isRecord(record) || !isRecord(record.reserved)) return null
    // A launch that has not come back is not a live pane: we do not know
    // whether anything is running in it, so nothing may be pasted into it
    // and nothing may call it live to a reader.
    if (record.reserved.resolvedAt === undefined) return null
    if (record.reserved.tab !== tab.id || tab.closed === true) return null
    const pane = (tab.panes ?? []).find(
      (candidate) =>
        candidate.id === record.reserved.pane &&
        candidate.generation === record.reserved.generation &&
        candidate.conversation === name,
    )
    return pane ?? null
  }

  /** The launch this capability names, with the ownership redemption gave. */
  #owner(launch, request) {
    for (const field of ['session', 'conversation', 'name', 'tab', 'pane']) {
      if (Object.hasOwn(request, field)) {
        throw new PaneError(
          `a controller writes for the launch it holds, not for a ${field} it names`,
          { code: 'names-a-conversation' },
        )
      }
    }
    const owner = ownerOf(launch)
    if (owner === null) throw new PaneError('this launch is over', { status: 403 })
    return owner
  }

  async #launchDirectory(owner) {
    const tab = await this.#tab(owner.tab)
    return tab.directory
  }

  async #tab(tabId) {
    const tab = await this.#tabs.get(tabId)
    if (tab === null) throw new PaneError(`no tab ${tabId}`, { status: 404 })
    return tab
  }

  /**
   * Opens the lead's window for a tab that already exists.
   *
   * The order is the whole point. The channel configuration is built FIRST,
   * because it is what the delivery watcher will need and it must be
   * recorded with the reservation, not after it — a lead that is already
   * running with an OpenCode port nobody wrote down cannot be delivered to.
   * Then the store reserves the lead and mints its launch id inside that
   * same mutation. Only then does the frame go out, and the reservation is
   * given back unless it did.
   */
  async #launchLead(tabId, { opened }) {
    const bridge = this.#requireBridge()
    // Before deciding warm or cold: a lead that opened with a marker may
    // have written its transcript since, and the id it minted is only
    // discoverable there. Binding it here is what makes the NEXT launch a
    // resume instead of a third cold window.
    await this.bindLead(tabId)
    const tab = await this.#openTab(tabId)
    const kind = tab.lead.harness
    const command = this.#harnessPath(kind)
    if (command === null || command === undefined) {
      await this.#unwindLeadBeforeAdmission(tabId, opened)
      throw new PaneError(`${kind} is not installed on this machine`, {
        status: 409,
        code: 'harness-missing',
      })
    }
    // A launch id is needed to name the channel's own files and its
    // password before the store has one, so it is minted here and handed to
    // the store to record. `leadAdmit` is what makes it the tab's launch.
    const launchId = randomUUID()
    const configuration = await this.#leadChannel(kind, launchId, tab.directory)
    let admitted
    try {
      admitted = await this.#store.leadAdmit(tab.directory, {
        tab: tabId,
        // Decided inside the mutation, from the record the store reads
        // there: a lead that already bound a session is resumed on it, and
        // only an unbound one is given a newly minted identity.
        launch: (record) => ({
          launchId,
          ...leadEvidence(kind, tabId, launchId, record.lead.nativeSession),
        }),
        channel: configuration.channel,
      })
    } catch (cause) {
      await this.#unwindLeadBeforeAdmission(tabId, opened)
      if (cause?.name === 'AdmissionError') throw admissionRefusal(cause, null)
      throw cause
    }
    // What the harness itself is told: `hosts/lib/runners.js` owns which
    // flag each one takes, so this asks it rather than knowing. A bound
    // lead resumes; an unbound one starts, carrying the nonce in its seed
    // when the harness insists on minting its own id.
    const session = this.#leadArgv(kind, admitted)
    const body = {
      id: admitted.pane.id,
      generation: admitted.pane.generation,
      launch: launchId,
      cwd: tab.directory,
      argv: [command, ...configuration.args, ...session.args],
      dropEnv: session.dropEnv,
      env: {
        ...leadEnv({
          tab: tabId,
          pane: admitted.pane.id,
          leadId: leadIdentity(tab),
          app: this.#app(),
          path: this.#env.PATH,
          node: this.#node,
        }),
        ...configuration.env,
      },
    }
    const sent = { transmitted: false }
    let answer
    try {
      answer = await this.#transmit(bridge, body, `the lead of ${tabId}`, sent)
    } catch (cause) {
      // Only a frame that NEVER left may be taken back. A transport that
      // rejected after writing leaves a lead that may be running, and
      // releasing its launch frees a reservation nothing will ever end.
      if (!sent.transmitted) {
        await this.#unwindLead(tabId, launchId, admitted)
        // Given back, so nothing is left to protect. A lead whose frame DID
        // go out keeps both, on the same rule the workers follow.
        this.#openEnded(paneIdentity(admitted.pane))
      }
      throw cause
    }
    if (answer?.ok !== true) {
      const negative = definiteNegative(answer)
      if (negative === null) {
        // The frame went out and nothing came back that we can act on. The
        // lead may well be running, so the reservation stands: a second
        // launch on this tab would be a second lead writing one store, and
        // only a matching `pane.exit` settles it.
        return { outcome: 'unknown', tab: tabId, pane: admitted.pane, launch: launchId }
      }
      // The pane host said plainly that it opened nothing. A tab whose lead
      // is not there is not a tab the page can draw, so this generation
      // goes back to suspended — the state `tab.resume` recovers from, and
      // the state a fresh tab was one step away from anyway.
      await this.#unwindLead(tabId, launchId, admitted)
      this.#openEnded(paneIdentity(admitted.pane))
      throw new PaneError(`the pane host refused to open the lead: ${negative}`, {
        status: 409,
        code: 'pane-refused',
      })
    }
    await this.#store.leadResolve(tab.directory, { tab: tabId, launchId, outcome: 'opened' })
    this.#openEnded(paneIdentity(admitted.pane))
    // An id we preallocated is bound the moment the window carrying it
    // opens: we minted it, we passed it, and the harness has no say. The
    // other shapes bind later, when something observes the session — which
    // is why `resumedSession` can be null and the answer says so.
    let bound = session.nativeSession
    if (session.nativeSession !== null && admitted.nativeSession === null) {
      const decision = await this.#refusable('bind-refused', () =>
        this.#store.leadBind(tab.directory, {
          tab: tabId,
          candidate: { sessionId: session.nativeSession },
          expect: { launchId },
        }),
      )
      bound = decision.nativeSession
    }
    return {
      outcome: 'opened',
      tab: tabId,
      pane: admitted.pane,
      launch: launchId,
      harness: kind,
      directory: tab.directory,
      resumedSession: session.resumed ? bound : null,
      nativeSession: bound,
      cold: session.resumed !== true,
      ...(session.reason === null ? {} : { reason: session.reason }),
    }
  }

  /**
   * Bind this tab's lead to the native session its launch left behind.
   *
   * The three shapes again, and the same evidence rule: an id we
   * preallocated is looked for where the harness would have put its file,
   * an id the harness reported is taken as reported, and everything else is
   * found by the `[consensflow launch <nonce>]` marker opening one of the
   * session's first five user turns. `discoverSessionWithEvidence` never
   * answers without evidence, and `leadBind` checks it again inside the
   * queue against the launch it is fenced to — so nothing binds on the
   * strength of having been asked nicely.
   *
   * Safe to call whenever: a lead already bound, a tab with no reservation
   * and a launch nothing can be found for all answer the same way, by
   * changing nothing.
   */
  async bindLead(tabId) {
    const tab = await this.#tabs.get(tabId)
    const reserved = isRecord(tab?.lead?.reserved) ? tab.lead.reserved : null
    if (tab === null || reserved === null) return { bound: false, reason: 'no lead launch' }
    if (typeof tab.lead.nativeSession === 'string' && tab.lead.nativeSession.length > 0) {
      return { bound: true, nativeSession: tab.lead.nativeSession, reason: 'already bound' }
    }
    const since = Date.parse(reserved.at ?? '')
    let found = null
    try {
      found = await discoverSessionWithEvidence(
        tab.lead.harness,
        tab.directory,
        Number.isFinite(since) ? since : 0,
        this.#env,
        {
          nonce: reserved.nonce ?? null,
          preallocatedId: reserved.preallocatedId ?? null,
          reportedId: reserved.reportedId ?? null,
        },
      )
    } catch (cause) {
      // A harness store we cannot read is not a binding failure worth
      // stopping a launch for: the lead opens cold and says so.
      this.#report(`lead discovery for ${tabId} failed: ${cause?.message ?? cause}`)
      return { bound: false, reason: 'discovery failed' }
    }
    if (found === null) return { bound: false, reason: 'no session carries this launch' }
    try {
      const decision = await this.#store.leadBind(tab.directory, {
        tab: tabId,
        candidate: {
          sessionId: found.sessionId,
          ...(found.turn === undefined ? {} : { turn: found.turn }),
        },
        expect: { launchId: reserved.launchId },
      })
      return { bound: true, ...decision }
    } catch (cause) {
      if (isStoreRefusal(cause)) return { bound: false, reason: cause.message }
      throw cause
    }
  }

  /**
   * The harness's own window on this lead's session.
   *
   * `interactiveResume` when there is a bound session — that is the whole
   * promise, that a tab's PM conversation survives its suspend — and
   * `interactiveStart` otherwise, seeded with the launch nonce for the
   * harnesses that mint their own id. Both come from `hosts/lib/runners.js`
   * so the flags live in ONE place; only the command is replaced, because
   * the pane host needs the absolute path and that module names a binary.
   *
   * The billing guard rides along in `dropEnv` for the same reason: a lead
   * is spawned by Rust rather than through `cf`, so the names that would
   * flip a subscription login to API-key billing are only stripped if the
   * invocation's own list reaches the frame.
   */
  #leadArgv(kind, admitted) {
    const agent = { kind }
    const reserved = admitted.evidence ?? {}
    if (admitted.nativeSession !== null && admitted.nativeSession !== undefined) {
      const resume = interactiveResume(agent, admitted.nativeSession)
      if (resume !== null) {
        return {
          args: resume.args,
          dropEnv: resume.dropEnv ?? [],
          nativeSession: admitted.nativeSession,
          resumed: true,
          reason: null,
        }
      }
    }
    const preallocated = reserved.preallocatedId ?? null
    // `[consensflow launch <nonce>]`, exactly as a worker's packet opens.
    // A bare nonce is not evidence: `bindEvidence` looks for the MARKER at
    // the head of one of the first five user turns, so a seed carrying the
    // id on its own could never bind and the lead stayed cold forever.
    const seed = typeof reserved.nonce === 'string' ? formatLaunchMarker(reserved.nonce) : undefined
    const start = interactiveStart(agent, preallocated, seed)
    return {
      args: start?.args ?? [],
      dropEnv: start?.dropEnv ?? [],
      nativeSession: preallocated,
      resumed: false,
      reason:
        admitted.nativeSession === null || admitted.nativeSession === undefined
          ? 'this lead never bound a native session, so it opens cold'
          : `${kind} has no interactive resume, so it opens cold`,
    }
  }

  /**
   * What this harness needs on its command line to be deliverable to.
   *
   * Only the harnesses a live probe confirmed have a channel beyond the two
   * that are always there, and `launchConfiguration` throws for the rest —
   * so `enabledChannels` is asked first rather than the answer guessed from
   * a name.
   */
  async #leadChannel(kind, launchId, workspace) {
    const extra = enabledChannels(kind).filter((channel) => channel !== 'pty-inline')
    if (!extra.some((channel) => channel !== 'cf-read')) {
      return { args: [], env: {}, channel: null }
    }
    return await launchConfiguration(kind, { launchId, workspace })
  }

  /**
   * A lead launch that definitely did not happen, given back.
   *
   * Release and suspend are ONE queued decision comparing pane, generation
   * and launch (`store.leadEnded`), so a tab whose lead never opened ends
   * suspended with no reservation — the only state `tab.resume` can recover
   * from. Doing half of it is what left a tab open with nothing holding it
   * and a retry answering "not suspended".
   */
  async #unwindLead(tabId, launchId, admitted) {
    try {
      await this.#store.leadEnded(await this.#directoryOf(tabId), {
        tab: tabId,
        pane: admitted.pane.id,
        generation: admitted.pane.generation,
        launchId,
      })
    } catch {
      // The tab stands either way; a failed tidy-up must not replace the
      // error that actually stopped the launch.
    }
  }

  /**
   * Nothing was admitted yet, so there is no launch to give back — only a
   * tab this call minted a moment ago, and only `tab.open` mints one.
   */
  async #unwindLeadBeforeAdmission(tabId, opened) {
    if (opened !== true) return
    try {
      await this.#tabs.suspend(tabId)
    } catch {
      // As above: a failed tidy-up must not replace the real error.
    }
  }

  async #directoryOf(tabId) {
    return (await this.#tabs.get(tabId))?.directory ?? null
  }

  /**
   * Encodes a `pane.open` body and sends it, refusing anything the frame
   * could not carry BEFORE it counts as sent — the same boundary `#open`
   * keeps, and for the same reason.
   */
  async #transmit(bridge, body, subject, sent) {
    const line = encodeFrame(body)
    if (line === null || byteLength(line) > bridge.maxFrameBytes) {
      throw new PaneError(
        line === null
          ? `${subject} could not be encoded for the pane host`
          : `${subject} does not fit one frame (${byteLength(line)} bytes)`,
        { status: 409, code: 'pane-refused' },
      )
    }
    // `request` writes the frame before it returns, so from here on the
    // bytes are the transport's and the outcome is not ours to assume — the
    // same boundary `#open` keeps, and nothing is awaited across it.
    const opening = this.#beginOpen(body)
    const pending = bridge.request('pane.open', body, { deadlineMs: this.#deadlineMs })
    sent.transmitted = true
    try {
      return await pending
    } finally {
      this.#openSettled(opening)
    }
  }

  async #openTab(tabId) {
    const tab = await this.#tab(tabId)
    if (tab.closed === true) {
      throw new PaneError(`the session ${tabId} is closed — resume it first`, {
        status: 409,
        code: 'session-closed',
      })
    }
    return tab
  }

  #requireBridge() {
    if (this.#bridge === null || this.#bridge.closed === true) {
      throw new PaneError('no pane host is connected', { status: 503, code: 'no-pane-host' })
    }
    return this.#bridge
  }

  /**
   * One answer per `opId`, within the credential that carried it: a lead's
   * tab, or a controller's launch. Keying by the subject as well as the id
   * is what stops one tab's replay collecting another tab's answer, and
   * neither subject's alphabet contains a colon, so the two parts of the
   * key can never run together.
   *
   * A completed op is replayed from the ledger; a replay that arrives while
   * the first is still in flight waits on the same promise, so a retry can
   * never become a second launch or a second recorded line. Only outcomes
   * with a side effect are kept — a refusal burns no opId.
   */
  #once(subject, opId, work) {
    const key = `${subject}:${opId}`
    const decided = this.#ledger.get(key)
    if (decided !== undefined) {
      return decided.ok ? Promise.resolve(decided.answer) : Promise.reject(decided.error)
    }
    const running = this.#inFlight.get(key)
    if (running !== undefined) return running
    // Claimed before the work starts, so nothing can run twice while the
    // first attempt is still deciding.
    const promise = work().then(
      (answer) => {
        this.#ledger.set(key, { ok: true, answer })
        this.#inFlight.delete(key)
        return answer
      },
      (error) => {
        // A failure is a decision too. The bytes may already be in the
        // pane — that is exactly the case this exists for — so the answer
        // is kept and replayed rather than the work being done again. A
        // caller that wants another attempt brings another opId.
        this.#ledger.set(key, { ok: false, error })
        this.#inFlight.delete(key)
        throw error
      },
    )
    this.#inFlight.set(key, promise)
    return promise
  }
}

// --- small local helpers -----------------------------------------------------

/**
 * The evidence a launch will be bound by, and the native session the pane
 * must take — decided from the row the store just read.
 *
 * A conversation that already HAS a native session is resumed on it: the
 * pane is told to reopen that session and the launch expects exactly it
 * back. Minting a fresh identity for a conversation that already had one
 * told a reopened Claude pane to be a session it was not, and the binding
 * then failed against the session the row still named.
 *
 * A fresh conversation gets an identity of its own: claude and pi bind on
 * an id we mint before they start (claude's `--session-id`, pi's own name);
 * the rest bind on the non-secret nonce their seed's first line carries,
 * and the nonce IS the launch id — launch-unique by construction.
 *
 * The two flags are `bin/cf.mjs`'s contract (tasks 19–20): `--launch
 * <nonce>` for the seed's first line, `--native-session <id>` for the
 * session to preallocate or resume.
 */
function launchEvidence(kind, name, launchId, row) {
  const bound =
    typeof row?.sessionId === 'string' && row.sessionId.length > 0 ? row.sessionId : null
  if (bound !== null) {
    return {
      nativeSession: bound,
      evidence: PREALLOCATED_KINDS.has(kind) ? { preallocatedId: bound } : { reportedId: bound },
    }
  }
  if (kind === 'pi') return { nativeSession: name, evidence: { preallocatedId: name } }
  if (kind === 'claude-code') {
    const minted = randomUUID()
    return { nativeSession: minted, evidence: { preallocatedId: minted } }
  }
  return { nativeSession: undefined, evidence: { nonce: launchId } }
}

/** What the follow-up paste says, when a consult continues a conversation. */
function consultText({ task, brief, context, handoffFile }) {
  const lines = [String(task).trim()]
  if (typeof brief === 'string' && brief.trim().length > 0) lines.push(`Brief: ${brief.trim()}`)
  if (typeof context === 'string' && context.trim().length > 0) {
    lines.push(`Context: ${context.trim()}`)
  }
  if (typeof handoffFile === 'string' && handoffFile.trim().length > 0) {
    lines.push(`Handoff: ${handoffFile.trim()} — read it before you answer.`)
  }
  return lines.join('\n\n')
}

function readConsult(request) {
  const opId = requireText(request.opId, 'opId')
  const agent = requireText(request.agent, 'agent').replace(/^@/, '')
  const task = requireText(request.task, 'task')
  if (request.session !== undefined && request.fresh === true) {
    throw new PaneError('a fresh conversation and a named one are two different asks')
  }
  const session =
    request.session === undefined ? undefined : requireText(request.session, 'session')
  if (request.notify !== undefined && request.notify !== 'auto' && request.notify !== 'manual') {
    throw new PaneError(`not a delivery preference: ${JSON.stringify(request.notify)}`)
  }
  return { opId, agent, task, fresh: request.fresh === true, session, notify: request.notify }
}

function refusedBy(answer) {
  return new PaneError(answer?.error ?? 'the pane host gave no answer', {
    status: answer?.error === 'deadline' ? 504 : 409,
    code: 'pane-refused',
  })
}

/** The launch a lead's write is for, read from the row it just admitted. */
function expectationOf(row) {
  const reserved = isRecord(row?.reserved) ? row.reserved : null
  if (reserved === null) return undefined
  return {
    launchId: reserved.launchId,
    tab: reserved.tab,
    pane: reserved.pane,
    generation: reserved.generation,
  }
}

/** The launch a controller's write is for — its capability names it. */
function expectationFor(launch, owner) {
  return { launchId: launch, tab: owner.tab, pane: owner.pane, generation: owner.generation }
}

/**
 * A conversation belongs to one agent on one harness. A consult that names
 * somebody else's conversation is a mistake, not an instruction: answering
 * it would paste one agent's task into another's pane and report the wrong
 * name back.
 */
function requireSameAgent(name, row, agent) {
  if (!isRecord(row)) return
  if (row.agent === agent.id && row.kind === agent.kind) return
  throw new PaneError(
    `${name} belongs to ${row.agent} (${row.kind}), not ${agent.id} (${agent.kind})`,
    { status: 400, code: 'agent-mismatch', detail: { conversation: name, agent: row.agent } },
  )
}

/**
 * What a reader may be told about a pane. `live` means a launch that came
 * back and a pane still serving its conversation; a launch we never heard
 * back from is reported as what it is, not as live.
 */
function paneLiveness(tab, pane, record, live) {
  if (pane.kind === 'lead') {
    const up = tab.closed !== true
    return { live: up, status: up ? 'live' : 'ended' }
  }
  if (live !== null) return { live: true, status: 'live' }
  if (isRecord(record?.reserved) && record.reserved.resolvedAt === undefined) {
    return { live: false, status: 'unresolved' }
  }
  return { live: false, status: 'ended' }
}

/**
 * The parts of a delivery, as the lead must see them.
 *
 * A `cf-read` record carries the parts it was planned with, under the
 * budget of the lead it was planned FOR — that is the split the pointer
 * promised and the split a receipt would be checked against, so it is used
 * as it stands. A record without them is split here, under this lead
 * harness's own tool-output budget.
 */
function deliveryParts(record, tab) {
  if (Array.isArray(record.parts) && record.parts.length > 0) return record.parts
  if (typeof record.answer !== 'string') {
    throw new PaneError(`delivery ${record.id} carries no answer to print`, {
      status: 409,
      code: 'no-such-delivery',
    })
  }
  const kind = tab.lead?.harness
  const budget = record.partBudget ?? DEFAULT_PART_BUDGETS[kind] ?? DEFAULT_PART_BUDGETS.default
  return partsFor(record.answer, record.id, budget)
}

/**
 * The transcript items the walk runs over, in the order the client read
 * them, without the tool calls.
 *
 * One side owns the walk and it is the server, which holds the store row
 * and the delivery records. What the client knows and the server cannot is
 * which items it just PRINTED to the lead, so that rides on each item:
 * `{id, role, printed}`. Bare ids are refused by name, because a client
 * sending them is a client whose walk would silently stop at the first
 * item it did not describe.
 *
 * Tool items are dropped: a tool item is never delivery-covered and never
 * printed, so leaving one in stops the frontier at the worker's first tool
 * call and everything read after it stays unmarked.
 */
function readItems(items) {
  if (!Array.isArray(items)) {
    throw new PaneError('items is an ordered array of transcript items {id, role, printed}')
  }
  return items
    .map((item) => {
      if (!isRecord(item)) {
        throw new PaneError(
          `each item is an object {id, role, printed}, not ${JSON.stringify(item)}`,
        )
      }
      requireText(item.id, 'item id')
      if (item.printed !== undefined && typeof item.printed !== 'boolean') {
        throw new PaneError(
          `printed says whether the client put this item in front of the lead: ${JSON.stringify(item.printed)} is not true or false`,
        )
      }
      return item
    })
    .filter((item) => item.role !== 'tool')
}

/**
 * A plain refusal from the pane host, or `null` for anything else.
 *
 * Only `{ok: false, error: '<why>'}` is Rust saying it did not open a pane.
 * A `null`, an empty object, a non-boolean `ok` or a missing reason is a
 * malformed answer — evidence of nothing — and a launch we cannot judge is
 * uncertain, not refused: treating it as a refusal released a reservation
 * for a pane that was running, and the next operation opened a second one.
 * `deadline` is the bridge's own word for "no answer came", which is the
 * same uncertainty by another route.
 */
function definiteNegative(answer) {
  if (!isRecord(answer)) return null
  if (answer.ok !== false) return null
  if (typeof answer.error !== 'string' || answer.error.length === 0) return null
  return answer.error === 'deadline' ? null : answer.error
}

/**
 * One entry of a `pane.list` answer, in the only shape a reconcile can
 * decide by: an identity to compare and a liveness to believe. A missing
 * or mistyped field is not a dead pane, it is an answer we cannot read.
 *
 * The generation must be a SAFE integer: past 2^53 JavaScript stops being
 * able to tell two of them apart, so a comparison there could match a row
 * against a pane that is not it and reconcile away a living launch.
 */
function isListedPane(entry) {
  return (
    isRecord(entry) &&
    typeof entry.id === 'string' &&
    entry.id.length > 0 &&
    Number.isSafeInteger(entry.generation) &&
    entry.generation >= 1 &&
    typeof entry.alive === 'boolean'
  )
}

/** The refusal that leaves a suspended tab exactly as it was. */
function unlistablePanes(tabId, reason) {
  return new PaneError(
    `the pane host could not say what it still holds, so the tab ${tabId} was left suspended: ${reason}`,
    { status: 503, code: 'pane-list-unavailable' },
  )
}

/** The frame as bytes, or `null` when this body has no encoding at all. */
function encodeFrame(body) {
  try {
    const line = JSON.stringify(body)
    return typeof line === 'string' ? line : null
  } catch {
    return null
  }
}

const byteLength = (text) => Buffer.byteLength(text, 'utf8')

function paneRef(pane) {
  return { id: pane.id, generation: pane.generation }
}

function leadPaneId(tab) {
  const pane = (tab.panes ?? []).find((candidate) => candidate.kind === 'lead')
  if (pane === undefined) throw new PaneError(`the session ${tab.id} has no lead pane`)
  return pane.id
}

function flag(name, value) {
  return typeof value === 'string' && value.trim().length > 0 ? [name, value] : []
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function requireText(value, label) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new PaneError(`${label} is required`)
  }
  return value
}
