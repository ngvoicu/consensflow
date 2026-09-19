import { initializeUpdates } from '../updates.js'
import { BoardView, element, TaskDrawer } from './board.js'
import { TerminalsView } from './terminals.js'

/**
 * The page: the sessions on the left, the chosen session's board (or its live
 * windows) on the right. Everything it shows comes from the new core through
 * the app's `core_request`, and it redraws when the core says something
 * changed. It keeps nothing of its own but what is on screen.
 */

const tauri = window.__TAURI__ ?? {}
const invoke = tauri.core?.invoke
const listen = tauri.event?.listen
const Channel = tauri.core?.Channel

const $ = (selector) => document.querySelector(selector)
const sessionList = $('#sessions')
const sessionTitle = $('#session-title')
const sessionDirectory = $('#session-directory')
const viewButtons = [...document.querySelectorAll('[data-view]')]
const inboxButton = $('#inbox-button')
const teamButton = $('#team-button')
const boardRoot = $('#board')
const stage = $('#stage')
const status = $('#status')

const state = {
  sessions: [],
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
        session: state.selected,
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
    state.view = 'terminals'
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
    act(() => core('task.accept', { session: state.selected, task: task.number })),
  onReopen: (task, text) =>
    act(() => core('task.reopen', { session: state.selected, task: task.number, body: text })),
  onCancel: (task) =>
    act(() => core('task.cancel', { session: state.selected, task: task.number })),
})

const terminals = new TerminalsView(stage, $('#parking'), {
  invoke: (command, args) => invoke(command, args),
  report,
  createEmulator: tauri.test?.createEmulator,
})

async function openTask(number) {
  const { task } = await core('task.get', { session: state.selected, task: number })
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
      const { sessions } = await core('sessions.list')
      state.sessions = sessions
      if (!sessions.some((session) => session.id === state.selected)) {
        state.selected = (sessions.find((s) => s.state === 'open') ?? sessions[0])?.id ?? null
      }
      if (state.selected === null) {
        state.board = null
        state.inbox = []
      } else {
        const [{ board: current }, { messages }] = await Promise.all([
          core('board.get', { session: state.selected }),
          core('inbox.get', { session: state.selected, participant: 'human' }),
        ])
        state.board = current
        state.inbox = messages
      }
      if (state.openTask !== null && drawer.open) await openTask(state.openTask)
      render()
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
  renderSessions()
  const session = state.sessions.find((s) => s.id === state.selected) ?? null
  sessionTitle.textContent = session?.name ?? 'No session'
  sessionDirectory.textContent = session?.directory ?? ''
  const waiting = state.inbox.filter((message) => message.state === 'queued').length
  inboxButton.textContent = waiting === 0 ? 'Inbox' : `Inbox (${waiting})`
  inboxButton.dataset.waiting = String(waiting > 0)
  teamButton.disabled = session === null
  for (const control of viewButtons) {
    control.setAttribute('aria-pressed', String(control.dataset.view === state.view))
    control.disabled = session === null
  }
  const lanes = state.board?.lanes ?? []
  boardRoot.hidden = state.view !== 'board'
  stage.hidden = state.view !== 'terminals'
  if (state.board === null) {
    boardRoot.replaceChildren(
      element(
        'p',
        'board-empty',
        'Start a session to see its board: choose New session and pick the project folder.',
      ),
    )
  } else {
    board.render({ board: state.board, inbox: state.inbox, agents: state.agents })
  }
  terminals.render(lanes, { visible: state.view === 'terminals', focus: state.focus })
}

function renderSessions() {
  const items = state.sessions.map((session) => {
    const item = element('li', 'session')
    item.dataset.state = session.state
    const select = element('button', 'session-select')
    select.type = 'button'
    select.setAttribute('aria-current', String(session.id === state.selected))
    select.append(
      element('span', 'session-name', session.name),
      element('span', 'session-state', session.state === 'open' ? 'Open' : 'Suspended'),
    )
    select.addEventListener('click', () => {
      state.selected = session.id
      state.focus = null
      state.openTask = null
      drawer.hide()
      void refresh()
    })
    item.append(select)
    if (session.state !== 'open') {
      const resume = element('button', 'quiet-button', 'Resume')
      resume.type = 'button'
      resume.setAttribute('aria-label', `Resume ${session.name}`)
      resume.addEventListener('click', () =>
        act(() => core('session.resume', { session: session.id })),
      )
      item.append(resume)
    }
    return item
  })
  if (items.length === 0) items.push(element('li', 'sessions-empty', 'No sessions yet.'))
  sessionList.replaceChildren(...items)
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

// New session: the native folder picker first, then the lead's harness.
const newSessionDialog = $('#new-session-dialog')
const newSessionForm = newSessionDialog.querySelector('form')
$('#new-session').addEventListener('click', async () => {
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
    newSessionForm.elements.directory.value = directory
    newSessionDialog.showModal()
  } catch (cause) {
    report(cause)
  }
})
newSessionForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const directory = newSessionForm.elements.directory.value
  const harness = newSessionForm.elements.harness.value
  newSessionDialog.close()
  void act(async () => {
    const { session } = await core('session.open', { directory, harness })
    state.selected = session.id
    state.view = 'board'
  })
})
newSessionDialog
  .querySelector('[value="cancel"]')
  .addEventListener('click', () => newSessionDialog.close())

// The session team: who the coordinators may hand work to.
const teamDialog = $('#team-dialog')
const teamForm = teamDialog.querySelector('form')
teamButton.addEventListener('click', () =>
  act(async () => {
    const { agents } = await core('agents.list')
    state.agents = agents
    const members = new Set(
      (state.board?.lanes ?? []).map((lane) => lane.participant.agent).filter(Boolean),
    )
    const list = $('#team-members')
    const rows = (state.board?.lanes ?? [])
      .filter((lane) => lane.participant.agent !== null)
      .map((lane) =>
        element(
          'li',
          null,
          `@${lane.participant.handle} · ${lane.participant.role} · ${lane.participant.harness}`,
        ),
      )
    list.replaceChildren(
      ...(rows.length
        ? rows
        : [element('li', 'team-empty', 'Nobody yet: add the agents this session may use.')]),
    )
    const picker = teamForm.elements.agent
    const choices = agents.filter((agent) => !members.has(agent.name))
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
    teamForm.querySelector('[type="submit"]').disabled = choices.length === 0
    teamDialog.showModal()
  }),
)
teamForm.addEventListener('submit', (event) => {
  event.preventDefault()
  const agent = teamForm.elements.agent.value
  const role = teamForm.elements.role.value
  teamDialog.close()
  void act(async () => {
    await core('member.add', { session: state.selected, agent, role })
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
              name: state.board.session.name,
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
