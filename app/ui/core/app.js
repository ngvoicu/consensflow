import { button, element, redraw } from '../dom.js'
import { initializeUpdates } from '../updates.js'
import { BoardView, HARNESS_NAMES, TaskDrawer } from './board.js'
import { Layout } from './layout.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board in the
 * middle, and the live windows on the right in a strip that scrolls sideways,
 * the chief first, so the human reads the board and talks to any of them.
 * Everything it shows comes from the new core through the app's
 * `core_request`, and it redraws when the core says something changed. It
 * keeps nothing of its own but what is on screen.
 */

const tauri = window.__TAURI__ ?? {}
const invoke = tauri.core?.invoke
const listen = tauri.event?.listen
const Channel = tauri.core?.Channel

const $ = (selector) => document.querySelector(selector)
const main = $('.main')
const projectList = $('#projects')
const projectTitle = $('#project-title')
const projectDirectory = $('#project-directory')
const inboxButton = $('#inbox-button')
const staffButton = $('#staff-button')
const boardRoot = $('#board')
const stage = $('#stage')
const status = $('#status')

/**
 * No notes for the human, in the shape the core reads their unread ones:
 * the newest one frame holds, how many there are and how many came.
 */
const NO_NOTES = { messages: [], total: 0, shown: 0 }

const state = {
  projects: [],
  selected: null,
  board: null,
  inbox: NO_NOTES,
  agents: [],
  focus: 'chief',
  openTask: null,
}

// The status line says what just happened, then goes: a note after a few
// seconds, a refusal a little later, so it is read; left, it sat over the
// windows long after it meant anything ("btb is deleted.").
const NOTE_MS = 6_000
const REFUSAL_MS = 12_000
let statusTimer = null
function say(text, tone, ms) {
  status.textContent = text
  status.dataset.tone = tone
  clearTimeout(statusTimer)
  statusTimer = setTimeout(() => {
    status.textContent = ''
    delete status.dataset.tone
  }, ms)
}

function report(cause) {
  say(cause instanceof Error ? cause.message : String(cause), 'error', REFUSAL_MS)
}

function note(text) {
  say(text, 'info', NOTE_MS)
}

async function core(operation, body = {}) {
  const result = await invoke('core_request', { operation, body })
  // A request the app could not hand to the daemon says why in `detail`.
  if (result?.ok !== true) {
    throw new Error(result?.detail ?? result?.error ?? `${operation} did not answer`)
  }
  return result
}

/** The saved agents, read again: what the board says members run, and what the pickers offer. */
async function readAgents() {
  state.agents = (await core('agents.list')).agents
}

// The daemon down: the banner says why and what comes next, and stays until
// the daemon is back, when everything is read again.
const coreDown = $('#core-down')
function coreStatus({ available, cause, retrying }) {
  const wasDown = !coreDown.hidden
  coreDown.hidden = available
  if (available) {
    if (wasDown) void act(readAgents)
    return
  }
  coreDown.replaceChildren(
    element('strong', null, 'The daemon is not running.'),
    ...(cause ? [` ${sentence(cause)}`] : []),
    retrying ? ' Starting it again…' : ' Quit ConsensFlow and open it again.',
  )
}

/** "It stopped." for "it stopped": a cause read as a sentence of its own. */
const sentence = (text) =>
  `${text.charAt(0).toUpperCase()}${text.slice(1)}${/[.!?…]$/.test(text) ? '' : '.'}`

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
  // Each control acts on the project of the board it is on: not yet the one
  // the human just chose, while that one's board is on its way.
  onOpenTask: (number) => act(() => openTask(state.board.project.id, number)),
  // Opening a session's closed terminal brings it back on its own
  // conversation and shows it in the dock, the dock unfolded. Closing it ends
  // its process and takes its card away with it.
  onOpenTerminal: (participant) =>
    act(async () => {
      await core('session.open', { project: participant.projectId, handle: participant.handle })
      layout.unfold('dock')
      state.focus = participant.handle
    }),
  onCloseTerminal: (participant) => closeTerminal(participant),
  onEndSession: (participant) =>
    act(async () => {
      await core('session.end', { project: participant.projectId, handle: participant.handle })
      note(`@${participant.handle} is gone; its tasks stay on @${participant.member}'s lane.`)
    }),
  onSwitchLead: (chief) => act(() => openSwitchLead(chief)),
  onResume: (project) =>
    act(async () => {
      await core('project.resume', { project: project.id })
      state.focus = 'chief'
    }),
})

// The drawer's actions are on its task's own project.
const drawer = new TaskDrawer($('#task-drawer'), {
  onClose: () => closeTask(),
  onCancel: (task) =>
    act(() => core('task.cancel', { project: task.projectId, task: task.number })),
  onPause: (task) =>
    act(async () => {
      await core('task.pause', { project: task.projectId, task: task.number })
      note(`T-${task.number} is paused; its work waits.`)
    }),
  onReassign: (task) =>
    act(async () => {
      await core('task.reassign', { project: task.projectId, task: task.number })
      note(
        `T-${task.number} is back on the board for another ${task.pool === 'designer' ? 'image designer' : `${task.tier} ${task.pool}`}.`,
      )
    }),
  onResume: (task) =>
    act(async () => {
      const { task: resumed } = await core('task.resume', {
        project: task.projectId,
        task: task.number,
      })
      note(
        resumed.state === 'open'
          ? `T-${task.number} is back on the board: the window that had it has ended.`
          : `T-${task.number} resumes in @${resumed.assignee}.`,
      )
    }),
  // What a task's window wrote, read when its fold opens.
  onTranscript: async (task) => {
    try {
      drawer.fill(
        task,
        await core('task.transcript', { project: task.projectId, task: task.number }),
      )
    } catch (cause) {
      report(cause)
    }
  },
})

// The packaged smoke watches acks and arrivals here, on the real paths.
let ackObserver = null
let outputObserver = null
/** Closing a window, from its board row or its card: its process ends and its card goes with it. */
function closeTerminal(participant) {
  return act(async () => {
    await core('session.close', { project: participant.projectId, handle: participant.handle })
    terminals.forget(participant.projectId, participant.handle)
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

// A fold changes the room the board and the windows have: both draw again.
const layout = new Layout({ onFold: () => render() })

/**
 * How many items a task's window wrote. Only the count: even that walks the
 * window's whole copy, so it is read when a drawer opens or its task changed.
 */
const written = async (project, number) =>
  (await core('task.transcript', { project, task: number, limit: 0 })).total

const closedProject = (id) => state.projects.find((project) => project.id === id)?.state !== 'open'

/** A task's drawer; one asked for in a project the human has left since stays shut. */
async function openTask(project, number) {
  const [{ task }, total] = await Promise.all([
    core('task.get', { project, task: number }),
    written(project, number),
  ])
  if (state.selected !== project) return
  state.openTask = { project, number, total }
  drawer.show(task, { total, closed: closedProject(project) })
}

function closeTask() {
  state.openTask = null
  drawer.hide()
}

/**
 * The open task read again, once the board is drawn: a task that went (its
 * project shown no more, closed or deleted) closes its drawer, and nothing
 * it says keeps the board from drawing. Its drawer changes only where the
 * task did.
 */
async function rereadTask() {
  const opened = state.openTask
  if (opened === null) return
  const { project, number } = opened
  if (state.board?.project.id !== project) {
    closeTask()
    return
  }
  try {
    const { task } = await core('task.get', { project, task: number })
    if (!drawer.shows(task)) opened.total = await written(project, number)
    // Closed, or another task opened, while these were on their way.
    if (state.openTask !== opened) return
    drawer.show(task, { total: opened.total, closed: closedProject(project) })
  } catch (cause) {
    closeTask()
    report(cause)
  }
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
      // A board is drawn only for the project it was read for: one the human
      // left while it was on its way is read again for the one chosen now.
      const selected = state.selected
      const open = projects.filter((project) => project.state === 'open')
      // The other open projects whose windows the dock holds: their own
      // boards say when those end. One that cannot be read says nothing yet.
      const elsewhere = open
        .filter((project) => project.id !== selected && terminals.holds(project.id))
        .map((project) =>
          core('board.get', { project: project.id }).then(
            (read) => read.board,
            () => null,
          ),
        )
      const [board, inbox, ...others] = await Promise.all([
        selected === null
          ? null
          : core('board.get', { project: selected }).then((read) => read.board),
        selected === null
          ? NO_NOTES
          : core('inbox.get', { project: selected, participant: 'human', unread: true }),
        ...elsewhere,
      ])
      if (state.selected !== selected) {
        again = true
        return
      }
      state.board = board
      state.inbox = inbox
      terminals.reconcile(
        [board, ...others].filter((read) => read !== null),
        new Set(open.map((project) => project.id)),
      )
      render()
      if (staffDialog.open) renderStaff()
      await rereadTask()
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
  // Everything here is the board's, under the project it was read for.
  const project = state.board?.project ?? null
  projectTitle.textContent = project?.name ?? 'No project'
  projectDirectory.textContent = project?.directory ?? ''
  // What For you lists for the human: their unread notes, shown or not.
  const waiting = state.inbox.total
  inboxButton.textContent = waiting === 0 ? 'Inbox' : `Inbox (${waiting})`
  inboxButton.dataset.waiting = String(waiting > 0)
  // A closed project is read-only: nothing runs, so nothing here may act on
  // it. Its board and its tasks still read.
  const suspended = project?.state === 'suspended'
  main.dataset.suspended = String(suspended)
  staffButton.disabled = project === null || suspended
  const lanes = state.board?.lanes ?? []
  if (!lanes.some((lane) => lane.participant.handle === state.focus)) state.focus = 'chief'
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
  }
  terminals.render(state.board, { focused: state.focus })
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
  const shown = state.board?.project.id ?? null
  const items = state.projects.map((project) => {
    const item = element('li', 'project')
    // The item names its project: a redraw keeps it only for that one.
    item.dataset.project = String(project.id)
    item.dataset.current = String(project.id === shown)
    const select = button('', 'project-select', () => {
      state.selected = project.id
      state.focus = 'chief'
      closeTask()
      void refresh()
    })
    select.setAttribute('aria-current', String(project.id === shown))
    // A closed project says so by its buttons (Delete, Resume), not by a pill.
    select.append(element('span', 'project-name', project.name))
    item.append(select)
    const open = project.state === 'open'
    const tools = element('div', 'project-tools')
    // A closed project may go for good; the ask is confirmed in a dialog.
    if (!open) {
      tools.append(
        button('Delete', 'quiet-button', () => askToDelete(project), `Delete ${project.name}`),
      )
    }
    const verb = open ? 'Close' : 'Resume'
    tools.append(
      button(
        verb,
        'quiet-button',
        () => act(() => core(open ? 'project.close' : 'project.resume', { project: project.id })),
        `${verb} ${project.name}`,
      ),
    )
    item.append(tools)
    return item
  })
  if (items.length === 0) items.push(element('li', 'projects-empty', 'No projects yet.'))
  redraw(projectList, items)
}

// The notes are in For you, at the top of the board: a folded board unfolds for them.
inboxButton.addEventListener('click', () => {
  layout.unfold('board')
  boardRoot.querySelector('.foryou')?.scrollIntoView({ block: 'start' })
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

/**
 * The two selects that add a member: a role first, then the saved agents
 * that do not hold it yet; any agent may take any role. The chosen agent
 * survives a redraw when it is still on offer, and a role picked refills
 * the agents from the staff drawn last.
 */
function rolePicker(roleSelect, agentSelect, hint, holding, onRefill = () => {}) {
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

/** The harness an agent names for a chief's kind: Claude Code's agents run on `claude`. */
const harnessOf = (kind) => (kind === 'claude-code' ? 'claude' : kind)

/** The chief harnesses installed here, as [kind, label]: what a lead may run in. */
const installedChiefs = (missing) =>
  Object.entries(HARNESS_NAMES).filter(([kind]) => !missing.includes(harnessOf(kind)))

const NO_HARNESS = 'No harness is installed here: install one from Agents, Harnesses.'

// New project: the native folder picker first, then the chief's harness, the
// staff (the last project's ticked already) and the approval setting.
const newProjectDialog = $('#new-project-dialog')
const newProjectForm = newProjectDialog.querySelector('form')
const newProjectStaff = $('#new-project-staff')
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
    const chiefs = installedChiefs(missing)
    if (chiefs.length === 0) {
      report(NO_HARNESS)
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
      const remove = button(
        'Remove',
        'quiet-button',
        () => {
          picked.splice(
            picked.findIndex((pick) => pick.agent === agent && pick.role === role),
            1,
          )
          drawNewProjectStaff()
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
        state.agents.length === 0
          ? 'No agents saved yet: add some under Agents first.'
          : 'Nobody yet: pick a role, then an agent whose model suits it.',
      ),
    )
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

// Switch the lead: the chief goes on in a new window on another harness or
// model, and the core hands it the lead (the dispatcher's switchChief).
const switchLeadDialog = $('#switch-lead-dialog')
const switchLeadForm = switchLeadDialog.querySelector('form')
/** The project whose lead the dialog switches: the one it was opened for. */
let switchingProject = null

async function openSwitchLead(chief) {
  const { agents, missing } = await core('agents.list')
  // Asked for in a project the human has left since: it stays shut.
  if (state.selected !== chief.projectId) return
  const chiefs = installedChiefs(missing)
  if (chiefs.length === 0) {
    report(NO_HARNESS)
    return
  }
  const select = switchLeadForm.elements.lead
  select.replaceChildren(
    ...chiefs.map(([kind, label]) => {
      const group = element('optgroup')
      group.label = label
      group.append(new Option(`${label}, on its own default model`, `harness:${kind}`))
      const runsHere = agents
        .filter((agent) => !agent.hidden && agent.harness === harnessOf(kind))
        .sort((a, b) => a.name.localeCompare(b.name))
      for (const agent of runsHere) {
        group.append(new Option(`${agent.name} · ${runsLabel(agent, null)}`, `agent:${agent.name}`))
      }
      return group
    }),
  )
  // What the lead runs on now is no switch.
  const current = chief.agent === null ? `harness:${chief.harness}` : `agent:${chief.agent}`
  for (const option of select.options) option.disabled = option.value === current
  select.value = [...select.options].find((option) => !option.disabled)?.value ?? ''
  // Each switch starts from the gentle one: the lead finishes its turn, unasked.
  switchLeadForm.elements.when.value = 'turn'
  switchLeadForm.elements.note.checked = false
  switchingProject = chief.projectId
  switchLeadDialog.showModal()
}

switchLeadForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const [type, name] = switchLeadForm.elements.lead.value.split(':')
  const when = switchLeadForm.elements.when.value
  const askFirst = switchLeadForm.elements.note.checked
  const project = switchingProject
  switchLeadDialog.close()
  void act(async () => {
    await core('chief.switch', {
      project,
      ...(type === 'agent' ? { agent: name } : { harness: name }),
      when,
      note: askFirst,
    })
    state.focus = 'chief'
  })
})
switchLeadDialog
  .querySelector('[value="cancel"]')
  .addEventListener('click', () => switchLeadDialog.close())

// The project staff: who the chief may hand work to.
const staffDialog = $('#staff-dialog')
const staffForm = staffDialog.querySelector('form')
const staffMembers = $('#staff-members')
const staffGate = $('#staff-gate')
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
  if (rows.length === 0) rows.push(nobodyRow('Nobody yet: add the agents this project may use.'))
  redraw(staffMembers, rows)
  staffGate.checked = state.board?.project.gate ?? false
  rolePicker(
    staffForm.elements.role,
    staffForm.elements.agent,
    $('#staff-hint'),
    (agent, role) =>
      members.some((member) => member.agent === agent && member.roles.includes(role)),
    () => {
      staffForm.querySelector('[type="submit"]').disabled = staffForm.elements.agent.disabled
    },
  )
}

/** A member as the board shown has it now, which a Remove kept across redraws acts on. */
const memberNow = (member) =>
  state.board?.lanes.find(
    (lane) => lane.participant.handle === member.handle && lane.participant.member === null,
  )?.participant ?? member

/**
 * A member's rows, one per role, each with the role it stands for: Remove
 * drops that role, or, for its last role, asks first and takes the member
 * off the staff.
 */
function memberRows(member) {
  const name = `@${member.handle}`
  // What the member runs, from its agent; the tier is the staff's own, which follows the agent.
  const saved = state.agents.find((agent) => agent.name === member.agent)
  const runs = saved
    ? runsLabel(saved, member.tier)
    : `no agent named ${member.agent} any more: define one under Agents, or remove it`
  if (removing === member.handle) {
    const row = element('tr')
    row.dataset.handle = member.handle
    row.append(whoCell(name, runs))
    // The ask takes the role's column and the Remove's.
    const cell = element('td')
    cell.colSpan = 2
    const keep = button(`Keep ${name}`, 'quiet-button', () => {
      removing = null
      renderStaff()
    })
    const yes = button(`Remove ${name}`, 'danger-button', () =>
      act(async () => {
        const { projectId, handle } = memberNow(member)
        await core('member.remove', { project: projectId, agent: handle })
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
    const remove = button(
      'Remove',
      'quiet-button',
      () => {
        const { projectId, handle, roles } = memberNow(member)
        if (roles.length === 1) {
          removing = handle
          renderStaff()
          return
        }
        void act(async () => {
          await core('member.roles', {
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

staffButton.addEventListener('click', () =>
  act(async () => {
    await readAgents()
    removing = null
    renderStaff()
    staffDialog.showModal()
  }),
)
// The dialog's staff is the shown board's, and so is what it changes.
staffGate.addEventListener('change', () =>
  act(async () => {
    await core('project.gate', { project: state.board.project.id, gate: staffGate.checked })
    note(
      staffGate.checked
        ? 'Every message between agents now waits for your approval.'
        : 'Messages between agents go straight through again.',
    )
  }),
)
staffForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const role = staffForm.elements.role.value
  const agent = staffForm.elements.agent.value
  if (!agent) return
  const { project, lanes } = state.board
  const member = lanes.find((lane) => lane.participant.agent === agent)
  void act(async () => {
    if (member === undefined) {
      await core('member.add', { project: project.id, agent, roles: [role] })
      note(`@${agent} joined the staff as ${ROLE_LABEL[role]}.`)
      return
    }
    await core('member.roles', {
      project: project.id,
      agent,
      roles: [...member.participant.roles, role],
    })
    note(`@${agent} is ${ROLE_LABEL[role]} now too.`)
  })
})
staffDialog.querySelector('[value="cancel"]').addEventListener('click', () => staffDialog.close())

// The agents screens open in their own window at the daemon's address: the
// board's page cannot frame them (WebKit blocks a plain-HTTP frame inside the
// app's secure page). Their edits show here once this window is back in front.
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
    await readAgents()
    if (staffDialog.open) renderStaff()
  })
})

async function start() {
  if (typeof invoke !== 'function') {
    report('ConsensFlow is not running this page.')
    return
  }
  // Listening comes first: subscribing to the output is when the app tells
  // the page the core's state, and an event sent before a listener is lost.
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
      await listen('core-status', (event) => coreStatus(event.payload))
    } catch (cause) {
      report(cause)
    }
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
  try {
    await readAgents()
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
