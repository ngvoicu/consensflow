import { element, redraw } from '../dom.js'

/**
 * Settings, Specs: what the app says of specs for every project that does not
 * say otherwise, and the Specmint it brings. Whether a chief plans big work as
 * specs, and the folder and the variant a project uses when it names none, are
 * the daemon's to keep (`specs.set`, in the agents file beside the human's other
 * choices) and each saves as it changes: the box and the variant at once, the
 * folder as the human leaves it or presses Enter. The daemon checks the folder as
 * a project's own is checked (inside the project, never `..`) and says why it
 * refuses one here, with what the human typed left where it is.
 *
 * The saves go one at a time. The app asks the daemon each on a worker of its
 * own, so two asked together may be kept, and answered, in either order: a save
 * waits for the one on its way, and then asks with the control as it is by then.
 * A control the human changed holds their word until a save of its own keeps it:
 * the answer of another control's save leaves it as it is, and a folder the daemon
 * refused stays as typed, with the refusal, until one is kept or the dialog opens
 * again.
 *
 * Below, the Specmint the build bundles: its version and each variant's name,
 * version, commit and what it is, as the daemon read them from the bundled files
 * (a version or a commit they do not say is unknown); none of it is written here,
 * so a Specmint that is vendored again shows itself. A change reaches a chief when
 * it next starts. The dialog is drawn from what the daemon last answered, and what
 * it answers is what every project's Specs section says following the app means.
 */
export class SpecsSettings {
  #dialog
  #form
  #on
  #folder
  #variant
  #refusal
  #version
  #variants
  #page
  /** How often the human has changed each control. */
  #edits = { on: 0, folder: 0, variant: 0 }
  /**
   * How often each had been changed when a save of it was last kept, or the dialog opened:
   * a control asked again with nothing changed since is not saved again.
   */
  #kept = { on: 0, folder: 0, variant: 0 }
  /**
   * How often each had been changed when a save of it was last sent: one that failed is
   * sent again when the human asks for it again (the daemon may have been out of reach),
   * but not as they leave the field.
   */
  #sent = { on: 0, folder: 0, variant: 0 }
  /** The control whose save is on its way; none where none is. */
  #sending = null
  /** The controls that hold the human's word and no save has kept it yet. */
  #drafts = new Set()
  /** The controls to save once the save on its way is answered, in the order they changed. */
  #queue = []
  /** The save on its way, until it is answered; none where none is. */
  #saving = null
  /** Why the folder was refused, said until a save of it is kept or the dialog opens again. */
  #folderRefused = ''
  /** Why the box or the variant was refused, said until the next save is kept. */
  #otherRefused = ''

  /**
   * `page.daemon` asks the daemon, `page.specs()` is what it last said of the
   * app's specs and the Specmint it bundles (`{ app, applied, specmint }`),
   * `page.readSpecs()` asks again and `page.setSpecs(specs)` hands the page what a
   * change answered, which every project's Specs section then says. `page.report`
   * says a refusal that comes once the dialog is shut, where the human still looks.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#on = this.#form.elements.on
    this.#folder = this.#form.elements.folder
    this.#variant = this.#form.elements.variant
    this.#refusal = dialog.querySelector('.dialog-refusal')
    this.#version = dialog.querySelector('.specmint-version')
    this.#variants = dialog.querySelector('.specmint tbody')
    this.#page = page
    for (const name of Object.keys(this.#edits)) {
      const control = this.#form.elements[name]
      control.addEventListener('input', () => {
        this.#edits[name] += 1
        this.#drafts.add(name)
      })
      // The folder is kept as the human leaves it; the box and the variant at once.
      control.addEventListener('change', () => this.#ask(name))
    }
    // Enter in the folder keeps it, each press once: taken as the key goes down, so the
    // browser neither sends the form nor says a change of it as well.
    this.#folder.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' || event.isComposing) return
      event.preventDefault()
      this.#ask('folder', true)
    })
    this.#form.addEventListener('submit', (event) => event.preventDefault())
    dialog.querySelector('[value="close"]').addEventListener('click', () => dialog.close())
  }

  /**
   * Opens on what the daemon keeps, read once the saves asked before are answered,
   * with no word of the human's and no refusal left from the last time.
   */
  async open() {
    await this.#page.readSpecs()
    this.#drafts.clear()
    this.#kept = { ...this.#edits }
    this.#folderRefused = ''
    this.#otherRefused = ''
    this.#draw()
    this.#dialog.showModal()
  }

  /** Once every save asked of the dialog is answered: what is kept is then the last word. */
  async settled() {
    while (this.#saving !== null) await this.#saving
  }

  /**
   * Asks for `name` to be saved, unless nothing changed in it since a save of it was kept,
   * or a save of it is pending: one waiting in the queue, which sends the control as it is
   * then, or the one on its way with nothing changed since it was sent. Nothing changed since
   * a save of it failed, it is sent again only when the human asks `again` (Enter).
   */
  #ask(name, again = false) {
    const edits = this.#edits[name]
    if (edits === this.#kept[name] || this.#queue.includes(name)) return
    if (this.#sent[name] === edits && (this.#sending === name || !again)) return
    this.#queue.push(name)
    if (this.#saving === null) this.#saving = this.#next()
  }

  /** Saves each control asked for, one at a time, each as it is when its save is asked. */
  async #next() {
    while (this.#queue.length > 0) {
      await this.#save(this.#queue.shift())
    }
    this.#saving = null
  }

  /**
   * Saves `name` as the control is now, and draws what the daemon answers but the
   * drafts. A refusal is said under the controls; a box or a variant refused goes back
   * to what is kept, and a folder stays as typed for the human to mend. One that comes
   * once the dialog is shut is said on the status line.
   */
  async #save(name) {
    const edits = this.#edits[name]
    const value = name === 'on' ? this.#on.checked : this.#form.elements[name].value
    this.#sent[name] = edits
    this.#sending = name
    try {
      this.#page.setSpecs(await this.#page.daemon('specs.set', { [name]: value }))
      this.#kept[name] = edits
      if (this.#edits[name] === edits) this.#drafts.delete(name)
      if (name === 'folder') this.#folderRefused = ''
      this.#otherRefused = ''
    } catch (cause) {
      const words = cause instanceof Error ? cause.message : String(cause)
      if (name === 'folder') {
        this.#folderRefused = words
      } else {
        this.#otherRefused = words
        if (this.#edits[name] === edits) this.#drafts.delete(name)
      }
      if (!this.#dialog.open) this.#page.report(words)
    }
    this.#sending = null
    this.#draw()
  }

  /**
   * Draws the dialog from what the daemon last said, every control but the drafts, and
   * says the refusal that stands: the box's or the variant's, else the folder's.
   */
  #draw() {
    const { app, applied, specmint } = this.#page.specs()
    if (!this.#drafts.has('on')) this.#on.checked = app.on
    if (!this.#drafts.has('folder')) this.#folder.value = app.folder ?? ''
    this.#folder.placeholder = applied.folder
    const chosen = this.#variant.value
    this.#variant.replaceChildren(
      ...specmint.variants.map(({ name }) => {
        const option = element('option', null, name)
        option.value = name
        return option
      }),
    )
    this.#variant.value = this.#drafts.has('variant') ? chosen : applied.variant
    const refused = this.#otherRefused || this.#folderRefused
    this.#refusal.textContent = refused
    this.#refusal.hidden = refused === ''
    this.#version.textContent =
      specmint.version === null
        ? 'Specmint, its version unknown, brought with this app.'
        : `Specmint ${specmint.version}, brought with this app.`
    redraw(
      this.#variants,
      specmint.variants.map((variant) => {
        const row = element('tr')
        row.dataset.variant = variant.name
        row.append(
          element('td', 'member-name', variant.name),
          element('td', 'member-meta', variant.version ?? UNKNOWN),
          element('td', 'member-meta', variant.commit ?? UNKNOWN),
          element('td', null, variant.about),
        )
        return row
      }),
    )
  }
}

/** What a version or a commit the bundled files do not say reads as. */
const UNKNOWN = 'unknown'
