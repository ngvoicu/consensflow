import { element, redraw } from '../dom.js'

/**
 * Project settings, Computer use (T-540): for each harness with a computer use of its
 * own, whether it is on for the project, shown only while the project has a member on
 * that harness, the chief included; and ConsensFlow's two macOS permissions, which every
 * project's computer use works through, shown once for the parts above them.
 *
 * The daemon says what each harness's own configuration says for the project's folder
 * (`computer_use.get`, read and never written): Claude Code keeps its switch in its own
 * configuration, which the human turns on in Claude Code itself, so the part only says
 * where. Each permission opens its pane of System Settings (`computer_use.open`); the
 * state is read again when the dialog opens and when the human comes back to the app.
 */

/** Each harness's part, by the harness's word, in the order they are shown. */
const PARTS = {
  'claude-code': {
    name: 'Claude Code',
    /** What the part says, of `said` (its harness's answer) on `system`. */
    lines(said, system) {
      if (system === 'windows') return ['Claude Code has no computer use on Windows.']
      if (system !== 'macos') return ['Claude Code has computer use only on macOS.']
      if (said.on) return [`On for this project (${said.folder}).`]
      return [
        'Off for this project.',
        `Turn it on in a Claude Code window in ${said.folder}: /mcp → computer-use → Enable.`,
      ]
    },
  },
}

/** ConsensFlow's permissions, by the daemon's word for each and the pane that gives it. */
const GRANTS = [
  { key: 'accessibility', pane: 'accessibility', name: 'Accessibility' },
  {
    key: 'screenRecording',
    pane: 'screen-recording',
    name: 'Screen & System Audio Recording',
  },
]

/** The harnesses the board's lanes run on: the chief's and the members'. */
export function harnessesOf(board) {
  return new Set(
    (board?.lanes ?? [])
      .map((lane) => lane.participant)
      .filter((participant) => participant.role !== 'human')
      .map((participant) => participant.harness)
      .filter(Boolean),
  )
}

export class ComputerUseSection {
  #section
  #harnesses
  #permissions
  #grants
  #page
  /** What the daemon last said for the project the section is drawn for. */
  #said = null
  /** The board it was last drawn from. */
  #board = null

  /**
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws, `page.report`
   * says a refusal, and `page.project()` is the project the dialog is for, none once shut.
   */
  constructor(section, page) {
    this.#section = section
    this.#harnesses = section.querySelector('.computer-use-harnesses')
    this.#permissions = section.querySelector('.computer-use-permissions')
    this.#grants = section.querySelector('.computer-use-grants')
    this.#page = page
    // Back from System Settings: what it was given there shows, for the project still open.
    window.addEventListener('focus', () => {
      const project = page.project()
      if (project === null) return
      void this.read(project).then((said) => {
        if (page.project() !== project) return
        this.show(said)
        this.draw(this.#board)
      })
    })
  }

  /** What the daemon says for `project`; none where it cannot say, and nothing is shown. */
  async read(project) {
    try {
      return await this.#page.daemon('computer_use.get', { project })
    } catch {
      return null
    }
  }

  /** Holds `said`, what was read for the project the dialog opens for, to draw from. */
  show(said) {
    this.#said = said
  }

  /** Draws the section from `board`: hidden where none of its harnesses has a part. */
  draw(board) {
    this.#board = board
    const said = this.#said
    const present = harnessesOf(board)
    const parts = Object.entries(PARTS).filter(
      ([harness]) => present.has(harness) && said?.harnesses?.[harness],
    )
    this.#section.hidden = parts.length === 0
    redraw(
      this.#harnesses,
      parts.map(([harness, part]) => {
        const block = element('div', 'computer-use-harness')
        block.dataset.harness = harness
        block.append(element('h3', 'computer-use-head', part.name))
        for (const line of part.lines(said.harnesses[harness], said.system)) {
          block.append(element('p', 'computer-use-line', line))
        }
        return block
      }),
    )
    const permissions = said?.permissions ?? null
    this.#permissions.hidden = parts.length === 0 || permissions === null
    redraw(
      this.#grants,
      permissions === null
        ? []
        : GRANTS.map(({ key, pane, name }) => {
            const row = element('li', 'computer-use-grant')
            row.dataset.grant = pane
            const allowed = permissions[key] === true
            row.append(
              element('span', 'computer-use-grant-name', `${name}: `),
              element(
                'span',
                allowed ? 'computer-use-allowed' : 'computer-use-missing',
                allowed ? 'allowed' : 'not allowed',
              ),
            )
            const open = element('button', 'quiet-button', 'Open System Settings')
            open.type = 'button'
            open.setAttribute('aria-label', `Open System Settings for ${name}`)
            open.addEventListener('click', () =>
              this.#page.act(() => this.#page.daemon('computer_use.open', { pane })),
            )
            row.append(open)
            return row
          }),
    )
  }
}

/**
 * The words a note of the engine's opens with when a Claude window started where its
 * computer use is on and ConsensFlow lacks a permission (cf-engine's `computer_use`).
 */
export const NOTE_OPENING = "Claude Code's computer use is on for this project, but ConsensFlow"

/** The panes a computer-use note's buttons open: the permissions it names. */
export function notePanes(message) {
  if (message.kind !== 'note' || !message.body?.startsWith(NOTE_OPENING)) return []
  const named = message.body.slice(message.body.indexOf('allow it '))
  return GRANTS.filter(({ name }) => named.includes(name)).map(({ pane, name }) => ({
    pane,
    name,
  }))
}
