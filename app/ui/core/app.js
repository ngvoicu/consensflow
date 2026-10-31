import { button, element, redraw } from '../dom.js'
import { initializeUpdates } from '../updates.js'
import { BoardView, TaskDrawer } from './board.js'
import { NewProjectDialog, ProjectSettingsDialog, SwitchChiefDialog } from './dialogs.js'
import { listenForFileDrops } from './file-drop.js'
import { HistoryViewer } from './history-viewer.js'
import { Layout } from './layout.js'
import { ProjectMenu } from './project-menu.js'
import { SpecsSettings } from './specs-settings.js'
import { SpecsViewer } from './specs-viewer.js'
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
const specsButton = $('#specs-button')
const projectSettingsButton = $('#project-settings-button')
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
  // Whether a project's specs folder holds a file (`{ project, present }`), read after its board
  // and never waited for: whether Specs shows. The daemon stops at the first file, and remembers
  // it for a moment.
  specsPresent: null,
  agents: [],
  /** What the app says of specs for every project and the Specmint it bundles, as last read. */
  specs: null,
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

/**
 * What the app says of specs for every project (whether, the folder and the variant, and what a
 * project that names none then uses) and the Specmint it bundles, read again: each dialog that
 * shows them reads as it opens, and Settings' Specs hands the page what a change answered. Read
 * once the saves Settings' Specs asked for are answered: the app asks each on a worker of its own,
 * and a read that overtook a save would leave the page an older answer than the save's.
 */
async function readSpecs() {
  await specsSettings.settled()
  state.specs = await daemon('specs.get')
}

// The daemon down: the banner says why and what comes next, and stays until
// the daemon is back, when everything is read again.
const daemonDown = $('#daemon-down')

/**
 * Where the banner is drawn. It is the page's line under the command bar, which an open modal
 * and its backdrop cover: so while one is open it is shown in the top layer, over the dialog,
 * where the human sees it; and shown again whenever another modal opens, to stay over that one.
 * It takes no clicks, so the dialog it is over is as it was.
 */
function placeDaemonDown() {
  const over = !daemonDown.hidden && document.querySelector('dialog[open]') !== null
  if (daemonDown.matches(':popover-open')) daemonDown.hidePopover()
  if (over) {
    daemonDown.popover = 'manual'
    daemonDown.showPopover()
  } else {
    daemonDown.removeAttribute('popover')
  }
}
new MutationObserver(placeDaemonDown).observe(document.body, {
  attributes: true,
  attributeFilter: ['open'],
  subtree: true,
})

function daemonStatus({ available, cause, retrying }) {
  const wasDown = !daemonDown.hidden
  daemonDown.hidden = available
  if (available) {
    placeDaemonDown()
    if (wasDown) void act(readAgents)
    return
  }
  daemonDown.replaceChildren(
    element('strong', null, 'The daemon is not running.'),
    ...(cause ? [` ${sentence(cause)}`] : []),
    retrying ? ' Starting it again…' : ' Quit ConsensFlow and open it again.',
  )
  placeDaemonDown()
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
  onSystemSettings: (pane) => act(() => daemon('computer_use.open', { pane })),
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
      deletable.length === 0
        ? 'No finished task can be deleted now'
        : `Delete ${deletable.length} finished task${deletable.length === 1 ? '' : 's'}?`,
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
  onFinish: (task) =>
    act(async () => {
      const { jobs = 0, agents = 0 } = await daemon('task.finish', {
        project: task.projectId,
        task: task.number,
      })
      // Finishing ends nothing the turn left: it runs until its window closes, or past it
      // where the host cannot tell what is the window's.
      const left =
        jobs + agents === 0
          ? 'Nothing it left running was ended.'
          : `What it waits on is not ended: ${jobs + agents === 1 ? 'it may run' : 'they may run'} on until its window closes, or past it where the host cannot tell what is the window's.`
      note(`T-${task.number} is finished: what its worker said last is its result. ${left}`)
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
  // Every card reads its participant's whole history.
  onHistory: (participant) => void historyViewer.open(participant),
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
  drawer.show(task, {
    total,
    closed: closedProject(project),
    terminal: terminalOf(task),
    held: heldOf(task),
  })
}

/**
 * What the worker of `task` is waited for on in the background (`{jobs, agents}`), on
 * the board shown: its lane says, and only while the task works. Null when it waits
 * on nothing.
 */
function heldOf(task) {
  const lane = state.board?.lanes.find((l) => l.participant.handle === task.assignee)
  return task.state === 'working' && lane?.held?.task === task.number ? lane.held : null
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
      held: heldOf(task),
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
      if (selected !== null) void readPresence(selected)
      terminals.reconcile(
        [board, ...others].filter((read) => read !== null),
        new Set(open.map((project) => project.id)),
      )
      render()
      projectSettings.render()
      projectMenu.follow()
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

/** Each read of whether a specs folder holds a file is numbered: only the last asked is shown. */
let presenceReads = 0
/** The read asked for again once the daemon no longer answers it from memory. */
let presenceAgain = null

/**
 * Whether the specs folder of project `project` holds a file, which shows the Specs button: read
 * apart from the board, which never waits for it (a folder on a drive that does not answer would
 * hold the board, its inbox and its windows until the daemon gave up on it), and shown when it
 * comes, for the project the board still shows. A folder that cannot be read shows no button. An
 * answer the daemon gave from memory says when it reads the folder again: it is asked once more
 * then, so the button never keeps what the folder said before a spec was written.
 */
async function readPresence(project) {
  const read = ++presenceReads
  clearTimeout(presenceAgain)
  presenceAgain = null
  const said = await daemon('specs.present', { project }).catch(() => ({ present: false }))
  if (read !== presenceReads || state.board?.project.id !== project) return
  state.specsPresent = { project, present: said.present === true }
  specsButton.hidden = !state.specsPresent.present
  if (typeof said.readAgainIn === 'number') {
    presenceAgain = setTimeout(() => void readPresence(project), said.readAgainIn + 50)
  }
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
  // A closed project's board is read-only: nothing runs, so nothing on it may act. Its board
  // and its tasks still read. Its settings change all the same, for they are what its chief
  // starts with, which the Resume does: Project settings opens for it as for an open project.
  const suspended = project?.state === 'suspended'
  main.dataset.suspended = String(suspended)
  projectSettingsButton.disabled = project === null
  // Specs shows while the project's specs folder holds a file: from the board's first read after
  // the chief writes its first spec, open or closed, specs on or off (`readPresence`).
  specsButton.hidden = !(
    project !== null &&
    state.specsPresent?.project === project.id &&
    state.specsPresent.present
  )
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
// With nothing that may go it only says so, and what keeps each task.
const deleteTasksDialog = $('#delete-tasks-dialog')
const deleteTasksTitle = $('#delete-tasks-title')
const deleteTasksHelp = $('#delete-tasks-help')
const deleteTasksNothing = $('#delete-tasks-nothing')
const deleteTasksGoing = $('#delete-tasks-going')
const deleteTasksKept = $('#delete-tasks-kept')
const deleteTasksKeep = $('#delete-tasks-keep')
const deleteTasksConfirm = $('#delete-tasks-confirm')
let deletingTasks = null
/** Task numbers in a sentence: "T-5", "T-5 and T-8", "T-5, T-8 and T-9". */
const tasksNamed = (numbers) => {
  const names = numbers.map((number) => `T-${number}`)
  return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`
}
/**
 * Asks to delete `numbers` of `project`, none when nothing may go. `kept` is
 * null for one task from its drawer; for the Finished heading it lists those
 * that stay, each with the tasks that need it.
 */
function askToDeleteTasks(project, numbers, title, kept = null) {
  deletingTasks = { project, numbers }
  const nothing = numbers.length === 0
  deleteTasksTitle.textContent = title
  deleteTasksHelp.hidden = nothing
  deleteTasksNothing.hidden = !nothing
  const going = kept !== null && !nothing
  deleteTasksGoing.hidden = !going
  deleteTasksGoing.textContent = going
    ? `${tasksNamed(numbers)} ${numbers.length === 1 ? 'goes' : 'go'}.`
    : ''
  deleteTasksKept.replaceChildren(
    ...(kept ?? []).map(({ number, neededBy }) =>
      element(
        'li',
        null,
        `T-${number} stays: ${tasksNamed(neededBy)} still need${neededBy.length === 1 ? 's' : ''} it, and it failed: the chief can still reopen it.`,
      ),
    ),
  )
  deleteTasksKept.hidden = (kept ?? []).length === 0
  deleteTasksConfirm.hidden = nothing
  deleteTasksKeep.textContent = nothing ? 'Close' : 'Keep'
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

/** The human chooses a project: its board is the one to read and show, its chief's window in front. */
function choose(id) {
  state.selected = id
  state.focus = 'chief'
  closeTask()
}

/**
 * Shows project `id`: chosen, unless it is the choice already (the human may have chosen
 * another, whose board is on its way while the board still shows `id`: asked for after it,
 * `id` is the choice again), and its board waited for unless the board shows it (a read
 * already on its way may be of the project chosen before, and the next one is of this).
 * Whether the board shown is its now, which it is not where the project is gone or its board
 * could not be read (the page has said why).
 */
async function showProject(id) {
  if (state.selected !== id) choose(id)
  if (state.board?.project.id !== id) {
    await refresh()
    while (refreshing !== null) await refreshing
  }
  return state.board?.project.id === id
}

function renderProjects() {
  const shown = state.board?.project.id ?? null
  const items = state.projects.map((project) => {
    const item = element('li', 'project')
    // The item names its project: a redraw keeps it only for that one.
    item.dataset.project = String(project.id)
    item.dataset.current = String(project.id === shown)
    const select = button('', 'project-select', () => {
      choose(project.id)
      void refresh()
    })
    select.setAttribute('aria-current', String(project.id === shown))
    // A closed project says so by its buttons (Delete, Resume), not by a pill.
    select.append(element('span', 'project-name', project.name))
    // The line the human wrote under the name, whole, on as many rows as it takes.
    if (project.description) {
      select.append(element('span', 'project-description', project.description))
    }
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

// A right-click on a project (or the context-menu key, or Shift+F10, on the one the
// keyboard is on) opens its menu: rename it, write a line under its name, or open its
// Project settings or its specs (the project is shown first, if the board shows another).
// An open menu follows each refresh; with its project gone and no other in the list, the
// keyboard goes to New project.
const projectMenu = new ProjectMenu(
  {
    list: projectList,
    menu: $('#project-menu'),
    rename: $('#rename-dialog'),
    describe: $('#describe-dialog'),
    settings: $('#project-settings-dialog'),
    specs: $('#specs-viewer-dialog'),
    fallback: $('#new-project'),
  },
  {
    project: (id) => state.projects.find((project) => project.id === id),
    daemon,
    act,
    note,
    openSettings: (id) => act(() => openSettings(id)),
    openSpecs: (id) => openSpecs(id),
  },
)

// The specs viewer: the specs folder of the project asked for, read through the daemon. The
// button asks for the project the board shows; the menu for its own, shown first.
const specsViewer = new SpecsViewer($('#specs-viewer-dialog'), { daemon })

// The history viewer: everything kept of a participant's conversations, read through the daemon.
const historyViewer = new HistoryViewer($('#history-dialog'), { daemon })
specsButton.addEventListener('click', () => {
  const { project } = state.board
  void specsViewer.open(project.id, project.name)
})

/** The specs viewer on project `id`, which is shown first if it is not. */
async function openSpecs(id) {
  try {
    if (!(await showProject(id))) return
    await specsViewer.open(id, state.board.project.name)
  } catch (cause) {
    report(cause)
  }
}

// The notes are in For you, at the top of the board: a folded board unfolds for them.
inboxButton.addEventListener('click', () => {
  layout.unfold('board')
  boardRoot.querySelector('.foryou')?.scrollIntoView({ block: 'start' })
})

// Project settings (the staff, human approval, wake-up calls and specs), for the project
// asked for: drawn from its board while the board shown is its, and from the agents as the
// page has them now; opening it reads the agents again. The button asks for the project
// the board shows.
const projectSettings = new ProjectSettingsDialog($('#project-settings-dialog'), {
  board: () => state.board,
  selected: () => state.selected,
  agents: () => state.agents,
  specs: () => state.specs,
  readSpecs,
  daemon,
  act,
  note,
  report,
})
projectSettingsButton.addEventListener('click', () =>
  act(() => openSettings(state.board.project.id)),
)

/**
 * Project settings opened on project `id`, which is shown first if it is not: the dialog of
 * the board's button, for the project the human asked for. One the human chooses another
 * project before it is up is not opened.
 */
async function openSettings(id) {
  if (!(await showProject(id))) return
  await readAgents()
  await projectSettings.open(id)
}

// New project: the native folder picker first, then the saved agent the chief
// runs on and the project's settings: the staff (the last project's ticked
// already), human approval, wake-up calls and specs, which the project starts
// with. The agents it reads are the page's too, and the project it starts is
// shown.
const newProject = new NewProjectDialog($('#new-project-dialog'), {
  agents: () => state.agents,
  specs: () => state.specs,
  readSpecs,
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
  board: () => state.board,
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
// have changed on it. Asked as the app starts, a screen's address waits for the
// daemon's start: the status line says so, and one screen is asked for at a
// time, so clicks meanwhile queue nothing (T-978).
const settingsButton = $('#settings-button')
const settingsDialog = $('#settings-dialog')
settingsButton.addEventListener('click', () => settingsDialog.showModal())
/** How long a screen's address is waited for before the status line says why. */
const SCREEN_WAIT_SAID_MS = 300
let screenAsked = false
for (const entry of settingsDialog.querySelectorAll('[data-agents-page]')) {
  const page = entry.dataset.agentsPage
  const dialog = document.getElementById(entry.getAttribute('aria-controls'))
  const frame = dialog.querySelector('iframe')
  entry.addEventListener('click', () => {
    settingsDialog.close()
    if (screenAsked) return
    screenAsked = true
    const waiting = `${entry.textContent.trim()} opens once ConsensFlow's daemon is up.`
    const saying = setTimeout(() => note(waiting), SCREEN_WAIT_SAID_MS)
    void act(async () => {
      try {
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
      } finally {
        clearTimeout(saying)
        if (status.textContent === waiting) {
          status.textContent = ''
          delete status.dataset.tone
        }
        screenAsked = false
      }
    })
  })
  window.addEventListener('message', (event) => {
    if (event.source === frame.contentWindow && event.data === 'consensflow:close') dialog.close()
  })
  dialog.addEventListener('close', () => {
    settingsButton.focus()
    void act(readAgents)
  })
}

// Specs is no daemon screen but a dialog of the page: what it reads and keeps is the
// page's own (`specs.get`, `specs.set`), the same that every project's Specs section says
// what following the app means by. It opens as the screens do, Settings closed behind it,
// and gives the focus back to the Settings button as it closes.
const specsSettings = new SpecsSettings($('#specs-dialog'), {
  daemon,
  report,
  specs: () => state.specs,
  readSpecs,
  setSpecs: (specs) => {
    state.specs = specs
  },
})
$('#specs-settings-button').addEventListener('click', () =>
  act(async () => {
    settingsDialog.close()
    await specsSettings.open()
  }),
)
$('#specs-dialog').addEventListener('close', () => settingsButton.focus())

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
      // A file dropped on a terminal window types its path there.
      await listenForFileDrops({
        listen,
        invoke,
        focus: (id) => invoke('focus_after_drop', { id }),
        dock: terminals,
        report,
      })
    } catch (cause) {
      report(cause)
    }
  }
  // Updates are the app's, not the daemon's, and listen for the menu's Check for
  // Updates before anything waits on the daemon: its start may wait on a slow login
  // shell for half a minute, and a click before a listener is lost.
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
}

void start()
