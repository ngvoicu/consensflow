import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { gridTemplate } from '../vendor/layout.js'
import { element } from './board.js'

/**
 * The live windows behind the board: one terminal per participant that has a
 * pane, the lead first. A terminal outlives the view: when the board is shown,
 * its card waits in an off-screen parking lot so the emulator keeps receiving
 * output and keeps its scrollback, its size and the human's half-typed input.
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

  /** Draw the session's windows; `visible` puts them on stage, `focus` names one. */
  render(lanes, { visible, focus = null }) {
    const live = lanes.filter((lane) => lane.pane !== null)
    const keys = new Set(live.map((lane) => paneKey(lane.pane)))
    for (const key of [...this.#cards.keys()]) {
      if (keys.has(key)) continue
      this.#cards.get(key).card.remove()
      this.#cards.delete(key)
      this.#link.retire(key)
    }
    const cards = live.map((lane, index) => {
      const card = this.#card(lane.pane, lane)
      card.style.gridArea = index === 0 ? 'lead' : `w${index}`
      card.dataset.focused = String(lane.participant.handle === focus)
      return card
    })
    if (!visible || cards.length === 0) {
      this.#parking.append(...cards)
      this.#stage.replaceChildren(
        ...(visible
          ? [element('p', 'stage-empty', 'No windows are open in this session yet.')]
          : []),
      )
      return
    }
    const layout = gridTemplate(cards.length)
    this.#stage.style.gridTemplateAreas = layout.areas
    this.#stage.style.gridTemplateColumns = `repeat(${layout.cols}, minmax(0, 1fr))`
    this.#stage.style.gridTemplateRows = `repeat(${layout.rows}, minmax(0, 1fr))`
    this.#stage.replaceChildren(...cards)
    for (const lane of live) this.#registry.fit(lane.pane.id, lane.pane.generation)
    const focused = live.find((lane) => lane.participant.handle === focus)
    if (focused) {
      requestAnimationFrame(() =>
        this.#registry.get(focused.pane.id, focused.pane.generation)?.terminal?.focus(),
      )
    }
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
      const name =
        { lead: 'Lead', pm: 'PM' }[lane.participant.handle] ?? `@${lane.participant.handle}`
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
