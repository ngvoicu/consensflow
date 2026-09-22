/**
 * The board: a kanban of the project's tasks. One row per participant, one
 * column per state, every task a card that stays where it ended, with its
 * result on it. A review is a task like any other, on its reviewer's row.
 * Above the grid, what waits for the human: questions to answer, results and
 * notes to read.
 *
 * Everything here is drawn from the core's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the core and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'paused', 'open']
/** What the lead (or the human) may stop: a task on the board or in a window. */
const PAUSABLE = ['open', 'queued', 'working', 'waiting']
/** What the human may give back to the board for another member of its tier. */
const REASSIGNABLE = ['queued', 'working', 'waiting', 'paused']
/** The columns, in reading order; failed and cancelled share the last one. */
const COLUMNS = [
  ['open', 'Backlog'],
  ['queued', 'Queued'],
  ['working', 'Working'],
  ['waiting', 'Waiting'],
  ['done', 'Done'],
  ['accepted', 'Accepted'],
  ['ended', 'Ended'],
]
const columnOf = (task) =>
  task.state === 'failed' || task.state === 'cancelled'
    ? 'ended'
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
/** The work tiers, in the order the composer offers them. */
const KIND_LABEL = {
  task: 'Task',
  result: 'Result',
  question: 'Question',
  answer: 'Answer',
  note: 'Note',
}

export function element(tag, className, text) {
  const node = document.createElement(tag)
  if (className) node.className = className
  if (text !== undefined && text !== null) node.textContent = text
  return node
}

function button(text, className, action, label) {
  const node = element('button', className, text)
  node.type = 'button'
  if (label) node.setAttribute('aria-label', label)
  node.addEventListener('click', action)
  return node
}

/** "just now", or "4m ago": how long ago something last changed. */
const ago = (iso, now) => {
  const since = age(iso, now)
  return since === 'just now' ? since : `${since} ago`
}

/** "just now", "4m", "2h", "3d": how long something has been in its state. */
export function age(iso, now = Date.now()) {
  const seconds = Math.max(0, Math.round((now - Date.parse(iso)) / 1000))
  if (!Number.isFinite(seconds) || seconds < 45) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 36) return `${hours}h`
  return `${Math.round(hours / 24)}d`
}

const COORDINATORS = ['human', 'lead']

/** The human and the lead first, then each member with its sessions right under it. */
/** The role a member's task is for: its pool's. */
const roleOf = (task, roles) => task.pool ?? roles[0]

/**
 * The board's rows: a member with several roles heads one row per role, each
 * followed by that role's sessions and holding that role's cards, so a worker
 * and a reviewer read as two things; everything else in lane order.
 */
export function boardRows(lanes) {
  const ordered = laneOrder(lanes)
  const members = new Set(
    ordered
      .filter((lane) => lane.participant.agent !== null && lane.participant.member === null)
      .map((lane) => lane.participant.handle),
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

export function laneOrder(lanes) {
  const rank = ({ participant }) => [
    COORDINATORS.includes(participant.role) ? 0 : 1,
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
const laneName = (participant) =>
  participant.member
    ? `@${participant.member} · ${participant.session}`
    : ({ human: 'You', lead: 'Lead' }[participant.handle] ?? `@${participant.handle}`)

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
    : `${open} terminal${open === 1 ? '' : 's'} open, one per task`
}

/** "T-3, T-4": task numbers in a sentence. */
const tasks = (numbers) => numbers.map((number) => `T-${number}`).join(', ')

/** Where a task is going or came from, on its card; what it waits for first. */
function route(task) {
  if (task.assignee === null) {
    const waitsFor =
      task.pool === 'designer' ? 'for an image designer' : `for a ${task.tier} ${task.pool}`
    return task.blockedBy.length === 0
      ? waitsFor
      : `blocked by ${tasks(task.blockedBy)} · ${waitsFor}`
  }
  return `from ${who(task.requester)}`
}

/** A member between tasks: its work runs in sessions, so it has no window of its own. */
const resting = (participant, activity) =>
  ['worker', 'advisor', 'reviewer'].includes(participant.role) &&
  participant.member === null &&
  (activity?.state ?? 'closed') === 'closed'

function lamp(activity) {
  const node = element('span', 'lamp')
  node.dataset.testid = 'lamp'
  node.dataset.state = activity?.state ?? 'closed'
  node.setAttribute('aria-hidden', 'true')
  return node
}

export class BoardView {
  #root
  #actions
  #drafts = new Map()

  constructor(root, actions) {
    this.#root = root
    this.#actions = actions
  }

  /** Redraw from the core's state, keeping an open composer and its text. */
  render({ board, inbox, agents = [], now = Date.now() }) {
    this.#saveDrafts()
    const models = new Map(agents.map((agent) => [agent.name, agent]))
    this.#root.replaceChildren(this.#forYou(inbox, board, now), this.#kanban(board, models, now))
    this.#restoreDrafts()
  }

  /** What waits for the human, and where a new task starts. */
  #forYou(inbox, board, now) {
    // The human's own inbox, what waits for the human's approval, then the
    // questions a coordinator has left unanswered too long.
    const waiting = [
      ...inbox.filter((message) => message.state === 'queued'),
      ...(board.gated ?? []),
      ...(board.overdue ?? []).map((message) => ({ ...message, overdue: true })),
    ]
    const section = element('section', 'foryou')
    section.setAttribute('role', 'region')
    section.setAttribute('aria-label', 'For you')
    const head = element('header', 'foryou-head')
    head.append(
      element('h2', 'foryou-name', 'For you'),
      element(
        'span',
        'foryou-status',
        waiting.length === 0 ? 'Nothing waiting' : `${waiting.length} waiting`,
      ),
    )
    section.append(head)
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', 'Waiting for you')
    for (const message of waiting) strips.append(this.#messageStrip(message, now))
    if (waiting.length === 0) {
      strips.append(
        element(
          'li',
          'bay-empty',
          board.project?.gate
            ? 'Every task, result, question and answer between your agents waits here for your approval.'
            : 'Questions from your agents appear here.',
        ),
      )
    }
    section.append(strips)
    return section
  }

  /** A plain question: the answer is typed. */
  #answerForm(message) {
    const form = element('form', 'answer')
    const field = element('textarea')
    field.name = 'answer'
    field.rows = 2
    field.required = true
    field.setAttribute('aria-label', `Answer to m-${message.id}`)
    field.placeholder = `Answer ${who(message.sender)}`
    form.append(field, element('button', 'primary-button', 'Send answer'))
    form.querySelector('button').type = 'submit'
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      if (field.value.trim()) this.#actions.onAnswer(message, field.value.trim())
    })
    return form
  }

  /**
   * A question with options, as the agent's own question tool asked it: each
   * question's options to pick (one, or several when it allows), and a line
   * for something else; every question needs a pick before the answer goes.
   */
  #choiceForm(message) {
    const form = element('form', 'answer answer-choices')
    form.setAttribute('aria-label', `Answer to m-${message.id}`)
    const groups = message.questions.map((question, at) => {
      const group = element('fieldset', 'choice')
      group.append(element('legend', null, `${question.header}: ${question.question}`))
      for (const option of question.options) {
        const label = element('label', 'choice-option')
        const input = element('input')
        input.type = question.multiple ? 'checkbox' : 'radio'
        input.name = `pick-${at}`
        input.value = option.label
        label.append(input, element('span', 'choice-label', option.label))
        if (option.description) label.append(element('span', 'choice-desc', option.description))
        group.append(label)
      }
      const custom = element('input', 'choice-custom')
      custom.type = 'text'
      custom.name = `custom-${at}`
      custom.placeholder = 'Something else'
      custom.setAttribute('aria-label', `Something else for ${question.header}`)
      group.append(custom)
      return group
    })
    const send = element('button', 'primary-button', 'Send answer')
    send.type = 'submit'
    form.append(...groups, send)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const choices = message.questions.map((_question, at) => {
        const picked = [...form.querySelectorAll(`input[name="pick-${at}"]:checked`)].map(
          (input) => input.value,
        )
        const custom = form.elements[`custom-${at}`].value.trim()
        return custom ? [...picked, custom] : picked
      })
      for (const [at, group] of groups.entries()) {
        group.classList.toggle('choice-missing', choices[at].length === 0)
      }
      if (choices.every((picks) => picks.length > 0)) this.#actions.onAnswer(message, null, choices)
    })
    return form
  }

  #messageStrip(message, now) {
    const item = element('li', 'strip strip-message')
    item.dataset.kind = message.kind
    item.dataset.message = String(message.id)
    const gated = message.state === 'gated'
    const toSomeone = message.overdue || gated
    const line = element('div', 'strip-line')
    line.append(
      element('span', 'strip-number', `m-${message.id}`),
      element('span', 'strip-title', message.body.split('\n')[0]),
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
    if (message.body.includes('\n') && !message.questions) {
      item.append(element('p', 'strip-body', message.body))
    }
    if (gated) {
      item.append(this.#gateActions(message))
    } else if (message.kind === 'question' && !message.overdue) {
      // A question put to the human is answered here; one the lead has left
      // unanswered is a notice: tell the lead in its terminal.
      item.append(message.questions ? this.#choiceForm(message) : this.#answerForm(message))
    } else {
      const actions = element('div', 'strip-actions')
      if (message.taskNumber) {
        actions.append(
          button('Open task', 'quiet-button', () => this.#actions.onOpenTask(message.taskNumber)),
        )
      }
      // An unanswered question of the lead's was never in the human's inbox.
      if (!message.overdue) {
        actions.append(
          button(
            'Mark read',
            'quiet-button',
            () => this.#actions.onRead(message),
            `Mark m-${message.id} read`,
          ),
        )
      }
      item.append(actions)
    }
    return item
  }

  /**
   * What the human may do with a message that waits for approval: pass it on,
   * or decline a task or an answer (its sender is told). The human writes to
   * no agent from here: a result or a question goes on to the lead, who
   * decides and answers in its terminal.
   */
  #gateActions(message) {
    const actions = element('div', 'strip-actions')
    actions.append(
      button(
        'Approve',
        'primary-button',
        () => this.#actions.onApprove(message),
        `Approve m-${message.id} for ${who(message.recipient)}`,
      ),
    )
    if (message.kind === 'task' || message.kind === 'answer') {
      actions.append(
        button(
          'Decline',
          'quiet-button',
          () => this.#actions.onDecline(message),
          `Decline m-${message.id}`,
        ),
      )
    }
    if (message.taskNumber) {
      actions.append(
        button('Open task', 'quiet-button', () => this.#actions.onOpenTask(message.taskNumber)),
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
    headRow.append(element('th', 'kanban-team', 'Team'))
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
    // A project with nobody on its team looks like any other board, and every
    // task the lead hands out is refused: say it where the members would be.
    if (!board.lanes.some((lane) => lane.participant.agent !== null)) {
      const row = element('tr', 'board-empty')
      const cell = element(
        'td',
        null,
        'No members yet: add the agents this project may use under Team.',
      )
      cell.colSpan = COLUMNS.length + 1
      row.append(cell)
      body.append(row)
    }
    table.append(head, body)
    return table
  }

  #row(lane, board, agent, now) {
    const { participant, activity, pane } = lane
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
        list.append(this.#card(task, now))
      }
      if (list.childElementCount > 0) cell.append(list)
      row.append(cell)
    }
    if (pane === null && !resting(participant, activity)) row.dataset.window = 'none'
    return row
  }

  #rowHead(lane, board, agent, now) {
    const { participant, activity, pane } = lane
    const head = element('th', 'row-head')
    head.setAttribute('scope', 'row')
    if (participant.role === 'human') {
      head.append(element('span', 'row-name', 'You'))
      return head
    }
    const coordinator = participant.role === 'lead'
    // A session's row says whose window it is; the member's row says what it is.
    const identity = (
      participant.member
        ? [
            `${participant.role} session of @${participant.member}`,
            participant.harness,
            agent?.model,
          ]
        : [
            coordinator ? null : participant.roles.join('+'),
            participant.tier,
            participant.harness,
            agent?.model,
          ]
    )
      .filter(Boolean)
      .join(' · ')
    const out = outOfQuota(participant, now)
    const status = element(
      'span',
      'row-status',
      out
        ? `Out of quota until ${clock(participant.outUntil)}`
        : activity?.state === 'waiting' && activity.reason
          ? `Waiting: ${activity.reason}`
          : resting(participant, activity)
            ? sessionsNote(lane, board)
            : participant.member !== null && (activity?.state ?? 'closed') === 'closed'
              ? 'Terminal closed'
              : (ACTIVITY_LABEL[activity?.state] ?? 'No window'),
    )
    status.dataset.state = out ? 'out' : (activity?.state ?? 'closed')
    const tools = element('div', 'row-tools')
    // A member's row heads its sessions and has no terminal of its own. A
    // session's closed terminal opens again on its own conversation; an open
    // one closes; the session is deleted from here too, and a closed one's
    // copy is on its last task's card.
    const heading =
      participant.agent !== null && participant.member === null && pane === null && !lane.ended
    const session = participant.member !== null
    const latest = lane.tasks.at(-1)
    if (!heading) {
      // An open terminal is already in the dock: only a closed one offers Open.
      if (pane === null) {
        const openTerminal = button(
          'Open terminal',
          'quiet-button',
          () => this.#actions.onOpenTerminal(participant),
          `Open ${laneName(participant)}'s terminal`,
        )
        openTerminal.disabled = !lane.ended && !session
        tools.append(openTerminal)
      }
      if (session && pane !== null) {
        tools.append(
          button(
            'Close terminal',
            'quiet-button',
            () => this.#actions.onCloseTerminal(participant),
            `Close ${laneName(participant)}'s terminal`,
          ),
        )
      }
      if (session && pane === null && latest !== undefined) {
        tools.append(
          button(
            'Transcript',
            'quiet-button',
            () => this.#actions.onOpenTask(latest.number),
            `What ${laneName(participant)}'s terminal wrote`,
          ),
        )
      }
    }
    if (session) {
      tools.append(
        button(
          'Delete session',
          'danger-button',
          () => this.#actions.onEndSession(participant),
          `Delete ${laneName(participant)}'s session`,
        ),
      )
    }
    const title = element('div', 'row-title')
    title.append(
      lamp(out ? { state: 'out' } : activity),
      element('span', 'row-name', laneName(participant)),
    )
    head.append(title, element('span', 'row-meta', identity), status, tools)
    return head
  }

  #card(task, now) {
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
      element('span', 'card-age', age(task.updatedAt, now)),
    )
    if (task.result) card.append(element('span', 'card-result', task.result))
    item.append(card)
    return item
  }

  /** What the human is writing in a text area survives a redraw. */
  #saveDrafts() {
    for (const field of this.#root.querySelectorAll('textarea')) {
      const key = field.dataset.draft ?? field.getAttribute('aria-label')
      if (field.value) this.#drafts.set(key, field.value)
      else this.#drafts.delete(key)
    }
  }

  #restoreDrafts() {
    for (const field of this.#root.querySelectorAll('textarea')) {
      const key = field.dataset.draft ?? field.getAttribute('aria-label')
      if (this.#drafts.has(key)) field.value = this.#drafts.get(key)
    }
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

  constructor(root, actions) {
    this.#root = root
    this.#actions = actions
  }

  get open() {
    return !this.#root.hidden
  }

  show(task, { transcript = { items: [], total: 0 }, now = Date.now() } = {}) {
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
    meta.dataset.state = task.state
    const sections = [head, meta]
    const panel = (name, label, count) => {
      const section = element(name === 'transcript' ? 'details' : 'section', 'drawer-section')
      section.dataset.section = name
      const heading = element(name === 'transcript' ? 'summary' : 'h3', 'drawer-section-head')
      heading.append(element('span', null, label))
      if (count !== undefined) heading.append(element('span', 'drawer-count', count))
      section.append(heading)
      return section
    }
    const brief = panel('brief', 'Brief')
    brief.append(element('p', 'drawer-brief', task.body))
    sections.push(brief)
    const result = task.messages.findLast((message) => message.kind === 'result')
    if (result !== undefined) {
      const block = panel('result', 'Result')
      block.append(element('p', 'drawer-result', result.body))
      sections.push(block)
    }
    // The thread keeps only what the rest of the drawer does not say: the
    // questions, answers, follow-ups and earlier results. Each window's
    // first task message is the brief it was given, a note is ConsensFlow
    // talking to the lead, and a withdrawn message reached nobody.
    const briefed = new Set()
    const rest = task.messages.filter((message) => {
      if (message === result || message.kind === 'note' || message.state === 'cancelled')
        return false
      if (message.kind !== 'task' || briefed.has(message.recipient)) return true
      briefed.add(message.recipient)
      return false
    })
    if (rest.length > 0) {
      const block = panel('thread', 'Thread', String(rest.length))
      const thread = element('ol', 'thread')
      thread.setAttribute('aria-label', `T-${task.number}'s thread`)
      for (const message of rest) {
        const item = element('li', 'thread-item')
        item.dataset.kind = message.kind
        item.append(
          element(
            'p',
            'thread-head',
            `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)} to ${who(message.recipient)} · ${message.state}${message.reason ? ` (${message.reason})` : ''}`,
          ),
          element('p', 'thread-body', message.body),
        )
        thread.append(item)
      }
      block.append(thread)
      sections.push(block)
    }
    if (transcript.items.length > 0) {
      const block = panel(
        'transcript',
        'What the agent did',
        `${transcript.total} item${transcript.total === 1 ? '' : 's'}`,
      )
      block.open = true
      if (transcript.total > transcript.items.length) {
        block.append(
          element(
            'p',
            'transcript-more',
            `The last ${transcript.items.length} of ${transcript.total} items.`,
          ),
        )
      }
      const list = element('ol', 'transcript')
      list.setAttribute('aria-label', `What T-${task.number}'s window wrote`)
      for (const item of transcript.items) {
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
      block.append(list)
      sections.push(block)
    }
    // What the human may do: nothing that writes to the agent (that is done
    // in its terminal), and no accepting (that is the lead's).
    const actions = element('div', 'drawer-actions')
    if (PAUSABLE.includes(task.state) && task.assignee !== 'lead') {
      actions.append(button('Pause', 'quiet-button', () => this.#actions.onPause(task)))
    }
    if (task.state === 'paused') {
      actions.append(button('Resume', 'primary-button', () => this.#actions.onResume(task)))
    }
    // Taken from its member and back on the board for its tier; the lead's
    // own work and work given by name have no tier to go back to.
    if (REASSIGNABLE.includes(task.state) && task.pool !== null && task.assignee !== null) {
      actions.append(
        button(
          'Reassign',
          'quiet-button',
          () => this.#actions.onReassign(task),
          `Reassign T-${task.number} to another member of its tier`,
        ),
      )
    }
    if (ACTIVE.includes(task.state)) {
      actions.append(button('Cancel task', 'danger-button', () => this.#actions.onCancel(task)))
    }
    this.#root.setAttribute('aria-label', `Task T-${task.number}`)
    this.#root.dataset.state = task.state
    this.#root.replaceChildren(...sections, ...(actions.childElementCount > 0 ? [actions] : []))
    this.#root.hidden = false
  }

  hide() {
    this.#root.hidden = true
    this.#root.replaceChildren()
  }
}
