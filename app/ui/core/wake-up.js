/**
 * The Staff dialog's "Wake-up calls" group: whether the project's chief is
 * nudged now and then, how often, and what the nudge says, so that a chief
 * that stopped while work is left carries on. The group reads the project's
 * setting from the daemon when the dialog opens and shows exactly what is
 * saved (the daemon says the interval in the largest unit that holds it
 * whole: 120 minutes come back as 2 hours), and one Save sends the whole of
 * it. A refusal is said in the group, where the human is looking.
 */

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
   * `group` is the fieldset, which holds the controls by their names.
   * `page.board()` is the board shown, `page.daemon` asks the daemon, `page.act`
   * runs a change and redraws the page, `page.note` tells the human what came of it.
   */
  constructor(group, page) {
    this.#on = group.querySelector('[name="wakeup-on"]')
    this.#every = group.querySelector('[name="wakeup-every"]')
    this.#unit = group.querySelector('[name="wakeup-unit"]')
    this.#text = group.querySelector('[name="wakeup-text"]')
    this.#hint = group.querySelector('.wakeup-hint')
    this.#page = page
    group.querySelector('[name="wakeup-save"]').addEventListener('click', () => void this.#save())
    // Enter in the number saves it, and sends nothing else the form holds.
    this.#every.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter') return
      event.preventDefault()
      void this.#save()
    })
  }

  /** Reads the project's calls from the daemon and shows them as saved. */
  async load(project) {
    const { wakeUp } = await this.#page.daemon('wakeup.get', { project })
    this.#show(wakeUp)
  }

  /** Shows `calls` as the daemon says them. */
  #show(calls) {
    this.#on.checked = calls.on
    this.#every.value = String(calls.every)
    this.#unit.value = calls.unit
    this.#text.value = calls.text
    this.#say('')
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  /** Saves what the group shows; the project is the board's, as the gate's is. */
  #save() {
    if (!this.#every.reportValidity()) return Promise.resolve()
    const body = {
      project: this.#page.board().project.id,
      on: this.#on.checked,
      every: this.#every.valueAsNumber,
      unit: this.#unit.value,
      text: this.#text.value,
    }
    return this.#page.act(async () => {
      try {
        const { wakeUp } = await this.#page.daemon('wakeup.set', body)
        this.#show(wakeUp)
        this.#page.note(saidOf(wakeUp))
      } catch (cause) {
        this.#say(cause instanceof Error ? cause.message : String(cause))
      }
    })
  }
}
