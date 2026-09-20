import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { element, laneOrder } from './board.js'

/**
 * The live windows beside the board: a horizontal strip of one terminal per
 * participant that has a pane, the lead first, then the members,
 * scrolling sideways. A window that ended stays in the strip, marked ended
 * and still readable, until its participant opens a new one or the human
 * closes it; the emulator keeps its scrollback, its size and the human's
 * half-typed input the whole time.
 */
export class TerminalsView {
  #stage
  #registry
  #link
  #cards = new Map()
  #onChange

  constructor(stage, { invoke, report, createEmulator, onChange = () => {} }) {
    this.#stage = stage
    this.#onChange = onChange
    this.#registry = new EmulatorRegistry({
      ...(createEmulator ? { createEmulator } : {}),
      onData: (pane, data) => void this.#link.input(pane, data),
      onReply: (pane, data) => void this.#link.reply(pane, data),
      onResize: (pane, cols, rows) => this.#link.resize(pane, cols, rows),
    })
    this.#link = new TerminalLink({ invoke, registry: this.#registry, report })
  }

  /** Bytes from the pane host; a pane the board has not drawn yet still gets them. */
  output(message) {
    this.#link.output(message, (pane) => this.#emulator(pane))
  }

  /** The emulators, keyed `id:generation`: what the packaged smoke reads the screen from. */
  get registry() {
    return this.#registry
  }

  /** Typed input for a pane, on the same path a keystroke takes. */
  input(pane, data) {
    return this.#link.input(pane, data)
  }

  /** Whether a participant has a window in the strip, live or ended. */
  has(handle) {
    return [...this.#cards.values()].some((entry) => entry.handle === handle)
  }

  /**
   * Keep a window for every lane that has one, in lane order; keep an ended
   * window until its participant opens a new one; bring `focused` into view.
   */
  render(lanes, { focused }) {
    const ordered = laneOrder(lanes)
    for (const [order, lane] of ordered.entries()) {
      if (lane.pane === null) continue
      for (const [key, entry] of this.#cards) {
        if (entry.handle === lane.participant.handle && key !== paneKey(lane.pane)) this.#drop(key)
      }
      this.#card(lane.pane, lane, order)
    }
    for (const entry of this.#cards.values()) {
      const lane = ordered.find((lane) => lane.participant.handle === entry.handle)
      const live = lane !== undefined && lane.pane !== null && paneKey(lane.pane) === entry.key
      entry.card.dataset.ended = String(!live)
      entry.ended.hidden = live
    }
    const cards = [...this.#cards.values()].sort((a, b) => a.order - b.order)
    if (cards.length === 0) {
      this.#stage.replaceChildren(element('p', 'stage-empty', 'No window is open yet.'))
      return
    }
    this.#stage.replaceChildren(...cards.map((entry) => entry.card))
    const shown = cards.find((entry) => entry.handle === focused) ?? cards[0]
    for (const entry of cards) entry.card.dataset.focused = String(entry === shown)
    for (const entry of cards) {
      const [id, generation] = [entry.pane.id, entry.pane.generation]
      this.#registry.fit(id, generation)
    }
    shown.card.scrollIntoView({ inline: 'nearest', block: 'nearest' })
    requestAnimationFrame(() =>
      this.#registry.get(shown.pane.id, shown.pane.generation)?.terminal?.focus(),
    )
  }

  /** Every card goes: a closed project has no windows to read. */
  clear() {
    for (const key of [...this.#cards.keys()]) this.#drop(key)
  }

  /** The human closes an ended window's card; a live one stays. */
  close(handle) {
    for (const [key, entry] of this.#cards) {
      if (entry.handle === handle && entry.card.dataset.ended === 'true') this.#drop(key)
    }
    this.#onChange()
  }

  #drop(key) {
    const entry = this.#cards.get(key)
    if (entry === undefined) return
    entry.card.remove()
    this.#cards.delete(key)
    this.#link.retire(key)
  }

  #card(pane, lane, order = Number.MAX_SAFE_INTEGER) {
    const key = paneKey(pane)
    let entry = this.#cards.get(key)
    if (entry === undefined) {
      const card = element('section', 'terminal-card')
      const head = element('header', 'terminal-head')
      const host = element('div', 'terminal-host')
      const ended = element('span', 'terminal-ended', 'ended')
      ended.hidden = true
      card.append(head, host)
      entry = { key, pane, card, head, host, ended, handle: null, order: Number.MAX_SAFE_INTEGER }
      this.#cards.set(key, entry)
      this.#registry.ensure(pane, host)
    }
    if (lane !== null) {
      const name = laneName(lane.participant)
      const lamp = element('span', 'lamp')
      lamp.dataset.state = lane.activity?.state ?? 'closed'
      lamp.setAttribute('aria-hidden', 'true')
      const close = element('button', 'quiet-button terminal-close', 'Close')
      close.type = 'button'
      close.setAttribute('aria-label', `Close ${name}'s ended window`)
      close.addEventListener('click', () => this.close(lane.participant.handle))
      entry.head.replaceChildren(
        lamp,
        element('span', 'terminal-name', name),
        element('span', 'terminal-meta', lane.participant.harness ?? ''),
        entry.ended,
        close,
      )
      entry.card.setAttribute('aria-label', `${name}'s terminal`)
      entry.card.dataset.handle = lane.participant.handle
      entry.handle = lane.participant.handle
      entry.order = order
    }
    return entry.card
  }

  #emulator(pane) {
    this.#card(pane, null)
    return this.#registry.get(pane.id, pane.generation)
  }
}

const laneName = (participant) =>
  participant.member
    ? `@${participant.member} · ${participant.session}`
    : ({ lead: 'Lead' }[participant.handle] ?? `@${participant.handle}`)
