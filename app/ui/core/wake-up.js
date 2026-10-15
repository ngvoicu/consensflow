/**
 * The "Wake-up calls" section of a project's settings: whether the project's
 * chief is nudged now and then, how often, and what the nudge says, so that a
 * chief that stopped while work is left carries on. In Project settings the
 * section reads the project's setting from the daemon when the dialog opens and
 * shows exactly what is saved (the daemon says the interval in the largest unit
 * that holds it whole: 120 minutes come back as 2 hours), and one Save sends the
 * whole of it, for the project the dialog is for. A refusal is said in the
 * section, where the human is looking. In New project there is nothing to save
 * yet: the section holds what the human sets, and `values` is what the project
 * starts with.
 */

/**
 * The words of a wake-up the human wrote none for: the ledger's
 * (`DEFAULT_WAKE_UP_TEXT`), which a test of the daemon holds this to.
 */
const DEFAULT_WAKE_UP_WORDS =
  'Do you still have work to do? If tasks are left on the board or in your plan, carry on.'

/** The calls of a project that never set them: off, an hour apart, the default words. */
const UNSET = { on: false, every: 1, unit: 'hours', text: DEFAULT_WAKE_UP_WORDS }

/** What the dialog says a saved setting comes to, for the status line. */
const saidOf = (calls) =>
  calls.on
    ? `Wake-up calls are on: the chief is asked every ${calls.every} ${
        calls.every === 1 ? calls.unit.replace(/s$/, '') : calls.unit
      }.`
    : 'Wake-up calls are off.'

export class WakeUpCalls {
  #on
  #every
  #unit
  #text
  #hint
  #page

  /**
   * `group` is the fieldset, which holds the controls by their names. With a
   * `page` the section saves, for the project the dialog is for: `page.project()`
   * is that project, not the one the page shows, `page.daemon` asks the daemon,
   * `page.act` runs a change and redraws the page, `page.note` tells the human
   * what came of it and `page.report` says a refusal on the status line. With
   * none it only holds what is set in it (`values`), as New project's does.
   */
  constructor(group, page = null) {
    this.#on = group.querySelector('[name="wakeup-on"]')
    this.#every = group.querySelector('[name="wakeup-every"]')
    this.#unit = group.querySelector('[name="wakeup-unit"]')
    this.#text = group.querySelector('[name="wakeup-text"]')
    this.#hint = group.querySelector('.wakeup-hint')
    this.#page = page
    if (page !== null) {
      group.querySelector('[name="wakeup-save"]').addEventListener('click', () => void this.#save())
    }
    // Enter in the number sends nothing else the form holds, and saves the
    // section where it saves: the form of New project starts the project.
    this.#every.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter') return
      event.preventDefault()
      if (page !== null) void this.#save()
    })
    this.reset()
  }

  /**
   * What the daemon has of `project`'s calls, as saved. It shows nothing: the
   * dialog shows it (`show`) once it is still for `project`, for the human may
   * have gone on to another while the daemon answered.
   */
  async read(project) {
    return (await this.#page.daemon('wakeup.get', { project })).wakeUp
  }

  /** Shows `calls` as the daemon says them. */
  show(calls) {
    this.#on.checked = calls.on
    this.#every.value = String(calls.every)
    this.#unit.value = calls.unit
    this.#text.value = calls.text
    this.#say('')
  }

  /** The section as it is for a project that never set it. */
  reset() {
    this.show(UNSET)
  }

  /**
   * What a new project starts with: none while the section is as it opened,
   * which leaves the project as one that never set its calls, else the whole of
   * the section, as `wakeup.set` takes it.
   */
  values() {
    const calls = this.#calls()
    return Object.keys(UNSET).every((part) => calls[part] === UNSET[part]) ? null : calls
  }

  /** What the controls say, as `wakeup.set` takes it. */
  #calls() {
    return {
      on: this.#on.checked,
      every: this.#every.valueAsNumber,
      unit: this.#unit.value,
      text: this.#text.value,
    }
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  /**
   * Saves what the group shows, for the project the dialog is for, by its id. What the
   * daemon answers is shown only while the dialog is still for that project: one left
   * (shut, or opened on another project since) is not the place for it, and a refusal
   * then goes to the status line.
   */
  #save() {
    if (!this.#every.reportValidity()) return Promise.resolve()
    const project = this.#page.project()
    const body = { project, ...this.#calls() }
    return this.#page.act(async () => {
      try {
        const { wakeUp } = await this.#page.daemon('wakeup.set', body)
        if (this.#page.project() === project) this.show(wakeUp)
        this.#page.note(saidOf(wakeUp))
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
