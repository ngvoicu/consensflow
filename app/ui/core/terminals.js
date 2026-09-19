import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { element } from './board.js'

/**
 * The live windows behind the board: one terminal per participant that has a
 * pane, and one of them docked beside the board at a time (the lead's, the
 * PM's, or a member's for a while). A terminal outlives the dock: while
 * another window is docked, its card waits in an off-screen parking lot so
 * the emulator keeps receiving output and keeps its scrollback, its size and
 * the human's half-typed input.
 */
export class TerminalsView {
  #stage
  #parking
  #registry
  #link
  #cards = new Map()

  constructor(stage, parking, { invoke, report, createEmulator }) {
    this.#stage = stage
    this.#parking = parking
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
    this.#link.output(message, (pane) => this.#emulator(pane, null))
  }

  /** The emulators, keyed `id:generation`: what the packaged smoke reads the screen from. */
  get registry() {
    return this.#registry
  }

  /** Typed input for a pane, on the same path a keystroke takes. */
  input(pane, data) {
    return this.#link.input(pane, data)
  }

  /** Keep a window for every lane that has one, and dock the one whose handle is `docked`. */
  render(lanes, { docked }) {
    const live = lanes.filter((lane) => lane.pane !== null)
    const keys = new Set(live.map((lane) => paneKey(lane.pane)))
    for (const key of [...this.#cards.keys()]) {
      if (keys.has(key)) continue
      this.#cards.get(key).card.remove()
      this.#cards.delete(key)
      this.#link.retire(key)
    }
    const cards = new Map(live.map((lane) => [lane, this.#card(lane.pane, lane)]))
    const shown = live.find((lane) => lane.participant.handle === docked) ?? null
    for (const [lane, card] of cards) if (lane !== shown) this.#parking.append(card)
    if (shown === null) {
      const wanted = lanes.find((lane) => lane.participant.handle === docked)
      this.#stage.replaceChildren(
        element(
          'p',
          'stage-empty',
          wanted === undefined
            ? 'No window is open yet.'
            : `${laneName(wanted.participant)} has no window open.`,
        ),
      )
      return
    }
    const card = cards.get(shown)
    card.dataset.focused = 'true'
    this.#stage.replaceChildren(card)
    this.#registry.fit(shown.pane.id, shown.pane.generation)
    requestAnimationFrame(() =>
      this.#registry.get(shown.pane.id, shown.pane.generation)?.terminal?.focus(),
    )
  }

  #card(pane, lane) {
    const key = paneKey(pane)
    let entry = this.#cards.get(key)
    if (entry === undefined) {
      const card = element('section', 'terminal-card')
      const head = element('header', 'terminal-head')
      const host = element('div', 'terminal-host')
      card.append(head, host)
      this.#parking.append(card)
      entry = { card, head, host }
      this.#cards.set(key, entry)
      this.#registry.ensure(pane, host)
    }
    if (lane !== null) {
      const name = laneName(lane.participant)
      const lamp = element('span', 'lamp')
      lamp.dataset.state = lane.activity?.state ?? 'closed'
      lamp.setAttribute('aria-hidden', 'true')
      entry.head.replaceChildren(
        lamp,
        element('span', 'terminal-name', name),
        element('span', 'terminal-meta', lane.participant.harness ?? ''),
      )
      entry.card.setAttribute('aria-label', `${name}'s terminal`)
      entry.card.dataset.handle = lane.participant.handle
    }
    return entry.card
  }

  #emulator(pane) {
    this.#card(pane, null)
    return this.#registry.get(pane.id, pane.generation)
  }
}

const laneName = (participant) =>
  ({ lead: 'Lead', pm: 'PM' })[participant.handle] ?? `@${participant.handle}`
