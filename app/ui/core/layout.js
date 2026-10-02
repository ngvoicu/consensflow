/**
 * How the page is laid out, kept in this browser: the projects and the
 * windows fold away to a rail, the board folds away for the windows, and the
 * divider between the board and the windows sets the board's width. A
 * per-viewer convenience: storage may be missing, so every touch is guarded.
 */

const $ = (selector) => document.querySelector(selector)
const foldKey = (name) => `cf.layout.${name}`
/** The board and the windows share one space: folding one brings the other back. */
const OPPOSITE = { board: 'dock', dock: 'board' }
const BOARD_WIDTH_KEY = foldKey('board-width')

function readFold(name) {
  try {
    return localStorage.getItem(foldKey(name)) === 'hidden' ? 'hidden' : 'shown'
  } catch {
    return 'shown'
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

  /** Draws the layout this browser kept; `onFold` runs once a fold has changed the room. */
  constructor({ onFold }) {
    for (const [name, , , toggle] of this.#folds) {
      toggle.addEventListener('click', () => {
        const next = readFold(name) === 'hidden' ? 'shown' : 'hidden'
        try {
          localStorage.setItem(foldKey(name), next)
          if (next === 'hidden' && OPPOSITE[name])
            localStorage.setItem(foldKey(OPPOSITE[name]), 'shown')
        } catch {
          // No storage: the fold still applies for this page.
        }
        this.#applyFolds()
        onFold()
      })
    }
    this.#applyFolds()
    try {
      const saved = Number(localStorage.getItem(BOARD_WIDTH_KEY))
      if (saved > 0) this.#setBoardWidth(saved)
    } catch {
      // No storage: the board keeps its default share.
    }
    this.#takeDivider()
  }

  /** A panel the page needs to show comes back unfolded. */
  unfold(name) {
    try {
      localStorage.setItem(foldKey(name), 'shown')
    } catch {
      // No storage: shown for this page.
    }
    this.#applyFolds()
  }

  #applyFolds() {
    for (const [name, host, attribute, toggle, noun] of this.#folds) {
      const hidden = readFold(name) === 'hidden'
      host.setAttribute(attribute, hidden ? 'hidden' : 'shown')
      toggle.setAttribute('aria-pressed', String(!hidden))
      toggle.setAttribute('aria-label', `${hidden ? 'Show' : 'Hide'} ${noun}`)
    }
  }

  /** The board keeps 280px and leaves the windows 300px beside the divider. */
  #setBoardWidth(pixels, { save = false } = {}) {
    const room = this.#main.getBoundingClientRect().width
    const width = Math.round(Math.min(Math.max(pixels, 280), Math.max(280, room - 318)))
    this.#main.style.setProperty('--board-width', `${width}px`)
    this.#divider.setAttribute('aria-valuenow', String(width))
    this.#divider.setAttribute('aria-valuemin', '280')
    this.#divider.setAttribute('aria-valuemax', String(Math.max(280, Math.round(room - 318))))
    if (!save) return
    try {
      localStorage.setItem(BOARD_WIDTH_KEY, String(width))
    } catch {
      // No storage: the width holds for this page.
    }
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
