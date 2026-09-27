import { initializeUpdates } from '../updates.js'
import { BoardView, element, TaskDrawer } from './board.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board in the
 * middle, and the live windows on the right in a strip that scrolls sideways,
 * the chief first, so the human reads the board and talks to any of them. Everything it shows comes from the new core through
 * the app's `core_request`, and it redraws when the core says something
 * changed. It keeps nothing of its own but what is on screen.
 */

const tauri = window.__TAURI__ ?? {}
const invoke = tauri.core?.invoke
const listen = tauri.event?.listen
const Channel = tauri.core?.Channel

const $ = (selector) => document.querySelector(selector)
const projectList = $('#projects')
const projectTitle = $('#project-title')
const projectDirectory = $('#project-directory')
const inboxButton = $('#inbox-button')
const teamButton = $('#staff-button')
const boardRoot = $('#board')
const stage = $('#stage')
const status = $('#status')

const state = {
  projects: [],
  selected: null,
  board: null,
  inbox: [],
  agents: [],
  focus: 'chief',
  openTask: null,
}

function report(cause) {
  status.textContent = cause instanceof Error ? cause.message : String(cause)
  status.dataset.tone = 'error'
}

function note(text) {
  status.textContent = text
  status.dataset.tone = 'info'
}

async function core(operation, body = {}) {
  const result = await invoke('core_request', { operation, body })
  if (result?.ok !== true) throw new Error(result?.error ?? `${operation} did not answer`)
  return result
}

/**
 * Runs an action and redraws; a refusal shows in the status line. The redraw
 * happens either way, so a control the human already moved (a role box, say)
 * goes back to what the ledger holds when the core refuses the change.
 */
async function act(work) {
  try {
    await work()
  } catch (cause) {
    report(cause)
  }
  await refresh()
}

const board = new BoardView(boardRoot, {
  // A question put to the human is answered here and marked read.
  onAnswer: (message, text, choices) =>
    act(async () => {
      await core('message.answer', {
        question: message.id,
        ...(choices === undefined ? { body: text } : { choices }),
      })
      await core('message.read', { message: message.id })
      note(`Answer sent to @${message.sender}.`)
    }),
  onRead: (message) => act(() => core('message.read', { message: message.id })),
  // What waits for the human's approval goes on, goes back, or is declined with a word to its sender.
  onApprove: (message) =>
    act(async () => {
      await core('message.approve', { message: message.id })
      note(`m-${message.id} goes on to @${message.recipient}.`)
    }),
  onDecline: (message) =>
    act(async () => {
      await core('message.decline', { message: message.id })
      note(`m-${message.id} declined; @${message.sender} is told.`)
    }),
  onOpenTask: (number) => act(() => openTask(number)),
  // Opening a session's closed terminal brings it back on its own
  // conversation and shows it in the dock, the dock unfolded. Closing it ends
  // its process and takes its card away with it.
  onOpenTerminal: (participant) =>
    act(async () => {
      if (participant.member !== null) {
        await core('session.open', { project: state.selected, handle: participant.handle })
      }
      unfold('dock')
      state.focus = participant.handle
    }),
  onCloseTerminal: (participant) => closeTerminal(participant),
  onEndSession: (participant) =>
    act(async () => {
      await core('session.end', { project: state.selected, handle: participant.handle })
      note(`@${participant.handle} is gone; its tasks stay on @${participant.member}'s lane.`)
    }),
  onRedraw: () => render(),
})

const drawer = new TaskDrawer($('#task-drawer'), {
  onClose: () => {
    state.openTask = null
    drawer.hide()
  },
  onCancel: (task) =>
    act(() => core('task.cancel', { project: state.selected, task: task.number })),
  onPause: (task) =>
    act(async () => {
      await core('task.pause', { project: state.selected, task: task.number })
      note(`T-${task.number} is paused; its work waits.`)
    }),
  onReassign: (task) =>
    act(async () => {
      await core('task.reassign', { project: state.selected, task: task.number })
      note(
        `T-${task.number} is back on the board for another ${task.pool === 'designer' ? 'image designer' : `${task.tier} ${task.pool}`}.`,
      )
    }),
  onResume: (task) =>
    act(async () => {
      const { task: resumed } = await core('task.resume', {
        project: state.selected,
        task: task.number,
      })
      note(
        resumed.state === 'open'
          ? `T-${task.number} is back on the board: the window that had it has ended.`
          : `T-${task.number} resumes in @${resumed.assignee}.`,
      )
    }),
})

// The packaged smoke watches acks and arrivals here, on the real paths.
let ackObserver = null
let outputObserver = null
/** Closing a window, from its board row or its card: its process ends and its card goes with it. */
function closeTerminal(participant) {
  return act(async () => {
    await core('session.close', { project: state.selected, handle: participant.handle })
    terminals.forget(state.selected, participant.handle)
  })
}

const terminals = new TerminalsView(stage, {
  invoke: (command, args) => {
    if (command === 'pane_ack') ackObserver?.(args)
    return invoke(command, args)
  },
  report,
  createEmulator: tauri.test?.createEmulator,
  onChange: () => render(),
  onClose: closeTerminal,
})

async function openTask(number) {
  const [{ task }, transcript] = await Promise.all([
    core('task.get', { project: state.selected, task: number }),
    core('task.transcript', { project: state.selected, task: number }),
  ])
  state.openTask = number
  drawer.show(task, { transcript })
}

let refreshing = null
let again = false
async function refresh() {
  if (refreshing !== null) {
    again = true
    return refreshing
  }
  refreshing = (async () => {
    try {
      const { projects } = await core('projects.list')
      state.projects = projects
      if (!projects.some((project) => project.id === state.selected)) {
        state.selected = (projects.find((s) => s.state === 'open') ?? projects[0])?.id ?? null
      }
      if (state.selected === null) {
        state.board = null
        state.inbox = []
      } else {
        const [{ board: current }, { messages }] = await Promise.all([
          core('board.get', { project: state.selected }),
          core('inbox.get', { project: state.selected, participant: 'human' }),
        ])
        state.board = current
        state.inbox = messages
      }
      if (state.openTask !== null && drawer.open) await openTask(state.openTask)
      render()
      if (teamDialog.open) renderStaff()
    } catch (cause) {
      report(cause)
    } finally {
      refreshing = null
      if (again) {
        again = false
        void refresh()
      }
    }
  })()
  return refreshing
}

function render() {
  renderProjects()
  const project = state.projects.find((s) => s.id === state.selected) ?? null
  projectTitle.textContent = project?.name ?? 'No project'
  projectDirectory.textContent = project?.directory ?? ''
  const waiting = state.inbox.filter((message) => message.state === 'queued').length
  inboxButton.textContent = waiting === 0 ? 'Inbox' : `Inbox (${waiting})`
  inboxButton.dataset.waiting = String(waiting > 0)
  // A closed project is read-only: nothing runs, so nothing here may act on it.
  const suspended = project?.state === 'suspended'
  main.dataset.suspended = String(suspended)
  teamButton.disabled = project === null || suspended
  const lanes = state.board?.lanes ?? []
  if (!lanes.some((lane) => lane.participant.handle === state.focus)) state.focus = 'chief'
  if (suspended) {
    terminals.clear(state.selected)
    drawer.hide()
  }
  // A row's Terminal button stays live while an ended window is still readable.
  for (const lane of lanes) lane.ended = terminals.has(state.selected, lane.participant.handle)
  if (state.board === null) {
    boardRoot.replaceChildren(
      element(
        'p',
        'board-empty',
        'Start a project to see its board: choose New project and pick the project folder.',
      ),
    )
  } else {
    board.render({ board: state.board, inbox: state.inbox, agents: state.agents })
    if (suspended) boardRoot.prepend(suspendedBanner(project))
  }
  terminals.render(lanes, { focused: state.focus, project: state.selected })
}

/** What a closed project shows in place of its actions: why it is still, and the one way on. */
function suspendedBanner(project) {
  const banner = element('section', 'suspended')
  banner.setAttribute('role', 'status')
  banner.append(
    element('strong', null, `${project.name} is closed.`),
    element(
      'span',
      null,
      ' Its windows are gone, its open work went back to the backlog, and nothing is delivered until you resume it.',
    ),
  )
  const resume = element('button', 'primary-button', 'Resume project')
  resume.type = 'button'
  resume.addEventListener('click', () =>
    act(async () => {
      await core('project.resume', { project: project.id })
      state.focus = 'chief'
    }),
  )
  banner.append(resume)
  return banner
}

// Deleting a project is confirmed in a dialog that names it.
const deleteDialog = $('#delete-dialog')
const deleteTitle = $('#delete-title')
let deleting = null
function askToDelete(project) {
  deleting = project
  deleteTitle.textContent = `Delete ${project.name}?`
  deleteDialog.showModal()
}
$('#delete-confirm').addEventListener('click', () => {
  const project = deleting
  deleteDialog.close()
  if (project === null) return
  void act(async () => {
    await core('project.delete', { project: project.id })
    if (state.selected === project.id) state.selected = null
    note(`${project.name} is deleted.`)
  })
})
deleteDialog.addEventListener('close', () => {
  deleting = null
})

function renderProjects() {
  const items = state.projects.map((project) => {
    const item = element('li', 'project')
    item.dataset.state = project.state
    item.dataset.current = String(project.id === state.selected)
    const select = element('button', 'project-select')
    select.type = 'button'
    select.setAttribute('aria-current', String(project.id === state.selected))
    // A closed project says so by its buttons (Delete, Resume), not by a pill.
    select.append(element('span', 'project-name', project.name))
    select.addEventListener('click', () => {
      state.selected = project.id
      state.focus = 'chief'
      state.openTask = null
      drawer.hide()
      void refresh()
    })
    item.append(select)
    const open = project.state === 'open'
    const tools = element('div', 'project-tools')
    // A closed project may go for good; the ask is confirmed in a dialog.
    if (!open) {
      const remove = element('button', 'quiet-button', 'Delete')
      remove.type = 'button'
      remove.setAttribute('aria-label', `Delete ${project.name}`)
      remove.addEventListener('click', () => askToDelete(project))
      tools.append(remove)
    }
    const toggle = element('button', 'quiet-button', open ? 'Close' : 'Resume')
    toggle.type = 'button'
    toggle.setAttribute('aria-label', `${open ? 'Close' : 'Resume'} ${project.name}`)
    toggle.addEventListener('click', () =>
      act(() => core(open ? 'project.close' : 'project.resume', { project: project.id })),
    )
    tools.append(toggle)
    item.append(tools)
    return item
  })
  if (items.length === 0) items.push(element('li', 'projects-empty', 'No projects yet.'))
  projectList.replaceChildren(...items)
}

inboxButton.addEventListener('click', () => {
  boardRoot.querySelector('[data-handle="human"]')?.scrollIntoView({ block: 'start' })
})

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
const HARNESS_ORDER = ['claude', 'codex', 'opencode', 'pi', 'kimi', 'devin', 'image']
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

/**
 * The two selects that add a member: a role first, then the saved agents
 * that do not hold it yet; any agent may take any role. The chosen agent
 * survives a redraw when it is still on offer.
 */
function rolePicker(roleSelect, agentSelect, hint, holding, onRefill = () => {}) {
  if (roleSelect.options.length === 0) {
    for (const role of ROLES) {
      const option = element('option', null, ROLE_LABEL[role])
      option.value = role
      roleSelect.append(option)
    }
    roleSelect.addEventListener('change', () => refill())
  }
  const refill = () => {
    const role = roleSelect.value
    const chosen = agentSelect.value
    const choices = state.agents.filter((agent) => !agent.hidden && !holding(agent.name, role))
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
    hint.textContent =
      choices.length === 0
        ? state.agents.length === 0
          ? 'No saved agents yet: add one under Settings, Agents.'
          : `Every saved agent is on the staff as ${ROLE_LABEL[role]} already.`
        : ''
    hint.hidden = choices.length > 0
    onRefill()
  }
  refill()
}

/** One row of a staff table: the agent, one of its roles, and a Remove for that role. */
function teamRow(who, role, remove) {
  const row = element('tr')
  row.dataset.role = role
  row.append(who, element('td', null, ROLE_LABEL[role]))
  const tools = element('td')
  tools.append(remove)
  row.append(tools)
  return row
}

// New project: the native folder picker first, then the chief's harness, the
// staff (the last project's ticked already) and the approval setting.
const newProjectDialog = $('#new-project-dialog')
const newProjectForm = newProjectDialog.querySelector('form')
const newProjectStaff = $('#new-project-staff')
// Every chief harness the form knows; the dialog offers those installed here.
const CHIEF_HARNESSES = [...newProjectForm.elements.harness.options].map((option) => [
  option.value,
  option.textContent,
])
const HARNESS_OF_KIND = { 'claude-code': 'claude' }
$('#new-project').addEventListener('click', async () => {
  if (typeof tauri.dialog?.open !== 'function') {
    report('The folder picker is not available in this window.')
    return
  }
  try {
    const directory = await tauri.dialog.open({
      title: 'Choose the project folder',
      directory: true,
      multiple: false,
    })
    if (typeof directory !== 'string' || directory.length === 0) return
    const [{ agents, missing = [] }, { staff }] = await Promise.all([
      core('agents.list'),
      core('staff.last'),
    ])
    const chiefs = CHIEF_HARNESSES.filter(
      ([kind]) => !missing.includes(HARNESS_OF_KIND[kind] ?? kind),
    )
    if (chiefs.length === 0) {
      report('No harness is installed here: install one from Agents, Harnesses.')
      return
    }
    newProjectForm.elements.harness.replaceChildren(
      ...chiefs.map(([kind, label]) => new Option(label, kind)),
    )
    state.agents = agents
    renderNewProjectStaff(staff)
    newProjectForm.elements.directory.value = directory
    newProjectDialog.showModal()
  } catch (cause) {
    report(cause)
  }
})

/** The agents and roles picked for the new project, the last staff's to start with. */
let picked = []

function renderNewProjectStaff(lastStaff) {
  picked = lastStaff.flatMap(({ agent, roles }) =>
    state.agents.some((saved) => saved.name === agent && !saved.notInstalled)
      ? roles.map((role) => ({ agent, role }))
      : [],
  )
  drawNewProjectStaff()
}

function drawNewProjectStaff() {
  const rows = picked
    .map((pick) => ({
      ...pick,
      name: pick.agent,
      tier: state.agents.find((candidate) => candidate.name === pick.agent)?.profile?.workTier,
    }))
    .sort(byRoleAndTier)
    .map(({ agent, role }) => {
      const saved = state.agents.find((candidate) => candidate.name === agent)
      const who = element('td')
      who.append(
        element('span', 'member-name', agent),
        element('br'),
        element('span', 'member-meta', runsLabel(saved)),
      )
      const remove = element('button', 'quiet-button', 'Remove')
      remove.type = 'button'
      remove.setAttribute('aria-label', `Remove ${ROLE_LABEL[role]} ${agent}`)
      remove.addEventListener('click', () => {
        picked.splice(
          picked.findIndex((pick) => pick.agent === agent && pick.role === role),
          1,
        )
        drawNewProjectStaff()
      })
      const row = teamRow(who, role, remove)
      row.dataset.agent = agent
      return row
    })
  if (rows.length === 0) {
    const row = element('tr', 'staff-empty')
    const cell = element(
      'td',
      null,
      state.agents.length === 0
        ? 'No agents saved yet: add some under Agents first.'
        : 'Nobody yet: pick a role, then an agent whose model suits it.',
    )
    cell.colSpan = 3
    row.append(cell)
    rows.push(row)
  }
  newProjectStaff.replaceChildren(...rows)
  rolePicker(
    newProjectForm.elements.pickRole,
    newProjectForm.elements.pickAgent,
    $('#new-project-hint'),
    (agent, role) => picked.some((pick) => pick.agent === agent && pick.role === role),
  )
}

$('#new-project-add').addEventListener('click', () => {
  const role = newProjectForm.elements.pickRole.value
  const agent = newProjectForm.elements.pickAgent.value
  if (!agent) return
  picked.push({ agent, role })
  drawNewProjectStaff()
})

/** The picked rows as the core takes a staff: each agent once, with its roles. */
function pickedStaff() {
  const staff = new Map()
  for (const { agent, role } of picked) {
    if (!staff.has(agent)) staff.set(agent, { agent, roles: [] })
    staff.get(agent).roles.push(role)
  }
  return [...staff.values()]
}

newProjectForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const directory = newProjectForm.elements.directory.value
  const harness = newProjectForm.elements.harness.value
  const gate = newProjectForm.elements.gate.checked
  const staff = pickedStaff()
  newProjectDialog.close()
  void act(async () => {
    const { project } = await core('project.open', { directory, harness, gate, staff })
    state.selected = project.id
    state.focus = 'chief'
  })
})
newProjectDialog
  .querySelector('[value="cancel"]')
  .addEventListener('click', () => newProjectDialog.close())

// The project staff: who the chief may hand work to.
const teamDialog = $('#staff-dialog')
const teamForm = teamDialog.querySelector('form')
const teamList = $('#staff-members')
const teamGate = $('#staff-gate')
/** The member whose removal waits for the human's yes, kept across redraws. */
let removing = null

/** Draws the staff from the board, in place, so it stays current while open. */
function renderStaff() {
  const lanes = state.board?.lanes ?? []
  // The members only: a member's sessions are lanes too, named after it.
  const members = lanes
    .filter((lane) => lane.participant.agent !== null && lane.participant.member === null)
    .map((lane) => lane.participant)
  const rows = members
    .flatMap((member) =>
      memberRows(member).map((entry) => ({ ...entry, tier: member.tier, name: member.handle })),
    )
    .sort(byRoleAndTier)
    .map((entry) => entry.row)
  teamList.replaceChildren(...(rows.length ? rows : [element('tr', 'staff-empty')]))
  if (rows.length === 0) {
    const cell = element('td', null, 'Nobody yet: add the agents this project may use.')
    cell.colSpan = 4
    teamList.firstChild.append(cell)
  }
  teamGate.checked = state.board?.project.gate ?? false
  rolePicker(
    teamForm.elements.role,
    teamForm.elements.agent,
    $('#staff-hint'),
    (agent, role) =>
      members.some((member) => member.agent === agent && member.roles.includes(role)),
    () => {
      teamForm.querySelector('[type="submit"]').disabled = teamForm.elements.agent.disabled
    },
  )
}

/**
 * A member's rows, one per role, each with the role it stands for: Remove
 * drops that role, or, for its last role, asks first and takes the member
 * off the staff.
 */
function memberRows(member) {
  const name = `@${member.handle}`
  // What the member runs, from its agent; the tier is the staff's own, which follows the agent.
  const saved = state.agents.find((agent) => agent.name === member.agent)
  const who = () => {
    const cell = element('td')
    cell.append(
      element('span', 'member-name', name),
      element('br'),
      element(
        'span',
        'member-meta',
        saved
          ? runsLabel(saved, member.tier)
          : `no agent named ${member.agent} any more: define one under Agents, or remove it`,
      ),
    )
    return cell
  }
  if (removing === member.handle) {
    const row = element('tr')
    row.dataset.handle = member.handle
    row.append(who())
    const cell = element('td')
    cell.colSpan = 3
    const keep = element('button', 'quiet-button', `Keep ${name}`)
    keep.type = 'button'
    keep.addEventListener('click', () => {
      removing = null
      renderStaff()
    })
    const yes = element('button', 'danger-button', `Remove ${name}`)
    yes.type = 'button'
    yes.addEventListener('click', () =>
      act(async () => {
        await core('member.remove', { project: state.selected, agent: member.handle })
        removing = null
        note(`${name} left the staff.`)
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
    const remove = element('button', 'quiet-button', 'Remove')
    remove.type = 'button'
    remove.setAttribute('aria-label', `Remove ${ROLE_LABEL[role]} ${name}`)
    remove.addEventListener('click', () => {
      if (member.roles.length === 1) {
        removing = member.handle
        renderStaff()
        return
      }
      void act(async () => {
        await core('member.roles', {
          project: state.selected,
          agent: member.handle,
          roles: member.roles.filter((held) => held !== role),
        })
      })
    })
    const row = teamRow(who(), role, remove)
    row.dataset.handle = member.handle
    return { role, row }
  })
}

teamButton.addEventListener('click', () =>
  act(async () => {
    state.agents = (await core('agents.list')).agents
    removing = null
    renderStaff()
    teamDialog.showModal()
  }),
)
teamGate.addEventListener('change', () =>
  act(async () => {
    await core('project.gate', { project: state.selected, gate: teamGate.checked })
    note(
      teamGate.checked
        ? 'Every message between agents now waits for your approval.'
        : 'Messages between agents go straight through again.',
    )
  }),
)
teamForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const role = teamForm.elements.role.value
  const agent = teamForm.elements.agent.value
  if (!agent) return
  const member = (state.board?.lanes ?? []).find((lane) => lane.participant.agent === agent)
  void act(async () => {
    if (member === undefined) {
      await core('member.add', { project: state.selected, agent, roles: [role] })
      note(`@${agent} joined the staff as ${ROLE_LABEL[role]}.`)
      return
    }
    await core('member.roles', {
      project: state.selected,
      agent,
      roles: [...member.participant.roles, role],
    })
    note(`@${agent} is ${ROLE_LABEL[role]} now too.`)
  })
})
teamDialog.querySelector('[value="cancel"]').addEventListener('click', () => teamDialog.close())

// The human's agents screens: the daemon's own pages, in a frame each, at
// the URL and token the app was handed. Closing one refreshes the board's
// view of the agents (a tag or a tier may have changed).
// The agents screens open in their own window at the daemon's address: the
// board's page cannot frame them (WebKit blocks a plain-HTTP frame inside the
// app's secure page). Their edits show here once this window is back in front.
// The two side panels fold away and stay folded in this browser (a
// per-viewer convenience: storage may be missing, so every touch is guarded).
const shell = $('.shell')
const main = $('.main')
const FOLDS = [
  ['projects', shell, 'data-projects', $('#toggle-projects'), 'projects'],
  ['dock', main, 'data-dock', $('#toggle-dock'), 'terminals'],
]
const foldKey = (name) => `cf.layout.${name}`
function readFold(name) {
  try {
    return localStorage.getItem(foldKey(name)) === 'hidden' ? 'hidden' : 'shown'
  } catch {
    return 'shown'
  }
}
function applyFolds() {
  for (const [name, host, attribute, button, noun] of FOLDS) {
    const hidden = readFold(name) === 'hidden'
    host.setAttribute(attribute, hidden ? 'hidden' : 'shown')
    button.setAttribute('aria-pressed', String(!hidden))
    button.setAttribute('aria-label', `${hidden ? 'Show' : 'Hide'} ${noun}`)
  }
}
for (const [name, , , button] of FOLDS) {
  button.addEventListener('click', () => {
    const next = readFold(name) === 'hidden' ? 'shown' : 'hidden'
    try {
      localStorage.setItem(foldKey(name), next)
    } catch {
      // No storage: the fold still applies for this page.
    }
    applyFolds()
    render()
  })
}
applyFolds()

/** A panel the page needs to show comes back unfolded. */
function unfold(name) {
  try {
    localStorage.setItem(foldKey(name), 'shown')
  } catch {
    // No storage: shown for this page.
  }
  applyFolds()
}

const settingsDialog = $('#settings-dialog')
$('#settings-button').addEventListener('click', () => settingsDialog.showModal())
for (const entry of settingsDialog.querySelectorAll('[data-agents-page]')) {
  const page = entry.dataset.agentsPage
  entry.addEventListener('click', () =>
    act(async () => {
      settingsDialog.close()
      const opened = await invoke('open_agents_window', { page })
      if (opened?.ok !== true) {
        throw new Error(opened?.error ?? 'The agents screens are not available.')
      }
    }),
  )
}
window.addEventListener('focus', () => {
  void act(async () => {
    state.agents = (await core('agents.list')).agents
    if (teamDialog.open) renderStaff()
  })
})

async function start() {
  if (typeof invoke !== 'function') {
    report('ConsensFlow is not running this page.')
    return
  }
  if (typeof Channel === 'function') {
    const channel = new Channel()
    channel.onmessage = (message) => {
      outputObserver?.(message)
      terminals.output(message)
    }
    // Once, for the life of the page: a second subscription ends the first.
    await invoke('subscribe_output', { onOutput: channel })
  }
  if (typeof listen === 'function') {
    try {
      let pending = false
      await listen('state-changed', () => {
        if (pending) return
        pending = true
        setTimeout(() => {
          pending = false
          void refresh()
        }, 50)
      })
    } catch (cause) {
      report(cause)
    }
  }
  try {
    state.agents = (await core('agents.list')).agents
  } catch (cause) {
    report(cause)
  }
  await refresh()
  document.body.dataset.ready = 'true'
  const selftest = window.__CONSENSFLOW_SELFTEST__
  if (selftest !== null && typeof selftest === 'object') {
    const { runSelftest } = await import('../selftest.js')
    void runSelftest({
      config: selftest,
      invoke,
      core: (operation, body) => invoke('core_request', { operation, body }),
      refresh,
      registry: terminals.registry,
      sendInput: (pane, data) => terminals.input(pane, data),
      onAck: (observer) => {
        ackObserver = observer
      },
      onOutput: (observer) => {
        outputObserver = observer
      },
    })
  }
  await initializeUpdates({
    invoke,
    listen,
    getState: () => ({
      tabs: state.board
        ? [
            {
              name: state.board.project.name,
              panes: state.board.lanes
                .filter((lane) => lane.pane !== null)
                .map((lane) => ({ ...lane.pane, name: `@${lane.participant.handle}` })),
            },
          ]
        : [],
    }),
  })
}

void start()
