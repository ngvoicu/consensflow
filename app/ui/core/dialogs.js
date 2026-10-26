import { button, element, redraw } from '../dom.js'
import { isMember } from './board.js'
import { ComputerUseSection } from './computer-use.js'
import { ProjectSections } from './project-sections.js'

/**
 * The dialogs that say who works on a project and how: New project (its
 * folder, the saved agent its chief runs on, and its settings: the first
 * staff, human approval, wake-up calls and specs), Switch chief and Project
 * settings (the same four, for a project that is open). Each keeps what it is
 * in the middle of to itself, and reads the page's board and saved agents as
 * they are when it draws or acts; what it changes on the page goes back
 * through the page's callbacks. A dialog that cannot open says why by
 * throwing, as the daemon's refusals do.
 */

const ROLES = ['worker', 'advisor', 'reviewer', 'tester', 'designer']
const ROLE_LABEL = {
  worker: 'Worker',
  advisor: 'Advisor',
  reviewer: 'Reviewer',
  tester: 'Tester',
  designer: 'Image designer',
}
/** The work tiers in the order the pick list groups them, most critical first, as the Agents screen names them. */
const TIER_LABEL = {
  critical: 'Critical work',
  complex: 'Complex work',
  standard: 'Standard work',
  light: 'Light work',
}
/** The harnesses in the order agents of one tier are listed. */
const HARNESS_ORDER = ['claude', 'codex', 'opencode', 'pi', 'devin']
/**
 * The efforts in the order agents of one harness are listed, the hardest
 * thinking first: Codex's ultra above max, and the levels below low (Pi's
 * off the last) before an agent that names none.
 */
const EFFORT_ORDER = ['ultra', 'max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'off']
const TIERS = Object.keys(TIER_LABEL)
const rank = (list, value) => (list.includes(value) ? list.indexOf(value) : list.length)
/** A staff reads by role, in the order the picker offers them, then by tier, the most critical first, then by name. */
const byRoleAndTier = (a, b) =>
  rank(ROLES, a.role) - rank(ROLES, b.role) ||
  rank(TIERS, a.tier) - rank(TIERS, b.tier) ||
  a.name.localeCompare(b.name)
/** The agents of one tier as a pick list has them: harness by harness, by effort, then by name. */
const byHarnessAndEffort = (a, b) =>
  rank(HARNESS_ORDER, a.harness) - rank(HARNESS_ORDER, b.harness) ||
  rank(EFFORT_ORDER, a.effort) - rank(EFFORT_ORDER, b.effort) ||
  a.name.localeCompare(b.name)
/**
 * "claude-sonnet-5 · claude · high · standard": what an agent runs and the
 * tier a task finds it by, effort included when it has one.
 */
const runsLabel = (agent, tier = agent.profile?.workTier) =>
  [agent.model ?? 'model unknown', agent.harness, agent.effort, tier].filter(Boolean).join(' · ')

/**
 * Whether an agent may take a role, by its designer flag: an image designer
 * is an image agent (a Codex agent that designs), and an image agent is
 * nothing else, the chief included. The daemon's `fitsRole`, which this page
 * cannot import.
 */
const fits = (agent, role) => (role === 'designer') === (agent.designer === true)

/**
 * A pick list's groups: one for each work tier, the most critical first, each
 * holding its agents in `byHarnessAndEffort` order, an option for each that
 * `option` makes.
 */
function tierGroups(agents, option) {
  const groups = new Map()
  for (const agent of agents) {
    const tier = agent.profile?.workTier
    if (!groups.has(tier)) groups.set(tier, [])
    groups.get(tier).push(agent)
  }
  return [...groups.entries()]
    .sort(([a], [b]) => rank(TIERS, a) - rank(TIERS, b))
    .map(([tier, agents]) => {
      const group = element('optgroup')
      group.label =
        tier in TIER_LABEL ? `T${TIERS.indexOf(tier) + 1} · ${TIER_LABEL[tier]}` : 'Tier unknown'
      for (const agent of agents.sort(byHarnessAndEffort)) group.append(option(agent))
      return group
    })
}

/** Why a role's pick list is empty: no agent saved, no image agent here, or each that fits holds it. */
function emptyHint(role, saved, fitting) {
  if (saved.length === 0) return 'No saved agents yet: add one under Settings, Agents.'
  if (role === 'designer') {
    return fitting.length === 0
      ? 'No image agent is on offer here: image agents run through Codex; install it from Agents, Harnesses.'
      : 'Every image agent is on the staff as Image designer already.'
  }
  return `Every saved agent is on the staff as ${ROLE_LABEL[role]} already.`
}

/**
 * The two selects that add a member: a role first, then the saved agents
 * that fit it (see `fits`) and do not hold it yet. `savedAgents()` is read
 * at each fill, so a role picked offers the agents saved by then. The
 * chosen agent survives a redraw when it is still on offer, and a role
 * picked refills the agents from the staff drawn last.
 */
function rolePicker(savedAgents, roleSelect, agentSelect, hint, holding, onRefill = () => {}) {
  if (roleSelect.options.length === 0) {
    for (const role of ROLES) {
      const option = element('option', null, ROLE_LABEL[role])
      option.value = role
      roleSelect.append(option)
    }
  }
  const refill = () => {
    const role = roleSelect.value
    const chosen = agentSelect.value
    const fitting = savedAgents().filter((agent) => !agent.hidden && fits(agent, role))
    const choices = fitting.filter((agent) => !holding(agent.name, role))
    // Every agent, the catalog's and the human's own, by the work it is for:
    // the most critical tier first, then harness by harness, by effort, by name.
    agentSelect.replaceChildren(
      ...tierGroups(choices, (agent) => {
        const option = element('option', null, `${agent.name} · ${runsLabel(agent, null)}`)
        option.value = agent.name
        return option
      }),
    )
    if (choices.some((agent) => agent.name === chosen)) agentSelect.value = chosen
    agentSelect.disabled = choices.length === 0
    // An empty list says why, so the answer is in the dialog, not in a guess.
    hint.textContent = choices.length === 0 ? emptyHint(role, savedAgents(), fitting) : ''
    hint.hidden = choices.length > 0
    onRefill()
  }
  // One handler, this drawing's: one added once kept the staff of the first.
  roleSelect.onchange = refill
  refill()
}

/** One row of a staff table: the agent, one of its roles, and a Remove for that role. */
function staffRow(who, role, remove) {
  const row = element('tr')
  row.dataset.role = role
  row.append(who, element('td', null, ROLE_LABEL[role]))
  const tools = element('td')
  tools.append(remove)
  row.append(tools)
  return row
}

/** A staff table's first cell: who, and what it runs. */
function whoCell(name, runs) {
  const cell = element('td')
  cell.append(
    element('span', 'member-name', name),
    element('br'),
    element('span', 'member-meta', runs),
  )
  return cell
}

/** A staff table with nobody in it says so, and what to do, across its three columns. */
function nobodyRow(text) {
  const row = element('tr', 'staff-empty')
  const cell = element('td', null, text)
  cell.colSpan = 3
  row.append(cell)
  return row
}

/** The harnesses a chief runs in, as the human knows them, in the order their agents are offered. */
export const HARNESS_NAMES = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  pi: 'Pi',
  devin: 'Devin',
}

/** The harness an agent names for a chief's kind: Claude Code's agents run on `claude`. */
const harnessOf = (kind) => (kind === 'claude-code' ? 'claude' : kind)

const NO_HARNESS = 'No harness is installed here: install one from Agents, Harnesses.'

/**
 * Fills a chief's pick list: the saved agents a chief may run on (no image
 * agent) on a harness a chief runs in that is installed here, grouped as the
 * staff's pick list is (`tierGroups`), after an empty choice that asks for one. A chief
 * runs on an agent the human picks, never on a harness's own default; the
 * agent it runs on now (`current`) is no switch. With no harness installed
 * here it refuses, and says where to get one.
 */
function fillChiefs(select, agents, missing, current = null) {
  const installed = Object.entries(HARNESS_NAMES).filter(
    ([kind]) => !missing.includes(harnessOf(kind)),
  )
  if (installed.length === 0) throw new Error(NO_HARNESS)
  const ask = new Option('Pick an agent', '')
  ask.disabled = true
  const harnesses = installed.map(([kind]) => harnessOf(kind))
  const runsHere = agents.filter(
    (agent) => !agent.hidden && harnesses.includes(agent.harness) && fits(agent, 'chief'),
  )
  const groups = tierGroups(runsHere, (agent) => {
    const option = new Option(`${agent.name} · ${runsLabel(agent, null)}`, agent.name)
    option.disabled = agent.name === current
    return option
  })
  select.replaceChildren(ask, ...groups)
  select.value = ''
}

/**
 * New project, on the folder the human picked: the saved agent its chief runs
 * on, among those on a harness installed here, and the project's settings, the
 * sections Project settings has (`ProjectSections`): the staff (the last
 * project's picked already), human approval, wake-up calls and specs. The
 * project starts with them all, in one request.
 */
export class NewProjectDialog {
  #dialog
  #form
  #staff
  #hint
  #refusal
  #sections
  #page
  /** The agents and roles picked for the new project, the last staff's to start with. */
  #picked = []
  /** Whether a start is on its way: a second ask waits for its answer, as the daemon's. */
  #starting = false

  /**
   * `page.agents()` are the page's saved agents as they are now, and
   * `page.onAgents(agents)` hands the page the ones read as the dialog
   * opens; `page.onStart(project)` is told of the project the daemon opened.
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws the page.
   * `page.specs()` is what the app says of specs as the page last read it, which
   * the Specs section says what following it means by, and `page.readSpecs()`
   * reads it again, as the dialog opens.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#staff = dialog.querySelector('#new-project-staff')
    this.#hint = dialog.querySelector('#new-project-hint')
    this.#refusal = dialog.querySelector('.dialog-refusal')
    this.#sections = new ProjectSections(
      dialog.querySelector('[data-project-sections]'),
      null,
      page.specs,
    )
    this.#page = page
    const form = this.#form
    dialog.querySelector('#new-project-add').addEventListener('click', () => {
      const role = form.elements.pickRole.value
      const agent = form.elements.pickAgent.value
      if (!agent) return
      this.#picked.push({ agent, role })
      this.#draw()
    })
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#start()
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /** Opens on `directory`; with no harness installed here it refuses, and says where to get one. */
  async open(directory) {
    const [{ agents, missing = [] }, { staff }] = await Promise.all([
      this.#page.daemon('agents.list'),
      this.#page.daemon('staff.last'),
      this.#page.readSpecs(),
    ])
    fillChiefs(this.#form.elements.chief, agents, missing)
    this.#page.onAgents(agents)
    this.#pickLast(staff)
    this.#sections.reset()
    this.#say('')
    this.#form.elements.directory.value = directory
    this.#dialog.showModal()
  }

  /** What is said above the buttons: the daemon's refusal of the start, until the next try or the next opening. */
  #say(words) {
    this.#refusal.textContent = words
    this.#refusal.hidden = words === ''
  }

  /**
   * Starts the project as the dialog says it, in one request: its chief, staff
   * and approval, and the calls and specs unless they are as the dialog opened.
   * The dialog stays until the daemon has opened the project. One it refuses (a
   * specs folder outside the project, an interval under a minute) is said here
   * in the daemon's own words, as the settings of a project say them, with
   * what the human chose still in the dialog, and no project is made.
   */
  async #start() {
    if (this.#starting) return
    this.#starting = true
    const form = this.#form
    const { gate, wakeUp, specs } = this.#sections.values()
    const body = {
      directory: form.elements.directory.value,
      agent: form.elements.chief.value,
      gate,
      staff: this.#pickedStaff(),
    }
    if (wakeUp !== null) body.wakeUp = wakeUp
    if (specs !== null) body.specs = specs
    this.#say('')
    let refused = null
    try {
      await this.#page.act(async () => {
        try {
          const { project } = await this.#page.daemon('project.open', body)
          this.#page.onStart(project)
          // Closed before the page is drawn for the project: its chief's terminal takes
          // the keyboard when its card is first shown, which a dialog over the page
          // would not let it.
          this.#dialog.close()
        } catch (cause) {
          // A dialog the human left meanwhile has no one to say it to: the status line does.
          if (!this.#dialog.open) throw cause
          refused = cause instanceof Error ? cause.message : String(cause)
        }
      })
    } finally {
      this.#starting = false
    }
    if (refused !== null) this.#say(refused)
  }

  /** The last project's staff, picked: its agents still saved and installed, in their roles. */
  #pickLast(lastStaff) {
    this.#picked = lastStaff.flatMap(({ agent, roles }) =>
      this.#page.agents().some((saved) => saved.name === agent && !saved.notInstalled)
        ? roles.map((role) => ({ agent, role }))
        : [],
    )
    this.#draw()
  }

  #draw() {
    const page = this.#page
    const rows = this.#picked
      .map((pick) => ({
        ...pick,
        name: pick.agent,
        tier: page.agents().find((candidate) => candidate.name === pick.agent)?.profile?.workTier,
      }))
      .sort(byRoleAndTier)
      .map(({ agent, role }) => {
        const saved = page.agents().find((candidate) => candidate.name === agent)
        const remove = button(
          'Remove',
          'quiet-button',
          () => {
            this.#picked.splice(
              this.#picked.findIndex((pick) => pick.agent === agent && pick.role === role),
              1,
            )
            this.#draw()
          },
          `Remove ${ROLE_LABEL[role]} ${agent}`,
        )
        const row = staffRow(whoCell(agent, runsLabel(saved)), role, remove)
        row.dataset.agent = agent
        return row
      })
    if (rows.length === 0) {
      rows.push(
        nobodyRow(
          page.agents().length === 0
            ? 'No agents saved yet: add some under Agents first.'
            : 'Nobody yet: pick a role, then an agent whose model suits it.',
        ),
      )
    }
    this.#staff.replaceChildren(...rows)
    rolePicker(
      page.agents,
      this.#form.elements.pickRole,
      this.#form.elements.pickAgent,
      this.#hint,
      (agent, role) => this.#picked.some((pick) => pick.agent === agent && pick.role === role),
    )
  }

  /** The picked rows as the daemon takes a staff: each agent once, with its roles. */
  #pickedStaff() {
    const staff = new Map()
    for (const { agent, role } of this.#picked) {
      if (!staff.has(agent)) staff.set(agent, { agent, roles: [] })
      staff.get(agent).roles.push(role)
    }
    return [...staff.values()]
  }
}

/**
 * Switch the chief: the chief goes on in a new window on another saved agent,
 * with its harness, model and effort, and the daemon hands the work over to it
 * (the dispatcher's switchChief).
 */
export class SwitchChiefDialog {
  #dialog
  #form
  #page
  /** The project whose chief the dialog switches: the one it was opened for. */
  #project = null

  /**
   * `page.selected()` is the project the human has chosen now, `page.board()`
   * its board, and `page.onSwitch()` is told once the daemon took the switch. `page.daemon`
   * asks the daemon, `page.act` runs a change and redraws the page.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#page = page
    const form = this.#form
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const agent = form.elements.chief.value
      const when = form.elements.when.value
      const askFirst = form.elements.note.checked
      const project = this.#project
      dialog.close()
      void page.act(async () => {
        await page.daemon('chief.switch', { project, agent, when, note: askFirst })
        page.onSwitch()
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /**
   * Opens for `chief`'s project: the saved agents on every harness installed
   * here, none picked yet. With none installed it refuses, and says where
   * to get one.
   */
  async open(chief) {
    const { agents, missing } = await this.#page.daemon('agents.list')
    // Asked for in a project the human has left since: it stays shut.
    if (this.#page.selected() !== chief.projectId) return
    const form = this.#form
    // A chief from before chiefs were agents runs on its harness's default: every agent is a switch.
    fillChiefs(form.elements.chief, agents, missing, chief.agent)
    // A chief whose agent is gone says so above the agents it may go on with.
    const lane = this.#page.board()?.lanes.find((each) => each.participant.id === chief.id)
    const gone = this.#dialog.querySelector('.chief-gone')
    gone.textContent = !lane?.agentMissing
      ? ''
      : lane.agentRetired
        ? `The chief runs on ${chief.agent}, which left the catalog: ${lane.agentRetired}. Pick another agent.`
        : `The chief runs on ${chief.agent}, which is no longer among your agents: pick another agent.`
    gone.hidden = gone.textContent === ''
    // Each switch starts from the gentle one: the chief finishes its turn, unasked.
    form.elements.when.value = 'turn'
    form.elements.note.checked = false
    this.#project = chief.projectId
    this.#dialog.showModal()
  }
}

/**
 * Project settings: the project's staff (who the chief may hand work to, drawn
 * from the project's board, and drawn again in place while open, so it stays
 * current), and the sections New project has too (`ProjectSections`): human
 * approval, wake-up calls and specs. Each saves as it always did: the staff as
 * it is added to or removed from, the approval as it is ticked, the calls and
 * the specs by their own Save.
 *
 * The dialog is for one project, the one it is asked for, from then until it
 * closes. Everything it shows is that project's and every change it makes goes
 * to it, by its id, whichever project the page shows meanwhile. It opens only
 * while the human is still on that project, and it takes the board of no other.
 */
export class ProjectSettingsDialog {
  #dialog
  #form
  #members
  #gate
  #hint
  #page
  /** The project's wake-up calls, read when the dialog opens and saved as a whole. */
  #wakeUp
  /** What the project says of specs: its own section of the dialog. */
  #specs
  /** Computer use: a part for each of the project's harnesses that has one. */
  #computerUse
  /** The project the dialog was last opened for, by id: the one it is for until it closes. */
  #project = null
  /** The board of that project, as the dialog last drew it. */
  #board = null
  /** The member whose removal waits for the human's yes, kept across redraws. */
  #removing = null

  /**
   * `page.board()` is the board shown, `page.selected()` the project the human
   * has chosen and `page.agents()` the saved agents, all as they are now;
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws the
   * page, `page.note` tells the human what came of it and `page.report` says
   * a refusal. `page.specs()` is what the app says of specs as the page last
   * read it, which the Specs section says what following it means by, and
   * `page.readSpecs()` reads it again, as the dialog opens.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#members = dialog.querySelector('#staff-members')
    this.#hint = dialog.querySelector('#staff-hint')
    this.#page = page
    // The sections save for the project the dialog is for, not for the one the page shows, and a
    // dialog that is shut is for none: what is answered after it is the status line's.
    const sections = new ProjectSections(
      dialog.querySelector('[data-project-sections]'),
      {
        project: () => (dialog.open ? this.#project : null),
        daemon: page.daemon,
        act: page.act,
        note: page.note,
        report: page.report,
      },
      page.specs,
    )
    this.#gate = sections.gate
    this.#wakeUp = sections.wakeUp
    this.#specs = sections.specs
    this.#computerUse = new ComputerUseSection(dialog.querySelector('.computer-use'), {
      daemon: page.daemon,
      act: page.act,
      report: page.report,
      project: () => (dialog.open ? this.#project : null),
    })
    // What the dialog changes is its project's, whichever board the page shows now.
    this.#gate.addEventListener('change', () => {
      const project = this.#project
      return page.act(async () => {
        await page.daemon('project.gate', { project, gate: this.#gate.checked })
        page.note(
          this.#gate.checked
            ? 'Every message between agents now waits for your approval.'
            : 'Messages between agents go straight through again.',
        )
      })
    })
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      const role = this.#form.elements.role.value
      const agent = this.#form.elements.agent.value
      if (!agent) return
      const project = this.#project
      // Already on the staff: the chief runs on an agent too, but is no member.
      const member = this.#board.lanes
        .map((lane) => lane.participant)
        .find((participant) => isMember(participant) && participant.agent === agent)
      void page.act(async () => {
        if (member === undefined) {
          await page.daemon('member.add', { project, agent, roles: [role] })
          page.note(`@${agent} joined the staff as ${ROLE_LABEL[role]}.`)
          return
        }
        await page.daemon('member.roles', { project, agent, roles: [...member.roles, role] })
        page.note(`@${agent} is ${ROLE_LABEL[role]} now too.`)
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /**
   * Opens for `project`, the one the human asked it for, with no removal waiting from
   * the last time: its wake-up calls as the daemon has them, and its staff, approval and
   * specs as its board says. It stays shut when, by the time the daemon has answered, the
   * human has gone on (another project is chosen, or the board shown is another's) or a
   * dialog is up already: what was asked for is not what the human is after any more.
   */
  async open(project) {
    const [calls, , computerUse] = await Promise.all([
      this.#wakeUp.read(project),
      this.#page.readSpecs(),
      this.#computerUse.read(project),
    ])
    const board = this.#page.board()
    if (this.#dialog.open || this.#page.selected() !== project || board?.project.id !== project) {
      return
    }
    this.#project = project
    this.#board = board
    this.#removing = null
    this.#wakeUp.show(calls)
    this.#computerUse.show(computerUse)
    this.#draw()
    this.#specs.draw(board.project, true)
    this.#dialog.showModal()
  }

  /**
   * Draws the staff again while the dialog is open, from the board of its project; the
   * board of another project is not its own, whenever it comes. A closed dialog is drawn
   * when it opens.
   */
  render() {
    const board = this.#page.board()
    if (!this.#dialog.open || board?.project.id !== this.#project) return
    this.#board = board
    this.#draw()
  }

  /** Draws the staff from the project's board, in place. */
  #draw() {
    const board = this.#board
    // The members only: a member's sessions are lanes too, named after it.
    const members = board.lanes.map((lane) => lane.participant).filter(isMember)
    const rows = members
      .flatMap((member) =>
        this.#memberRows(member).map((entry) => ({
          ...entry,
          tier: member.tier,
          name: member.handle,
        })),
      )
      .sort(byRoleAndTier)
      .map((entry) => entry.row)
    if (rows.length === 0) rows.push(nobodyRow('Nobody yet: add the agents this project may use.'))
    redraw(this.#members, rows)
    this.#gate.checked = board.project.gate ?? false
    this.#specs.draw(board.project)
    this.#computerUse.draw(board)
    rolePicker(
      this.#page.agents,
      this.#form.elements.role,
      this.#form.elements.agent,
      this.#hint,
      (agent, role) =>
        members.some((member) => member.agent === agent && member.roles.includes(role)),
      () => {
        this.#form.querySelector('[type="submit"]').disabled = this.#form.elements.agent.disabled
      },
    )
  }

  /** A member as the project's board has it now, which a Remove kept across redraws acts on. */
  #memberNow(member) {
    return (
      this.#board.lanes.find(
        (lane) => lane.participant.handle === member.handle && isMember(lane.participant),
      )?.participant ?? member
    )
  }

  /**
   * A member's rows, one per role, each with the role it stands for: Remove
   * drops that role, or, for its last role, asks first and takes the member
   * off the staff.
   */
  #memberRows(member) {
    const page = this.#page
    const name = `@${member.handle}`
    // What the member runs, from its agent; the tier is the staff's own, which follows the agent.
    const saved = page.agents().find((agent) => agent.name === member.agent)
    const retired = this.#board.lanes.find(
      (lane) => lane.participant.id === member.id,
    )?.agentRetired
    const runs = saved
      ? runsLabel(saved, member.tier)
      : retired
        ? `${member.agent} left the catalog: ${retired}. Remove it and add another agent`
        : `no agent named ${member.agent} any more: define one under Agents, or remove it`
    if (this.#removing === member.handle) {
      const row = element('tr')
      row.dataset.handle = member.handle
      row.append(whoCell(name, runs))
      // The ask takes the role's column and the Remove's.
      const cell = element('td')
      cell.colSpan = 2
      const keep = button(`Keep ${name}`, 'quiet-button', () => {
        this.#removing = null
        this.#draw()
      })
      const yes = button(`Remove ${name}`, 'danger-button', () =>
        page.act(async () => {
          const { handle } = this.#memberNow(member)
          await page.daemon('member.remove', { project: this.#project, agent: handle })
          this.#removing = null
          page.note(`${name} left the staff.`)
        }),
      )
      cell.append(
        element('span', 'member-confirm', `Remove ${name}? Its open tasks are cancelled. `),
        keep,
        yes,
      )
      row.append(cell)
      return [{ role: member.roles[0], row }]
    }
    return member.roles.map((role) => {
      const remove = button(
        'Remove',
        'quiet-button',
        () => {
          const { handle, roles } = this.#memberNow(member)
          if (roles.length === 1) {
            this.#removing = handle
            this.#draw()
            return
          }
          void page.act(async () => {
            await page.daemon('member.roles', {
              project: this.#project,
              agent: handle,
              roles: roles.filter((held) => held !== role),
            })
          })
        },
        `Remove ${ROLE_LABEL[role]} ${name}`,
      )
      const row = staffRow(whoCell(name, runs), role, remove)
      row.dataset.handle = member.handle
      return { role, row }
    })
  }
}
