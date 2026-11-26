import { element } from '../dom.js'

/** How long a glide along the row takes, start to rest. */
const GLIDE_MS = 220
/** A swipe has ended once its wheel has been still this long (a trackpad's coast included). */
const SETTLE_MS = 140
/** Less than this sideways, a swipe was a slip of the hand: the row goes back where it was. */
const NUDGE_PX = 24

const still = () => globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true

/**
 * The person's way along the dock's row of terminals (T-2180). The row moves only when they move
 * it, and then a whole card at a time:
 * - **Sideways** (a trackpad's swipe, a tilted wheel, Shift and the wheel) is the row's over any
 *   card, never its program's: a terminal took a swipe for its scrollback, or a program that
 *   took the mouse or the alternate screen for its own keys. Up and down stay the terminal's.
 *   The row follows the hand and, once still, comes to rest at the next card on the way.
 * - **Ctrl+Tab and Ctrl+Shift+Tab** go to the next and the previous window, with the keyboard.
 * - **The map** in the dock's head names every window in the row's order, marks the part of the
 *   row in view and the window with the keyboard, and goes to the one chosen.
 * - **A click into a window the dock's edge cuts off** brings it whole into view: the person went
 *   to it. A drag that selects moves nothing.
 * - **Typing moves nothing.** The browser keeps the caret of the field a terminal types through
 *   in view: the field sits at the terminal's cursor, so a key put into it while the cursor was
 *   out of view scrolled the row there, and the window beside went out of view.
 * A card rests with its start at the row's padding, or the row at its end: where the page's
 * snap puts it, and where a terminal shown is brought.
 */
export class DockStrip {
  #stage
  #map
  /** Gives a card's window the keyboard; nothing for a card with no window. */
  #focus
  /** The cards in the row, in its order: each its card and the names its chip shows. */
  #cards = []
  #chips = new Map()
  /** The glide under way: its frame, or null. */
  #glide = null
  /** The swipe under way: where the row was when it began, how far it went, its timer. */
  #swipe = null
  /** Where the row was when a key or a text came into a terminal, until the frame after. */
  #held = null
  /** Where a press began in a terminal, to tell a click from a drag. */
  #press = null
  #drawing = 0

  constructor(stage, map, { focus }) {
    this.#stage = stage
    this.#map = map
    this.#focus = focus
    // Before any terminal's own: a capture on the row runs first, and a stop there keeps the
    // event from the terminal under it.
    stage.addEventListener('wheel', (event) => this.#wheel(event), {
      capture: true,
      passive: false,
    })
    for (const type of [
      'keydown',
      'beforeinput',
      'compositionstart',
      'compositionupdate',
      'paste',
    ]) {
      stage.addEventListener(type, () => this.#hold(), { capture: true })
    }
    stage.addEventListener('scroll', () => this.#scrolled(), { passive: true })
    stage.addEventListener('pointerdown', (event) => this.#pressed(event), { capture: true })
    stage.addEventListener('click', (event) => this.#clicked(event), { capture: true })
    document.addEventListener('keydown', (event) => this.#key(event), { capture: true })
    new ResizeObserver(() => this.#draw()).observe(stage)
  }

  /**
   * The cards now in the row, in its order: `{ card, name, label }`, the label being the short
   * one its chip shows.
   */
  update(cards) {
    this.#cards = cards
    if (this.#map === null) return
    const chips = cards.map(({ card, name, label }) => {
      let chip = this.#chips.get(card)
      if (chip === undefined) {
        chip = element('button', 'dock-chip')
        chip.type = 'button'
        chip.append(element('span', 'lamp'), element('span', 'dock-chip-name'))
        chip.addEventListener('click', () => {
          const entry = this.#cards.find((each) => each.card === card)
          if (entry !== undefined) this.go(entry)
        })
        this.#chips.set(card, chip)
      }
      chip.setAttribute('aria-label', `Go to ${name}'s terminal`)
      chip.dataset.name = name
      chip.lastElementChild.textContent = label
      return chip
    })
    for (const [card, chip] of this.#chips) {
      if (cards.some((entry) => entry.card === card)) continue
      this.#chips.delete(card)
      chip.remove()
    }
    // In place, as the row's cards: a chip taken out and put back would lose the keyboard.
    let next = this.#map.firstElementChild
    for (const chip of chips) {
      if (chip === next) next = next.nextElementSibling
      else this.#map.insertBefore(chip, next)
    }
    this.#map.hidden = chips.length < 2
    this.#draw()
  }

  /** A card shown by the person from its lane or a task is brought into view: the row stops. */
  halt() {
    this.#stop()
    this.#held = null
  }

  /** Goes to `entry`'s card: whole in view, its window with the keyboard. */
  go(entry) {
    this.#glideTo(this.#restFor(entry.card))
    this.#focus(entry.card)
  }

  /** Ctrl+Tab and Ctrl+Shift+Tab: the next window, or the one before; past either end, round. */
  #key(event) {
    if (event.key !== 'Tab' || !event.ctrlKey || event.altKey || event.metaKey) return
    if (event.target?.closest?.('dialog') || document.querySelector('dialog[open]')) return
    // The dock folded away, or nothing in it.
    if (this.#cards.length === 0 || this.#stage.offsetParent === null) return
    event.preventDefault()
    event.stopPropagation()
    const count = this.#cards.length
    const here = this.#current()
    this.go(this.#cards[(here + (event.shiftKey ? count - 1 : 1)) % count])
  }

  /** The card the keyboard is in; else the one most in view; else the first. */
  #current() {
    const holding = document.activeElement?.closest?.('.terminal-card')
    const index = this.#cards.findIndex((entry) => entry.card === holding)
    if (index >= 0) return index
    let best = 0
    let most = -1
    this.#cards.forEach((entry, at) => {
      const [from, to] = this.#seen(entry.card)
      if (to - from > most) [best, most] = [at, to - from]
    })
    return best
  }

  #wheel(event) {
    let across = event.deltaX
    let down = event.deltaY
    // Shift and a wheel that turns only one way: sideways, as every system has it (a Mac's
    // own already gives it sideways).
    if (event.shiftKey && across === 0) [across, down] = [down, 0]
    if (Math.abs(across) <= Math.abs(down)) return
    event.preventDefault()
    event.stopPropagation()
    const scale = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? this.#stage.clientWidth : 1
    if (this.#glide !== null) this.#stop()
    this.#held = null
    if (this.#swipe === null) this.#swipe = { from: this.#stage.scrollLeft, went: 0, timer: 0 }
    const swipe = this.#swipe
    this.#stage.dataset.moving = 'true'
    this.#stage.scrollLeft += across * scale
    swipe.went += across * scale
    clearTimeout(swipe.timer)
    swipe.timer = setTimeout(() => this.#settle(), SETTLE_MS)
  }

  /** A swipe has ended: to the next resting place on its way, or back for a nudge. */
  #settle() {
    const swipe = this.#swipe
    this.#swipe = null
    if (swipe === null) return
    const rests = this.#rests()
    const now = this.#stage.scrollLeft
    let to
    if (Math.abs(swipe.went) < NUDGE_PX) {
      to = rests.reduce((best, rest) =>
        Math.abs(rest - swipe.from) < Math.abs(best - swipe.from) ? rest : best,
      )
    } else if (swipe.went > 0) {
      to = rests.find((rest) => rest >= now - 1) ?? rests.at(-1)
    } else {
      to = rests.findLast((rest) => rest <= now + 1) ?? rests[0]
    }
    this.#glideTo(to)
  }

  /**
   * Where the row may rest: each card's start at the row's padding, the row's end for the last
   * ones, in order.
   */
  #rests() {
    const end = this.#end()
    const places = this.#cards.map(({ card }) =>
      Math.round(Math.min(end, Math.max(0, this.#left(card) - this.#padding()))),
    )
    return [...new Set([0, ...places, end])].sort((a, b) => a - b)
  }

  /** The resting place nearest the row's place now from which `card` is whole in view. */
  #restFor(card) {
    const now = this.#stage.scrollLeft
    const left = this.#left(card)
    const right = left + card.getBoundingClientRect().width
    const fits = this.#rests().filter(
      (rest) => left >= rest - 1 && right <= rest + this.#stage.clientWidth + 1,
    )
    if (fits.length === 0) return Math.min(this.#end(), Math.max(0, left - this.#padding()))
    return fits.reduce((best, rest) => (Math.abs(rest - now) < Math.abs(best - now) ? rest : best))
  }

  /** A card's left edge in the row's own measure, which scrolls. */
  #left(card) {
    return (
      card.getBoundingClientRect().left -
      this.#stage.getBoundingClientRect().left +
      this.#stage.scrollLeft
    )
  }

  #padding() {
    return Number.parseFloat(getComputedStyle(this.#stage).paddingLeft) || 0
  }

  #end() {
    return Math.max(0, this.#stage.scrollWidth - this.#stage.clientWidth)
  }

  #glideTo(to) {
    this.#stop()
    this.#held = null
    const stage = this.#stage
    const from = stage.scrollLeft
    if (Math.abs(to - from) < 0.5 || still()) {
      stage.scrollLeft = to
      this.#rest()
      return
    }
    stage.dataset.moving = 'true'
    const start = performance.now()
    const step = (time) => {
      const done = Math.min(1, (time - start) / GLIDE_MS)
      stage.scrollLeft = from + (to - from) * (1 - (1 - done) ** 3)
      if (done < 1) this.#glide = requestAnimationFrame(step)
      else this.#rest()
    }
    this.#glide = requestAnimationFrame(step)
  }

  /** The row at rest: the page's snap is on again, which leaves a resting place where it is. */
  #rest() {
    this.#glide = null
    delete this.#stage.dataset.moving
    this.#draw()
  }

  /** Whatever moves the row now stops: a glide, a swipe the person has not let go of. */
  #stop() {
    if (this.#glide !== null) cancelAnimationFrame(this.#glide)
    this.#glide = null
    if (this.#swipe !== null) clearTimeout(this.#swipe.timer)
    this.#swipe = null
    delete this.#stage.dataset.moving
  }

  /**
   * A key or a text is coming into a terminal: wherever the browser scrolls the row to keep that
   * terminal's caret in view, the row goes back. Until the frame after, when its scroll is told.
   */
  #hold() {
    if (this.#glide !== null || this.#swipe !== null) return
    // Keys come faster than frames: the place held is the one before the first of them, not
    // one the browser has scrolled to since and not yet told.
    const held = this.#held ?? { left: this.#stage.scrollLeft, keys: 0 }
    held.keys += 1
    const keys = held.keys
    this.#held = held
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (this.#held === held && held.keys === keys) this.#held = null
      }),
    )
  }

  #scrolled() {
    const held = this.#held
    if (held !== null && this.#glide === null && this.#swipe === null) {
      if (Math.abs(this.#stage.scrollLeft - held.left) > 0.5) this.#stage.scrollLeft = held.left
    }
    this.#draw()
  }

  #pressed(event) {
    const host = event.target?.closest?.('.terminal-host')
    this.#press = host ? { x: event.clientX, y: event.clientY, host } : null
  }

  #clicked(event) {
    const press = this.#press
    this.#press = null
    if (press === null || event.detail !== 1 || event.button !== 0) return
    if (event.target?.closest?.('.terminal-host') !== press.host) return
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) > 4) return
    const card = press.host.closest('.terminal-card')
    const [from, to] = this.#seen(card)
    if (from <= 0.001 && to >= 0.999) return
    this.#glideTo(this.#restFor(card))
  }

  /** The share of `card`'s width in view, from its left and to its right, each 0 to 1. */
  #seen(card) {
    const box = card.getBoundingClientRect()
    const room = this.#stage.getBoundingClientRect()
    const left = room.left + this.#stage.clientLeft
    const right = left + this.#stage.clientWidth
    const part = (x) => Math.min(1, Math.max(0, (x - box.left) / box.width))
    return box.width === 0 ? [0, 0] : [part(left), part(right)]
  }

  /** The map as the row is now, at most once a frame. */
  #draw() {
    if (this.#map === null || this.#drawing !== 0) return
    this.#drawing = requestAnimationFrame(() => {
      this.#drawing = 0
      const holding = document.activeElement?.closest?.('.terminal-card')
      for (const { card } of this.#cards) {
        const chip = this.#chips.get(card)
        if (chip === undefined) continue
        const [from, to] = this.#seen(card)
        const view = to - from < 0.005 ? 'none' : from <= 0.005 && to >= 0.995 ? 'whole' : 'part'
        chip.dataset.view = view
        chip.style.setProperty('--seen-from', from.toFixed(3))
        chip.style.setProperty('--seen-to', to.toFixed(3))
        chip.dataset.keys = String(card.dataset.focused === 'true')
        chip.dataset.typing = String(card === holding)
        chip.firstElementChild.dataset.state =
          card.querySelector('.terminal-head .lamp')?.dataset.state ?? 'closed'
        const where =
          view === 'whole'
            ? 'In view'
            : view === 'part'
              ? 'Partly in view'
              : to <= 0
                ? 'Out of view, to the right'
                : 'Out of view, to the left'
        chip.title = `${chip.dataset.name}: ${where.toLowerCase()}. Ctrl+Tab and Ctrl+Shift+Tab go from window to window.`
      }
    })
  }

  /** The map again: what a card says (its lamp, the window lit) changed with the board. */
  redraw() {
    this.#draw()
  }
}
