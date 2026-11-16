/** The shared own-hands section: a new project's choice, or a saved project's setting. */
export class OwnHands {
  #control
  #hint
  #page
  #shown = false

  constructor(group, page) {
    this.#control = group.querySelector('[name="own-hands"]')
    this.#hint = group.querySelector('.own-hands-hint')
    this.#page = page
    this.#control.addEventListener('change', () => this.#say(''))
    group
      .querySelector('[name="own-hands-save"]')
      .addEventListener('click', () => void this.#save())
    this.show(null)
  }

  show(project) {
    this.#shown = project?.ownHands === true
    this.#control.checked = this.#shown
    this.#say('')
  }

  // A board event updates a changed saved value without erasing an unsaved edit on an unrelated event.
  update(project) {
    if ((project?.ownHands === true) !== this.#shown) this.show(project)
  }

  reset() {
    this.show(null)
  }

  values() {
    return this.#control.checked
  }

  #say(words) {
    this.#hint.textContent = words
    this.#hint.hidden = words === ''
  }

  #save() {
    const project = this.#page.project()
    const ownHands = this.#control.checked
    return this.#page.act(async () => {
      try {
        const answer = await this.#page.daemon('project.own_hands', { project, ownHands })
        if (this.#page.project() === project) this.show(answer.project)
        this.#page.note(
          answer.project.ownHands === true
            ? 'The chief does the work itself and uses the board only when you ask.'
            : 'The chief works through the staff again.',
        )
      } catch (cause) {
        const words = cause instanceof Error ? cause.message : String(cause)
        if (this.#page.project() === project) this.#say(words)
        else this.#page.report(words)
      }
    })
  }
}
