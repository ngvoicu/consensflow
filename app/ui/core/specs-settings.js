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
 * Below, the Specmint the build bundles: its version and each variant's name,
 * version, commit and what it is, as the daemon read them from the bundled files
 * (a version or a commit they do not say is unknown); none of it is written here,
 * so a Specmint that is vendored again shows itself. A
 * change reaches a chief when it next starts. The dialog is drawn from what the
 * daemon last answered, and what it answers is what every project's Specs
 * section says following the app means.
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
  /**
   * The folder as the daemon keeps it, or as it was last asked to keep it while the
   * answer is on its way: a save of the same folder again is not made. Enter in the
   * folder is both a change and a submit, and the second must not ask again.
   */
  #kept = ''
  /**
   * How often the human has changed each control. A save's answer sets a control from
   * what is kept only where nothing was changed in it since the save was asked: what
   * was typed or chosen meanwhile is the human's, and goes in with a save of its own.
   */
  #edits = { on: 0, folder: 0, variant: 0 }

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
      this.#form.elements[name].addEventListener('input', () => {
        this.#edits[name] += 1
      })
    }
    this.#on.addEventListener('change', () => void this.#save({ on: this.#on.checked }))
    this.#variant.addEventListener(
      'change',
      () => void this.#save({ variant: this.#variant.value }),
    )
    this.#folder.addEventListener('change', () => void this.#saveFolder())
    // Enter in the folder keeps it, and the form has no other way to send.
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#saveFolder()
    })
    dialog.querySelector('[value="close"]').addEventListener('click', () => dialog.close())
  }

  /** Reads what the daemon says again and opens, with no refusal left from the last time. */
  async open() {
    await this.#page.readSpecs()
    this.#say('')
    this.#draw(() => true)
    this.#dialog.showModal()
  }

  /** What is said under the controls: the daemon's refusal, until the next change or opening. */
  #say(words) {
    this.#refusal.textContent = words
    this.#refusal.hidden = words === ''
  }

  /** Keeps the folder the human left in the field, unless it is the one kept, or asked for, already. */
  async #saveFolder() {
    const folder = this.#folder.value
    if (folder.trim() === this.#kept) return
    this.#kept = folder.trim()
    await this.#save({ folder })
  }

  /**
   * Keeps `part` (`on`, `folder` or `variant`) and draws what the daemon answers, but in a
   * control the human changed while it was on its way. A refusal is said under the
   * controls, and the controls go back to what is kept, but for the folder, which keeps
   * what was typed for the human to mend; one that comes once the dialog is shut is said
   * on the status line.
   */
  async #save(part) {
    this.#say('')
    const asked = { ...this.#edits }
    const untouched = (name) => this.#edits[name] === asked[name]
    try {
      this.#page.setSpecs(await this.#page.daemon('specs.set', part))
      this.#draw(untouched)
    } catch (cause) {
      const words = cause instanceof Error ? cause.message : String(cause)
      if (this.#dialog.open) this.#say(words)
      else this.#page.report(words)
      this.#draw((name) => name !== 'folder' && untouched(name))
      // The folder refused may be asked for again.
      this.#kept = this.#page.specs().app.folder ?? ''
    }
  }

  /**
   * Draws the dialog from what the daemon last said, setting each control `fresh` says
   * (`on`, `folder`, `variant`) to what is kept, and leaving the others as they are.
   */
  #draw(fresh) {
    const { app, applied, specmint } = this.#page.specs()
    if (fresh('on')) this.#on.checked = app.on
    if (fresh('folder')) {
      this.#folder.value = app.folder ?? ''
      this.#kept = this.#folder.value
    }
    this.#folder.placeholder = applied.folder
    const chosen = this.#variant.value
    this.#variant.replaceChildren(
      ...specmint.variants.map(({ name }) => {
        const option = element('option', null, name)
        option.value = name
        return option
      }),
    )
    this.#variant.value = fresh('variant') ? applied.variant : chosen
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
