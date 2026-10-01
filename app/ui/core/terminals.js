import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { element, laneOrder } from './board.js'

/**
 * The live windows beside the board: a horizontal strip of one terminal per
 * participant that has a pane, the chief first, then the members,
 * scrolling sideways. Only live windows: one that ends leaves with its card,
 * and its lane says it is closed and opens it again on its conversation.
 * While a window lives, its emulator keeps its scrollback, its size and the
 * human's half-typed input, across project switches too.
 */
export class TerminalsView {
  #stage
  #registry
  #link
  #cards = new Map()
  /** Windows the human closed: their cards go at once and never come back. */
  #dismissed = new Set()
  #onChange
  /** The human closing a live window from its card: the same as from its board row. */
  #onClose
  /** The card last brought into view: a redraw scrolls only when it changes. */
  #shownKey = null

  constructor(stage, { invoke, report, createEmulator, onChange = () => {}, onClose = () => {} }) {
    this.#stage = stage
    this.#onChange = onChange
    this.#onClose = onClose
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

  /**
   * Keep a terminal for every lane of the project shown that has a live one,
   * in lane order, and bring `focused` into view. Another project's terminals
   * stay alive, off screen, with their scrollback: switching projects loses
   * nothing.
   */
  render(lanes, { focused, project }) {
    const ordered = laneOrder(lanes)
    for (const [order, lane] of ordered.entries()) {
      if (lane.pane === null || this.#dismissed.has(paneKey(lane.pane))) continue
      this.#card(lane.pane, lane, order, project)
    }
    // A window that ended, or gave way to a newer one, goes with its card: a
    // last frame left in the strip still shows the agent's prompt and reads
    // as open beside a lane that says it is closed.
    for (const entry of [...this.#cards.values()]) {
      if (entry.project !== project) continue
      const lane = ordered.find((lane) => lane.participant.handle === entry.handle)
      if (lane === undefined || lane.pane === null || paneKey(lane.pane) !== entry.key) {
        this.#drop(entry.key)
      }
    }
    const cards = [...this.#cards.values()]
      .filter((entry) => entry.project === project)
      .sort((a, b) => a.order - b.order)
    if (cards.length === 0) {
      this.#stage.replaceChildren(element('p', 'stage-empty', 'No terminal is open yet.'))
      return
    }
    // Re-inserting a card blurs whatever has the keyboard inside it: the
    // stage is touched only when its cards or their order change.
    const wanted = cards.map((entry) => entry.card)
    if (
      wanted.length !== this.#stage.children.length ||
      wanted.some((card, at) => this.#stage.children[at] !== card)
    ) {
      this.#stage.replaceChildren(...wanted)
    }
    // The chief's window takes a whole column; the members' go two to a
    // column, and the last one left alone takes its column whole.
    const chiefs = new Set(
      ordered.filter((lane) => lane.participant.role === 'chief').map((l) => l.participant.handle),
    )
    const members = cards.filter((entry) => !chiefs.has(entry.handle))
    const alone = members.length % 2 === 1 ? members.at(-1) : null
    for (const entry of cards) {
      entry.card.dataset.tall = String(chiefs.has(entry.handle) || entry === alone)
    }
    const shown = cards.find((entry) => entry.handle === focused) ?? cards[0]
    for (const entry of cards) entry.card.dataset.focused = String(entry === shown)
    for (const entry of cards) {
      const [id, generation] = [entry.pane.id, entry.pane.generation]
      this.#registry.fit(id, generation)
    }
    // A redraw follows the board every few seconds: scrolling and focusing
    // on each one would drag the human back from a card they scrolled to.
    if (shown.key === this.#shownKey) return
    this.#shownKey = shown.key
    shown.card.scrollIntoView({ inline: 'nearest', block: 'nearest' })
    requestAnimationFrame(() =>
      this.#registry.get(shown.pane.id, shown.pane.generation)?.terminal?.focus(),
    )
  }

  /** A closed project's cards go: it has no terminals to read. */
  clear(project) {
    for (const [key, entry] of [...this.#cards]) {
      if (entry.project === project) this.#drop(key)
    }
  }

  /**
   * The human closed a participant's terminal: its card goes now, even while
   * the window is still on its way out, and does not come back for it. A new
   * terminal of the same participant is a new pane and shows as usual.
   */
  forget(project, handle) {
    for (const [key, entry] of this.#cards) {
      if (entry.project !== project || entry.handle !== handle) continue
      this.#dismissed.add(key)
      this.#drop(key)
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

  /**
   * A pane's card and emulator. Output may arrive before the board has drawn
   * the pane (or for a project not shown): its card waits, with no project,
   * until a render places it.
   */
  #card(pane, lane, order = Number.MAX_SAFE_INTEGER, project = null) {
    const key = paneKey(pane)
    let entry = this.#cards.get(key)
    if (entry === undefined) {
      const card = element('section', 'terminal-card')
      const head = element('header', 'terminal-head')
      const host = element('div', 'terminal-host')
      card.append(head, host)
      entry = {
        key,
        pane,
        card,
        head,
        host,
        handle: null,
        project: null,
        order: Number.MAX_SAFE_INTEGER,
      }
      this.#cards.set(key, entry)
      this.#registry.ensure(pane, host)
    }
    if (lane !== null) {
      const name = laneName(lane.participant)
      const lamp = element('span', 'lamp')
      lamp.dataset.state = lane.activity?.state ?? 'closed'
      lamp.setAttribute('aria-hidden', 'true')
      // Only a session's window closes by hand, as on its board row; the
      // chief's stays with the project.
      const session = lane.participant.member !== null
      const stop = element('button', 'quiet-button terminal-stop', 'Close')
      stop.type = 'button'
      stop.setAttribute('aria-label', `Close ${name}'s terminal`)
      stop.addEventListener('click', () => this.#onClose(lane.participant))
      entry.head.replaceChildren(
        lamp,
        element('span', 'terminal-name', name),
        element('span', 'terminal-meta', lane.participant.harness ?? ''),
        ...(session ? [stop] : []),
      )
      entry.card.setAttribute('aria-label', `${name}'s terminal`)
      entry.card.dataset.handle = lane.participant.handle
      entry.handle = lane.participant.handle
      entry.project = project
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
    : ({ chief: 'Chief of Staff' }[participant.handle] ?? `@${participant.handle}`)
