/**
 * The menu of a project in the list on the left: Rename… and Description…,
 * each in a small dialog of its own. It opens on a right-click, and from the
 * keyboard on the project that has it (the context-menu key, or Shift+F10).
 * Escape closes it, and so does a click anywhere else, and the keyboard goes
 * back to the project.
 *
 * The name has the rules a name has when a project is opened (some text, at
 * most 100 characters); the description is one line of at most 200, and left
 * empty it is taken away. The daemon checks both, and a refusal is said in the
 * dialog, where the human typed, which keeps what they typed. The project's
 * folder never changes, and the description is the human's alone: no agent is
 * told it.
 *
 * The menu and the dialogs are outside the list, which is drawn again whenever
 * the board is, so the menu keeps only the project's id and finds its button
 * when the keyboard goes back to it.
 */

/** The context-menu key, and Shift+F10 where a keyboard has none. */
const isMenuKey = (event) => event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)
/** The room the menu keeps between itself and the window's edge. */
const EDGE = 8

/**
 * Where the keyboard goes when the menu shuts: to the project's button at
 * once (Escape, Tab); to it unless whatever shut the menu put it somewhere
 * (a click on another control keeps its own focus); or nowhere, for the
 * dialog an item opens.
 */
const BACK = { now: 'now', unlessMoved: 'unless-moved', nowhere: 'nowhere' }

/** What each dialog edits: the text it starts with, what it asks of the daemon, what it says once saved. */
const FIELDS = {
  rename: {
    start: (project) => project.name,
    ask: (project, text) => ['project.rename', { project: project.id, name: text.trim() }],
    said: (before, after) =>
      before.name === after.name
        ? `${after.name} keeps its name.`
        : `${before.name} is now ${after.name}.`,
  },
  describe: {
    start: (project) => project.description ?? '',
    ask: (project, text) => ['project.describe', { project: project.id, description: text.trim() }],
    said: (_, after) =>
      after.description === undefined
        ? `${after.name} has no description.`
        : `The description of ${after.name} is saved.`,
  },
}

/** A dialog of one line of text for a project: Enter saves it, Escape or Cancel leaves it as it was. */
class FieldDialog {
  #dialog
  #input
  #refusal
  #field
  #page
  /** The project it was opened for, as the page had it then. */
  #project = null
  #saving = false

  /**
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws the
   * page, `page.note` tells the human what came of it; `back(id)` gives the
   * keyboard back to a project once the dialog is closed, however it was.
   */
  constructor(dialog, field, page, back) {
    this.#dialog = dialog
    this.#input = dialog.querySelector('input')
    this.#refusal = dialog.querySelector('.dialog-refusal')
    this.#field = field
    this.#page = page
    dialog.querySelector('form').addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#save()
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
    dialog.addEventListener('close', () => {
      if (this.#project !== null) back(this.#project.id)
      this.#project = null
    })
  }

  /** Opens on what `project` has now, the text selected to be typed over. */
  open(project) {
    this.#project = project
    this.#input.value = this.#field.start(project)
    this.#say('')
    this.#dialog.showModal()
    this.#input.select()
  }

  /** What is said under the field: the daemon's refusal, until the next try or the next opening. */
  #say(words) {
    this.#refusal.textContent = words
    this.#refusal.hidden = words === ''
  }

  /** Saves what the field says; a refusal leaves the dialog open on what was typed. */
  async #save() {
    const project = this.#project
    if (project === null || this.#saving) return
    this.#saving = true
    const [operation, body] = this.#field.ask(project, this.#input.value)
    let answered = null
    let refused = null
    try {
      await this.#page.act(async () => {
        try {
          answered = (await this.#page.daemon(operation, body)).project
        } catch (cause) {
          refused = cause instanceof Error ? cause.message : String(cause)
        }
      })
    } finally {
      this.#saving = false
    }
    if (refused !== null) {
      this.#say(refused)
      return
    }
    this.#dialog.close()
    this.#page.note(this.#field.said(project, answered))
  }
}

export class ProjectMenu {
  #list
  #menu
  #items
  #page
  #dialogs
  /** The project the menu is open for, by id; null while it is shut. */
  #open = null

  /**
   * `elements` are the list of projects (each of its items holds its id in
   * `data-project` and its button in `.project-select`), the menu and the two
   * dialogs. `page.project(id)` is the project as the page has it now; `daemon`,
   * `act` and `note` are the page's, as the dialogs use them.
   */
  constructor({ list, menu, rename, describe }, page) {
    this.#list = list
    this.#menu = menu
    this.#items = [...menu.querySelectorAll('[role="menuitem"]')]
    this.#page = page
    const back = (id) => this.#focus(id)
    this.#dialogs = {
      rename: new FieldDialog(rename, FIELDS.rename, page, back),
      describe: new FieldDialog(describe, FIELDS.describe, page, back),
    }
    for (const item of this.#items) {
      item.addEventListener('click', () => this.#choose(item.dataset.action))
    }
    // One listener each for the whole list, which stays while its items are drawn again.
    list.addEventListener('contextmenu', (event) => {
      const card = event.target.closest('li.project')
      if (card === null) return
      event.preventDefault()
      this.#show(card, { x: event.clientX, y: event.clientY })
    })
    list.addEventListener('keydown', (event) => {
      const card = isMenuKey(event) ? event.target.closest('li.project') : null
      if (card === null) return
      event.preventDefault()
      const { left, bottom } = card.querySelector('.project-select').getBoundingClientRect()
      this.#show(card, { x: left, y: bottom })
    })
    menu.addEventListener('keydown', (event) => this.#key(event))
    // The menu is no place for the browser's own.
    menu.addEventListener('contextmenu', (event) => event.preventDefault())
    // Anywhere else shuts it: a click, Escape, the window moving on or the list scrolling.
    document.addEventListener('pointerdown', (event) => this.#outside(event), true)
    document.addEventListener(
      'keydown',
      (event) => {
        if (this.#open === null || event.key !== 'Escape') return
        event.preventDefault()
        this.#hide(BACK.now)
      },
      true,
    )
    for (const [target, name] of [
      [window, 'blur'],
      [window, 'resize'],
      [list.closest('.sidebar') ?? list, 'scroll'],
    ]) {
      target.addEventListener(name, () => this.#hide(BACK.unlessMoved))
    }
  }

  /** Opens the menu for the project of `card` at `at`, kept inside the window, on its first item. */
  #show(card, { x, y }) {
    const id = Number(card.dataset.project)
    const project = this.#page.project(id)
    if (project === undefined) return
    this.#open = id
    this.#menu.setAttribute('aria-label', project.name)
    this.#menu.hidden = false
    const { width, height } = this.#menu.getBoundingClientRect()
    const within = (at, size, room) => Math.max(EDGE, Math.min(at, room - size - EDGE))
    this.#menu.style.left = `${within(x, width, window.innerWidth)}px`
    this.#menu.style.top = `${within(y, height, window.innerHeight)}px`
    this.#items[0].focus()
  }

  /** The arrow keys move between the items, and Tab leaves the menu, back to the project. */
  #key(event) {
    const last = this.#items.length - 1
    const at = this.#items.indexOf(document.activeElement)
    const to = {
      ArrowDown: at >= last ? 0 : at + 1,
      ArrowUp: at <= 0 ? last : at - 1,
      Home: 0,
      End: last,
    }[event.key]
    if (to !== undefined) {
      event.preventDefault()
      this.#items[to].focus()
    } else if (event.key === 'Tab') {
      event.preventDefault()
      this.#hide(BACK.now)
    } else if (isMenuKey(event)) {
      event.preventDefault()
    }
  }

  /** A press anywhere that is not the menu shuts it, whatever the press then does. */
  #outside(event) {
    if (this.#open !== null && !this.#menu.contains(event.target)) this.#hide(BACK.unlessMoved)
  }

  /** What an item says: the dialog for it, on the project as the page has it now. */
  #choose(action) {
    const project = this.#page.project(this.#open)
    this.#hide(BACK.nowhere)
    if (project !== undefined) this.#dialogs[action].open(project)
  }

  /** Shuts the menu, and gives the keyboard where `back` says. */
  #hide(back) {
    const id = this.#open
    if (id === null) return
    this.#open = null
    this.#menu.hidden = true
    if (back === BACK.now) {
      this.#focus(id)
    } else if (back === BACK.unlessMoved) {
      // Once the click has had its way with the focus: where it left it nowhere, or on the
      // menu that has just gone, the project has it; not if another menu opened meanwhile
      // (a right-click on another project), and not from a control the click went to.
      setTimeout(() => {
        const focused = document.activeElement
        const nowhere =
          focused === null || focused === document.body || this.#menu.contains(focused)
        if (nowhere && this.#open === null) this.#focus(id)
      })
    }
  }

  /** The keyboard to the project's button, looked for now: the list may have been drawn again. */
  #focus(id) {
    this.#list.querySelector(`li.project[data-project="${id}"] .project-select`)?.focus()
  }
}
