import { button, element, redraw } from '../dom.js'
import { initializeUpdates } from '../updates.js'
import { BoardView, TaskDrawer } from './board.js'
import { NewProjectDialog, StaffDialog, SwitchLeadDialog } from './dialogs.js'
import { Layout } from './layout.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board in the
 * middle, and on the right, in a strip that scrolls sideways, the chief's
 * window and the sessions' the human asks to see, so the human reads the
 * board and talks to any of them.
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
  // A session's terminal is in the dock only once the human asks to see it:
  // showing it unfolds the dock and brings it to the front, hiding it takes
  // its card out while its window works on. Opening a closed one brings it
  // back on its own conversation, shown the same way. Closing it ends its
  // process and takes its card away with it.
  onShowTerminal: (participant) => showTerminal(participant),
  onHideTerminal: (participant) => terminals.hide(participant.projectId, participant.handle),
  onOpenTerminal: (participant) =>
    act(async () => {
      await core('session.open', { project: participant.projectId, handle: participant.handle })
      showTerminal(participant)
    }),
  onCloseTerminal: (participant) => closeTerminal(participant),
  onEndSession: (participant) =>
    act(async () => {
      await core('session.end', { project: participant.projectId, handle: participant.handle })
      note(`@${participant.handle} is gone; its tasks stay on @${participant.member}'s lane.`)
    }),
  onSwitchLead: (chief) => act(() => switchLead.open(chief)),
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

/** A session's terminal the human asked to see: in the dock, unfolded, in front. */
function showTerminal(participant) {
  layout.unfold('dock')
  state.focus = participant.handle
  terminals.show(participant.projectId, participant.handle)
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
      staff.render()
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
    board.render({
      board: state.board,
      inbox: state.inbox,
      agents: state.agents,
      shows: (participant) => terminals.shows(participant.projectId, participant.handle),
    })
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

// The project staff, drawn from the board shown and the agents as the page
// has them now; Staff reads the agents again before it opens.
const staff = new StaffDialog($('#staff-dialog'), {
  board: () => state.board,
  agents: () => state.agents,
  core,
  act,
  note,
})
staffButton.addEventListener('click', () =>
  act(async () => {
    await readAgents()
    staff.open()
  }),
)

// New project: the native folder picker first, then the saved agent the lead
// runs on, the staff (the last project's ticked already) and the approval
// setting. The agents it reads are the page's too, and the project it starts
// is shown.
const newProject = new NewProjectDialog($('#new-project-dialog'), {
  agents: () => state.agents,
  core,
  act,
  onAgents: (agents) => {
    state.agents = agents
  },
  onStart: (project) => {
    state.selected = project.id
    state.focus = 'chief'
  },
})
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
    await newProject.open(directory)
  } catch (cause) {
    report(cause)
  }
})

// Switch lead opens only while the project it was asked for is the one
// chosen; once the switch is taken, the lead's window is the one in front.
const switchLead = new SwitchLeadDialog($('#switch-lead-dialog'), {
  selected: () => state.selected,
  core,
  act,
  onSwitch: () => {
    state.focus = 'chief'
  },
})

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
    staff.render()
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
