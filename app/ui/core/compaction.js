/**
 * The "Compaction" section of Project settings: the size at which the human has every
 * member window of the project compact, whatever its harness, or each harness's own
 * point (the default, which changes nothing). Each member's launch gives it its harness
 * by that harness's own rule (smaller-contexts, SCX-28 to SCX-30), from 100,000 to
 * 1,000,000 tokens; a size above a model's own window leaves the harness's own point.
 * The chief is never given it. Only the
 * human chooses it, here: no `cf` command does. The section shows the choice the
 * project's board has when the dialog opens, checks a size as the human types
 * (a whole number in the range; Save waits until it is one), and one Save sends the
 * choice for the project the dialog is for. A refusal is said in the section, where
 * the human is looking.
 */

/** The smallest size: the ledger's `COMPACTION_MIN`, which a test of the daemon holds this to. */
const LEAST = 100_000

/** The largest size: the ledger's `COMPACTION_MAX`. */
const MOST = 1_000_000

/** The size a first choice starts from: the one the project proposal measured. */
const START_AT = 400_000

/** A number as the hint says it: 100,000. */
const said = (count) => count.toLocaleString('en')

/**
 * The size `text` is, or null where it is no whole number of tokens in the range:
 * what the ledger takes.
 */
export function sizeOf(text) {
  const trimmed = text.trim()
  if (!/^\d+$/.test(trimmed)) return null
  const size = Number(trimmed)
  return size >= LEAST && size <= MOST ? size : null
}

export class Compaction {
  #own
  #size
  #amount
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
    this.#own = group.querySelector('[name="members-compaction-choice"][value="own"]')
    this.#size = group.querySelector('[name="members-compaction-choice"][value="size"]')
    this.#amount = group.querySelector('[name="members-compaction-size"]')
    this.#hint = group.querySelector('.members-compaction-hint')
    this.#save = group.querySelector('[name="members-compaction-save"]')
    this.#page = page
    for (const choice of [this.#own, this.#size]) {
      choice.addEventListener('change', () => this.#chosen())
    }
    this.#amount.addEventListener('input', () => this.#checked())
    this.#save.addEventListener('click', () => void this.#saved())
    this.show(null)
  }

  /** Shows the choice `project` has, as its board says it: the harness's own point for a project with none. */
  show(project) {
    const size = project?.compaction
    this.#own.checked = size === undefined
    this.#size.checked = size !== undefined
    this.#amount.value = size === undefined ? '' : String(size)
    this.#say('')
    this.#checked()
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  /** A choice between the harness's own point and a size: a first size starts from the proposal's. */
  #chosen() {
    if (this.#size.checked && this.#amount.value.trim() === '') {
      this.#amount.value = String(START_AT)
    }
    this.#say('')
    this.#checked()
  }

  /** The size the section holds against the range; Save waits while a size is chosen and is none. */
  #checked() {
    const sized = this.#size.checked
    const typed = this.#amount.value
    const bad = sized && sizeOf(typed) === null
    this.#amount.disabled = !sized
    this.#amount.setAttribute('aria-invalid', String(bad))
    this.#save.disabled = bad
    this.#say(bad ? `A size is a whole number from ${said(LEAST)} to ${said(MOST)}.` : '')
  }

  /**
   * Saves what the section shows, for the project the dialog is for, by its id. What the
   * daemon answers is shown only while the dialog is still for that project, and a refusal
   * then goes to the status line.
   */
  #saved() {
    const project = this.#page.project()
    const compaction = this.#size.checked ? sizeOf(this.#amount.value) : null
    return this.#page.act(async () => {
      try {
        const answer = await this.#page.daemon('project.compaction', { project, compaction })
        if (this.#page.project() === project) this.show(answer.project)
        const kept = answer.project.compaction
        this.#page.note(
          kept === undefined
            ? "The project's members use their harness's own point again: windows that start from now on are given no size."
            : `The project's members compact at about ${said(kept)} tokens: each window is given it when it next starts.`,
        )
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
