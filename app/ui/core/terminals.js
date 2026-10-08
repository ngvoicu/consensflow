import { button, element, iconButton, redraw } from '../dom.js'
import { EmulatorRegistry, paneKey } from '../term.js'
import { TerminalLink } from '../terminal-link.js'
import { ICONS, identity, lamp, laneName, laneOrder, laneStatus, tryAgain } from './board.js'
import { consoleText } from './console-text.js'

/** The windows that read key presses on Windows, where the console drops a non-ASCII mark. */
const KEY_READERS = new Set(['devin', 'codex'])
const WINDOWS = /Windows/.test(globalThis.navigator?.userAgent ?? '')

/** A session in `#showing`: its project's id and its handle. */
const showingKey = (project, handle) => `${project}:${handle}`

/**
 * What the chief is doing, then what it runs on: the line under its name on
 * its card; out of quota, with the human's way to try it again now.
 */
function about(lane, board, agent, now, onTryAgain) {
  const [state, text] = laneStatus(lane, board, now)
  const status = element('span', 'terminal-status', text)
  status.dataset.state = state
  const line = element('div', 'terminal-about')
  line.append(status, element('span', 'terminal-meta', identity(lane.participant, agent)))
  if (state === 'out') line.append(tryAgain(lane.participant, () => onTryAgain(lane.participant)))
  return line
}

/**
 * The live windows beside the board: one row of terminals, scrolling
 * sideways, each as tall as the dock (how wide is the page's: the chief's
 * the whole dock beside the board, two-thirds of it with the board folded).
 * The chief's comes first, then the sessions' the human asked to see, in the
 * order they came into the dock. A terminal that comes in opens at the right end, scrolled
 * into view and with the keyboard; no terminal already open is resized or
 * moved by it, and one that leaves closes its gap, the cards to its right
 * moving left. The chief's card is always in the dock: the board has a row
 * for the chief only while a task is on it, so its card says what the chief
 * is doing and runs on, and switches it. While the chief's window is down
 * (starting, being switched, its agent gone) its card is there all the
 * same, with no terminal. A session's terminal (a worker's, an advisor's, a
 * reviewer's or an image designer's task) stays out of the dock until the
 * human shows it from its lane, and leaves when they hide it, from its lane
 * or its card, which also lets its window go once nothing holds it open.
 * Only live windows: one that ends leaves with its card, and its lane says
 * it is closed and opens it again on its conversation. While a window
 * lives, in the dock or not, its emulator takes all its output and keeps its
 * scrollback, its size and the human's half-typed input, across project
 * switches too.
 */
export class TerminalsView {
  #stage
  #registry
  #link
  #cards = new Map()
  #onChange
  /** The human asking to switch the chief, from its card. */
  #onSwitchChief
  /** The human trying the chief again before its quota resets, from its card. */
  #onTryAgain
  /** The human hiding a session's terminal, from its card: the page takes the card out and tells the daemon. */
  #onHideTerminal
  /** The chief's card while its window is down, made the first time it is: its head alone. */
  #windowless = null
  /** The card last brought into view: a redraw scrolls only when it changes. */
  #shownKey = null
  /** The number the next card to come into the dock takes: the dock keeps its cards in that order. */
  #arrivals = 0
  /**
   * The sessions whose terminals the human asked to see, by `showingKey`.
   * One stays in the dock while its window lives; the session's next window
   * waits to be shown again. The page keeps it, not the browser: a showing
   * belongs to one window's life, so a reload starts with the chief's
   * terminal alone, and Show terminal brings a session's back.
   */
  #showing = new Set()

  constructor(
    stage,
    {
      invoke,
      report,
      createEmulator,
      onChange = () => {},
      onSwitchChief = () => {},
      onTryAgain = () => {},
      onHideTerminal = () => {},
    },
  ) {
    this.#stage = stage
    this.#onChange = onChange
    this.#onSwitchChief = onSwitchChief
    this.#onTryAgain = onTryAgain
    this.#onHideTerminal = onHideTerminal
    this.#registry = new EmulatorRegistry({
      ...(createEmulator ? { createEmulator } : {}),
      onData: (pane, data) => void this.#link.input(pane, this.#typed(pane, data)),
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
    return this.#link.input(pane, this.#typed(pane, data))
  }

  /**
   * What the human typed, as the window can take it: on Windows a Devin or
   * Codex window reads key presses, and the console drops every non-ASCII
   * mark among them, so there those go in ASCII, as ConsensFlow's own
   * messages to Devin do (console-text.js).
   */
  #typed(pane, data) {
    const harness = this.#cards.get(paneKey(pane))?.harness
    return WINDOWS && KEY_READERS.has(harness) ? consoleText(data) : data
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
   * live one (a closed project has none); dock those the human may see now,
   * the chief's card first and the others in the order they came into the
   * dock, and bring `focused` into view if it is one of them. A card in the
   * dock keeps its place and its size: one that comes in goes after the
   * others (those that come together, in lane order) and one that leaves
   * closes its gap. The rest stay alive, off screen, with their scrollback:
   * a session's not shown, and another project's. Switching projects loses
   * nothing. `agents` are the saved agents, which say what the chief runs
   * on.
   */
  render(board, { focused, agents = [] }) {
    const project = board?.project.id ?? null
    const ordered = board?.project.state === 'open' ? laneOrder(board.lanes) : []
    const agentOf = (lane) => agents.find((agent) => agent.name === lane.participant.agent)
    for (const lane of ordered) {
      // A window over for good never comes back, whatever a board says.
      if (lane.pane === null || this.#link.retired(paneKey(lane.pane))) continue
      const entry = this.#card(lane.pane, { lane, board, agent: agentOf(lane) })
      // The next number puts a card that comes in at the right end; one
      // that leaves gives its number up, and takes a new one if it comes back.
      if (!this.#docked(entry)) entry.arrival = null
      else if (entry.arrival === null) entry.arrival = this.#arrivals++
    }
    const live = [...this.#cards.values()].filter((entry) => entry.project === project)
    const chief = ordered.find((lane) => lane.participant.role === 'chief')
    const rank = (entry) => (entry.handle === chief?.participant.handle ? 0 : 1)
    const cards = live
      .filter((entry) => this.#docked(entry))
      .sort((a, b) => rank(a) - rank(b) || a.arrival - b.arrival)
    // The chief's window down, its card stays: switching the chief may be the way on.
    const windowless =
      chief === undefined || live.some((entry) => entry.handle === chief.participant.handle)
        ? []
        : [this.#windowlessCard(chief, board, agentOf(chief))]
    const wanted = [...windowless, ...cards.map((entry) => entry.card)]
    if (wanted.length === 0) {
      this.#stage.replaceChildren(element('p', 'stage-empty', 'No terminal is open yet.'))
      return
    }
    this.#place(wanted)
    // With no terminal in the dock, there is none to bring into view.
    if (cards.length === 0) return
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
   * The stage's cards, `wanted` in order, touched only where they change: a
   * card that left is taken out, one that came in is put in its place, and
   * the cards already there stay as they are. A card taken out and put back
   * blurs the terminal with the keyboard and reflows the row.
   */
  #place(wanted) {
    for (const child of [...this.#stage.children]) {
      if (!wanted.includes(child)) child.remove()
    }
    let next = this.#stage.firstElementChild
    for (const card of wanted) {
      if (card === next) next = next.nextElementSibling
      else this.#stage.insertBefore(card, next)
    }
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
   * before its window is up (a closed one shown) comes in when it is. One
   * already in the dock keeps its place and comes into view again, though
   * it was the last brought into view and the human has scrolled away.
   */
  show(project, handle) {
    this.#showing.add(showingKey(project, handle))
    const entry = [...this.#cards.values()].find(
      (other) => other.project === project && other.handle === handle,
    )
    if (entry?.key === this.#shownKey) this.#shownKey = null
    this.#onChange()
  }

  /**
   * A session's terminal is hidden: its card leaves the dock. Telling the
   * daemon, which closes the window once it is free, is the page's
   * (`onHideTerminal`), for a card's Hide as for a lane's.
   */
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
   * until a board places it: `placed` is its lane, the board, and the saved
   * agent it runs on. `arrival` is the number it took coming into the dock,
   * null while it is not in it.
   */
  #card(pane, placed = null) {
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
        harness: null,
        participant: null,
        project: null,
        session: false,
        arrival: null,
      }
      this.#cards.set(key, entry)
      this.#registry.ensure(pane, host)
    }
    if (placed !== null) {
      const { lane, board, agent } = placed
      this.#head(entry, lane, board, agent)
      entry.handle = lane.participant.handle
      entry.harness = lane.participant.harness
      entry.project = board.project.id
      entry.session = lane.participant.member !== null
    }
    return entry
  }

  /** The chief's card while its window is down: its head, and no terminal. */
  #windowlessCard(lane, board, agent) {
    if (this.#windowless === null) {
      const card = element('section', 'terminal-card')
      const head = element('header', 'terminal-head')
      card.append(head)
      this.#windowless = { card, head, participant: null }
    }
    this.#head(this.#windowless, lane, board, agent)
    return this.#windowless.card
  }

  /**
   * A card's head: whose window it is, its lamp, and what is done with it
   * there. The chief's says what the chief is doing and runs on, and switches
   * it. Only a session's terminal hides, as on its board row, and the daemon
   * closes its window once nothing holds it open; deleting the session is its
   * row's alone. Only an open project's cards are drawn: nothing on a closed
   * one acts.
   */
  #head(entry, lane, board, agent) {
    const { participant } = lane
    const name = laneName(participant)
    const now = Date.now()
    // A button kept across redraws acts on the participant as it is now.
    entry.participant = participant
    const tools =
      participant.role === 'chief'
        ? [
            button(
              'Switch chief',
              'quiet-button',
              () => this.#onSwitchChief(entry.participant),
              'Switch the chief to another agent',
            ),
            about(lane, board, agent, now, this.#onTryAgain),
          ]
        : [
            element('span', 'terminal-meta', participant.harness ?? ''),
            ...(participant.member === null
              ? []
              : [
                  iconButton(
                    ICONS.hide,
                    'Hide terminal',
                    () => this.#onHideTerminal(entry.participant),
                    `Hide ${name}'s terminal`,
                  ),
                ]),
          ]
    redraw(entry.head, [lamp(lane, now), element('span', 'terminal-name', name), ...tools])
    entry.card.setAttribute('aria-label', `${name}'s terminal`)
    entry.card.dataset.handle = participant.handle
    entry.card.dataset.role = participant.role
  }

  /**
   * The emulator a message from the pane host is for. Its pane is the
   * message's id and generation, not the message itself, which a card made
   * for it would keep, bytes and all, for as long as its window lives.
   */
  #emulator({ id, generation }) {
    const pane = { id, generation }
    this.#card(pane)
    return this.#registry.get(pane)
  }
}
