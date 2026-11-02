/**
 * The "Rules for the staff" section of Project settings: plain text the human
 * writes for the project's staff, which every member's window is given after its
 * role, and the chief's as what every member already has. Only the human writes
 * it, here: no `cf` command does. The section shows the rules the project's
 * board has when the dialog opens, counts what is written against the ledger's
 * bounds as the human types (in bytes and in lines, as the ledger counts them, on
 * the text it keeps: trimmed), and one Save sends the whole of it for the project
 * the dialog is for. Text over a bound is not sent: the count says which, and
 * Save waits until it fits. A refusal is said in the section, where the human is
 * looking.
 */

/** The most bytes the rules may have: the ledger's `STAFF_RULES_MAX_BYTES`, which a test of the daemon holds this to. */
const MOST_BYTES = 8_000

/** The most lines the rules may have: the ledger's `STAFF_RULES_MAX_LINES`. */
const MOST_LINES = 150

const encoder = new TextEncoder()

/** A number as the count says it: 8,000. */
const said = (count) => count.toLocaleString('en')

/**
 * What the ledger counts of `text`: its bytes (of UTF-8) and its lines, of the
 * text it keeps, trimmed. Blank text is none: no bytes and no lines.
 */
export function countOf(text) {
  const kept = text.trim()
  return { bytes: encoder.encode(kept).length, lines: kept === '' ? 0 : kept.split('\n').length }
}

export class StaffRules {
  #text
  #count
  #hint
  #save
  #page

  /**
   * `group` is the fieldset. `page.project()` is the project the dialog is for,
   * not the one the page shows, `page.daemon` asks the daemon, `page.act` runs a
   * change and redraws the page, `page.note` tells the human what came of it and
   * `page.report` says a refusal on the status line.
   */
  constructor(group, page) {
    this.#text = group.querySelector('[name="staff-rules"]')
    this.#count = group.querySelector('.staff-rules-count')
    this.#hint = group.querySelector('.staff-rules-hint')
    this.#save = group.querySelector('[name="staff-rules-save"]')
    this.#page = page
    this.#text.addEventListener('input', () => this.#counted())
    this.#save.addEventListener('click', () => void this.#saved())
    this.show(null)
  }

  /** Shows the rules `project` has, as its board says them: none for a project with none. */
  show(project) {
    this.#text.value = project?.staffRules ?? ''
    this.#say('')
    this.#counted()
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  /** The count of what is written, against the bounds; Save waits while it is over one. */
  #counted() {
    const { bytes, lines } = countOf(this.#text.value)
    const over = bytes > MOST_BYTES || lines > MOST_LINES
    this.#count.textContent = `${said(bytes)} of ${said(MOST_BYTES)} bytes · ${said(lines)} of ${said(MOST_LINES)} lines`
    this.#count.classList.toggle('over', over)
    this.#text.setAttribute('aria-invalid', String(over))
    this.#save.disabled = over
  }

  /**
   * Saves what the section shows, for the project the dialog is for, by its id. What the
   * daemon answers is shown only while the dialog is still for that project, and a refusal
   * then goes to the status line.
   */
  #saved() {
    const project = this.#page.project()
    const rules = this.#text.value
    return this.#page.act(async () => {
      try {
        const answer = await this.#page.daemon('project.rules', { project, rules })
        if (this.#page.project() === project) this.show(answer.project)
        this.#page.note(
          answer.project.staffRules === undefined
            ? 'The project has no rules for the staff now: windows that start from now on are given none.'
            : 'The rules for the staff are saved: each window of the project is given them when it next starts.',
        )
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
