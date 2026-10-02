import { button, element, redraw } from '../dom.js'

/**
 * The board: a kanban of the project's tasks. One row per participant, one
 * column per state, every task a card that stays where it ended, with its
 * result on it. A review is a task like any other, on its reviewer's row.
 * Above the grid, what waits for the human: messages to approve when the
 * project asks for approval, notes to read, and questions a coordinator has
 * left unanswered. The human is asked nothing here: the chief asks in its
 * terminal.
 *
 * Everything here is drawn from the core's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the core and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'paused', 'open']
/** The harnesses a lead runs on, as the human knows them, in the order they are offered. */
export const HARNESS_NAMES = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  pi: 'Pi',
  devin: 'Devin',
}
/** What the chief (or the human) may stop: a task on the board or in a window. */
const PAUSABLE = ['open', 'queued', 'working', 'waiting']
/** What the human may give back to the board for another member of its tier. */
const REASSIGNABLE = ['queued', 'working', 'waiting', 'paused']
/** The columns, in reading order; whatever is over, accepted or not, shares the last one. */
const COLUMNS = [
  ['open', 'Backlog'],
  ['queued', 'Queued'],
  ['working', 'Working'],
  ['waiting', 'Waiting'],
  ['done', 'Done'],
  ['finished', 'Finished'],
]
const columnOf = (task) =>
  ['accepted', 'failed', 'cancelled'].includes(task.state)
    ? 'finished'
    : task.state === 'paused'
      ? 'queued'
      : task.state
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
 * its own, neither one of its sessions nor the lead, which runs on a saved
 * agent too once the lead is switched to one.
 */
export const isMember = (participant) =>
  participant.agent !== null && participant.member === null && participant.role !== 'chief'

/** The role a member's task is for: its pool's. */
const roleOf = (task, roles) => task.pool ?? roles[0]

/**
 * The board's rows: a member with several roles heads one row per role, each
 * followed by that role's sessions and holding that role's cards, so a worker
 * and a reviewer read as two things; everything else in lane order. The human
 * has no row: nothing assigns them a task, and what is for them is in For you.
 */
function boardRows(lanes) {
  const ordered = laneOrder(lanes).filter((lane) => lane.participant.role !== 'human')
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
/** "18:30": when a member out of quota is back. */
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

/** A member's row: how many of its sessions' terminals (of the row's role) are open now. */
function sessionsNote(lane, board) {
  const open = board.lanes.filter(
    (other) =>
      other.participant.member === lane.participant.handle &&
      other.participant.role === lane.participant.role &&
      other.pane !== null,
  ).length
  return open === 0
    ? 'Free: a terminal opens with its next task'
    : `${plural(open, 'terminal')} open, one per task`
}

/** "T-3, T-4": task numbers in a sentence. */
const taskNumbers = (numbers) => numbers.map((number) => `T-${number}`).join(', ')

/** Where a task is going or came from, on its card; what it waits for first. */
function route(task) {
  if (task.assignee === null) {
    const waitsFor =
      task.pool === 'designer' ? 'for an image designer' : `for a ${task.tier} ${task.pool}`
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

/** A session's row says whose window it is; the member's row says what it is. */
const identity = (participant, agent) =>
  (participant.member
    ? [`${participant.role} session of @${participant.member}`, participant.harness, agent?.model]
    : [
        participant.role === 'chief' ? null : participant.roles.join('+'),
        participant.tier,
        participant.harness,
        agent?.model,
      ]
  )
    .filter(Boolean)
    .join(' · ')

/**
 * What a row says its participant is doing, and the state that colours it:
 * an agent gone, a lead being switched or a member out of quota say so
 * before anything its window does.
 */
function rowStatus(lane, board, now) {
  const { participant, activity } = lane
  if (lane.agentMissing) {
    const remedy =
      participant.role === 'chief'
        ? 'switch the lead'
        : `remove @${participant.handle} from the staff`
    return [
      'missing',
      `No agent named ${participant.agent} any more: define one under Agents, or ${remedy}`,
    ]
  }
  if (lane.switching) {
    const { agent, harness } = lane.switching
    return [
      'switching',
      `Switching the lead to ${agent ?? HARNESS_NAMES[harness] ?? harness} after this turn`,
    ]
  }
  if (outOfQuota(participant, now)) {
    return ['out', `Out of quota until ${clock(participant.outUntil)}`]
  }
  const state = activity?.state ?? 'closed'
  if (state === 'waiting' && activity.reason) return [state, `Waiting: ${activity.reason}`]
  if (resting(participant, activity)) return [state, sessionsNote(lane, board)]
  if (participant.member !== null && state === 'closed') return [state, 'Terminal closed']
  return [state, ACTIVITY_LABEL[state] ?? 'No window']
}

/** A participant's lamp: what its window is doing, at a glance. */
export function lamp(activity) {
  const node = element('span', 'lamp')
  node.dataset.testid = 'lamp'
  node.dataset.state = activity?.state ?? 'closed'
  node.setAttribute('aria-hidden', 'true')
  return node
}

/**
 * Whether anything on a board acts: a closed project's board reads (its
 * cards open, its messages and transcripts show) and has no control that
 * would change it, for the mouse or the keyboard.
 */
const acts = (board) => board.project.state === 'open'

export class BoardView {
  #root
  #actions
  /** The board drawn last: a row's button kept across redraws acts on its lane as it is now. */
  #board = null
  /** Whether the human asked to see a session's terminal, which the dock keeps out until then. */
  #shows = null

  constructor(root, actions) {
    this.#root = root
    this.#actions = actions
  }

  /** Redraw from the core's state; `shows(participant)` says whose terminals the human asked to see. */
  render({ board, inbox, agents = [], shows, now = Date.now() }) {
    this.#board = board
    this.#shows = shows
    const models = new Map(agents.map((agent) => [agent.name, agent]))
    redraw(this.#root, [
      ...(acts(board) ? [] : [this.#closed(board.project)]),
      this.#forYou(inbox, board, now),
      this.#kanban(board, models, now),
    ])
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
    // own list. The core reads the newest one frame holds, and says how many
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
    if (waiting.length === 0) {
      strips.append(
        element(
          'li',
          'bay-empty',
          board.project?.gate
            ? 'Every task, result, question and answer between your agents waits here for your approval.'
            : 'Nothing waits for you on the board: the chief asks in its terminal.',
        ),
      )
    }
    section.append(strips)
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
      headRow.append(cell)
    }
    head.append(headRow)
    const body = element('tbody')
    for (const lane of boardRows(board.lanes)) {
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

  #row(lane, board, agent, now) {
    const { participant } = lane
    const row = element('tr')
    row.dataset.handle = participant.handle
    row.dataset.role = participant.role
    if (participant.member) row.dataset.session = participant.member
    row.append(this.#rowHead(lane, board, agent, now))
    // A task waiting for a member sits in its requester's backlog.
    const mine = [
      ...lane.tasks,
      ...board.open.filter((task) => task.requester === participant.handle),
    ].sort((a, b) => b.number - a.number)
    for (const [state] of COLUMNS) {
      const cell = element('td')
      cell.dataset.state = state
      const list = element('ol', 'cards')
      for (const task of mine) {
        if (columnOf(task) !== state) continue
        list.append(this.#card(task))
      }
      if (list.childElementCount > 0) cell.append(list)
      row.append(cell)
    }
    return row
  }

  #rowHead(lane, board, agent, now) {
    const { participant, activity } = lane
    const head = element('th', 'row-head')
    head.setAttribute('scope', 'row')
    const title = element('div', 'row-title')
    title.append(
      lamp(outOfQuota(participant, now) ? { state: 'out' } : activity),
      element('span', 'row-name', laneName(participant)),
    )
    const [state, text] = rowStatus(lane, board, now)
    const status = element('span', 'row-status', text)
    status.dataset.state = state
    head.append(
      title,
      element('span', 'row-meta', identity(participant, agent)),
      status,
      this.#rowTools(lane, board),
    )
    return head
  }

  /**
   * Only a session's terminal is the human's to open and close: a member's
   * row heads its sessions and has no terminal of its own, and the chief's
   * opens and closes with the project. An open terminal stays out of the
   * dock until the human shows it, and hides again; a closed one opens
   * again on its own conversation, and its copy is on its last task's card.
   * The session is deleted from here too. A closed project's rows keep only
   * the copy.
   */
  #rowTools(lane, board) {
    const { participant, pane } = lane
    const tools = element('div', 'row-tools')
    const session = participant.member !== null
    const acting = acts(board)
    const name = laneName(participant)
    if (session && pane !== null && acting) {
      tools.append(
        this.#shows(participant)
          ? button(
              'Hide terminal',
              'quiet-button',
              this.#onLane(this.#actions.onHideTerminal, participant),
              `Hide ${name}'s terminal`,
            )
          : button(
              'Show terminal',
              'quiet-button',
              this.#onLane(this.#actions.onShowTerminal, participant),
              `Show ${name}'s terminal`,
            ),
      )
    }
    if (session && pane === null && acting) {
      tools.append(
        button(
          'Open terminal',
          'quiet-button',
          this.#onLane(this.#actions.onOpenTerminal, participant),
          `Open ${name}'s terminal`,
        ),
      )
    }
    if (session && pane === null && lane.tasks.length > 0) {
      tools.append(
        button(
          'Transcript',
          'quiet-button',
          () => this.#actions.onOpenTask(this.#lane(participant).tasks.at(-1).number),
          `What ${name}'s terminal wrote`,
        ),
      )
    }
    if (session && pane !== null && acting) {
      tools.append(
        button(
          'Close terminal',
          'quiet-button',
          this.#onLane(this.#actions.onCloseTerminal, participant),
          `Close ${name}'s terminal`,
        ),
      )
    }
    if (participant.role === 'chief' && acting) {
      tools.append(
        button(
          'Switch lead',
          'quiet-button',
          this.#onLane(this.#actions.onSwitchLead, participant),
          'Switch the lead to another harness or model',
        ),
      )
    }
    if (session && acting) {
      tools.append(
        button(
          'Delete session',
          'danger-button',
          this.#onLane(this.#actions.onEndSession, participant),
          `Delete ${name}'s session`,
        ),
      )
    }
    return tools
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
    if (task.result) card.append(element('span', 'card-result', task.result))
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
 * One task: its brief, its result apart from it, the rest of its thread,
 * what its window wrote (ConsensFlow's own
 * copy of the conversation, kept after the window is gone), and what the
 * human may do next.
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
   * place: its fold open or shut, and the drawer where it was scrolled.
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
      `${who(task.requester)} asked ${task.assignee === null ? `for a ${task.tier} ${task.pool}` : who(task.assignee)} · ${STATE_LABEL[task.state]} · updated ${ago(task.updatedAt, now)}${task.needs.length === 0 ? '' : ` · needs ${task.needs.map((need) => `T-${need.number} (${need.state})`).join(', ')}`}`,
    )
    const brief = panel('brief', 'Brief')
    brief.append(element('p', 'drawer-brief', task.body), ...cutNote(task, task.number))
    const sections = [head, meta, brief]
    const result = task.messages.findLast((message) => message.kind === 'result')
    if (result !== undefined) {
      const block = panel('result', 'Result')
      block.append(element('p', 'drawer-result', result.body), ...cutNote(result, task.number))
      sections.push(block)
    }
    const thread = threadOf(task.messages, result)
    if (thread.length > 0 || (task.messagesLeftOut ?? 0) > 0) {
      sections.push(threadPanel(task, thread))
    }
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

/** Each part of a task is a panel of its own, headed by what it is. */
function panel(name, label, count) {
  const section = element('section', 'drawer-section')
  section.dataset.section = name
  section.append(sectionHead('h3', label, count))
  return section
}

/** What the core cut to carry task T-`number` in one frame says where it reads whole. */
const cutNote = (part, number) =>
  part.bodyCut
    ? [element('p', 'drawer-cut', `Cut to fit here: cf task get T-${number} shows it whole.`)]
    : []

/**
 * The thread keeps only what the rest of the drawer does not say: the
 * questions, answers, follow-ups and earlier results. Each window's first
 * task message is the brief it was given, a note is ConsensFlow talking to
 * the chief, and a withdrawn message reached nobody.
 */
function threadOf(messages, result) {
  const briefed = new Set()
  return messages.filter((message) => {
    if (message === result || message.kind === 'note' || message.state === 'cancelled') {
      return false
    }
    if (message.kind !== 'task' || briefed.has(message.recipient)) return true
    briefed.add(message.recipient)
    return false
  })
}

/** The thread's panel; the earliest messages of a thread too long for one frame are left out. */
function threadPanel(task, messages) {
  const block = panel('thread', 'Thread', String(messages.length))
  const left = task.messagesLeftOut ?? 0
  if (left > 0) {
    block.append(
      element(
        'p',
        'thread-more',
        `${plural(left, 'earlier message')} not shown: cf task get T-${task.number} shows the whole thread.`,
      ),
    )
  }
  const thread = element('ol', 'thread')
  thread.setAttribute('aria-label', `T-${task.number}'s thread`)
  for (const message of messages) {
    const item = element('li', 'thread-item')
    item.dataset.kind = message.kind
    item.append(
      element(
        'p',
        'thread-head',
        `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)} to ${who(message.recipient)} · ${message.state}${message.reason ? ` (${message.reason})` : ''}`,
      ),
      element('p', 'thread-body', message.body),
      ...cutNote(message, task.number),
    )
    thread.append(item)
  }
  block.append(thread)
  return block
}
