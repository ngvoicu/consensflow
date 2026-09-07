import path from 'node:path'
import { AGENT_KINDS } from '../hosts/lib/state.js'
import { nowIso } from '../hosts/lib/utils.js'
import { allocatePaneId } from './store.js'

/**
 * The tab store (Phase 2, IMPL-PANE-14) — the semantics of tabs on top of
 * `src/store.js`'s ONE queue. Every mutation here is a
 * `store.mutate(directory, 'tab.*', …)`, so the serialization proofs in
 * `tests/store.test.mjs` cover tab writes; what this module owns is what a
 * tab IS:
 *
 * - A tab is app-owned and persisted: id, directory, lead
 *   `{harness, generation, nativeSession?}`, panes `[{id, kind, order,
 *   generation, conversation?}]`, human policy. `create(dir, harness)`
 *   mints it whole, with its lead pane, and answers
 *   `{id, generation: 1, leadId: 'tab:<id>:1'}`. Its `policy` is UNSET:
 *   the field records a HUMAN's explicit choice, which `hosts/lib/policy.js`
 *   ranks above the lead's own `--notify` preference. A default written
 *   here would be indistinguishable from that choice, and would silently
 *   outrank every lead preference on the machine.
 * - The lead identity `tab:<id>:<generation>` is derived, never stored
 *   twice. Two tabs may share a directory; sharing a directory never
 *   shares a lead.
 * - A pane is `(id, generation)`: `resume` reopens the lead pane under the
 *   SAME id with a NEW generation, and a pane id with a new generation is
 *   not the old pane.
 * - `suspend` closes the tab (its process trees are the app's to stop);
 *   `resume` mints generation + 1 and a new leadId. A restart —
 *   `Store.open()`'s recovery — reads every tab closed; this module only
 *   records what happened.
 * - Pane order is append-only and stable: removing a pane keeps the
 *   survivors' order, a new pane appends after them. The lead pane is the
 *   tab itself and cannot be removed.
 * - Pane identity is issued by ONE allocator, `store.js`'s
 *   `allocatePaneId` — the same one the reservation writers use, because a
 *   pane is a pane whoever creates it. Two tabs never share an identity, a
 *   minted `p-<n>` is never handed out twice, and an explicit id comes back
 *   only at a newer generation. Removing a pane names the expected
 *   generation, so a delayed removal never takes its replacement.
 */

const PANE_KINDS = ['lead', 'worker', 'shell']
// An image preset is not a CLI that can hold a pane — a lead runs a real harness.
const LEAD_HARNESSES = AGENT_KINDS.filter((kind) => kind !== 'image')

/** The app-owned identity of a tab's lead: `tab:<id>:<generation>`. */
export function leadIdentity(tab) {
  return `tab:${tab.id}:${tab.lead.generation}`
}

/** A pane is `(id, generation)` — its identity, for one line. */
export function paneIdentity(pane) {
  return `${pane.id}:${pane.generation}`
}

/** Same id AND same generation, or it is not the same pane. */
export function samePane(a, b) {
  return paneIdentity(a) === paneIdentity(b)
}

export class Tabs {
  /**
   * @param {import('./store.js').Store} store — every tab write goes
   * through its queue; this class adds no file access of its own.
   */
  constructor(store) {
    if (
      store === null ||
      typeof store !== 'object' ||
      typeof store.mutate !== 'function' ||
      typeof store.readTabs !== 'function'
    ) {
      throw new Error('Tabs needs the app store — it owns the queue every tab write goes through')
    }
    this.store = store
  }

  /** Creates a tab with its lead pane; answers the identity triple. */
  async create(dir, harness) {
    requireText(dir, 'directory')
    requireOneOf(harness, LEAD_HARNESSES, 'harness')
    const directory = path.resolve(dir)
    return this.store.mutate(directory, 'tab.create', async (io) => {
      const envelope = await io.readTabsEnvelope()
      const at = nowIso()
      const id = `t-${nextTabId(envelope.tabs)}`
      const tab = {
        id,
        directory,
        closed: false,
        lead: { harness, generation: 1, nativeSession: null },
        panes: [
          {
            id: allocatePaneId(envelope, { generation: 1 }),
            kind: 'lead',
            conversation: null,
            generation: 1,
            order: 0,
          },
        ],
        createdAt: at,
        updatedAt: at,
      }
      envelope.tabs.push(tab)
      await io.writeTabsEnvelope(envelope)
      return { id, generation: 1, leadId: leadIdentity(tab) }
    })
  }

  /** Appends a pane to the tab; the id is minted app-wide when absent. */
  async addPane(tabId, { id = null, kind, conversation = null, generation = 1 } = {}) {
    requireText(tabId, 'tab id')
    requireOneOf(kind, PANE_KINDS, 'pane kind')
    if (conversation !== null) requireText(conversation, 'conversation')
    if (!Number.isInteger(generation) || generation < 1) {
      throw new Error('pane generation must be a positive integer')
    }
    return this.store.mutate(await this.#tabDirectory(tabId), 'tab.addPane', async (io) => {
      const envelope = await io.readTabsEnvelope()
      const tab = findTab(envelope.tabs, tabId)
      const pane = {
        id: allocatePaneId(envelope, { id, generation }),
        kind,
        conversation,
        generation,
        order: nextPaneOrder(tab),
      }
      tab.panes.push(pane)
      tab.updatedAt = nowIso()
      await io.writeTabsEnvelope(envelope)
      return pane
    })
  }

  /**
   * Drops one pane, keeping the survivors' order. The expected generation
   * is compared inside the queued mutation: a delayed removal naming an
   * old generation refuses instead of taking the replacement. The lead
   * pane is refused: closing a lead suspends its tab — that is the only
   * shape a tab without its lead pane could have.
   */
  async removePane(tabId, paneId, generation) {
    requireText(tabId, 'tab id')
    requireText(paneId, 'pane id')
    if (!Number.isInteger(generation) || generation < 1) {
      throw new Error('pane generation must be a positive integer')
    }
    return this.store.mutate(await this.#tabDirectory(tabId), 'tab.removePane', async (io) => {
      const tabs = await io.readTabs()
      const tab = findTab(tabs, tabId)
      const pane = tab.panes.find((candidate) => isRecord(candidate) && candidate.id === paneId)
      if (pane === undefined) return false
      if (pane.generation !== generation) {
        throw new Error(
          `no pane ${paneId} at generation ${generation} in tab ${tabId} — a stale removal never takes its replacement`,
        )
      }
      if (pane.kind === 'lead') {
        throw new Error(`the lead pane is the tab itself — suspend the tab ${tabId} instead`)
      }
      tab.panes = tab.panes.filter((candidate) => candidate.id !== paneId)
      tab.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return true
    })
  }

  /** Marks the tab closed — its process trees are the app's to stop. */
  async suspend(tabId) {
    requireText(tabId, 'tab id')
    return this.store.mutate(await this.#tabDirectory(tabId), 'tab.suspend', async (io) => {
      const tabs = await io.readTabs()
      const tab = findTab(tabs, tabId)
      if (tab.closed !== true) {
        tab.closed = true
        tab.updatedAt = nowIso()
        await io.writeTabs(tabs)
      }
      return tab
    })
  }

  /**
   * Reopens the tab: generation + 1, a new leadId, and the lead pane
   * reopened under the same id with a new generation — a new pane, not the
   * old one.
   */
  async resume(tabId) {
    requireText(tabId, 'tab id')
    return this.store.mutate(await this.#tabDirectory(tabId), 'tab.resume', async (io) => {
      const tabs = await io.readTabs()
      const tab = findTab(tabs, tabId)
      if (tab.closed !== true) {
        throw new Error(`the tab ${tabId} is not suspended`)
      }
      tab.closed = false
      tab.lead.generation += 1
      const leadPane = tab.panes.find((candidate) => candidate.kind === 'lead')
      if (leadPane !== undefined) leadPane.generation += 1
      tab.updatedAt = nowIso()
      await io.writeTabs(tabs)
      return { id: tab.id, generation: tab.lead.generation, leadId: leadIdentity(tab) }
    })
  }

  async get(tabId) {
    return (await this.store.readTabs()).find((tab) => tab.id === tabId) ?? null
  }

  /** Creation order — the order the sidebar shows. */
  async list() {
    return await this.store.readTabs()
  }

  /** The tab's own directory is the audit cwd for its ops. */
  async #tabDirectory(tabId) {
    const tab = (await this.store.readTabs()).find(
      (candidate) => isRecord(candidate) && candidate.id === tabId,
    )
    if (tab === undefined) throw new Error(`no tab ${tabId}`)
    return tab.directory ?? null
  }
}

// --- small local helpers -----------------------------------------------------

function findTab(tabs, tabId) {
  const tab = tabs.find((candidate) => isRecord(candidate) && candidate.id === tabId)
  if (tab === undefined) throw new Error(`no tab ${tabId}`)
  if (!Array.isArray(tab.panes)) tab.panes = []
  return tab
}

function nextTabId(tabs) {
  let max = 0
  for (const tab of tabs) {
    const match = isRecord(tab) ? /^t-(\d+)$/.exec(tab.id) : null
    if (match !== null) max = Math.max(max, Number(match[1]))
  }
  return max + 1
}

function nextPaneOrder(tab) {
  let max = -1
  for (const pane of tab.panes) {
    if (isRecord(pane) && Number.isInteger(pane.order) && pane.order > max) max = pane.order
  }
  return max + 1
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function requireText(value, label) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new Error(`${label} is required`)
  }
}

function requireOneOf(value, allowed, label) {
  if (!allowed.includes(value)) {
    throw new Error(`${label} must be one of: ${allowed.join(', ')}`)
  }
}
