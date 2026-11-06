import { ClaudeCompaction } from './claude-compaction.js'
import { ComputerUseSection } from './computer-use.js'
import { SpecsGroup } from './specs.js'
import { StaffRules } from './staff-rules.js'
import { WakeUpCalls } from './wake-up.js'

/**
 * The sections of a project's settings that Project settings and New project
 * both have, after Staff, which each dialog draws for itself: the rules for the
 * staff, the compaction, Human approval, Wake-up calls, Specs and Computer use.
 * They are one markup (the page's `project-sections` template, copied into each
 * dialog) and one code for what each holds, shows and refuses, so the two
 * dialogs never drift apart: a section is added to the template, and both have
 * it (T-1268). They differ in what a section does with what the human sets:
 *
 * - in Project settings (`page` is given) each saves as it always did, for the
 *   project the dialog is for: the approval when it is ticked or unticked, the
 *   rules, the compaction, the calls and the specs by their own Save;
 * - in New project (no `page`) nothing is saved, for there is no project yet:
 *   the sections hold what the human sets, and `values` is what the project
 *   starts with, kept in the same write that makes it. What needs a saved
 *   project (a Save, and when a change reaches the chief) is marked `data-live`
 *   in the markup and is not shown there.
 *
 * Computer use is the one section with nothing to save: it says what the harnesses'
 * own configuration says, read for the project's folder or, before there is a
 * project, for the folder picked (`screen.scope`), and leads to the panes of
 * System Settings.
 */
export class ProjectSections {
  #gate
  #wakeUp
  #specs
  #rules
  #compaction
  #computerUse

  /**
   * Puts the sections where `mount` is. `page` is what Project settings'
   * sections save through: the project the dialog is for, the daemon, and how a
   * change is run and told (each section says which it uses). `app()` is
   * what the daemon last said of the app's specs and the Specmint it bundles,
   * which the Specs section says what following the app means by. `screen` is
   * what Computer use asks the daemon through: `daemon`, `act`, `report`, and
   * `scope()`, the body that names what `computer_use.get` is asked about, the
   * project or the folder, none while the dialog is shut.
   */
  constructor(mount, page = null, app = () => null, screen = null) {
    const sections = document.querySelector('#project-sections').content.cloneNode(true)
    this.#rules = new StaffRules(sections.querySelector('.staff-rules'), page)
    this.#compaction = new ClaudeCompaction(sections.querySelector('.claude-compaction'), page)
    this.#gate = sections.querySelector('[name="gate"]')
    this.#wakeUp = new WakeUpCalls(sections.querySelector('.wakeup-section'), page)
    this.#specs = new SpecsGroup(sections.querySelector('.project-specs'), page, app)
    this.#computerUse = new ComputerUseSection(sections.querySelector('.computer-use'), screen)
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

  get rules() {
    return this.#rules
  }

  get compaction() {
    return this.#compaction
  }

  get computerUse() {
    return this.#computerUse
  }

  /**
   * What a new project starts with: whether the human approves each message,
   * and the rules, the compaction, the calls and the specs as the human set
   * them, each none while its section is as it opened (the project is then as
   * one that never said it).
   */
  values() {
    return {
      gate: this.#gate.checked,
      wakeUp: this.#wakeUp.values(),
      specs: this.#specs.values(),
      rules: this.#rules.values(),
      compaction: this.#compaction.values(),
    }
  }

  /**
   * The words that hold a new project's start back, where a section holds what the
   * project cannot start with: the first such section's, in the order they are shown, or
   * none. In Project settings the same sections hold their own Save back.
   */
  blocked() {
    return this.#rules.blocked() ?? this.#compaction.blocked()
  }

  /** Every section as a project has it that never said anything: a new project's start. */
  reset() {
    this.#gate.checked = false
    this.#rules.reset()
    this.#compaction.reset()
    this.#wakeUp.reset()
    this.#specs.reset()
  }
}
