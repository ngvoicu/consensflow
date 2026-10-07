import { button, element, redraw } from '../dom.js'
import { initializeUpdates } from '../updates.js'
import { BoardView, TaskDrawer } from './board.js'
import { NewProjectDialog, StaffDialog, SwitchChiefDialog } from './dialogs.js'
import { Layout } from './layout.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the projects on the left, the chosen project's board in the
 * middle, and on the right, in a strip that scrolls sideways, the chief's
 * window and the sessions' the human asks to see, so the human reads the
 * board and talks to any of them.
 * Everything it shows comes from the daemon through the app's
 * `daemon_request`, and it redraws when the daemon says something changed. It
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
 * No notes for the human, in the shape the daemon reads their unread ones:
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

async function daemon(operation, body = {}) {
  const result = await invoke('daemon_request', { operation, body })
  // A request the app could not hand to the daemon says why in `detail`.
  if (result?.ok !== true) {
    throw new Error(result?.detail ?? result?.error ?? `${operation} did not answer`)
  }
  return result
}

/** The saved agents, read again: what the board says members run, and what the pickers offer. */
async function readAgents() {
  state.agents = (await daemon('agents.list')).agents
}

// The daemon down: the banner says why and what comes next, and stays until
// the daemon is back, when everything is read again.
const daemonDown = $('#daemon-down')
function daemonStatus({ available, cause, retrying }) {
  const wasDown = !daemonDown.hidden
  daemonDown.hidden = available
  if (available) {
    if (wasDown) void act(readAgents)
    return
  }
  daemonDown.replaceChildren(
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
 * goes back to what the ledger holds when the daemon refuses the change.
 */
async function act(work) {
  try {
    await work()
  } catch (cause) {
    report(cause)
  }
  await refresh()
}

/**
 * A member, or the chief, out of quota tried again before its reset: the
 * human ran its harness on another account, or a bigger plan.
 */
const tryAgain = (participant) =>
  act(async () => {
    await daemon('member.back', { project: participant.projectId, participant: participant.handle })
    note(`Trying @${participant.handle} again: what waited for it goes on.`)
  })

// A crowded cell's cards are listed in the stack dialog, the board's own.
const board = new BoardView(boardRoot, $('#stack-dialog'), {
  onRead: (message) => act(() => daemon('message.read', { message: message.id })),
  // What waits for the human's approval goes on, goes back, or is declined with a word to its sender.
  onApprove: (message) =>
    act(async () => {
      await daemon('message.approve', { message: message.id })
      note(`m-${message.id} goes on to @${message.recipient}.`)
    }),
  onDecline: (message) =>
    act(async () => {
      await daemon('message.decline', { message: message.id })
      note(`m-${message.id} declined; @${message.sender} is told.`)
    }),
  // Each control acts on the project of the board it is on: not yet the one
  // the human just chose, while that one's board is on its way.
  onOpenTask: (number) => act(() => openTask(state.board.project.id, number)),
  onShowTerminal: (participant, opened) => showSession(participant, opened),
  onHideTerminal: (participant) => hideSession(participant),
  onTryAgain: (participant) => tryAgain(participant),
  onEndSession: (participant) =>
    act(async () => {
      await daemon('session.end', { project: participant.projectId, handle: participant.handle })
      note(
        `@${participant.handle} is off the board, its conversation kept: a follow-up brings it back. Its tasks stay on @${participant.member}'s lane.`,
      )
    }),
  onResume: (project) =>
    act(async () => {
      await daemon('project.resume', { project: project.id })
      state.focus = 'chief'
    }),
  onDeleteFinished: (project, { deletable, kept }) =>
    askToDeleteTasks(
      project.id,
      deletable,
      `Delete ${deletable.length} finished task${deletable.length === 1 ? '' : 's'}?`,
      kept,
    ),
})

// The drawer's actions are on its task's own project.
const drawer = new TaskDrawer($('#task-drawer'), {
  onClose: () => closeTask(),
  onCancel: (task) =>
    act(() => daemon('task.cancel', { project: task.projectId, task: task.number })),
  onPause: (task) =>
    act(async () => {
      await daemon('task.pause', { project: task.projectId, task: task.number })
      note(`T-${task.number} is paused; its work waits.`)
    }),
  onReassign: (task) =>
    act(async () => {
      await daemon('task.reassign', { project: task.projectId, task: task.number })
      note(
        `T-${task.number} is back on the board for another ${task.pool === 'designer' ? 'image designer' : `${task.tier} ${task.pool}`}.`,
      )
    }),
  onResume: (task) =>
    act(async () => {
      const { task: resumed } = await daemon('task.resume', {
        project: task.projectId,
        task: task.number,
      })
      // A task paused in the backlog had no window: it only waits for a member again.
      const back =
        task.assignee === null
          ? `T-${task.number} is back on the board.`
          : `T-${task.number} is back on the board: the window that had it has ended.`
      note(resumed.state === 'open' ? back : `T-${task.number} resumes in @${resumed.assignee}.`)
    }),
  onDelete: (task) => askToDeleteTasks(task.projectId, [task.number], `Delete T-${task.number}?`),
  // The drawer covers the dock: it closes, and the task's window is in front.
  onShowTerminal: (participant, opened) => {
    closeTask()
    showSession(participant, opened)
  },
  // What a task's window wrote, read when its fold opens.
  onTranscript: async (task) => {
    try {
      drawer.fill(
        task,
        await daemon('task.transcript', { project: task.projectId, task: task.number }),
      )
    } catch (cause) {
      report(cause)
    }
  },
})

// The packaged smoke watches acks and arrivals here, on the real paths.
let ackObserver = null
let outputObserver = null

/**
 * A session's terminal is in the dock only once the human asks to see it:
 * showing it unfolds the dock and brings it to the front, and a closed one
 * opens first, on its own conversation. A window opened so stays until the
 * human hides it, or its session or its project ends; one they hid that has
 * not closed yet is theirs again. A window open for its task, which they
 * only look at, closes with the task.
 */
function showSession(participant, { closed, hidden }) {
  if (!closed && !hidden) {
    showTerminal(participant)
    return
  }
  void act(async () => {
    await daemon('session.open', { project: participant.projectId, handle: participant.handle })
    showTerminal(participant)
  })
}

/**
 * The human hides a session's terminal, from its lane or its card: the card
 * leaves the dock at once, and the daemon, told the window is no longer the
 * human's, closes it once it holds no task and its agent is not at work.
 */
function hideSession(participant) {
  terminals.hide(participant.projectId, participant.handle)
  void act(() =>
    daemon('session.hide', { project: participant.projectId, handle: participant.handle }),
  )
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
  // The chief is switched from its card in the dock, and tried again there when out of quota.
  onSwitchChief: (chief) => act(() => switchChief.open(chief)),
  onTryAgain: (chief) => tryAgain(chief),
  // A session's terminal is hidden from its card as from its lane.
  onHideTerminal: (participant) => hideSession(participant),
})

// A fold changes the room the board and the windows have: both draw again.
const layout = new Layout({ onFold: () => render() })

/**
 * How many items a task's window wrote. Only the count: even that walks the
 * window's whole copy, so it is read when a drawer opens or its task changed.
 */
const written = async (project, number) =>
  (await daemon('task.transcript', { project, task: number, limit: 0 })).total

const closedProject = (id) => state.projects.find((project) => project.id === id)?.state !== 'open'

/** A task's drawer; one asked for in a project the human has left since stays shut. */
async function openTask(project, number) {
  const [{ task }, total] = await Promise.all([
    daemon('task.get', { project, task: number }),
    written(project, number),
  ])
  if (state.selected !== project) return
  state.openTask = { project, number, total }
  drawer.show(task, { total, closed: closedProject(project), terminal: terminalOf(task) })
}

/**
 * The window that works on `task`, on the board shown, and whether it is
 * closed: a session's, or the chief's own; a member itself has none.
 */
function terminalOf(task) {
  const lane = state.board?.lanes.find((l) => l.participant.handle === task.assignee)
  if (lane === undefined) return null
  const { participant } = lane
  if (participant.member === null && participant.role !== 'chief') return null
  return { participant, closed: lane.pane === null, hidden: lane.hidden === true }
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
    const { task } = await daemon('task.get', { project, task: number })
    if (!drawer.shows(task)) opened.total = await written(project, number)
    // Closed, or another task opened, while these were on their way.
    if (state.openTask !== opened) return
    drawer.show(task, {
      total: opened.total,
      closed: closedProject(project),
      terminal: terminalOf(task),
    })
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
      const { projects } = await daemon('projects.list')
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
          daemon('board.get', { project: project.id }).then(
            (read) => read.board,
            () => null,
          ),
        )
      const [board, inbox, ...others] = await Promise.all([
        selected === null
          ? null
          : daemon('board.get', { project: selected }).then((read) => read.board),
        selected === null
          ? NO_NOTES
          : daemon('inbox.get', { project: selected, participant: 'human', unread: true }),
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
  terminals.render(state.board, { focused: state.focus, agents: state.agents })
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
    await daemon('project.delete', { project: project.id })
    if (state.selected === project.id) state.selected = null
    note(`${project.name} is deleted.`)
  })
})
deleteDialog.addEventListener('close', () => {
  deleting = null
})

// Deleting finished tasks is confirmed in a dialog that says what goes and
// what stays: one task from its drawer, or every finished task that may go
// from the Finished heading, with those kept because a task still needs them.
const deleteTasksDialog = $('#delete-tasks-dialog')
const deleteTasksTitle = $('#delete-tasks-title')
const deleteTasksKept = $('#delete-tasks-kept')
let deletingTasks = null
function askToDeleteTasks(project, numbers, title, kept = []) {
  deletingTasks = { project, numbers }
  deleteTasksTitle.textContent = title
  deleteTasksKept.replaceChildren(
    ...kept.map(({ number, neededBy }) =>
      element(
        'li',
        null,
        `T-${number} stays: ${neededBy.map((other) => `T-${other}`).join(', ')} still need${neededBy.length === 1 ? 's' : ''} it.`,
      ),
    ),
  )
  deleteTasksKept.hidden = kept.length === 0
  deleteTasksDialog.showModal()
}
// The ask is used up here, not when the dialog closes: a close event comes
// later than the close, and would cancel an ask made again in between.
$('#delete-tasks-confirm').addEventListener('click', () => {
  const asked = deletingTasks
  deletingTasks = null
  deleteTasksDialog.close()
  if (asked === null) return
  const { project, numbers } = asked
  void act(async () => {
    await daemon('tasks.delete', { project, tasks: numbers })
    if (state.openTask?.project === project && numbers.includes(state.openTask.number)) {
      closeTask()
    }
    note(
      numbers.length === 1
        ? `T-${numbers[0]} is deleted.`
        : `${numbers.length} finished tasks are deleted.`,
    )
  })
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
        () => act(() => daemon(open ? 'project.close' : 'project.resume', { project: project.id })),
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
  daemon,
  act,
  note,
})
staffButton.addEventListener('click', () =>
  act(async () => {
    await readAgents()
    staff.open()
  }),
)

// New project: the native folder picker first, then the saved agent the chief
// runs on, the staff (the last project's ticked already) and the approval
// setting. The agents it reads are the page's too, and the project it starts
// is shown.
const newProject = new NewProjectDialog($('#new-project-dialog'), {
  agents: () => state.agents,
  daemon,
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

// Switch chief opens only while the project it was asked for is the one
// chosen; once the switch is taken, the chief's window is the one in front.
const switchChief = new SwitchChiefDialog($('#switch-chief-dialog'), {
  selected: () => state.selected,
  daemon,
  act,
  onSwitch: () => {
    state.focus = 'chief'
  },
})

// The agents screens are the daemon's own pages, each framed in a dialog of
// its own at the address and token the app hands the page. A key pressed in a
// frame never reaches the dialog around it, so a screen hands Escape up.
// Closing one gives the focus back to the Settings button (its dialog closed
// as the screen opened) and reads the agents again: a model or a tier may
// have changed on it.
const settingsButton = $('#settings-button')
const settingsDialog = $('#settings-dialog')
settingsButton.addEventListener('click', () => settingsDialog.showModal())
for (const entry of settingsDialog.querySelectorAll('[data-agents-page]')) {
  const page = entry.dataset.agentsPage
  const dialog = document.getElementById(entry.getAttribute('aria-controls'))
  const frame = dialog.querySelector('iframe')
  entry.addEventListener('click', () =>
    act(async () => {
      settingsDialog.close()
      const screen = await invoke('agents_screen', { page })
      if (screen?.ok !== true) {
        throw new Error(screen?.error ?? 'The agents screens are not available.')
      }
      // A screen opened before keeps what the human left on it, its filters
      // and drafts, and is told the agents may have changed since.
      if (frame.src === screen.url) {
        frame.contentWindow.postMessage('consensflow:refresh-agents', new URL(screen.url).origin)
      } else {
        frame.src = screen.url
      }
      dialog.showModal()
    }),
  )
  window.addEventListener('message', (event) => {
    if (event.source === frame.contentWindow && event.data === 'consensflow:close') dialog.close()
  })
  dialog.addEventListener('close', () => {
    settingsButton.focus()
    void act(readAgents)
  })
}

async function start() {
  if (typeof invoke !== 'function') {
    report('ConsensFlow is not running this page.')
    return
  }
  // Listening comes first: subscribing to the output is when the app tells
  // the page the daemon's state, and an event sent before a listener is lost.
  if (typeof listen === 'function') {
    try {
      let pending = false
      await listen('state-changed', (event) => {
        // A window that wrote more changes only what an open fold shows.
        if (event?.payload?.reason === 'transcript') {
          drawer.reread()
          return
        }
        if (pending) return
        pending = true
        setTimeout(() => {
          pending = false
          void refresh()
        }, 50)
      })
      await listen('daemon-status', (event) => daemonStatus(event.payload))
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
      daemon: (operation, body) => invoke('daemon_request', { operation, body }),
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
