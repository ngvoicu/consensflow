/**
 * The "Prompt suggestion" section of Project settings: whether the project's Claude Code
 * windows, the chief's and the members', make Claude Code's prompt suggestion. One switch, off by
 * default: after each turn Claude Code guesses the person's next prompt with a model call of its
 * own (a fork of the whole conversation, read again each time) and shows it in grey in the input
 * box, and a project's Claude Code windows are launched without that call unless the human ticks
 * this. A ticked project gives them no word about it, so the owner's own Claude Code setting
 * decides. It applies to Claude Code only: the other harnesses have no such call. A change
 * reaches each window when it next starts: a window already open keeps what it began with. Only
 * the human chooses it, here: no `cf` command does. The section shows the choice the project's
 * board has when the dialog opens, and one Save sends it for the project the dialog is for. A
 * refusal is said in the section, where the human is looking.
 */
export class PromptSuggestion {
  #suggestion
  #hint
  #save
  #page

  /**
   * `group` is the fieldset. `page.project()` is the project the dialog is for, not the one
   * the page shows, `page.daemon` asks the daemon, `page.act` runs a change and redraws the
   * page, `page.note` tells the human what came of it and `page.report` says a refusal on the
   * status line.
   */
  constructor(group, page) {
    this.#suggestion = group.querySelector('[name="prompt-suggestion"]')
    this.#hint = group.querySelector('.prompt-suggestion-hint')
    this.#save = group.querySelector('[name="prompt-suggestion-save"]')
    this.#page = page
    this.#suggestion.addEventListener('change', () => this.#say(''))
    this.#save.addEventListener('click', () => void this.#saved())
    this.show(null)
  }

  /** Shows the choice `project` has, as its board says it: off for a project that never ticked it. */
  show(project) {
    this.#suggestion.checked = project?.promptSuggestion === true
    this.#say('')
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  /** The section as it is for a project that never ticked it: a new project's start. */
  reset() {
    this.show(null)
  }

  /** What a new project starts with: whether its Claude Code windows make the suggestion. */
  values() {
    return this.#suggestion.checked
  }

  /**
   * Saves what the section shows, for the project the dialog is for, by its id. What the
   * daemon answers is shown only while the dialog is still for that project, and a refusal
   * then goes to the status line.
   */
  #saved() {
    const project = this.#page.project()
    const promptSuggestion = this.#suggestion.checked
    return this.#page.act(async () => {
      try {
        const answer = await this.#page.daemon('project.prompt_suggestion', {
          project,
          promptSuggestion,
        })
        if (this.#page.project() === project) this.show(answer.project)
        this.#page.note(
          answer.project.promptSuggestion === true
            ? "The project's Claude Code windows, the chief's too, follow your own Claude Code setting for the prompt suggestion: each window is given that when it next starts."
            : "The project's Claude Code windows, the chief's too, make no prompt suggestion: windows that start from now on are launched without it.",
        )
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
