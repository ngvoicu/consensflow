/**
 * The Specs section of a project's settings: whether the chief plans big work
 * as specs (Specmint's), in which folder of the project and with which variant.
 * What a project leaves unsaid is what the app says (Settings, Specs), and the
 * section says what that is now: "Follow the app's setting" names whether the app
 * has specs on, the folder's placeholder is the app's folder, the variant's first
 * choice, "As the app says (core-html)", names none, and the Specmint version of
 * the variant that will be used is beside it. What the app says, and the Specmint
 * it bundles, come from the daemon (`app()`, as `specs.get` answers): nothing of
 * it is typed here. The daemon checks the folder (inside the project, never `..`)
 * and keeps all three. In Project settings the section saves, and a change reaches
 * the chief when it next starts, which the dialog says; the controls keep what the
 * human typed while the board is drawn again, and are set again when what the
 * project says changed. In New project there is nothing to save yet: the section
 * holds what the human sets, and `values` is what the project starts with, and its
 * chief starts with.
 */

/** What the first select says, by what the project says (`on`: true, false, or nothing to follow). */
function modeOf(on) {
  if (on === true) return 'on'
  if (on === false) return 'off'
  return 'follow'
}

/** What the project says for each choice of the first select. */
const SAID = { on: true, off: false, follow: null }

/** Sets `node[property]` only where it differs: a control the human has open is left as it is. */
function show(node, property, value) {
  if (node[property] !== value) node[property] = value
}

export class SpecsGroup {
  #mode
  #folder
  #variant
  #version
  #note
  #page
  #app
  /** The variants the select offers after its first choice, by name. */
  #offered = []
  /** The project and what it said when the controls were last set: they change with it. */
  #shown = null

  /**
   * `group` is the fieldset, which holds the controls by their names. With a
   * `page` the section saves, for the project the dialog is for: `page.project()`
   * is that project, not the one the page shows, `page.daemon` asks the daemon,
   * `page.act` runs a change and redraws the page, `page.note` tells the human
   * what came of it and `page.report` says a refusal on the status line. With
   * none it only holds what is set in it (`values`), as New project's does.
   * `app()` is what the daemon last said of the app's specs and the Specmint it
   * bundles (`{ app, applied, specmint }`), or null where the page has none yet.
   */
  constructor(group, page = null, app = () => null) {
    this.#mode = group.querySelector('[name="specs"]')
    this.#folder = group.querySelector('[name="specsFolder"]')
    this.#variant = group.querySelector('[name="specsVariant"]')
    this.#version = group.querySelector('.specs-version')
    this.#note = group.querySelector('.specs-note')
    this.#page = page
    this.#app = app
    this.#mode.addEventListener('change', () => this.#enable())
    this.#variant.addEventListener('change', () => this.#showVersion())
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
   * What the app says is shown either way, as it is now.
   */
  draw(project, force = false) {
    this.#showApp()
    const said = JSON.stringify([project?.id ?? null, project?.specs ?? null])
    if (!force && said === this.#shown) return
    this.#shown = said
    const specs = project?.specs ?? {}
    this.#mode.value = modeOf(specs.on)
    this.#folder.value = specs.folder ?? ''
    this.#pick(specs.variant ?? '')
    this.#showVersion()
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
    const asOpened = said.on === null && said.folder === null && said.variant === null
    return asOpened ? null : said
  }

  /** What the controls say, as `project.specs` takes it: the variant none while it is the app's. */
  #said() {
    const folder = this.#folder.value.trim()
    return {
      on: SAID[this.#mode.value],
      folder: folder === '' ? null : folder,
      variant: this.#variant.value === '' ? null : this.#variant.value,
    }
  }

  /**
   * Says what following the app means now: whether it has specs on, its folder, and its
   * variant, and offers the variants Specmint has, the version of the one chosen beside.
   */
  #showApp() {
    const specs = this.#app()
    const follow = this.#mode.querySelector('[value="follow"]')
    show(
      follow,
      'textContent',
      specs === null
        ? "Follow the app's setting"
        : `Follow the app's setting (${specs.app.on ? 'on' : 'off'})`,
    )
    show(this.#folder, 'placeholder', specs?.applied.folder ?? '')
    show(
      this.#variant.options[0],
      'textContent',
      specs === null ? 'As the app says' : `As the app says (${specs.applied.variant})`,
    )
    const names = specs?.specmint.variants.map((variant) => variant.name) ?? []
    if (names.join('\n') !== this.#offered.join('\n')) {
      const chosen = this.#variant.value
      this.#offered = names
      this.#variant.replaceChildren(
        this.#variant.options[0],
        ...names.map((name) => new Option(name, name)),
      )
      this.#pick(chosen)
    }
    this.#showVersion()
  }

  /** Chooses `variant`, which the select offers whatever Specmint has: a project keeps the one it said. */
  #pick(variant) {
    if (variant !== '' && ![...this.#variant.options].some((option) => option.value === variant)) {
      this.#variant.append(new Option(variant, variant))
    }
    this.#variant.value = variant
  }

  /**
   * The Specmint version of the variant the project will plan with, beside the choice:
   * unknown where its bundled files do not say it.
   */
  #showVersion() {
    const specs = this.#app()
    const name = this.#variant.value === '' ? specs?.applied.variant : this.#variant.value
    const bundled = specs?.specmint.variants.find((variant) => variant.name === name)
    let label = ''
    if (bundled !== undefined) {
      label = bundled.version === null ? 'Specmint version unknown' : `Specmint ${bundled.version}`
    }
    show(this.#version, 'textContent', label)
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
