/**
 * How the page is laid out, kept in this browser: the projects and the
 * windows fold away to a rail, the board folds away for the windows, and the
 * divider between the board and the windows sets how they share the width.
 * The windows' share is of the whole window less the divider, half to start
 * with: the projects' panel and the board have the rest, so opening or
 * folding the projects gives the board the room and resizes no terminal. A
 * per-viewer convenience: storage may be missing, so every touch is guarded.
 */

const $ = (selector) => document.querySelector(selector)
/** The divider's own width between the board and the windows. */
const DIVIDER = 18
/**
 * Where this browser keeps the windows' share of the window. What older
 * builds kept was a share of another room, and is left alone: the windows
 * start at half.
 */
const SHARE = 'dock-share-of-window'
const storageKey = (name) => `cf.layout.${name}`
/** The board and the windows share one space: folding one brings the other back. */
const OPPOSITE = { board: 'dock', dock: 'board' }

/** What this browser kept under `name`, or null: nothing kept, or no storage. */
function kept(name) {
  try {
    return localStorage.getItem(storageKey(name))
  } catch {
    return null
  }
}

/** Keeps `value` under `name` in this browser; with no storage it holds for this page alone. */
function keep(name, value) {
  try {
    localStorage.setItem(storageKey(name), value)
  } catch {
    // No storage: the layout is the page's own until it is reloaded.
  }
}

export class Layout {
  #main = $('.main')
  #divider = $('#board-resize')
  /** Each fold: its name, the element that carries it, the attribute, its button, what it folds. */
  #folds = [
    ['projects', $('.shell'), 'data-projects', $('#toggle-projects'), 'projects'],
    ['dock', this.#main, 'data-dock', $('#toggle-dock'), 'terminals'],
    ['board', this.#main, 'data-board', $('#toggle-board'), 'board'],
  ]
  /** The panels folded now: this browser's to start with, then the page's, kept as they change. */
  #hidden = new Set(this.#folds.map(([name]) => name).filter((name) => kept(name) === 'hidden'))

  /** Draws the layout this browser kept; `onFold` runs once a fold has changed the room. */
  constructor({ onFold }) {
    for (const [name, , , toggle] of this.#folds) {
      toggle.addEventListener('click', () => {
        if (this.#hidden.has(name)) {
          this.#show(name)
        } else {
          this.#hidden.add(name)
          keep(name, 'hidden')
          if (OPPOSITE[name]) this.#show(OPPOSITE[name])
        }
        this.#applyFolds()
        this.#showDivider()
        onFold()
      })
    }
    this.#applyFolds()
    this.#keepDock()
    this.#takeDivider()
    // A window resized widens or narrows both, and the divider says where it stands now.
    window.addEventListener('resize', () => this.#showDivider())
  }

  /** The windows' share of the window, as this browser kept it; else the page's own, half. */
  #keepDock() {
    const share = Number(kept(SHARE))
    if (share > 0 && share < 1) this.#main.style.setProperty('--dock-share', String(share))
    this.#showDivider()
  }

  /** A panel the page needs to show comes back unfolded. */
  unfold(name) {
    this.#show(name)
    this.#applyFolds()
  }

  #show(name) {
    this.#hidden.delete(name)
    keep(name, 'shown')
  }

  #applyFolds() {
    for (const [name, host, attribute, toggle, noun] of this.#folds) {
      const hidden = this.#hidden.has(name)
      host.setAttribute(attribute, hidden ? 'hidden' : 'shown')
      toggle.setAttribute('aria-pressed', String(!hidden))
      toggle.setAttribute('aria-label', `${hidden ? 'Show' : 'Hide'} ${noun}`)
    }
  }

  /**
   * The board at `pixels` wide, the windows given the rest: the board keeps
   * 280px and leaves the windows 300px beside the divider. What is kept is
   * the windows' share of the whole window less the divider, which opening
   * or folding the projects leaves as it is.
   */
  #setBoardWidth(pixels, { save = false } = {}) {
    const room = this.#main.getBoundingClientRect().width
    const board = Math.round(Math.min(Math.max(pixels, 280), Math.max(280, room - 318)))
    const share = Math.min(Math.max((room - DIVIDER - board) / (window.innerWidth - DIVIDER), 0), 1)
    this.#main.style.setProperty('--dock-share', String(share))
    this.#showDivider()
    if (save) keep(SHARE, String(share))
  }

  /** The divider says where it stands: the board's width, between 280px and what leaves the windows 300px. */
  #showDivider() {
    const room = this.#main.getBoundingClientRect().width
    const board = Math.round($('#board').getBoundingClientRect().width)
    this.#divider.setAttribute('aria-valuenow', String(board))
    this.#divider.setAttribute('aria-valuemin', '280')
    this.#divider.setAttribute('aria-valuemax', String(Math.max(280, Math.round(room - 318))))
  }

  /** The divider, dragged or moved with the arrow keys, sets the board's width. */
  #takeDivider() {
    const divider = this.#divider
    const main = this.#main
    divider.addEventListener('pointerdown', (event) => {
      event.preventDefault()
      divider.setPointerCapture(event.pointerId)
      main.dataset.resizing = 'true'
      const left = $('#board').getBoundingClientRect().left
      const move = (moved) => this.#setBoardWidth(moved.clientX - left)
      const done = (ended) => {
        divider.removeEventListener('pointermove', move)
        divider.removeEventListener('pointerup', done)
        divider.removeEventListener('pointercancel', done)
        delete main.dataset.resizing
        this.#setBoardWidth(ended.clientX - left, { save: true })
      }
      divider.addEventListener('pointermove', move)
      divider.addEventListener('pointerup', done)
      divider.addEventListener('pointercancel', done)
    })
    divider.addEventListener('keydown', (event) => {
      const step = { ArrowLeft: -32, ArrowRight: 32 }[event.key]
      if (step === undefined) return
      event.preventDefault()
      this.#setBoardWidth($('#board').getBoundingClientRect().width + step, { save: true })
    })
  }
}
