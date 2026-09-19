import { initializeUpdates } from '../updates.js'
import { BoardView, element, TaskDrawer, teamOf } from './board.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board (or one
 * team's live windows: the lead's or the PM's) on the right. Everything it
 * shows comes from the new core through the app's `core_request`, and it
 * redraws when the core says something changed. It keeps nothing of its own
 * but what is on screen.
 */

const tauri = window.__TAURI__ ?? {}
const invoke = tauri.core?.invoke
const listen = tauri.event?.listen
const Channel = tauri.core?.Channel

const $ = (selector) => document.querySelector(selector)
const projectList = $('#projects')
const projectTitle = $('#project-title')
const projectDirectory = $('#project-directory')
const viewButtons = [...document.querySelectorAll('[data-view]')]
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
  view: 'board',
  focus: null,
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

/** Runs an action and redraws; a refusal shows in the status line. */
async function act(work) {
  try {
    await work()
    await refresh()
  } catch (cause) {
    report(cause)
  }
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
  onAnswer: (message, text) =>
    act(async () => {
      await core('message.answer', { question: message.id, body: text })
      await core('message.read', { message: message.id })
      note(`Answer sent to @${message.sender}.`)
    }),
  onRead: (message) => act(() => core('message.read', { message: message.id })),
  onOpenTask: (number) => act(() => openTask(number)),
  onOpenTerminal: (participant) => {
    state.view = teamOf(participant)
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
})

const terminals = new TerminalsView(stage, $('#parking'), {
  invoke: (command, args) => invoke(command, args),
  report,
  createEmulator: tauri.test?.createEmulator,
})

async function openTask(number) {
  const { task } = await core('task.get', { project: state.selected, task: number })
  state.openTask = number
  drawer.show(task)
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
  teamButton.disabled = project === null
  const lanes = state.board?.lanes ?? []
  const hasPm = lanes.some((lane) => lane.participant.role === 'pm')
  if (state.view === 'pm' && !hasPm) state.view = 'board'
  for (const control of viewButtons) {
    control.setAttribute('aria-pressed', String(control.dataset.view === state.view))
    control.disabled = project === null
    if (control.dataset.view === 'pm') control.hidden = !hasPm
  }
  boardRoot.hidden = state.view !== 'board'
  stage.hidden = state.view === 'board'
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
  terminals.render(lanes, {
    team: state.view === 'board' ? null : state.view,
    focus: state.focus,
  })
}

function renderProjects() {
  const items = state.projects.map((project) => {
    const item = element('li', 'project')
    item.dataset.state = project.state
    const select = element('button', 'project-select')
    select.type = 'button'
    select.setAttribute('aria-current', String(project.id === state.selected))
    select.append(
      element('span', 'project-name', project.name),
      element('span', 'project-state', project.state === 'open' ? 'Open' : 'Suspended'),
    )
    select.addEventListener('click', () => {
      state.selected = project.id
      state.focus = null
      state.openTask = null
      drawer.hide()
      void refresh()
    })
    item.append(select)
    if (project.state !== 'open') {
      const resume = element('button', 'quiet-button', 'Resume')
      resume.type = 'button'
      resume.setAttribute('aria-label', `Resume ${project.name}`)
      resume.addEventListener('click', () =>
        act(() => core('project.resume', { project: project.id })),
      )
      item.append(resume)
    }
    return item
  })
  if (items.length === 0) items.push(element('li', 'projects-empty', 'No projects yet.'))
  projectList.replaceChildren(...items)
}

for (const control of viewButtons) {
  control.addEventListener('click', () => {
    state.view = control.dataset.view
    render()
  })
}

inboxButton.addEventListener('click', () => {
  state.view = 'board'
  render()
  boardRoot.querySelector('[data-handle="human"]')?.scrollIntoView({ block: 'start' })
})

// New project: the native folder picker first, then the lead's harness.
const newProjectDialog = $('#new-project-dialog')
const newProjectForm = newProjectDialog.querySelector('form')
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
    newProjectForm.elements.directory.value = directory
    newProjectDialog.showModal()
  } catch (cause) {
    report(cause)
  }
})
newProjectForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const directory = newProjectForm.elements.directory.value
  const harness = newProjectForm.elements.harness.value
  newProjectDialog.close()
  void act(async () => {
    const { project } = await core('project.open', { directory, harness })
    state.selected = project.id
    state.view = 'board'
  })
})
newProjectDialog
  .querySelector('[value="cancel"]')
  .addEventListener('click', () => newProjectDialog.close())

// The project team: who the coordinators may hand work to, and the PM.
const teamDialog = $('#team-dialog')
const teamForm = teamDialog.querySelector('form')
const teamList = $('#team-members')
const pmState = $('#team-pm-state')
const pmAdd = $('#team-pm-add')
/** The member whose removal waits for the human's yes, kept across redraws. */
let removing = null

/** Draws the team from the board, in place, so it stays current while open. */
function renderTeam() {
  const lanes = state.board?.lanes ?? []
  const members = lanes.filter((lane) => lane.participant.agent !== null)
  teamList.replaceChildren(
    ...(members.length
      ? members.map((lane) => memberRow(lane.participant))
      : [element('li', 'team-empty', 'Nobody yet: add the agents this project may use.')]),
  )
  const pm = lanes.find((lane) => lane.participant.role === 'pm')
  pmState.textContent =
    pm === undefined ? 'No PM yet.' : `PM · ${pm.participant.harness ?? 'harness unknown'}`
  pmAdd.hidden = pm !== undefined
  const onTeam = new Set(members.map((lane) => lane.participant.agent))
  const choices = state.agents.filter((agent) => !onTeam.has(agent.name))
  const picker = teamForm.elements.agent
  const chosen = picker.value
  picker.replaceChildren(
    ...choices.map((agent) => {
      const option = element(
        'option',
        null,
        `${agent.name} · ${agent.harness} · ${agent.model ?? 'model unknown'}`,
      )
      option.value = agent.name
      return option
    }),
  )
  if (choices.some((agent) => agent.name === chosen)) picker.value = chosen
  teamForm.querySelector('[type="submit"]').disabled = choices.length === 0
}

/** One member, with a Remove that asks first: leaving cancels its open tasks. */
function memberRow(member) {
  const row = element('li')
  const name = `@${member.handle}`
  const choose = (handle) => () => {
    removing = handle
    renderTeam()
  }
  if (removing !== member.handle) {
    const remove = element('button', 'quiet-button', 'Remove')
    remove.type = 'button'
    remove.setAttribute('aria-label', `Remove ${name} from the team`)
    remove.addEventListener('click', choose(member.handle))
    row.append(
      element('span', 'member-line', `${name} · ${member.role} · ${member.harness}`),
      remove,
    )
    return row
  }
  const keep = element('button', 'quiet-button', `Keep ${name}`)
  keep.type = 'button'
  keep.addEventListener('click', choose(null))
  const yes = element('button', 'danger-button', `Remove ${name}`)
  yes.type = 'button'
  yes.addEventListener('click', () =>
    act(async () => {
      await core('member.remove', { project: state.selected, agent: member.handle })
      removing = null
      note(`${name} left the team.`)
    }),
  )
  row.append(
    element('span', 'member-confirm', `Remove ${name}? Its open tasks are cancelled.`),
    keep,
    yes,
  )
  return row
}

teamButton.addEventListener('click', () =>
  act(async () => {
    state.agents = (await core('agents.list')).agents
    removing = null
    renderTeam()
    teamDialog.showModal()
  }),
)
$('#add-pm').addEventListener('click', () =>
  act(async () => {
    await core('pm.add', { project: state.selected, harness: teamForm.elements.pmHarness.value })
    note('The PM is on the team. Its window opens with its first task.')
  }),
)
teamForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const agent = teamForm.elements.agent.value
  const role = teamForm.elements.role.value
  teamDialog.close()
  void act(async () => {
    await core('member.add', { project: state.selected, agent, role })
    note(`@${agent} joined the team as ${role}.`)
  })
})
teamDialog.querySelector('[value="cancel"]').addEventListener('click', () => teamDialog.close())

async function start() {
  if (typeof invoke !== 'function') {
    report('ConsensFlow is not running this page.')
    return
  }
  if (typeof Channel === 'function') {
    const channel = new Channel()
    channel.onmessage = (message) => terminals.output(message)
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
