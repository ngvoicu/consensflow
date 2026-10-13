/**
 * The Specs group of a project's settings dialog: whether the chief plans big
 * work as specs (Specmint's), in which folder of the project and with which
 * variant. A project follows what the human set for every project (Settings,
 * Agents) until it says on or off itself; the folder and the variant are its
 * own. The daemon checks the folder (inside the project, never `..`) and keeps
 * all three; a change reaches the chief when it next starts, and the dialog
 * says so. The controls keep what the human typed while the board is drawn
 * again, and are set again when what the project says changed.
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
   * `page.board()` is the board shown, `page.daemon` asks the daemon,
   * `page.act` runs a change and redraws the page and `page.note` tells the
   * human what came of it.
   */
  constructor(group, page) {
    this.#mode = group.querySelector('[name="specs"]')
    this.#folder = group.querySelector('[name="specsFolder"]')
    this.#variant = group.querySelector('[name="specsVariant"]')
    this.#note = group.querySelector('#staff-specs-note')
    this.#page = page
    this.#folder.placeholder = DEFAULT_FOLDER
    this.#mode.addEventListener('change', () => this.#enable())
    group.querySelector('#staff-specs-save').addEventListener('click', () => this.#save())
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

  /** Says all three to the daemon, which refuses a folder that leaves the project. */
  async #save() {
    const project = this.#page.board()?.project
    if (project === undefined) return
    const folder = this.#folder.value.trim()
    const body = {
      project: project.id,
      on: SAID[this.#mode.value],
      folder: folder === '' ? null : folder,
      variant: this.#variant.value,
    }
    let refused = null
    await this.#page.act(async () => {
      try {
        await this.#page.daemon('project.specs', body)
      } catch (cause) {
        refused = cause instanceof Error ? cause.message : String(cause)
      }
    })
    // Said where the human is typing, after the board was drawn again.
    const saved =
      body.on === false
        ? 'Specs are off in this project: the chief is told so when it next starts.'
        : 'Saved: the specs reach the chief when it next starts.'
    this.#say(refused ?? saved)
    if (refused === null) this.#page.note(saved)
  }
}
