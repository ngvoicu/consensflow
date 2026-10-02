import { button, element, redraw } from '../dom.js'
import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { lamp, laneName, laneOrder } from './board.js'

/** A session in `#showing`: its project's id and its handle. */
const showingKey = (project, handle) => `${project}:${handle}`

/**
 * The live windows beside the board: a horizontal strip of terminals, the
 * chief's first, then the sessions' the human asked to see, scrolling
 * sideways. The chief's terminal is in the dock while its window lives; a
 * session's (a worker's, an advisor's, a reviewer's or an image designer's
 * task) stays out of it until the human shows it from its lane, and leaves
 * when they hide it. Only live windows: one that ends leaves with its card,
 * and its lane says it is closed and opens it again on its conversation.
 * While a window lives, in the dock or not, its emulator takes all its
 * output and keeps its scrollback, its size and the human's half-typed
 * input, across project switches too.
 */
export class TerminalsView {
  #stage
  #registry
  #link
  #cards = new Map()
  #onChange
  /** The human closing a live window from its card: the same as from its board row. */
  #onClose
  /** The card last brought into view: a redraw scrolls only when it changes. */
  #shownKey = null
  /**
   * The sessions whose terminals the human asked to see, by `showingKey`.
   * One stays in the dock while its window lives; the session's next window
   * waits to be shown again. The page keeps it, not the browser: a showing
   * belongs to one window's life, so a reload starts with the chief's
   * terminal alone, and Show terminal brings a session's back.
   */
  #showing = new Set()

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
    this.#link.output(message, (sent) => this.#emulator(sent))
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
   * Whether the dock may hold a window of `project`: one its board placed
   * here, or one whose output came before any board showed it.
   */
  holds(project) {
    return [...this.#cards.values()].some(
      (entry) => entry.project === project || entry.project === null,
    )
  }

  /**
   * What the boards just read say about the windows here, whichever project
   * is shown. A window that ended, or gave way to a newer one, goes with its
   * card: a last frame left in the strip still shows the agent's prompt and
   * reads as open beside a lane that says it is closed. Only its own
   * project's board says so (its lane has no window or another one, its
   * session is gone), or the project itself, closed or deleted. A window
   * whose output came first is placed once a board shows it. `open` holds
   * the ids of the projects open now.
   */
  reconcile(boards, open) {
    const lanes = boards.flatMap((board) =>
      board.lanes.map((lane) => ({ project: board.project.id, lane })),
    )
    for (const entry of [...this.#cards.values()]) {
      if (entry.project === null) {
        const shown = lanes.find(({ lane }) => lane.pane?.id === entry.pane.id)
        // No board shows it yet, or one read before it opened: it waits.
        if (shown === undefined || shown.lane.pane.generation < entry.pane.generation) continue
        if (shown.lane.pane.generation > entry.pane.generation) {
          this.#retire(entry.key)
          continue
        }
        entry.project = shown.project
        entry.handle = shown.lane.participant.handle
      }
      if (!open.has(entry.project)) {
        this.#retire(entry.key)
        continue
      }
      const board = boards.find((board) => board.project.id === entry.project)
      if (board === undefined) continue
      const lane = board.lanes.find((lane) => lane.participant.handle === entry.handle)
      if (lane === undefined || lane.pane === null || paneKey(lane.pane) !== entry.key) {
        this.#retire(entry.key)
      }
    }
  }

  /**
   * Keep a terminal for every lane of the project `board` is for that has a
   * live one (a closed project has none), in lane order; dock those the
   * human may see now, and bring `focused` into view if it is one of them.
   * The rest stay alive, off screen, with their scrollback: a session's not
   * shown, and another project's. Switching projects loses nothing.
   */
  render(board, { focused }) {
    const project = board?.project.id ?? null
    const ordered = board?.project.state === 'open' ? laneOrder(board.lanes) : []
    for (const [order, lane] of ordered.entries()) {
      // A window over for good never comes back, whatever a board says.
      if (lane.pane === null || this.#link.retired(paneKey(lane.pane))) continue
      this.#card(lane.pane, lane, order, project)
    }
    const live = [...this.#cards.values()].filter((entry) => entry.project === project)
    const cards = live.filter((entry) => this.#docked(entry)).sort((a, b) => a.order - b.order)
    if (cards.length === 0) {
      // Sessions at work while the chief's window is down (a switch of lead,
      // a launch that failed): their terminals are there to be shown.
      this.#stage.replaceChildren(
        element(
          'p',
          'stage-empty',
          live.length === 0
            ? 'No terminal is open yet.'
            : "Choose Show terminal on a session's row to see its terminal here.",
        ),
      )
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
    for (const { pane } of cards) this.#registry.fit(pane)
    // A redraw follows the board every few seconds: scrolling and focusing
    // on each one would drag the human back from a card they scrolled to.
    if (shown.key === this.#shownKey) return
    this.#shownKey = shown.key
    shown.card.scrollIntoView({ inline: 'nearest', block: 'nearest' })
    requestAnimationFrame(() => this.#registry.get(shown.pane)?.terminal?.focus())
  }

  /**
   * The human closed a participant's terminal: its card goes now, even while
   * the window is still on its way out, and does not come back for it. A new
   * terminal of the same participant is a new pane and shows as usual.
   */
  forget(project, handle) {
    for (const [key, entry] of [...this.#cards]) {
      if (entry.project === project && entry.handle === handle) this.#retire(key)
    }
    this.#onChange()
  }

  /** Whether the human asked to see a session's terminal: its lane offers to hide it. */
  shows(project, handle) {
    return this.#showing.has(showingKey(project, handle))
  }

  /**
   * The human asks to see a session's terminal: its card comes into the
   * dock with everything its window wrote since it opened. One asked for
   * before its window is up (Open terminal) comes in when it is.
   */
  show(project, handle) {
    this.#showing.add(showingKey(project, handle))
    this.#onChange()
  }

  /** The human hides a session's terminal: its card leaves the dock, and its window works on. */
  hide(project, handle) {
    this.#showing.delete(showingKey(project, handle))
    this.#onChange()
  }

  /** Whether a card is in the dock: a session's once the human shows it, any other while it lives. */
  #docked(entry) {
    return !entry.session || this.#showing.has(showingKey(entry.project, entry.handle))
  }

  /**
   * A window over for good: its card and emulator go, and its pane never
   * gets them again. Its session's next window waits to be shown.
   */
  #retire(key) {
    const entry = this.#cards.get(key)
    if (entry !== undefined) {
      entry.card.remove()
      this.#showing.delete(showingKey(entry.project, entry.handle))
    }
    this.#cards.delete(key)
    this.#link.retire(key)
  }

  /**
   * A pane's card and emulator. Output may arrive before any board has shown
   * the pane (or for a project not shown): its card waits, with no project,
   * until a board places it.
   */
  #card(pane, lane, order, project) {
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
        session: false,
        order: Number.MAX_SAFE_INTEGER,
      }
      this.#cards.set(key, entry)
      this.#registry.ensure(pane, host)
    }
    if (lane !== null) {
      const name = laneName(lane.participant)
      const { handle } = lane.participant
      // Only a session's terminal hides and its window closes by hand, as on
      // its board row; the chief's stays with the project. Hide is the one
      // that leaves the window at work.
      const session = lane.participant.member !== null
      redraw(entry.head, [
        lamp(lane.activity),
        element('span', 'terminal-name', name),
        element('span', 'terminal-meta', lane.participant.harness ?? ''),
        ...(session
          ? [
              button(
                'Hide',
                'quiet-button',
                () => this.hide(project, handle),
                `Hide ${name}'s terminal`,
              ),
              button(
                'Close',
                'quiet-button',
                () => this.#onClose(lane.participant),
                `Close ${name}'s terminal`,
              ),
            ]
          : []),
      ])
      entry.card.setAttribute('aria-label', `${name}'s terminal`)
      entry.card.dataset.handle = handle
      entry.handle = handle
      entry.project = project
      entry.session = session
      entry.order = order
    }
  }

  /**
   * The emulator a message from the pane host is for. Its pane is the
   * message's id and generation, not the message itself, which a card made
   * for it would keep, bytes and all, for as long as its window lives.
   */
  #emulator({ id, generation }) {
    const pane = { id, generation }
    this.#card(pane, null)
    return this.#registry.get(pane)
  }
}
