import { SpecsGroup } from './specs.js'
import { WakeUpCalls } from './wake-up.js'

/**
 * The sections of a project's settings that Project settings and New project
 * both have, after Staff: Human approval, Wake-up calls and Specs. They are one
 * markup (the page's `project-sections` template, copied into each dialog) and
 * one code for what each holds, shows and refuses, so the two dialogs never
 * drift apart. They differ in what a section does with what the human sets:
 *
 * - in Project settings (`page` is given) each saves as it always did, for the
 *   project the dialog is for: the approval when it is ticked or unticked, the
 *   calls and the specs by their own Save;
 * - in New project (no `page`) nothing is saved, for there is no project yet:
 *   the sections hold what the human sets, and `values` is what the project
 *   starts with, kept in the same write that makes it.
 */
export class ProjectSections {
  #gate
  #wakeUp
  #specs

  /**
   * Puts the sections where `mount` is. `page` is what Project settings'
   * sections save through: the project the dialog is for, the daemon, and how a
   * change is run and told (`WakeUpCalls` and `SpecsGroup` say which).
   */
  constructor(mount, page = null) {
    const sections = document.querySelector('#project-sections').content.cloneNode(true)
    this.#gate = sections.querySelector('[name="gate"]')
    this.#wakeUp = new WakeUpCalls(sections.querySelector('.wakeup-section'), page)
    this.#specs = new SpecsGroup(sections.querySelector('.project-specs'), page)
    // What only a saved project has (a Save, and when a change reaches the chief) is not in New project.
    for (const live of sections.querySelectorAll('[data-live]')) live.hidden = page === null
    mount.replaceWith(sections)
  }

  /** The approval's box: a project's is saved as it changes, a new project's is read at its start. */
  get gate() {
    return this.#gate
  }

  get wakeUp() {
    return this.#wakeUp
  }

  get specs() {
    return this.#specs
  }

  /**
   * What a new project starts with: whether the human approves each message,
   * and the calls and the specs as the human set them, each none while its
   * section is as it opened (the project is then as one that never said it).
   */
  values() {
    return {
      gate: this.#gate.checked,
      wakeUp: this.#wakeUp.values(),
      specs: this.#specs.values(),
    }
  }

  /** Every section as a project has it that never said anything: a new project's start. */
  reset() {
    this.#gate.checked = false
    this.#wakeUp.reset()
    this.#specs.reset()
  }
}
