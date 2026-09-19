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
  onPutTask: ({ pool, tier, tags, purpose }, text) =>
    act(async () => {
      const { task } = await core('task.add', {
        project: state.selected,
        pool,
        tier,
        tags,
        ...(purpose === undefined ? {} : { purpose }),
        body: text,
      })
      note(`T-${task.number} is on the board for a ${tier} ${pool}.`)
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
  if (!lanes.some((lane) => lane.participant.handle === state.focus)) state.focus = 'lead'
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
  }
  terminals.render(lanes, { focused: state.focus })
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
      state.focus = 'lead'
      state.openTask = null
      drawer.hide()
      void refresh()
    })
    item.append(select)
    const open = project.state === 'open'
    const toggle = element('button', 'quiet-button', open ? 'Close' : 'Resume')
    toggle.type = 'button'
    toggle.setAttribute('aria-label', `${open ? 'Close' : 'Resume'} ${project.name}`)
    toggle.addEventListener('click', () =>
      act(() => core(open ? 'project.close' : 'project.resume', { project: project.id })),
    )
    item.append(toggle)
    return item
  })
  if (items.length === 0) items.push(element('li', 'projects-empty', 'No projects yet.'))
  projectList.replaceChildren(...items)
}

inboxButton.addEventListener('click', () => {
  boardRoot.querySelector('[data-handle="human"]')?.scrollIntoView({ block: 'start' })
})

const ROLES = ['worker', 'advisor', 'reviewer']
const ROLE_LABEL = { worker: 'Worker', advisor: 'Advisor', reviewer: 'Reviewer' }

/** A checkbox for one role of a member, or of an agent about to join. */
function roleBox(role, checked, name, onChange) {
  const cell = element('td', 'role-cell')
  const box = element('input')
  box.type = 'checkbox'
  box.value = role
  box.checked = checked
  box.setAttribute('aria-label', `${ROLE_LABEL[role]} ${name}`)
  if (onChange) box.addEventListener('change', () => onChange(box))
  cell.append(box)
  return cell
}

/** The roles ticked for each agent in a team table: the agents with none are not on the team. */
function pickedTeam(rows) {
  return [...rows.querySelectorAll('tr[data-agent]')]
    .map((row) => ({
      agent: row.dataset.agent,
      roles: [...row.querySelectorAll('input:checked')].map((box) => box.value),
    }))
    .filter(({ roles }) => roles.length > 0)
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
    : 'Reviews need a reviewer on the team: tick Reviewer for one of the agents.'
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

/** Every saved agent as a row of role boxes, the last team's roles ticked. */
function renderNewProjectTeam(lastTeam) {
  const rows = state.agents.map((agent) => {
    const row = element('tr')
    row.dataset.agent = agent.name
    const who = element('td')
    who.append(
      element('span', 'member-name', agent.name),
      element('br'),
      element('span', 'member-meta', `${agent.harness} · ${agent.model ?? 'model unknown'}`),
    )
    row.append(who)
    const saved = lastTeam.find((member) => member.agent === agent.name)?.roles ?? []
    for (const role of ROLES) {
      row.append(roleBox(role, saved.includes(role), agent.name, () => guardNewProjectReview()))
    }
    return row
  })
  if (rows.length === 0) {
    const row = element('tr', 'team-empty')
    const cell = element('td', null, 'No agents saved yet: add some under Agents first.')
    cell.colSpan = 4
    row.append(cell)
    rows.push(row)
  }
  newProjectTeam.replaceChildren(...rows)
  newProjectForm.elements.review.value = lastTeam.some(({ roles }) => roles.includes('reviewer'))
    ? 'members'
    : 'none'
  guardNewProjectReview()
}

const guardNewProjectReview = () =>
  guardReview(newProjectForm.elements.review, newProjectWarning, pickedTeam(newProjectTeam))

newProjectForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const directory = newProjectForm.elements.directory.value
  const harness = newProjectForm.elements.harness.value
  const review = newProjectForm.elements.review.value
  const team = pickedTeam(newProjectTeam)
  newProjectDialog.close()
  void act(async () => {
    const { project } = await core('project.open', { directory, harness, review, team })
    state.selected = project.id
    state.focus = 'lead'
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
const teamReview = $('#team-review')
const teamWarning = $('#team-warning')
/** The member whose removal waits for the human's yes, kept across redraws. */
let removing = null

/** Draws the team from the board, in place, so it stays current while open. */
function renderTeam() {
  const lanes = state.board?.lanes ?? []
  const members = lanes.filter((lane) => lane.participant.agent !== null)
  teamList.replaceChildren(
    ...(members.length
      ? members.map((lane) => memberRow(lane.participant))
      : [element('tr', 'team-empty')]),
  )
  if (members.length === 0) {
    const cell = element('td', null, 'Nobody yet: add the agents this project may use.')
    cell.colSpan = 7
    teamList.firstChild.append(cell)
  }
  const pm = lanes.find((lane) => lane.participant.role === 'pm')
  pmState.textContent =
    pm === undefined ? 'No PM yet.' : `PM · ${pm.participant.harness ?? 'harness unknown'}`
  pmAdd.hidden = pm !== undefined
  teamReview.value = state.board?.project.review ?? 'none'
  guardReview(
    teamReview,
    teamWarning,
    members.map((lane) => ({ roles: lane.participant.roles })),
  )
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

/** One member: its roles as checkboxes, its tier and tags, and a Remove that asks first. */
function memberRow(member) {
  const row = element('tr')
  row.dataset.handle = member.handle
  const name = `@${member.handle}`
  const choose = (handle) => () => {
    removing = handle
    renderTeam()
  }
  const who = element('td')
  who.append(
    element('span', 'member-name', name),
    element('br'),
    element('span', 'member-meta', member.harness ?? ''),
  )
  row.append(who)
  if (removing === member.handle) {
    const cell = element('td')
    cell.colSpan = 6
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
    cell.append(
      element('span', 'member-confirm', `Remove ${name}? Its open tasks are cancelled. `),
      keep,
      yes,
    )
    row.append(cell)
    return row
  }
  for (const role of ROLES) {
    row.append(
      roleBox(role, member.roles.includes(role), name, (box) => {
        const roles = ROLES.filter((r) => (r === role ? box.checked : member.roles.includes(r)))
        if (roles.length === 0) {
          box.checked = true
          report(`${name} needs at least one role.`)
          return
        }
        void act(async () => {
          await core('member.roles', { project: state.selected, agent: member.handle, roles })
        })
      }),
    )
  }
  row.append(element('td', null, member.tier ?? ''), element('td', null, member.tags.join(', ')))
  const remove = element('button', 'quiet-button', 'Remove')
  remove.type = 'button'
  remove.setAttribute('aria-label', `Remove ${name} from the team`)
  remove.addEventListener('click', choose(member.handle))
  const tools = element('td')
  tools.append(remove)
  row.append(tools)
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
teamReview.addEventListener('change', () =>
  act(async () => {
    await core('project.review', { project: state.selected, review: teamReview.value })
    note(
      {
        none: 'Finished work goes straight to whoever asked.',
        members: "Members' finished work gets a second review.",
        all: 'All finished work gets a second review.',
      }[teamReview.value],
    )
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
  const roles = [...teamForm.querySelectorAll('input[name="roles"]:checked')].map(
    (box) => box.value,
  )
  if (!agent) return
  if (roles.length === 0) {
    report('Tick at least one role for the new member.')
    return
  }
  void act(async () => {
    await core('member.add', { project: state.selected, agent, roles })
    note(`@${agent} joined the team as ${roles.join(' and ')}.`)
  })
})
teamDialog.querySelector('[value="cancel"]').addEventListener('click', () => teamDialog.close())

// The human's agents screens: the daemon's own pages, in a frame each, at
// the URL and token the app was handed. Closing one refreshes the board's
// view of the agents (a tag or a tier may have changed).
// The agents screens open in their own window at the daemon's address: the
// board's page cannot frame them (WebKit blocks a plain-HTTP frame inside the
// app's secure page). Their edits show here once this window is back in front.
for (const [buttonId, page] of [
  ['#agents-button', ''],
  ['#library-button', 'library'],
  ['#harnesses-button', 'harnesses'],
]) {
  $(buttonId).addEventListener('click', () =>
    act(async () => {
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
