import { initializeUpdates } from '../updates.js'
import { BoardView, element, TaskDrawer } from './board.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board in the
 * middle, and the live windows on the right in a strip that scrolls sideways,
 * the lead first, so the human reads the board and talks to any of them. Everything it shows comes from the new core through
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
const teamButton = $('#team-button')
const boardRoot = $('#board')
const stage = $('#stage')
const status = $('#status')

const state = {
  projects: [],
  selected: null,
  board: null,
  inbox: [],
  agents: [],
  focus: 'lead',
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
  onGiveTask: (participant, text) =>
    act(async () => {
      const { task } = await core('task.add', {
        project: state.selected,
        to: participant.handle,
        body: text,
      })
      note(
        `T-${task.number} queued for ${participant.handle === 'lead' ? 'the lead' : `@${participant.handle}`}.`,
      )
    }),
  onPutTask: ({ pool, tier, purpose, needs }, text) =>
    act(async () => {
      const { task } = await core('task.add', {
        project: state.selected,
        pool,
        ...(tier === undefined ? {} : { tier }),
        ...(purpose === undefined ? {} : { purpose }),
        ...(needs === undefined ? {} : { needs }),
        body: text,
      })
      const waits =
        task.blockedBy.length === 0
          ? ''
          : ` It waits until ${task.blockedBy.map((number) => `T-${number}`).join(', ')} ${task.blockedBy.length === 1 ? 'is' : 'are'} accepted.`
      note(
        `T-${task.number} is on the board for ${pool === 'designer' ? 'an image designer' : `a ${tier} ${pool}`}.${waits}`,
      )
    }),
  // A question put to the lead (left unanswered, or still waiting for the
  // human's approval) is answered here too; it was never in the human's
  // inbox, so there is nothing to mark read.
  onAnswer: (message, text, choices) =>
    act(async () => {
      await core('message.answer', {
        question: message.id,
        ...(choices === undefined ? { body: text } : { choices }),
      })
      if (message.recipient === 'human') await core('message.read', { message: message.id })
      note(`Answer sent to @${message.sender}.`)
    }),
  onRead: (message) => act(() => core('message.read', { message: message.id })),
  // What waits for the human's approval goes on, goes back, or is declined with a word to its sender.
  onApprove: (message) =>
    act(async () => {
      await core('message.approve', { message: message.id })
      note(`m-${message.id} goes on to @${message.recipient}.`)
    }),
  onDecline: (message, reason) =>
    act(async () => {
      await core('message.decline', {
        message: message.id,
        ...(reason ? { reason } : {}),
      })
      note(`m-${message.id} declined; @${message.sender} is told.`)
    }),
  onSendBack: (message, text) =>
    act(async () => {
      await core('task.reopen', { project: state.selected, task: message.taskNumber, body: text })
      note(`T-${message.taskNumber} goes back to @${message.sender}.`)
    }),
  onOpenTask: (number) => act(() => openTask(number)),
  onOpenTerminal: (participant) => {
    state.focus = participant.handle
    render()
  },
  onRedraw: () => render(),
})

const drawer = new TaskDrawer($('#task-drawer'), {
  onClose: () => {
    state.openTask = null
    drawer.hide()
  },
  onAccept: (task) =>
    act(() => core('task.accept', { project: state.selected, task: task.number })),
  onReopen: (task, text) =>
    act(() => core('task.reopen', { project: state.selected, task: task.number, body: text })),
  onCancel: (task) =>
    act(() => core('task.cancel', { project: state.selected, task: task.number })),
  onReview: (task) =>
    act(async () => {
      await core('task.review', { project: state.selected, task: task.number })
      note(`T-${task.number} goes to an independent reviewer.`)
    }),
})

// The packaged smoke watches acks and arrivals here, on the real paths.
let ackObserver = null
let outputObserver = null
const terminals = new TerminalsView(stage, {
  invoke: (command, args) => {
    if (command === 'pane_ack') ackObserver?.(args)
    return invoke(command, args)
  },
  report,
  createEmulator: tauri.test?.createEmulator,
  onChange: () => render(),
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
      if (teamDialog.open) renderTeam()
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
  if (!lanes.some((lane) => lane.participant.handle === state.focus)) state.focus = 'lead'
  if (suspended) {
    terminals.clear()
    drawer.hide()
  }
  // A row's Terminal button stays live while an ended window is still readable.
  for (const lane of lanes) lane.ended = terminals.has(lane.participant.handle)
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
  terminals.render(lanes, { focused: state.focus })
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
      state.focus = 'lead'
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
    select.append(
      element('span', 'project-name', project.name),
      element('span', 'project-state', project.state === 'open' ? 'Open' : 'Suspended'),
    )
    select.addEventListener('click', () => {
      state.selected = project.id
      state.focus = 'lead'
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
/** Whether a saved agent's model suits a role, as the Agents screen's pills say. */
const suits = (agent, role) => (agent.profile?.categories ?? []).includes(role)
/** "zeus · claude · claude-sonnet-5 · high": what an agent runs, effort included when it has one. */
const agentLabel = (agent) =>
  [agent.name, agent.harness, agent.model ?? 'model unknown', agent.effort]
    .filter(Boolean)
    .join(' · ')

/**
 * The two selects that add a member: a role first, then the saved agents
 * whose model suits it and do not hold it yet. The chosen agent survives a
 * redraw when it is still on offer.
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
    const choices = state.agents.filter((agent) => suits(agent, role) && !holding(agent.name, role))
    agentSelect.replaceChildren(
      ...choices.map((agent) => {
        const option = element('option', null, agentLabel(agent))
        option.value = agent.name
        return option
      }),
    )
    if (choices.some((agent) => agent.name === chosen)) agentSelect.value = chosen
    agentSelect.disabled = choices.length === 0
    // An empty list says why, so the answer is in the dialog, not in a guess.
    hint.textContent =
      choices.length === 0
        ? state.agents.some((agent) => suits(agent, role))
          ? `Every saved agent that suits ${ROLE_LABEL[role]} is on the team in that role already.`
          : `No saved agent suits ${ROLE_LABEL[role]} yet: add one under Settings, Agents.`
        : ''
    hint.hidden = choices.length > 0
    onRefill()
  }
  refill()
}

/** One row of a team table: the agent, one of its roles, and a Remove for that role. */
function teamRow(who, role, remove) {
  const row = element('tr')
  row.dataset.role = role
  row.append(who, element('td', null, ROLE_LABEL[role]))
  const tools = element('td')
  tools.append(remove)
  row.append(tools)
  return row
}

/**
 * The review choices need a reviewer: without one only "none" can be picked,
 * and the warning says what to tick.
 */
function guardReview(select, warning, team) {
  const reviewers = team.some(({ roles }) => roles.includes('reviewer'))
  for (const option of select.options) option.disabled = option.value !== 'none' && !reviewers
  if (!reviewers) select.value = 'none'
  warning.textContent = reviewers
    ? ''
    : 'Reviews need a reviewer on the team: add one of the agents as Reviewer.'
  warning.hidden = reviewers
}

// New project: the native folder picker first, then the lead's harness, the
// team (the last project's ticked already) and the review policy.
const newProjectDialog = $('#new-project-dialog')
const newProjectForm = newProjectDialog.querySelector('form')
const newProjectTeam = $('#new-project-team')
const newProjectWarning = $('#new-project-warning')
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
    const [{ agents }, { team }] = await Promise.all([core('agents.list'), core('team.last')])
    state.agents = agents
    renderNewProjectTeam(team)
    newProjectForm.elements.directory.value = directory
    newProjectDialog.showModal()
  } catch (cause) {
    report(cause)
  }
})

/** The agents and roles picked for the new project, the last team's to start with. */
let picked = []

function renderNewProjectTeam(lastTeam) {
  picked = lastTeam.flatMap(({ agent, roles }) =>
    state.agents.some((saved) => saved.name === agent)
      ? roles.map((role) => ({ agent, role }))
      : [],
  )
  drawNewProjectTeam()
  newProjectForm.elements.review.value = picked.some(({ role }) => role === 'reviewer')
    ? 'members'
    : 'none'
  guardNewProjectReview()
}

function drawNewProjectTeam() {
  const rows = picked.map(({ agent, role }, at) => {
    const saved = state.agents.find((candidate) => candidate.name === agent)
    const who = element('td')
    who.append(
      element('span', 'member-name', agent),
      element('br'),
      element('span', 'member-meta', agentLabel(saved).slice(agent.length + 3)),
    )
    const remove = element('button', 'quiet-button', 'Remove')
    remove.type = 'button'
    remove.setAttribute('aria-label', `Remove ${ROLE_LABEL[role]} ${agent}`)
    remove.addEventListener('click', () => {
      picked.splice(at, 1)
      drawNewProjectTeam()
      guardNewProjectReview()
    })
    const row = teamRow(who, role, remove)
    row.dataset.agent = agent
    return row
  })
  if (rows.length === 0) {
    const row = element('tr', 'team-empty')
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
  newProjectTeam.replaceChildren(...rows)
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
  drawNewProjectTeam()
  guardNewProjectReview()
})

/** The picked rows as the core takes a team: each agent once, with its roles. */
function pickedTeam() {
  const team = new Map()
  for (const { agent, role } of picked) {
    if (!team.has(agent)) team.set(agent, { agent, roles: [] })
    team.get(agent).roles.push(role)
  }
  return [...team.values()]
}

const guardNewProjectReview = () =>
  guardReview(newProjectForm.elements.review, newProjectWarning, pickedTeam())

newProjectForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const directory = newProjectForm.elements.directory.value
  const harness = newProjectForm.elements.harness.value
  const review = newProjectForm.elements.review.value
  const gate = newProjectForm.elements.gate.checked
  const team = pickedTeam()
  newProjectDialog.close()
  void act(async () => {
    const { project } = await core('project.open', { directory, harness, review, gate, team })
    state.selected = project.id
    state.focus = 'lead'
  })
})
newProjectDialog
  .querySelector('[value="cancel"]')
  .addEventListener('click', () => newProjectDialog.close())

// The project team: who the lead may hand work to.
const teamDialog = $('#team-dialog')
const teamForm = teamDialog.querySelector('form')
const teamList = $('#team-members')
const teamReview = $('#team-review')
const teamGate = $('#team-gate')
const teamWarning = $('#team-warning')
/** The member whose removal waits for the human's yes, kept across redraws. */
let removing = null

/** Draws the team from the board, in place, so it stays current while open. */
function renderTeam() {
  const lanes = state.board?.lanes ?? []
  const members = lanes
    .filter((lane) => lane.participant.agent !== null)
    .map((lane) => lane.participant)
  const rows = members.flatMap((member) => memberRows(member))
  teamList.replaceChildren(...(rows.length ? rows : [element('tr', 'team-empty')]))
  if (rows.length === 0) {
    const cell = element('td', null, 'Nobody yet: add the agents this project may use.')
    cell.colSpan = 4
    teamList.firstChild.append(cell)
  }
  teamReview.value = state.board?.project.review ?? 'none'
  teamGate.checked = state.board?.project.gate ?? false
  guardReview(
    teamReview,
    teamWarning,
    members.map((member) => ({ roles: member.roles })),
  )
  rolePicker(
    teamForm.elements.role,
    teamForm.elements.agent,
    $('#team-hint'),
    (agent, role) =>
      members.some((member) => member.agent === agent && member.roles.includes(role)),
    () => {
      teamForm.querySelector('[type="submit"]').disabled = teamForm.elements.agent.disabled
    },
  )
}

/**
 * A member's rows, one per role: Remove drops that role, or, for its last
 * role, asks first and takes the member off the team.
 */
function memberRows(member) {
  const name = `@${member.handle}`
  const who = () => {
    const cell = element('td')
    cell.append(
      element('span', 'member-name', name),
      element('br'),
      element('span', 'member-meta', `${member.harness ?? ''} · ${member.tier ?? ''}`),
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
      renderTeam()
    })
    const yes = element('button', 'danger-button', `Remove ${name}`)
    yes.type = 'button'
    yes.addEventListener('click', () =>
      act(async () => {
        await core('member.remove', { project: state.selected, agent: member.handle })
        removing = null
        note(`${name} left the team.`)
      }),
    )
    cell.append(
      element('span', 'member-confirm', `Remove ${name}? Its open tasks are cancelled. `),
      keep,
      yes,
    )
    row.append(cell)
    return [row]
  }
  return member.roles.map((role) => {
    const remove = element('button', 'quiet-button', 'Remove')
    remove.type = 'button'
    remove.setAttribute('aria-label', `Remove ${ROLE_LABEL[role]} ${name}`)
    remove.addEventListener('click', () => {
      if (member.roles.length === 1) {
        removing = member.handle
        renderTeam()
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
    return row
  })
}

teamButton.addEventListener('click', () =>
  act(async () => {
    state.agents = (await core('agents.list')).agents
    removing = null
    renderTeam()
    teamDialog.showModal()
  }),
)
teamReview.addEventListener('change', () =>
  act(async () => {
    await core('project.review', { project: state.selected, review: teamReview.value })
    note(
      {
        none: 'Finished work goes straight to whoever asked.',
        members: "Workers' finished work gets a second review.",
      }[teamReview.value],
    )
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
      note(`@${agent} joined the team as ${ROLE_LABEL[role]}.`)
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
  ['dock', main, 'data-dock', $('#toggle-dock'), 'windows'],
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
    if (teamDialog.open) renderTeam()
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
