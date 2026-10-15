/**
 * The Specs section of a project's settings: whether the chief plans big work
 * as specs (Specmint's), in which folder of the project and with which variant.
 * A project follows what the human set for every project (Settings, Agents)
 * until it says on or off itself; the folder and the variant are its own. The
 * daemon checks the folder (inside the project, never `..`) and keeps all
 * three. In Project settings the section saves, and a change reaches the chief
 * when it next starts, which the dialog says; the controls keep what the human
 * typed while the board is drawn again, and are set again when what the project
 * says changed. In New project there is nothing to save yet: the section holds
 * what the human sets, and `values` is what the project starts with, and its
 * chief starts with.
 */

/** The folder and the variant a project has when it names none: the daemon's. */
const DEFAULT_FOLDER = '.specs'
const DEFAULT_VARIANT = 'core'

/** What the first select says, by what the project says (`on`: true, false, or nothing to follow). */
function modeOf(on) {
  if (on === true) return 'on'
  if (on === false) return 'off'
  return 'follow'
}

/** What the project says for each choice of the first select. */
const SAID = { on: true, off: false, follow: null }

export class SpecsGroup {
  #mode
  #folder
  #variant
  #note
  #page
  /** The project and what it said when the controls were last set: they change with it. */
  #shown = null

  /**
   * `group` is the fieldset, which holds the controls by their names. With a
   * `page` the section saves, for the project the dialog is for: `page.project()`
   * is that project, not the one the page shows, `page.daemon` asks the daemon,
   * `page.act` runs a change and redraws the page, `page.note` tells the human
   * what came of it and `page.report` says a refusal on the status line. With
   * none it only holds what is set in it (`values`), as New project's does.
   */
  constructor(group, page = null) {
    this.#mode = group.querySelector('[name="specs"]')
    this.#folder = group.querySelector('[name="specsFolder"]')
    this.#variant = group.querySelector('[name="specsVariant"]')
    this.#note = group.querySelector('.specs-note')
    this.#page = page
    this.#folder.placeholder = DEFAULT_FOLDER
    this.#mode.addEventListener('change', () => this.#enable())
    if (page !== null) {
      group.querySelector('[name="specs-save"]').addEventListener('click', () => this.#save())
    }
    // Enter in the folder saves the specs where they are saved, and sends
    // nothing else the form holds: the form of New project starts the project.
    this.#folder.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter') return
      event.preventDefault()
      if (page !== null) void this.#save()
    })
    this.reset()
  }

  /**
   * Sets the controls to what `project` says, unless they show it already
   * (the board is drawn again while the dialog is open, and the human may be
   * typing in it) or `force`: a dialog that opens shows the project as it is.
   */
  draw(project, force = false) {
    const said = JSON.stringify([project?.id ?? null, project?.specs ?? null])
    if (!force && said === this.#shown) return
    this.#shown = said
    const specs = project?.specs ?? {}
    this.#mode.value = modeOf(specs.on)
    this.#folder.value = specs.folder ?? ''
    this.#variant.value = specs.variant ?? DEFAULT_VARIANT
    this.#say('')
    this.#enable()
  }

  /** The section as it is for a project that says nothing of specs. */
  reset() {
    this.draw(null, true)
  }

  /**
   * What a new project says of specs: none while the section is as it opened,
   * which leaves the project following the app's setting as one that said
   * nothing does, else all three, as `project.specs` takes them.
   */
  values() {
    const said = this.#said()
    const asOpened = said.on === null && said.folder === null && said.variant === DEFAULT_VARIANT
    return asOpened ? null : said
  }

  /** What the controls say, as `project.specs` takes it. */
  #said() {
    const folder = this.#folder.value.trim()
    return {
      on: SAID[this.#mode.value],
      folder: folder === '' ? null : folder,
      variant: this.#variant.value,
    }
  }

  /** A project that says off has no folder or variant to name. */
  #enable() {
    const off = this.#mode.value === 'off'
    this.#folder.disabled = off
    this.#variant.disabled = off
  }

  /** What is said under the controls: a refusal, or what came of a change; nothing clears it. */
  #say(text) {
    this.#note.textContent = text
    this.#note.hidden = text === ''
  }

  /**
   * Says all three to the daemon, which refuses a folder that leaves the project,
   * for the project the dialog is for, by its id.
   */
  async #save() {
    const project = this.#page.project()
    const body = { project, ...this.#said() }
    let refused = null
    await this.#page.act(async () => {
      try {
        await this.#page.daemon('project.specs', body)
      } catch (cause) {
        refused = cause instanceof Error ? cause.message : String(cause)
      }
    })
    const saved =
      body.on === false
        ? 'Specs are off in this project: the chief is told so when it next starts.'
        : 'Saved: the specs reach the chief when it next starts.'
    // Said where the human is typing, after the board was drawn again; a dialog left (shut, or
    // opened on another project since) is not the place for a refusal, and the status line says it.
    const here = this.#page.project() === project
    if (here) this.#say(refused ?? saved)
    if (refused === null) this.#page.note(saved)
    else if (!here) this.#page.report(refused)
  }
}
