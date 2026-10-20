/**
 * The menu of a project in the list on the left: Rename… and Description…,
 * each in a small dialog of its own, and Project settings… and Specs…, which
 * open the dialogs of the board's Project settings and Specs buttons for this
 * project (the page shows the project first, if the board shows another). Specs…
 * is offered whether or not the project has a specs folder: the viewer says so
 * where it has none. It opens on a
 * right-click, and from the keyboard on the project that has it (the
 * context-menu key, or Shift+F10). Escape closes it, and so does a click
 * anywhere else, and the keyboard goes back to the project.
 *
 * The name has the rules a name has when a project is opened (some text, at
 * most 100 characters); the description is one line of at most 200, and left
 * empty it is taken away. The daemon checks both, and a refusal is said in the
 * dialog, where the human typed, which keeps what they typed. The project's
 * folder never changes, and the description is the human's alone: no agent is
 * told it. A closed project's board is read-only, but its settings are not: its
 * menu offers Project settings… as an open project's does, for what it says of
 * the chief's specs and wake-up calls is what the chief starts with, which the
 * Resume does.
 *
 * The menu and the dialogs are outside the list, which is drawn again whenever
 * the board is, so the menu keeps only the project's id and finds its button
 * when the keyboard goes back to it. A menu that is open follows each drawing
 * (`follow`): it is for its project as the page has it now, and when that project
 * is gone it shuts, and the keyboard goes to the project that took its place.
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
  /**
   * What the dialog is open for, none while it is shut: the project as the page had it
   * when it opened, and whether a save of it is on its way. Each opening is a new one,
   * so what an earlier opening still waits for (a save asked and then cancelled) is not
   * this one's, and its answer touches nothing of it (`#save`).
   */
  #opening = null

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
      const opening = this.#opening
      this.#opening = null
      if (opening !== null) back(opening.project.id)
    })
  }

  /** Opens on what `project` has now, the text selected to be typed over. */
  open(project) {
    this.#opening = { project, saving: false }
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

  /**
   * Saves what the field says; a refusal leaves the dialog open on what was typed.
   *
   * The answer is of the opening that asked. Cancel does not take a request back, so
   * the human may have left that opening, and opened the dialog again (on this project
   * or another), before it comes: then it closes nothing and says nothing in the editor
   * that is open now. A change that went through is still told, on the status line, as
   * it is on the list; a refusal of an editor that is gone has no one to be said to.
   */
  async #save() {
    const opening = this.#opening
    if (opening === null || opening.saving) return
    opening.saving = true
    const { project } = opening
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
      opening.saving = false
    }
    const current = opening === this.#opening
    if (refused !== null) {
      if (current) this.#say(refused)
      return
    }
    if (current) this.#dialog.close()
    this.#page.note(this.#field.said(project, answered))
  }
}

export class ProjectMenu {
  #list
  #menu
  #items
  #page
  #dialogs
  #fallback
  /** The project the menu is open for, by id; null while it is shut. */
  #open = null
  /** Where that project's card was in the list when it was last drawn: where the keyboard goes if it is gone. */
  #at = 0
  /**
   * The dialogs the page opens for a project (Project settings, Specs), by the
   * item that asks for each: the dialog, how the page opens it, and the project
   * the menu asked it for, until it closes.
   */
  #opened

  /**
   * `elements` are the list of projects (each of its items holds its id in
   * `data-project` and its button in `.project-select`), the menu, the two
   * dialogs of a line of text, the dialogs of Project settings and of Specs, and
   * `fallback`, the control the keyboard goes to when the project of an open
   * menu is gone and the list has no other. `page.project(id)` is the project as
   * the page has it now; `daemon`, `act` and `note` are the page's, as the
   * dialogs use them; `page.openSettings(id)` and `page.openSpecs(id)` show the
   * project if the board shows another, and open Project settings or the specs
   * viewer on it.
   */
  constructor({ list, menu, rename, describe, settings, specs, fallback }, page) {
    this.#list = list
    this.#menu = menu
    this.#fallback = fallback
    this.#items = [...menu.querySelectorAll('[role="menuitem"]')]
    this.#page = page
    const back = (id) => this.#focus(id)
    this.#dialogs = {
      rename: new FieldDialog(rename, FIELDS.rename, page, back),
      describe: new FieldDialog(describe, FIELDS.describe, page, back),
    }
    this.#opened = {
      settings: { dialog: settings, open: (id) => page.openSettings(id), for: null },
      specs: { dialog: specs, open: (id) => page.openSpecs(id), for: null },
    }
    // The keyboard goes back to the project whichever way its dialog is left.
    for (const opened of Object.values(this.#opened)) {
      opened.dialog.addEventListener('close', () => {
        const project = opened.for
        opened.for = null
        if (project !== null) back(project)
      })
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
    this.#adopt(project)
    this.#menu.hidden = false
    const { width, height } = this.#menu.getBoundingClientRect()
    const within = (at, size, room) => Math.max(EDGE, Math.min(at, room - size - EDGE))
    this.#menu.style.left = `${within(x, width, window.innerWidth)}px`
    this.#menu.style.top = `${within(y, height, window.innerHeight)}px`
    this.#items[0].focus()
  }

  /**
   * The page has drawn its projects again. An open menu is for its project as the
   * page has it now, so it takes the project's name; and if the project is gone (deleted
   * from elsewhere, say) the menu shuts.
   */
  follow() {
    const id = this.#open
    if (id === null) return
    const project = this.#page.project(id)
    if (project === undefined) {
      this.#lose()
      return
    }
    this.#adopt(project)
  }

  /**
   * The open menu is for `project` as the page has it: it takes its name, and notes where its
   * card is.
   */
  #adopt(project) {
    this.#menu.setAttribute('aria-label', project.name)
    this.#at = this.#placeOf(project.id)
  }

  /**
   * The project of the open menu is gone, and so is the button the keyboard would go
   * back to. The menu shuts and, if the keyboard was in it, goes to the project that
   * took the place of the one that went (the last one, if it was the last), and where
   * there is none to the `fallback`: the keyboard was on a menu that is no more, and
   * would be left on nothing. If it was elsewhere it stays there.
   */
  #lose() {
    const focused = document.activeElement
    const held = focused === null || focused === document.body || this.#menu.contains(focused)
    this.#hide(BACK.nowhere)
    if (!held) return
    const cards = [...this.#list.querySelectorAll('li.project')]
    const near = cards[Math.min(this.#at, cards.length - 1)]?.querySelector('.project-select')
    const target = near ?? this.#fallback
    target.focus()
  }

  /** Where the card of the project `id` is among those of the list: its place, as it is drawn now. */
  #placeOf(id) {
    return [...this.#list.querySelectorAll('li.project')].findIndex(
      (card) => card.dataset.project === String(id),
    )
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
    if (project === undefined) return
    if (action in this.#opened) void this.#openFor(this.#opened[action], project.id)
    else this.#dialogs[action].open(project)
  }

  /**
   * Project settings or the specs viewer on a project: the page shows it first, if the board
   * shows another, and opens the dialog, which gives the keyboard back to the project as it
   * closes. A dialog the page could not open (the project is gone, or its board could not be
   * read) leaves nothing of the ask for the next one.
   */
  async #openFor(opened, id) {
    opened.for = id
    await opened.open(id)
    if (!opened.dialog.open) opened.for = null
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
