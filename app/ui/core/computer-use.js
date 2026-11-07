import { element, redraw } from '../dom.js'

/**
 * Project settings' and New project's Computer use (T-540, drawn again in T-1266, in both
 * dialogs since T-1268): a block for each harness with a computer use of its own, shown only
 * while the project has a member on that harness, the chief included (in New project, the
 * chief and the staff picked so far), and one block for the macOS permissions those harnesses
 * work through. Every
 * block is headed the same way: its name, its status on the heading's line, and a muted line
 * under it where there is something to explain. A permission reads the same way in every row:
 * a pane's name, each app's status (allowed, not allowed, or unknown where it can't be read),
 * and one button, whichever apps need that pane.
 *
 * The daemon says what each harness's own configuration says for the project's folder
 * (`computer_use.get`, read and never written): Claude Code keeps its switch in its own
 * configuration, which the human turns on in Claude Code itself, so the block only says
 * where. Codex's comes from the ChatGPT app, for every folder, and works through the
 * permissions of its own app, Codex Computer Use (T-548): the permissions table has a column
 * for it, where it says unknown (the daemon can't read another app's permissions), so that
 * the two panes, which both apps need, are opened by one button each. Each button opens its
 * pane of System Settings (`computer_use.open`); the state is read again when the dialog opens
 * and when the human comes back to the app. It is read for the project's folder, or, where
 * there is no project yet, for the folder picked: Claude Code keeps its switch by folder.
 */

/**
 * Each harness's block, by the harness's word, in the order they are shown: `ours` where it
 * works through ConsensFlow's permissions, and `own` the app whose permissions it works
 * through where they are another app's.
 */
const PARTS = {
  'claude-code': {
    name: 'Claude Code',
    ours: true,
    /**
     * What the block says, of `said` (its harness's answer) on `system`: `status` with its
     * `state` (`on`, `off` or `none`), and the muted `line` under the heading where there is one.
     */
    show(said, system) {
      if (system === 'windows') return { status: 'Not available on Windows', state: 'none' }
      if (system !== 'macos') return { status: 'Only on macOS', state: 'none' }
      if (said.on) {
        return {
          status: 'On for this project',
          state: 'on',
          line: "It works through ConsensFlow's permissions, below.",
        }
      }
      return {
        status: 'Off for this project',
        state: 'off',
        line: 'Turn it on in a Claude Code window of this project: /mcp → computer-use → Enable.',
      }
    },
  },
  codex: {
    name: 'Codex',
    own: 'Codex Computer Use',
    show(said, system) {
      if (system === 'windows') return { status: 'Not available on Windows', state: 'none' }
      if (system !== 'macos') return { status: 'Only on macOS', state: 'none' }
      if (said.setUp) {
        return {
          status: 'Set up',
          state: 'on',
          line: "Set up in the ChatGPT app. It works through its own app's permissions, below, which ConsensFlow can't read.",
        }
      }
      return {
        status: 'Not set up',
        state: 'off',
        line: 'Set it up in the ChatGPT app, then allow Codex Computer Use below.',
      }
    },
  },
}

/**
 * What a missing permission may mean when ConsensFlow is on in System Settings all the same
 * (T-903): macOS keeps a grant for one copy of the app, and a build made on this Mac that was
 * allowed leaves one the installed app is never found to satisfy. The engine's note says the
 * same words (cf-engine's `computer_use`), which name no permission: a note opens the pane of
 * each name it has.
 */
export const MEND =
  'If ConsensFlow is already on in that list, macOS may hold the grant for another copy of the app, so remove it with − and add /Applications/ConsensFlow.app with +.'

/** ConsensFlow's permissions, by the daemon's word for each and the pane that gives it. */
const GRANTS = [
  { key: 'accessibility', pane: 'accessibility', name: 'Accessibility' },
  {
    key: 'screenRecording',
    pane: 'screen-recording',
    name: 'Screen & System Audio Recording',
  },
]

/** What a permission is said to be, by what the daemon read of it: allowed, not, or unknown. */
const STATUS = {
  true: { state: 'allowed', words: 'allowed' },
  false: { state: 'missing', words: 'not allowed' },
  unknown: { state: 'unknown', words: 'unknown' },
}

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
  #apps
  #grants
  #grantsOf
  #mend
  #page
  /** What the daemon last said for the project or folder the section is drawn for. */
  #said = null
  /** The harnesses it was last drawn for. */
  #present = new Set()

  /**
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws, `page.report`
   * says a refusal, and `page.scope()` is the body that names what the dialog is for, a
   * project (`{ project }`) or a folder (`{ directory }`), none once shut.
   */
  constructor(section, page) {
    this.#section = section
    this.#harnesses = section.querySelector('.computer-use-harnesses')
    this.#permissions = section.querySelector('.computer-use-permissions')
    this.#apps = section.querySelector('.computer-use-apps')
    this.#grants = section.querySelector('.computer-use-grants')
    this.#grantsOf = section.querySelector('.computer-use-grants-of')
    this.#mend = section.querySelector('.computer-use-mend')
    this.#mend.textContent = MEND
    this.#page = page
    // Back from System Settings: what it was given there shows, for the project still open.
    window.addEventListener('focus', () => {
      const scope = page.scope()
      if (scope === null) return
      void this.read(scope).then((said) => {
        if (JSON.stringify(page.scope()) !== JSON.stringify(scope)) return
        this.show(said)
        this.draw(this.#present)
      })
    })
  }

  /**
   * What the daemon says for `scope`, `{ project }` or `{ directory }`; none where it
   * cannot say, and nothing is shown.
   */
  async read(scope) {
    try {
      return await this.#page.daemon('computer_use.get', scope)
    } catch {
      return null
    }
  }

  /** Holds `said`, what was read for the project the dialog opens for, to draw from. */
  show(said) {
    this.#said = said
  }

  /**
   * Draws the section for the harnesses `present` (their kinds, a `Set`: a project's chief and
   * members, or what New project has picked so far): hidden where none of them has a block.
   */
  draw(present) {
    this.#present = present
    const said = this.#said
    const parts = Object.entries(PARTS).filter(
      ([harness]) => present.has(harness) && said?.harnesses?.[harness],
    )
    this.#section.hidden = parts.length === 0
    redraw(
      this.#harnesses,
      parts.map(([harness, part]) => {
        const { status, state, line } = part.show(said.harnesses[harness], said.system)
        const block = element('div', 'computer-use-block')
        block.dataset.harness = harness
        const title = element('div', 'computer-use-title')
        const badge = element('span', 'computer-use-status', status)
        badge.dataset.state = state
        title.append(element('h3', 'computer-use-head', part.name), badge)
        block.append(title)
        if (line !== undefined) block.append(element('p', 'dialog-help', line))
        return block
      }),
    )
    // One block for the permissions, with a column for each app the harnesses shown work
    // through: ConsensFlow for Claude Code, whose permissions the daemon reads, and Codex
    // Computer Use for Codex, whose it can't. A pane both need has one row and one button.
    const permissions = said?.permissions ?? null
    const apps =
      said?.system === 'macos'
        ? [
            ...(parts.some(([, part]) => part.ours)
              ? [{ name: 'ConsensFlow', read: (key) => permissions?.[key] }]
              : []),
            ...parts
              .filter(([, part]) => part.own !== undefined)
              .map(([, part]) => ({ name: part.own, read: () => undefined })),
          ]
        : []
    this.#permissions.hidden = apps.length === 0
    // A ConsensFlow started from another app (a `cf ui` in a terminal) has that app's
    // permissions, which are the ones shown.
    const grantsOf = typeof said?.grantsOf === 'string' ? said.grantsOf : null
    this.#grantsOf.hidden = grantsOf === null || !apps.some(({ name }) => name === 'ConsensFlow')
    this.#grantsOf.textContent = this.#grantsOf.hidden
      ? ''
      : `This ConsensFlow was started from ${grantsOf}, so these are that app's permissions, not ConsensFlow's.`
    // Shown while ConsensFlow's own permission reads missing, and only then (the words name
    // ConsensFlow.app, so they are not for another app's column).
    this.#mend.hidden =
      permissions === null ||
      !apps.some(({ name }) => name === 'ConsensFlow') ||
      GRANTS.every(({ key }) => permissions[key] === true)
    // The columns: the permission, each app's status, and the button's, which has no heading.
    redraw(this.#apps, [
      element('th', null, 'Permission'),
      ...apps.map(({ name }) => element('th', null, name)),
      element('th', null, ''),
    ])
    redraw(
      this.#grants,
      apps.length === 0
        ? []
        : GRANTS.map(({ key, pane, name }) => {
            const row = element('tr')
            row.dataset.grant = pane
            const head = element('th', null, name)
            head.scope = 'row'
            row.append(head)
            for (const app of apps) {
              const { state, words } = STATUS[app.read(key) ?? 'unknown']
              const cell = element('td', 'computer-use-status', words)
              cell.dataset.state = state
              row.append(cell)
            }
            const opener = element('td')
            opener.append(this.#opener(pane, name))
            row.append(opener)
            return row
          }),
    )
  }

  /** The button that opens System Settings at `pane`, named for `what` it is opened for. */
  #opener(pane, what) {
    const open = element('button', 'quiet-button', 'Open System Settings')
    open.type = 'button'
    open.setAttribute('aria-label', `Open System Settings for ${what}`)
    open.addEventListener('click', () =>
      this.#page.act(() => this.#page.daemon('computer_use.open', { pane })),
    )
    return open
  }
}

/**
 * The words a note of the engine's opens with when a Claude window started where its
 * computer use is on and ConsensFlow lacks a permission (cf-engine's `computer_use`).
 */
export const NOTE_OPENING = "Claude Code's computer use is on for this project, but ConsensFlow"

/** The panes a computer-use note's buttons open: the permissions it names. */
export function notePanes(message) {
  // ConsensFlow's own note only: it has no sender, and a member's with the same words is a member's.
  if (message.kind !== 'note' || message.sender != null) return []
  if (!message.body?.startsWith(NOTE_OPENING)) return []
  // The names after `allow it`, up to where the note says where to allow them.
  const from = message.body.indexOf('allow it ')
  const to = message.body.indexOf(' in System Settings', from)
  const named = message.body.slice(from, to === -1 ? undefined : to)
  return GRANTS.filter(({ name }) => named.includes(name)).map(({ pane, name }) => ({
    pane,
    name,
  }))
}
