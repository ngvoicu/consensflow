import { button, element, redraw } from '../dom.js'
import { isMember } from './board.js'

/**
 * The dialogs that say who works on a project: New project (its folder, the
 * saved agent its lead runs on, its first staff), Switch lead and the
 * project staff. Each keeps what it is in the middle of to itself, and reads
 * the page's board and saved agents as they are when it draws or acts; what
 * it changes on the page goes back through the page's callbacks. A dialog
 * that cannot open says why by throwing, as the core's refusals do.
 */

const ROLES = ['worker', 'advisor', 'reviewer', 'designer']
const ROLE_LABEL = {
  worker: 'Worker',
  advisor: 'Advisor',
  reviewer: 'Reviewer',
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
const HARNESS_ORDER = ['claude', 'codex', 'opencode', 'pi', 'devin', 'image']
const TIERS = Object.keys(TIER_LABEL)
const rank = (list, value) => (list.includes(value) ? list.indexOf(value) : list.length)
/** A staff reads by role, in the order the picker offers them, then by tier, the most critical first, then by name. */
const byRoleAndTier = (a, b) =>
  rank(ROLES, a.role) - rank(ROLES, b.role) ||
  rank(TIERS, a.tier) - rank(TIERS, b.tier) ||
  a.name.localeCompare(b.name)
/**
 * "claude-sonnet-5 · claude · high · standard": what an agent runs and the
 * tier a task finds it by, effort included when it has one.
 */
const runsLabel = (agent, tier = agent.profile?.workTier) =>
  [
    agent.model ?? 'model unknown',
    // An image agent runs through Codex: Codex is its harness to the human.
    agent.harness === 'image' ? 'codex' : agent.harness,
    agent.effort,
    tier,
  ]
    .filter(Boolean)
    .join(' · ')

/** Whether an agent may take a role: an image designer is an image agent, and an image agent is nothing else. */
const fits = (agent, role) => (role === 'designer') === (agent.harness === 'image')

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
    // the most critical tier first, then harness by harness, by name.
    const groups = new Map()
    for (const agent of choices) {
      const tier = agent.profile?.workTier
      if (!groups.has(tier)) groups.set(tier, [])
      groups.get(tier).push(agent)
    }
    agentSelect.replaceChildren(
      ...[...groups.entries()]
        .sort(([a], [b]) => rank(TIERS, a) - rank(TIERS, b))
        .map(([tier, agents]) => {
          const group = element('optgroup')
          group.label =
            tier in TIER_LABEL
              ? `T${TIERS.indexOf(tier) + 1} · ${TIER_LABEL[tier]}`
              : 'Tier unknown'
          agents.sort(
            (a, b) =>
              rank(HARNESS_ORDER, a.harness) - rank(HARNESS_ORDER, b.harness) ||
              a.name.localeCompare(b.name),
          )
          for (const agent of agents) {
            const option = element('option', null, `${agent.name} · ${runsLabel(agent, null)}`)
            option.value = agent.name
            group.append(option)
          }
          return group
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

/** The harnesses a lead runs in, as the human knows them, in the order their agents are offered. */
const HARNESS_NAMES = {
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
 * Fills a lead's pick list: the saved agents a lead may run on, harness by
 * harness for each harness a lead runs in that is installed here, by name,
 * after an empty choice that asks for one. A lead runs on an agent the human
 * picks, never on a harness's own default; the agent it runs on now
 * (`current`) is no switch. With no harness installed here it refuses, and
 * says where to get one.
 */
function fillLeads(select, agents, missing, current = null) {
  const installed = Object.entries(HARNESS_NAMES).filter(
    ([kind]) => !missing.includes(harnessOf(kind)),
  )
  if (installed.length === 0) throw new Error(NO_HARNESS)
  const ask = new Option('Pick an agent', '')
  ask.disabled = true
  const groups = installed.flatMap(([kind, label]) => {
    const runsHere = agents
      .filter((agent) => !agent.hidden && agent.harness === harnessOf(kind))
      .sort((a, b) => a.name.localeCompare(b.name))
    if (runsHere.length === 0) return []
    const group = element('optgroup')
    group.label = label
    for (const agent of runsHere) {
      const option = new Option(`${agent.name} · ${runsLabel(agent, null)}`, agent.name)
      option.disabled = agent.name === current
      group.append(option)
    }
    return [group]
  })
  select.replaceChildren(ask, ...groups)
  select.value = ''
}

/**
 * New project, on the folder the human picked: the saved agent its lead runs
 * on, among those on a harness installed here, the staff (the last
 * project's picked already) and the approval setting.
 */
export class NewProjectDialog {
  #dialog
  #form
  #staff
  #hint
  #page
  /** The agents and roles picked for the new project, the last staff's to start with. */
  #picked = []

  /**
   * `page.agents()` are the page's saved agents as they are now, and
   * `page.onAgents(agents)` hands the page the ones read as the dialog
   * opens; `page.onStart(project)` is told of the project the core opened.
   * `page.core` asks the core, `page.act` runs a change and redraws the page.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#staff = dialog.querySelector('#new-project-staff')
    this.#hint = dialog.querySelector('#new-project-hint')
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
      const directory = form.elements.directory.value
      const agent = form.elements.lead.value
      const gate = form.elements.gate.checked
      const staff = this.#pickedStaff()
      dialog.close()
      void page.act(async () => {
        const { project } = await page.core('project.open', { directory, agent, gate, staff })
        page.onStart(project)
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /** Opens on `directory`; with no harness installed here it refuses, and says where to get one. */
  async open(directory) {
    const [{ agents, missing = [] }, { staff }] = await Promise.all([
      this.#page.core('agents.list'),
      this.#page.core('staff.last'),
    ])
    fillLeads(this.#form.elements.lead, agents, missing)
    this.#page.onAgents(agents)
    this.#pickLast(staff)
    this.#form.elements.directory.value = directory
    this.#dialog.showModal()
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

  /** The picked rows as the core takes a staff: each agent once, with its roles. */
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
 * Switch the lead: the chief goes on in a new window on another saved agent,
 * with its harness, model and effort, and the core hands it the lead (the
 * dispatcher's switchChief).
 */
export class SwitchLeadDialog {
  #dialog
  #form
  #page
  /** The project whose lead the dialog switches: the one it was opened for. */
  #project = null

  /**
   * `page.selected()` is the project the human has chosen now, and
   * `page.onSwitch()` is told once the core took the switch. `page.core`
   * asks the core, `page.act` runs a change and redraws the page.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#page = page
    const form = this.#form
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const agent = form.elements.lead.value
      const when = form.elements.when.value
      const askFirst = form.elements.note.checked
      const project = this.#project
      dialog.close()
      void page.act(async () => {
        await page.core('chief.switch', { project, agent, when, note: askFirst })
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
    const { agents, missing } = await this.#page.core('agents.list')
    // Asked for in a project the human has left since: it stays shut.
    if (this.#page.selected() !== chief.projectId) return
    const form = this.#form
    // A lead from before leads were agents runs on its harness's default: every agent is a switch.
    fillLeads(form.elements.lead, agents, missing, chief.agent)
    // Each switch starts from the gentle one: the lead finishes its turn, unasked.
    form.elements.when.value = 'turn'
    form.elements.note.checked = false
    this.#project = chief.projectId
    this.#dialog.showModal()
  }
}

/**
 * The project staff: who the chief may hand work to, drawn from the board
 * shown, and drawn again in place while open, so it stays current.
 */
export class StaffDialog {
  #dialog
  #form
  #members
  #gate
  #hint
  #page
  /** The member whose removal waits for the human's yes, kept across redraws. */
  #removing = null

  /**
   * `page.board()` and `page.agents()` are the page's as they are now;
   * `page.core` asks the core, `page.act` runs a change and redraws the
   * page, `page.note` tells the human what came of it.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#members = dialog.querySelector('#staff-members')
    this.#gate = dialog.querySelector('#staff-gate')
    this.#hint = dialog.querySelector('#staff-hint')
    this.#page = page
    // The dialog's staff is the shown board's, and so is what it changes.
    this.#gate.addEventListener('change', () =>
      page.act(async () => {
        await page.core('project.gate', {
          project: page.board().project.id,
          gate: this.#gate.checked,
        })
        page.note(
          this.#gate.checked
            ? 'Every message between agents now waits for your approval.'
            : 'Messages between agents go straight through again.',
        )
      }),
    )
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      const role = this.#form.elements.role.value
      const agent = this.#form.elements.agent.value
      if (!agent) return
      const { project, lanes } = page.board()
      // Already on the staff: the lead runs on an agent too, but is no member.
      const member = lanes
        .map((lane) => lane.participant)
        .find((participant) => isMember(participant) && participant.agent === agent)
      void page.act(async () => {
        if (member === undefined) {
          await page.core('member.add', { project: project.id, agent, roles: [role] })
          page.note(`@${agent} joined the staff as ${ROLE_LABEL[role]}.`)
          return
        }
        await page.core('member.roles', {
          project: project.id,
          agent,
          roles: [...member.roles, role],
        })
        page.note(`@${agent} is ${ROLE_LABEL[role]} now too.`)
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /** Opens on the board shown, with no removal waiting from the last time. */
  open() {
    this.#removing = null
    this.#draw()
    this.#dialog.showModal()
  }

  /** Draws the staff again while the dialog is open; a closed one is drawn when it opens. */
  render() {
    if (this.#dialog.open) this.#draw()
  }

  /** Draws the staff from the board, in place. */
  #draw() {
    const lanes = this.#page.board()?.lanes ?? []
    // The members only: a member's sessions are lanes too, named after it.
    const members = lanes.map((lane) => lane.participant).filter(isMember)
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
    this.#gate.checked = this.#page.board()?.project.gate ?? false
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

  /** A member as the board shown has it now, which a Remove kept across redraws acts on. */
  #memberNow(member) {
    const board = this.#page.board()
    return (
      board?.lanes.find(
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
    const runs = saved
      ? runsLabel(saved, member.tier)
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
          const { projectId, handle } = this.#memberNow(member)
          await page.core('member.remove', { project: projectId, agent: handle })
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
          const { projectId, handle, roles } = this.#memberNow(member)
          if (roles.length === 1) {
            this.#removing = handle
            this.#draw()
            return
          }
          void page.act(async () => {
            await page.core('member.roles', {
              project: projectId,
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
