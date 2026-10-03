import { button, element, icon, iconButton, redraw } from '../dom.js'
import { preview, render } from './markdown.js'

/**
 * The board: a kanban of the project's tasks. One row per participant, one
 * column per state, every task a card that stays where it ended, with its
 * result on it. A review is a task like any other, on its reviewer's row.
 * Above the grid, what waits for the human: messages to approve when the
 * project asks for approval, notes to read, and questions a coordinator has
 * left unanswered. The human is asked nothing here: the chief asks in its
 * terminal.
 *
 * Everything here is drawn from the daemon's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the daemon and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'paused', 'open']
/** What the chief (or the human) may stop: a task on the board or in a window. */
const PAUSABLE = ['open', 'queued', 'working', 'waiting']
/** What the human may give back to the board for another member of its tier. */
const REASSIGNABLE = ['queued', 'working', 'waiting', 'paused']
/** What is over, accepted or not: the human may delete it from the board. */
const FINISHED = ['accepted', 'failed', 'cancelled']
/** The columns, in reading order; whatever is over, accepted or not, shares the last one. */
const COLUMNS = [
  ['open', 'Backlog'],
  ['queued', 'Queued'],
  ['working', 'Working'],
  ['waiting', 'Waiting'],
  ['done', 'Done'],
  ['finished', 'Finished'],
]
/**
 * A session's tools, on its row and its card in the dock, and a crowded
 * cell's stack, each drawn as an icon: path data on a 24-unit grid.
 */
export const ICONS = {
  show: ['M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z', 'M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z'],
  hide: [
    'M3 3l18 18',
    'M10.6 5.1A10.4 10.4 0 0 1 12 5c6.4 0 10 7 10 7a17.6 17.6 0 0 1-2.2 3.2',
    'M6.6 6.6C3.8 8.5 2 12 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6',
    'M9.9 9.9a3 3 0 0 0 4.2 4.2',
  ],
  remove: ['M4 7h16', 'M9 7V4h6v3', 'M6 7l1 13h10l1-13', 'M10 11v6', 'M14 11v6'],
  stack: [
    'M4 11h16a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1Z',
    'M5 7.5h14',
    'M7 4h10',
  ],
}
/** How many cards a cell shows: more are one tile, a stack and how many, whose cards open in a dialog. */
const CELL_HOLDS = 3
const columnOf = (task) =>
  FINISHED.includes(task.state) ? 'finished' : task.state === 'paused' ? 'queued' : task.state
const STATE_LABEL = {
  open: 'Open',
  queued: 'Queued',
  working: 'Working',
  waiting: 'Waiting',
  paused: 'Paused',
  done: 'Done',
  accepted: 'Accepted',
  failed: 'Failed',
  cancelled: 'Cancelled',
}
const ACTIVITY_LABEL = {
  working: 'Working',
  idle: 'Idle',
  waiting: 'Waiting for you',
  starting: 'Starting',
  closed: 'No window',
  unknown: 'Unknown',
  out: 'Out of quota',
}
/** How a message's kind reads. */
const KIND_LABEL = {
  task: 'Task',
  result: 'Result',
  question: 'Question',
  answer: 'Answer',
  note: 'Note',
}

/** "just now", or "4m ago": how long ago something last changed. */
const ago = (iso, now) => {
  const since = age(iso, now)
  return since === 'just now' ? since : `${since} ago`
}

/** "just now", "4m", "2h", "3d": how long something has been in its state. */
function age(iso, now = Date.now()) {
  const seconds = Math.max(0, Math.round((now - Date.parse(iso)) / 1000))
  if (!Number.isFinite(seconds) || seconds < 45) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 36) return `${hours}h`
  return `${Math.round(hours / 24)}d`
}

/**
 * A member of the staff, as the ledger counts one: the lane of an agent of
 * its own, neither one of its sessions nor the chief, which runs on a saved
 * agent too.
 */
export const isMember = (participant) =>
  participant.agent !== null && participant.member === null && participant.role !== 'chief'

/** The role a member's task is for: its pool's. */
const roleOf = (task, roles) => task.pool ?? roles[0]

/**
 * A row's tasks, the newest first: its own, and those it asked for that
 * wait for a member, which sit in its backlog.
 */
const rowTasks = (lane, board) =>
  [...lane.tasks, ...board.open.filter((task) => task.requester === lane.participant.handle)].sort(
    (a, b) => b.number - a.number,
  )

/**
 * The board's rows: a member with several roles heads one row per role, each
 * followed by that role's sessions and holding that role's cards, so a worker
 * and a reviewer read as two things; everything else in lane order. The human
 * has no row: nothing assigns them a task, and what is for them is in For you.
 * The chief has one only while a task is on it, its own or one it asked for:
 * what it is doing is on its card in the dock.
 */
function boardRows(board) {
  const ordered = laneOrder(board.lanes).filter(
    (lane) =>
      lane.participant.role !== 'human' &&
      (lane.participant.role !== 'chief' || rowTasks(lane, board).length > 0),
  )
  const members = new Set(
    ordered.filter((lane) => isMember(lane.participant)).map((lane) => lane.participant.handle),
  )
  const rows = []
  for (const lane of ordered) {
    const { participant } = lane
    // A session is placed under its member's row for its role.
    if (participant.member !== null && members.has(participant.member)) continue
    if (!members.has(participant.handle)) {
      rows.push(lane)
      continue
    }
    const sessions = ordered.filter((other) => other.participant.member === participant.handle)
    const roles = participant.roles.length > 0 ? participant.roles : [participant.role]
    for (const role of roles) {
      rows.push({
        ...lane,
        participant: { ...participant, role, roles: [role] },
        tasks: lane.tasks.filter((task) => roleOf(task, roles) === role),
      })
      rows.push(...sessions.filter((session) => session.participant.role === role))
    }
    // A session of a role the member no longer holds still shows, last.
    rows.push(...sessions.filter((session) => !roles.includes(session.participant.role)))
  }
  return rows
}

/** The human and the chief first, then each member with its sessions right under it. */
export function laneOrder(lanes) {
  const rank = ({ participant }) => [
    participant.role === 'chief' ? 0 : 1,
    participant.memberId ?? participant.id,
    participant.memberId === null ? 0 : participant.id,
  ]
  return [...lanes].sort((a, b) => {
    const [x, y] = [rank(a), rank(b)]
    return x[0] - y[0] || x[1] - y[1] || x[2] - y[2]
  })
}

const who = (handle) => (handle === null || handle === undefined ? 'ConsensFlow' : `@${handle}`)
/** "18:30": a time of day, as when a member out of quota is back. */
const clock = (iso) =>
  new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
const outOfQuota = (participant, now) =>
  participant.outUntil !== null && Date.parse(participant.outUntil) > now
/** "@zeus · amber-pine" for a session; the member's own name otherwise. */
export const laneName = (participant) =>
  participant.member
    ? `@${participant.member} · ${participant.session}`
    : ({ human: 'You', chief: 'Chief of Staff' }[participant.handle] ?? `@${participant.handle}`)

/** "1 note", "3 notes": how many of something. */
const plural = (count, noun) => `${count} ${noun}${count === 1 ? '' : 's'}`

/** A member's row: how many of its sessions' terminals (of the row's role) are open now; null for none. */
function sessionsNote(lane, board) {
  const open = board.lanes.filter(
    (other) =>
      other.participant.member === lane.participant.handle &&
      other.participant.role === lane.participant.role &&
      other.pane !== null,
  ).length
  return open === 0 ? null : `${plural(open, 'terminal')} open, one per task`
}

/** "T-3, T-4": task numbers in a sentence. */
const taskNumbers = (numbers) => numbers.map((number) => `T-${number}`).join(', ')

/** Whom a task given by tier waits for: "a light worker", "an image designer". */
const aPool = (task) =>
  task.pool === 'designer' ? 'an image designer' : `a ${task.tier} ${task.pool}`

/** Where a task is going or came from, on its card; what it waits for first. */
function route(task) {
  if (task.assignee === null) {
    const waitsFor = `for ${aPool(task)}`
    return task.blockedBy.length === 0
      ? waitsFor
      : `blocked by ${taskNumbers(task.blockedBy)} · ${waitsFor}`
  }
  if (task.state === 'paused' && task.heldUntil) {
    return `out of quota until ${clock(task.heldUntil)} · from ${who(task.requester)}`
  }
  return task.blockedBy.length === 0
    ? `from ${who(task.requester)}`
    : `blocked by ${taskNumbers(task.blockedBy)} · from ${who(task.requester)}`
}

/** A member between tasks: its work runs in sessions, so it has no window of its own. */
const resting = (participant, activity) =>
  isMember(participant) && (activity?.state ?? 'closed') === 'closed'

/**
 * A session's row says whose window it is; the member's row says what it
 * is. Each says what its agent runs: its model, and its effort when it has
 * one, as the staff dialog does; so does the chief's card in the dock.
 */
export const identity = (participant, agent) =>
  (participant.member
    ? [
        `${participant.role} session of @${participant.member}`,
        participant.harness,
        agent?.model,
        agent?.effort,
      ]
    : [
        participant.role === 'chief' ? null : participant.roles.join('+'),
        participant.tier,
        participant.harness,
        agent?.model,
        agent?.effort,
      ]
  )
    .filter(Boolean)
    .join(' · ')

/**
 * What a row, or the chief's card in the dock, says its participant is
 * doing, and the state that colours it: an agent gone, a chief being
 * switched or a member out of quota say so before anything its window
 * does. A member with no terminal open says nothing (null): its row has no
 * status line.
 */
export function laneStatus(lane, board, now) {
  const { participant, activity } = lane
  if (lane.agentMissing) {
    const remedy =
      participant.role === 'chief'
        ? 'switch the chief'
        : `remove @${participant.handle} from the staff`
    return [
      'missing',
      `No agent named ${participant.agent} any more: define one under Agents, or ${remedy}`,
    ]
  }
  if (lane.switching) {
    return ['switching', `Switching the chief to ${lane.switching.agent} after this turn`]
  }
  if (outOfQuota(participant, now)) {
    return ['out', `Out of quota until ${clock(participant.outUntil)}`]
  }
  const state = activity?.state ?? 'closed'
  if (state === 'waiting' && activity.reason) return [state, `Waiting: ${activity.reason}`]
  if (resting(participant, activity)) {
    const note = sessionsNote(lane, board)
    return note === null ? null : [state, note]
  }
  // A closed session's row says nothing: its lamp and its Show terminal say it.
  if (participant.member !== null && state === 'closed') return null
  return [state, ACTIVITY_LABEL[state] ?? 'No window']
}

/** A participant's lamp: what its window is doing, at a glance, or that it is out of quota. */
export function lamp({ participant, activity }, now) {
  const node = element('span', 'lamp')
  node.dataset.testid = 'lamp'
  node.dataset.state = outOfQuota(participant, now) ? 'out' : (activity?.state ?? 'closed')
  node.setAttribute('aria-hidden', 'true')
  return node
}

/**
 * Whether anything on a board acts: a closed project's board reads (its
 * cards open, its messages and transcripts show) and has no control that
 * would change it, for the mouse or the keyboard.
 */
const acts = (board) => board.project.state === 'open'

/**
 * The finished tasks on a board as Delete finished takes them: those that
 * may go, and those kept because a task not yet finished needs them, each
 * with the tasks that do. The daemon decides again when it deletes them.
 */
function finishedTasks(board) {
  const tasks = [...board.open, ...board.lanes.flatMap((lane) => lane.tasks)].sort(
    (a, b) => a.number - b.number,
  )
  const unfinished = tasks.filter((task) => !FINISHED.includes(task.state))
  const finished = tasks
    .filter((task) => FINISHED.includes(task.state))
    .map((task) => ({
      number: task.number,
      neededBy: unfinished
        .filter((other) => other.needs.some((need) => need.number === task.number))
        .map((other) => other.number),
    }))
  return {
    deletable: finished.filter((task) => task.neededBy.length === 0).map((task) => task.number),
    kept: finished.filter((task) => task.neededBy.length > 0),
  }
}

export class BoardView {
  #root
  #actions
  /** The board drawn last: a row's button kept across redraws acts on its lane as it is now. */
  #board = null
  /** Whether the human asked to see a session's terminal, which the dock keeps out until then. */
  #shows = null
  /** The dialog that lists a crowded cell's cards, its title and its list. */
  #stack
  #stackTitle
  #stackCards
  /** The cell the stack dialog was opened on: its project, its row's handle and role, its column. */
  #stacked = null

  constructor(root, stack, actions) {
    this.#root = root
    this.#stack = stack
    this.#stackTitle = stack.querySelector('#stack-title')
    this.#stackCards = stack.querySelector('#stack-cards')
    this.#actions = actions
    // A card chosen opens its task, as on the board, and the dialog goes.
    this.#stackCards.addEventListener('click', (event) => {
      if (event.target.closest('.card') !== null) stack.close()
    })
  }

  /** Redraw from the daemon's state; `shows(participant)` says whose terminals the human asked to see. */
  render({ board, inbox, agents = [], shows, now = Date.now() }) {
    this.#board = board
    this.#shows = shows
    const models = new Map(agents.map((agent) => [agent.name, agent]))
    redraw(this.#root, [
      ...(acts(board) ? [] : [this.#closed(board.project)]),
      this.#forYou(inbox, board, now),
      this.#kanban(board, models, now),
    ])
    // An open stack dialog follows the board, as the cell it lists does.
    if (this.#stack.open) this.#drawStack()
  }

  /** `participant`'s lane on the board drawn last, which a kept button acts on. */
  #lane(participant) {
    return this.#board.lanes.find((lane) => lane.participant.handle === participant.handle)
  }

  /** A row's action, on its participant as the board drawn last has it. */
  #onLane(action, participant) {
    return () => action(this.#lane(participant).participant)
  }

  /** What a closed project shows above its board: why it is still, and the one way on. */
  #closed(project) {
    const banner = element('section', 'suspended')
    banner.setAttribute('role', 'status')
    banner.append(
      element('strong', null, `${project.name} is closed.`),
      element(
        'span',
        null,
        ' Its windows are gone, its open work went back to the backlog, and nothing is delivered until you resume it.',
      ),
      button('Resume project', 'primary-button', () => this.#actions.onResume(this.#board.project)),
    )
    return banner
  }

  /** What waits for the human. */
  #forYou(inbox, board, now) {
    // What waits for the human's approval, then the questions a coordinator
    // has left unanswered too long.
    const waiting = [
      ...(board.gated ?? []),
      ...(board.overdue ?? []).map((message) => ({ ...message, overdue: true })),
    ]
    // The human's notes not yet read: each reads and is marked read, in its
    // own list. The daemon reads the newest one frame holds, and says how many
    // there are.
    const { messages: notes, total: unread, shown } = inbox
    const section = element('section', 'foryou')
    section.setAttribute('role', 'region')
    section.setAttribute('aria-label', 'For you')
    const head = element('header', 'foryou-head')
    head.append(
      element('h2', 'foryou-name', 'For you'),
      element(
        'span',
        'foryou-status',
        waiting.length === 0 && unread === 0
          ? 'Nothing waiting'
          : [
              waiting.length === 0 ? null : `${waiting.length} waiting`,
              unread === 0 ? null : plural(unread, 'note'),
            ]
              .filter(Boolean)
              .join(' · '),
      ),
    )
    section.append(head)
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', 'Waiting for you')
    for (const message of waiting) strips.append(this.#messageStrip(message, board, now))
    // A gated project says what will wait here; otherwise the head's "Nothing waiting" says it all.
    if (waiting.length === 0 && board.project?.gate) {
      strips.append(
        element(
          'li',
          'bay-empty',
          'Every task, result, question and answer between your agents waits here for your approval.',
        ),
      )
    }
    if (strips.childElementCount > 0) section.append(strips)
    if (unread > 0) {
      const list = element('ol', 'strips')
      list.setAttribute('aria-label', 'Notes for you')
      for (const message of notes) list.append(this.#messageStrip(message, board, now))
      // Those shown, once read, make room for the earlier ones.
      const earlier = unread - shown
      if (earlier > 0) {
        list.append(
          element(
            'li',
            'strips-more',
            `${plural(earlier, 'earlier note')} not shown: mark these read to see ${earlier === 1 ? 'it' : 'them'}.`,
          ),
        )
      }
      section.append(element('h3', 'foryou-sub', 'Notes from your agents'), list)
    }
    return section
  }

  #messageStrip(message, board, now) {
    const item = element('li', 'strip strip-message')
    item.dataset.kind = message.kind
    item.dataset.message = String(message.id)
    const gated = message.state === 'gated'
    const toSomeone = message.overdue || gated
    const line = element('div', 'strip-line')
    line.append(
      element('span', 'strip-number', `m-${message.id}`),
      // The whole message, wrapped, read where it is answered. One with
      // options says nothing here: its choices under it are the questions.
      element('span', 'strip-title', message.questions ? '' : message.body),
      element(
        'span',
        'strip-route',
        `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)}${toSomeone ? ` to ${who(message.recipient)}` : ''}${message.taskNumber ? ` · T-${message.taskNumber}` : ''}${message.overdue ? ' · unanswered' : ''}${gated ? ' · needs your approval' : ''}`,
      ),
      element('span', 'strip-age', age(message.createdAt, now)),
    )
    if (message.overdue) item.dataset.overdue = 'true'
    if (gated) item.dataset.gated = 'true'
    item.append(line)
    const actions = element('div', 'strip-actions')
    if (gated && acts(board)) actions.append(...this.#gateActions(message))
    if (message.taskNumber) {
      actions.append(
        button('Open task', 'quiet-button', () => this.#actions.onOpenTask(message.taskNumber)),
      )
    }
    // An unanswered question of the chief's was never in the human's inbox.
    if (!gated && !message.overdue && acts(board)) {
      actions.append(
        button(
          'Mark read',
          'quiet-button',
          () => this.#actions.onRead(message),
          `Mark m-${message.id} read`,
        ),
      )
    }
    if (actions.childElementCount > 0) item.append(actions)
    return item
  }

  /**
   * What the human may do with a message that waits for approval: pass it on,
   * or decline a task or an answer (its sender is told). The human writes to
   * no agent from here: a result or a question goes on to the chief, who
   * decides and answers in its terminal.
   */
  #gateActions(message) {
    const actions = [
      button(
        'Approve',
        'primary-button',
        () => this.#actions.onApprove(message),
        `Approve m-${message.id} for ${who(message.recipient)}`,
      ),
    ]
    if (message.kind === 'task' || message.kind === 'answer') {
      actions.push(
        button(
          'Decline',
          'quiet-button',
          () => this.#actions.onDecline(message),
          `Decline m-${message.id}`,
        ),
      )
    }
    return actions
  }

  /** The grid: a row per participant in lane order, a column per state. */
  #kanban(board, models, now) {
    const table = element('table', 'kanban')
    table.setAttribute('aria-label', 'Tasks')
    const head = element('thead')
    const headRow = element('tr')
    headRow.append(element('th', 'kanban-staff', 'Staff'))
    for (const [state, label] of COLUMNS) {
      const cell = element('th', null, label)
      cell.dataset.state = state
      if (state === 'finished') cell.append(...this.#deleteFinished(board))
      headRow.append(cell)
    }
    head.append(headRow)
    const body = element('tbody')
    for (const lane of boardRows(board)) {
      body.append(this.#row(lane, board, models.get(lane.participant.agent), now))
    }
    // A project with nobody on its staff looks like any other board, and every
    // task the chief hands out is refused: say it where the members would be.
    if (!board.lanes.some((lane) => isMember(lane.participant))) {
      const row = element('tr', 'board-empty')
      const cell = element(
        'td',
        null,
        'No members yet: add the agents this project may use under Staff.',
      )
      cell.colSpan = COLUMNS.length + 1
      row.append(cell)
      body.append(row)
    }
    table.append(head, body)
    return table
  }

  /**
   * The Finished heading's control: every finished task that may go leaves
   * the board, once the human confirms; nothing while the project is closed.
   */
  #deleteFinished(board) {
    if (!acts(board) || finishedTasks(board).deletable.length === 0) return []
    return [
      iconButton(
        ICONS.remove,
        'Delete finished tasks',
        () => this.#actions.onDeleteFinished(this.#board.project, finishedTasks(this.#board)),
        'Delete finished',
        'danger-button',
      ),
    ]
  }

  #row(lane, board, agent, now) {
    const { participant } = lane
    const row = element('tr')
    row.dataset.handle = participant.handle
    row.dataset.role = participant.role
    if (participant.member) row.dataset.session = participant.member
    row.append(this.#rowHead(lane, board, agent, now))
    const mine = rowTasks(lane, board)
    for (const [state, label] of COLUMNS) {
      const cell = element('td')
      cell.dataset.state = state
      const held = mine.filter((task) => columnOf(task) === state)
      if (held.length > CELL_HOLDS) {
        cell.append(this.#stackTile(held.length, state, label, participant))
      } else if (held.length > 0) {
        const list = element('ol', 'cards')
        list.append(...held.map((task) => this.#card(task)))
        cell.append(list)
      }
      row.append(cell)
    }
    return row
  }

  #rowHead(lane, board, agent, now) {
    const { participant } = lane
    const head = element('th', 'row-head')
    head.setAttribute('scope', 'row')
    const title = element('div', 'row-title')
    title.append(lamp(lane, now), element('span', 'row-name', laneName(participant)))
    head.append(title)
    // The chief's row only holds its tasks: what it is doing, what it runs on
    // and Switch chief are on its card in the dock.
    if (participant.role === 'chief') return head
    head.append(element('span', 'row-meta', identity(participant, agent)))
    const said = laneStatus(lane, board, now)
    if (said !== null) {
      const [state, text] = said
      const status = element('span', 'row-status', text)
      status.dataset.state = state
      head.append(status)
    }
    head.append(this.#rowTools(lane, board))
    return head
  }

  /**
   * A session's terminal is the human's to show and hide: shown, its card is
   * in the dock; hidden, the card goes while its window works on. One whose
   * window has closed opens again on its own conversation when shown, and
   * its copy is on its last task's card. The session is deleted from here
   * too. Each of these is an icon, named in its tip; a closed project's rows
   * have none.
   */
  #rowTools(lane, board) {
    const { participant } = lane
    const tools = element('div', 'row-tools')
    const session = participant.member !== null
    const acting = acts(board)
    const name = laneName(participant)
    if (session && acting) {
      tools.append(
        this.#shows(participant)
          ? iconButton(
              ICONS.hide,
              'Hide terminal',
              this.#onLane(this.#actions.onHideTerminal, participant),
              `Hide ${name}'s terminal`,
            )
          : iconButton(
              ICONS.show,
              'Show terminal',
              () => {
                const shown = this.#lane(participant)
                this.#actions.onShowTerminal(shown.participant, { closed: shown.pane === null })
              },
              `Show ${name}'s terminal`,
            ),
      )
    }
    if (session && acting) {
      tools.append(
        iconButton(
          ICONS.remove,
          'Delete session',
          this.#onLane(this.#actions.onEndSession, participant),
          `Delete ${name}'s session`,
          'danger-button',
        ),
      )
    }
    return tools
  }

  /**
   * A crowded cell, `count` cards of the column `state` on `participant`'s
   * row: one tile, a stack and how many, named for what it holds. It opens
   * them in the stack dialog.
   */
  #stackTile(count, state, label, participant) {
    const tile = button(
      '',
      'stack',
      () => this.#openStack(participant, state),
      `${count} ${label.toLowerCase()} tasks of ${laneName(participant)}`,
    )
    // Across redraws its column tells it from the others, whatever its count.
    tile.dataset.stack = state
    tile.append(icon(ICONS.stack), element('span', 'stack-count', String(count)))
    return tile
  }

  /** Opens the stack dialog on the cell of the column `state` on a participant's row. */
  #openStack({ projectId, handle, role }, state) {
    this.#stacked = { project: projectId, handle, role, state }
    this.#drawStack()
    this.#stack.showModal()
  }

  /**
   * The stack dialog's cards: its cell's as the board drawn last has them,
   * the newest first, each opening its task as on the board. A cell left
   * with none (its tasks moved on, another project shown) closes it.
   */
  #drawStack() {
    const { project, handle, role, state } = this.#stacked
    const row =
      this.#board.project.id === project
        ? boardRows(this.#board).find(
            ({ participant }) => participant.handle === handle && participant.role === role,
          )
        : undefined
    const held =
      row === undefined ? [] : rowTasks(row, this.#board).filter((task) => columnOf(task) === state)
    if (held.length === 0) {
      this.#stack.close()
      return
    }
    const [, label] = COLUMNS.find(([column]) => column === state)
    this.#stackTitle.textContent = `${label} tasks of ${laneName(row.participant)}`
    redraw(
      this.#stackCards,
      held.map((task) => this.#card(task)),
    )
  }

  #card(task) {
    const item = element('li', 'card-item')
    const card = button('', 'card', () => this.#actions.onOpenTask(task.number))
    card.dataset.task = String(task.number)
    card.dataset.state = task.state
    card.setAttribute(
      'aria-label',
      `T-${task.number}, ${task.title}, ${STATE_LABEL[task.state]}, from ${who(task.requester)}`,
    )
    card.append(
      element('span', 'card-number', `T-${task.number}`),
      element('span', 'card-title', task.title),
      element('span', 'card-route', route(task)),
      element('span', 'card-state', STATE_LABEL[task.state]),
    )
    // A result reads by its first line: its drawer has it whole.
    if (task.result) card.append(element('span', 'card-result', task.result.trim().split('\n')[0]))
    item.append(card)
    return item
  }
}

/** How a transcript item's role reads on the card. */
const TRANSCRIPT_ROLE = {
  user: 'Sent to the window',
  assistant: 'The agent',
  tool: 'Tool output',
  custom: 'Note',
}

/**
 * One task: its story (each request, question, answer and result, in the
 * order they came), what its window wrote (ConsensFlow's own copy of the
 * conversation, kept after the window is gone), and what the human may do
 * next.
 */
export class TaskDrawer {
  #root
  #actions
  /** The task drawn, as it was read. */
  #task = null
  /** Its fold of what its window wrote, kept open or shut while the task is shown. */
  #fold = null

  constructor(root, actions) {
    this.#root = root
    this.#actions = actions
  }

  /** Whether `task` is the one drawn, unchanged since. */
  shows(task) {
    return JSON.stringify(task) === JSON.stringify(this.#task)
  }

  /** Whether `task` is the one drawn, as it was then or since. */
  #drawn(task) {
    return this.#task?.projectId === task.projectId && this.#task?.number === task.number
  }

  /**
   * Draws `task`. `total` is how many items its window wrote: the fold reads
   * them when it opens, and again when the task changed while it is open.
   * `closed`: the task's project is closed, and its task reads with nothing
   * to do on it. The same task drawn again keeps whatever did not change in
   * place: each step of its story and its fold open or shut, and the drawer
   * where it was scrolled.
   */
  show(task, { total, closed = false, now = Date.now() }) {
    const same = this.#drawn(task)
    const changed = !this.shows(task)
    const head = element('header', 'drawer-head')
    const title = element('h2', 'drawer-title')
    title.append(
      element('span', 'card-number', `T-${task.number}`),
      element('span', null, task.title),
    )
    head.append(
      title,
      button('Close', 'quiet-button', () => this.#actions.onClose(), 'Close the task'),
    )
    const meta = element(
      'p',
      'drawer-meta',
      `${who(task.requester)} asked ${task.assignee === null ? `for ${aPool(task)}` : who(task.assignee)} · ${STATE_LABEL[task.state]}${task.deletedAt ? ' · deleted from the board' : ''} · updated ${ago(task.updatedAt, now)}${task.needs.length === 0 ? '' : ` · needs ${task.needs.map((need) => `T-${need.number} (${need.state})`).join(', ')}`}`,
    )
    const sections = [head, meta, this.#story(task, same, now)]
    // What its window wrote is read only when asked for, folded until then.
    this.#fold = total > 0 ? ((same ? this.#fold : null) ?? this.#newFold()) : null
    if (this.#fold !== null) {
      redraw(this.#fold, [transcriptHead(total), ...[...this.#fold.children].slice(1)])
      sections.push(this.#fold)
    }
    const actions = element('div', 'drawer-actions')
    if (!closed) actions.append(...this.#taskActions(task))
    if (actions.childElementCount > 0) sections.push(actions)
    this.#task = task
    this.#root.setAttribute('aria-label', `Task T-${task.number}`)
    this.#root.dataset.state = task.state
    if (same) {
      redraw(this.#root, sections)
    } else {
      this.#root.replaceChildren(...sections)
      this.#root.scrollTop = 0
    }
    this.#root.hidden = false
    if (same && changed && this.#fold?.open) this.#actions.onTranscript(task)
  }

  /**
   * The task's story, a step for each part of it, oldest first. Only the
   * newest step is open until the human opens or shuts one: the same task
   * drawn again keeps each step as the human left it, and a step new since
   * opens when it is the newest. The earliest messages of a story too long
   * for one frame are left out after its first request.
   */
  #story(task, same, now) {
    // Each step drawn now, open or shut, by its message ('body' for the
    // request made from the task's body), read before it is drawn again.
    const shown = new Map()
    if (same) {
      for (const step of this.#root.querySelectorAll('details.step')) {
        shown.set(step.dataset.message ?? 'body', step.open)
      }
    }
    const steps = storyOf(task)
    const list = element('ol', 'story')
    list.setAttribute('aria-label', `T-${task.number}'s story`)
    const left = task.messagesLeftOut ?? 0
    for (const [at, step] of steps.entries()) {
      const open = shown.get(String(step.id ?? 'body')) ?? at === steps.length - 1
      list.append(storyStep(step, task.number, open, now))
      if (at === 0 && left > 0) {
        list.append(
          element(
            'li',
            'story-more',
            `${plural(left, 'earlier message')} not shown: cf task get T-${task.number} shows the whole thread.`,
          ),
        )
      }
    }
    const story = element('section', 'drawer-section')
    story.dataset.section = 'story'
    story.append(sectionHead('h3', 'Story'), list)
    return story
  }

  /** The open fold read again: its window wrote more since. */
  reread() {
    if (this.#task !== null && this.#fold?.open) this.#actions.onTranscript(this.#task)
  }

  /** What the task's window wrote, read for its open fold: the last items one frame holds. */
  fill(task, { items, total, shown }) {
    if (!this.#drawn(task) || this.#fold === null) return
    const list = element('ol', 'transcript')
    list.setAttribute('aria-label', `What T-${task.number}'s window wrote`)
    for (const item of items) {
      const entry = element('li', 'transcript-item')
      entry.dataset.role = item.role
      entry.append(
        element(
          'div',
          'transcript-head',
          `${TRANSCRIPT_ROLE[item.role] ?? item.role}${item.complete ? '' : ' · still writing'}`,
        ),
        element('pre', 'transcript-body', item.text),
      )
      list.append(entry)
    }
    redraw(this.#fold, [
      transcriptHead(total),
      ...(total > shown
        ? [element('p', 'transcript-more', `The last ${shown} of ${total} items.`)]
        : []),
      list,
    ])
  }

  #newFold() {
    const fold = element('details', 'drawer-section')
    fold.dataset.section = 'transcript'
    fold.addEventListener('toggle', () => {
      if (fold.open) this.#actions.onTranscript(this.#task)
    })
    return fold
  }

  /**
   * What the human may do with a task: nothing that writes to the agent
   * (that is done in its terminal), and no accepting (that is the chief's).
   */
  #taskActions(task) {
    // A button kept across redraws acts on the task as it was read last.
    const on = (action) => () => action(this.#task)
    const actions = []
    if (PAUSABLE.includes(task.state) && task.assignee !== 'chief') {
      actions.push(button('Pause', 'quiet-button', on(this.#actions.onPause)))
    }
    if (task.state === 'paused') {
      actions.push(button('Resume', 'primary-button', on(this.#actions.onResume)))
    }
    // Taken from its member and back on the board for its tier; the chief's
    // own work and work given by name have no tier to go back to.
    if (REASSIGNABLE.includes(task.state) && task.pool !== null && task.assignee !== null) {
      actions.push(
        button(
          'Reassign',
          'quiet-button',
          on(this.#actions.onReassign),
          `Reassign T-${task.number} to another member of its tier`,
        ),
      )
    }
    if (ACTIVE.includes(task.state)) {
      actions.push(button('Cancel task', 'danger-button', on(this.#actions.onCancel)))
    }
    // Over, it may leave the board for good, once the human confirms.
    if (FINISHED.includes(task.state) && task.deletedAt === null) {
      actions.push(button('Delete task', 'danger-button', on(this.#actions.onDelete)))
    }
    return actions
  }

  hide() {
    this.#task = null
    this.#fold = null
    this.#root.hidden = true
    this.#root.replaceChildren()
  }
}

/** A drawer section's heading: what it is, and how many when that says something. */
function sectionHead(tag, label, count) {
  const heading = element(tag, 'drawer-section-head')
  heading.append(element('span', null, label))
  if (count !== undefined) heading.append(element('span', 'drawer-count', count))
  return heading
}

const transcriptHead = (total) =>
  sectionHead('summary', 'What the agent did', plural(total, 'item'))

/** What the daemon cut to carry task T-`number` in one frame says where it reads whole. */
const cutNote = (part, number) =>
  part.bodyCut
    ? [element('p', 'drawer-cut', `Cut to fit here: cf task get T-${number} shows it whole.`)]
    : []

/**
 * A task's story, oldest first: every message but a note (ConsensFlow
 * talking to the chief) and a withdrawn one (it reached nobody), each with
 * who sent it to whom and how its delivery went. A request is numbered,
 * and a result takes the number of the request it answers.
 */
function storyOf(task) {
  const told = task.messages.filter(
    (message) => message.kind !== 'note' && message.state !== 'cancelled',
  )
  const steps = told.map((message) => ({
    ...message,
    route: `${who(message.sender)} → ${who(message.recipient)}`,
    delivery: delivery(message),
  }))
  // A task sent to nobody yet (open, waiting on what it needs, the chief's
  // own) asks in its body, from its requester. It was never sent: no time.
  if (!told.some((message) => message.kind === 'task')) {
    steps.unshift({
      id: null,
      kind: 'task',
      route: `${who(task.requester)} → ${task.assignee === null ? aPool(task) : who(task.assignee)}`,
      delivery: null,
      createdAt: null,
      body: task.body,
      bodyCut: task.bodyCut,
    })
  }
  let round = 0
  return steps.map((step) => {
    if (step.kind === 'task') round += 1
    const label =
      step.kind === 'task'
        ? `Request ${round}`
        : step.kind === 'result' && round > 0
          ? `Result ${round}`
          : (KIND_LABEL[step.kind] ?? step.kind)
    return { ...step, label }
  })
}

/** How a message's delivery reads while it has not reached its window. */
const DELIVERY = {
  queued: 'queued',
  delivering: 'delivering',
  gated: 'needs your approval',
  failed: 'not delivered',
}

/**
 * What a step says of its message's delivery, and why: nothing once it
 * reached its window, or was read at once (an answer picked from options).
 */
function delivery({ state, reason }) {
  if (state === 'delivered' || state === 'read') return null
  const said = DELIVERY[state] ?? state
  return reason ? `${said}: ${reason}` : said
}

/** When a message was sent: "14:05" today, "Sep 30 14:05" before; all of it on hover. */
function sentAt(iso, now) {
  const sent = new Date(iso)
  const day =
    sent.toDateString() === new Date(now).toDateString()
      ? ''
      : `${sent.toLocaleDateString([], { month: 'short', day: 'numeric' })} `
  const node = element('time', 'step-time', `${day}${clock(iso)}`)
  node.dateTime = iso
  node.title = sent.toLocaleString([], { dateStyle: 'full', timeStyle: 'medium', hour12: false })
  return node
}

/**
 * A step of a task's story: a fold whose line says what it is, who sent it
 * to whom, how its delivery went and when, then the first line of its
 * body; open, it reads whole, its markdown drawn. Its body is drawn once
 * it is open, so a long story draws only what is read.
 */
function storyStep(step, number, open, now) {
  const line = element('summary', 'step-head')
  line.append(element('span', 'step-label', step.label), element('span', 'step-route', step.route))
  if (step.delivery !== null) {
    const state = element('span', 'step-state', step.delivery)
    state.dataset.state = step.state
    line.append(state)
  }
  if (step.createdAt !== null) line.append(sentAt(step.createdAt, now))
  line.append(element('span', 'step-preview', preview(step.body)))
  const fold = element('details', 'step')
  fold.dataset.kind = step.kind
  if (step.id !== null) fold.dataset.message = String(step.id)
  fold.open = open
  fold.append(line)
  const drawBody = () => {
    const body = element('div', 'step-body')
    body.append(...render(step.body))
    fold.append(body, ...cutNote(step, number))
  }
  if (open) drawBody()
  // The click that opens it draws its body before it opens, unless it is there from before.
  line.addEventListener('click', () => {
    if (!fold.open && fold.childElementCount === 1) drawBody()
  })
  const item = element('li')
  item.append(fold)
  return item
}
