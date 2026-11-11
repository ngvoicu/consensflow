/**
 * The "Advisor tool" section of Project settings: whether the project's members run with
 * Claude Code's own advisor tool. One switch, off by default: a Claude Code member is launched
 * without the tool unless the human ticks this, and a ticked project gives its members the tool
 * as the owner's own Claude Code setting does. It applies to Claude Code members only; the chief
 * is not affected and keeps its harness's own setting, and the other harnesses have no advisor
 * tool. A change reaches each member when it next starts: a window already open keeps what it
 * began with. Only the human chooses it, here: no `cf` command does. The section shows the
 * choice the project's board has when the dialog opens, and one Save sends it for the project
 * the dialog is for. A refusal is said in the section, where the human is looking.
 */
export class AdvisorTool {
  #tool
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
    this.#tool = group.querySelector('[name="members-advisor-tool"]')
    this.#hint = group.querySelector('.members-advisor-tool-hint')
    this.#save = group.querySelector('[name="members-advisor-tool-save"]')
    this.#page = page
    this.#tool.addEventListener('change', () => this.#say(''))
    this.#save.addEventListener('click', () => void this.#saved())
    this.show(null)
  }

  /** Shows the choice `project` has, as its board says it: off for a project that never ticked it. */
  show(project) {
    this.#tool.checked = project?.advisorTool === true
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

  /** What a new project starts with: whether its members run with the tool. */
  values() {
    return this.#tool.checked
  }

  /**
   * Saves what the section shows, for the project the dialog is for, by its id. What the
   * daemon answers is shown only while the dialog is still for that project, and a refusal
   * then goes to the status line.
   */
  #saved() {
    const project = this.#page.project()
    const advisorTool = this.#tool.checked
    return this.#page.act(async () => {
      try {
        const answer = await this.#page.daemon('project.advisor_tool', { project, advisorTool })
        if (this.#page.project() === project) this.show(answer.project)
        this.#page.note(
          answer.project.advisorTool === true
            ? "The project's Claude Code members have the advisor tool as your own Claude Code setting gives it: each window is given that when it next starts."
            : "The project's Claude Code members run without the advisor tool: windows that start from now on are launched without it.",
        )
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
