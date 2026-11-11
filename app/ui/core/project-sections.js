import { AdvisorTool } from './advisor-tool.js'
import { Compaction } from './compaction.js'
import { ComputerUseSection } from './computer-use.js'
import { PromptSuggestion } from './prompt-suggestion.js'
import { SpecsGroup } from './specs.js'
import { StaffRules } from './staff-rules.js'
import { WakeUpCalls } from './wake-up.js'

/**
 * Enter in a field of `root` submits nothing: a dialog's form starts the project (New
 * project) or adds a member (Project settings), and a field of a section is not where the
 * human means either (T-1268). Every field that Enter would submit from is held, whatever its
 * kind (a text, a number, a checkbox, a radio, a select), so a field added later is held too;
 * Enter keeps what it is for in a text area (a new line) and on a button (its press). A field
 * that does something of its own on Enter (the number of the wake-up calls and the specs'
 * folder save their section) does it first: this only keeps the key from the form.
 */
export function holdEnter(root) {
  root.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' || event.isComposing) return
    if (event.target.matches('input:not([type=button], [type=submit], [type=reset]), select')) {
      event.preventDefault()
    }
  })
}

/**
 * The sections of a project's settings that Project settings and New project
 * both have, after Staff, which each dialog draws for itself: the rules for the
 * staff, the compaction, the advisor tool, the prompt suggestion, Human approval, Wake-up calls, Specs and
 * Computer use.
 * They are one markup (the page's `project-sections` template, copied into each
 * dialog) and one code for what each holds, shows and refuses, so the two
 * dialogs never drift apart: a section is added to the template, and both have
 * it (T-1268). They differ in what a section does with what the human sets:
 *
 * - in Project settings (`page` is given) each saves as it always did, for the
 *   project the dialog is for: the approval when it is ticked or unticked, the
 *   rules, the compaction, the advisor tool, the prompt suggestion, the calls and the specs by
 *   their own Save;
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
  #advisorTool
  #promptSuggestion
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
    this.#compaction = new Compaction(sections.querySelector('.members-compaction'), page)
    this.#advisorTool = new AdvisorTool(sections.querySelector('.members-advisor-tool'), page)
    this.#promptSuggestion = new PromptSuggestion(
      sections.querySelector('.prompt-suggestion'),
      page,
    )
    this.#gate = sections.querySelector('[name="gate"]')
    this.#wakeUp = new WakeUpCalls(sections.querySelector('.wakeup-section'), page)
    this.#specs = new SpecsGroup(sections.querySelector('.project-specs'), page, app)
    this.#computerUse = new ComputerUseSection(sections.querySelector('.computer-use'), screen)
    // What only a saved project has (a Save, and when a change reaches the chief) is not in New project.
    for (const live of sections.querySelectorAll('[data-live]')) live.hidden = page === null
    // Enter in a field of a section is that section's own, never its dialog's form's.
    for (const section of sections.querySelectorAll('fieldset.project-section')) holdEnter(section)
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

  get advisorTool() {
    return this.#advisorTool
  }

  get promptSuggestion() {
    return this.#promptSuggestion
  }

  get computerUse() {
    return this.#computerUse
  }

  /**
   * What a new project starts with: whether the human approves each message,
   * and the rules, the compaction, the advisor tool, the prompt suggestion, the calls and the specs as the human set
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
      advisorTool: this.#advisorTool.values(),
      promptSuggestion: this.#promptSuggestion.values(),
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
    this.#advisorTool.reset()
    this.#promptSuggestion.reset()
    this.#wakeUp.reset()
    this.#specs.reset()
  }
}
