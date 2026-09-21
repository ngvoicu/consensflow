/**
 * The board: a kanban of the project's tasks. One row per participant, one
 * column per state, every task a card that stays where it ended, with its
 * result on it. A task's reviews sit under its card, never as cards of their
 * own. Above the grid, what waits for the human: questions to answer, results
 * and notes to read, and the composer that puts a new task on the board.
 *
 * Everything here is drawn from the core's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the core and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'paused', 'review', 'open']
/** What the lead (or the human) may stop: work on the board or in a window, never a review. */
const PAUSABLE = ['open', 'queued', 'working', 'waiting']
/** The columns, in reading order; failed and cancelled share the last one. */
const COLUMNS = [
  ['open', 'Backlog'],
  ['queued', 'Queued'],
  ['working', 'Working'],
  ['waiting', 'Waiting'],
  ['review', 'In review'],
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
  review: 'In review',
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
const TIERS = ['critical', 'complex', 'standard', 'light']
const PURPOSES = ['critical-review', 'architecture', 'hard-problem', 'important-question']
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
/** The role a member's task is for: a review is a reviewer's, the rest its pool's. */
const roleOf = (task, roles) => (task.kind === 'review' ? 'reviewer' : (task.pool ?? roles[0]))

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

/** The fields a redraw must not lose: text areas, and the New task form's choices. */
const DRAFT_FIELDS = 'textarea, form.composer select, form.composer input'

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

const stateLabel = (task) =>
  task.state === 'review' ? `In review · round ${task.round + 1}` : STATE_LABEL[task.state]

/** One line per review, as it reads under the task it reviews. */
const reviewLine = (review) =>
  review.state === 'done'
    ? `Reviewed by ${who(review.reviewer)}, round ${review.round}: ${review.verdict ?? 'pass (no verdict line)'}`
    : `${who(review.reviewer)} is reviewing, round ${review.round}`

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
  #composing = null
  #composingOpen = false
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
      button('New task', 'quiet-button', () => {
        this.#composingOpen = !this.#composingOpen
        this.#actions.onRedraw()
      }),
    )
    section.append(head)
    if (this.#composingOpen) section.append(this.#openComposer(board))
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
            : 'Questions from your agents and the results you asked for appear here.',
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
    } else if (message.kind === 'question') {
      item.append(message.questions ? this.#choiceForm(message) : this.#answerForm(message))
    } else {
      const actions = element('div', 'strip-actions')
      if (message.taskNumber) {
        actions.append(
          button('Open task', 'quiet-button', () => this.#actions.onOpenTask(message.taskNumber)),
        )
      }
      actions.append(
        button(
          'Mark read',
          'quiet-button',
          () => this.#actions.onRead(message),
          `Mark m-${message.id} read`,
        ),
      )
      item.append(actions)
    }
    return item
  }

  /**
   * What the human may do with a message that waits for approval: pass it on
   * as it is, or the one other thing its kind allows. A task or an answer is
   * declined with a word to its sender; a result goes back to its window with
   * a follow-up; a question is answered here instead of by the one asked.
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
    if (message.kind === 'result') {
      const form = element('form', 'reopen')
      const field = element('textarea')
      field.rows = 2
      field.required = true
      field.setAttribute('aria-label', `Follow-up for T-${message.taskNumber}`)
      field.placeholder = `What should ${who(message.sender)} change?`
      const submit = element('button', 'quiet-button', 'Send back')
      submit.type = 'submit'
      form.append(field, submit)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        if (field.value.trim()) this.#actions.onSendBack(message, field.value.trim())
      })
      actions.append(form)
    } else if (message.kind === 'question') {
      actions.append(message.questions ? this.#choiceForm(message) : this.#answerForm(message))
    } else {
      const form = element('form', 'decline')
      const field = element('input')
      field.type = 'text'
      field.setAttribute('aria-label', `Why m-${message.id} is declined`)
      field.placeholder = 'Why (optional)'
      const submit = element('button', 'quiet-button', 'Decline')
      submit.type = 'submit'
      form.append(field, submit)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        this.#actions.onDecline(message, field.value.trim())
      })
      actions.append(form)
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
    // Reviews hang under the task they review, wherever the reviewer sits.
    const reviews = new Map()
    for (const lane of board.lanes) {
      for (const task of lane.tasks) {
        if (task.kind !== 'review') continue
        const list = reviews.get(task.reviewOf) ?? []
        list.push({ ...task, reviewer: lane.participant.handle })
        reviews.set(task.reviewOf, list)
      }
    }
    for (const lane of boardRows(board.lanes)) {
      body.append(this.#row(lane, board, models.get(lane.participant.agent), reviews, now))
    }
    table.append(head, body)
    return table
  }

  #row(lane, board, agent, reviews, now) {
    const { participant, activity, pane } = lane
    const row = element('tr')
    row.dataset.handle = participant.handle
    row.dataset.role = participant.role
    if (participant.member) row.dataset.session = participant.member
    row.append(this.#rowHead(lane, board, agent, now))
    // A task waiting for a member sits in its requester's backlog.
    const mine = [
      ...lane.tasks.filter((task) => task.kind !== 'review'),
      ...board.open.filter((task) => task.requester === participant.handle),
    ].sort((a, b) => b.number - a.number)
    // A review is never a card of its own: it hangs under the task it reviews,
    // and the reviewer's row only says so in its status.
    for (const [state] of COLUMNS) {
      const cell = element('td')
      cell.dataset.state = state
      const list = element('ol', 'cards')
      for (const task of mine) {
        if (columnOf(task) !== state) continue
        list.append(this.#card(task, reviews.get(task.number) ?? [], now))
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
    const reviewing = lane.tasks.find((task) => task.kind === 'review' && task.state !== 'done')
    const status = element(
      'span',
      'row-status',
      out
        ? `Out of quota until ${clock(participant.outUntil)}`
        : activity?.state === 'waiting' && activity.reason
          ? `Waiting: ${activity.reason}`
          : reviewing !== undefined
            ? `Reviewing T-${reviewing.reviewOf}`
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
    // Only the lead takes a task by name; members get theirs from the board by tier.
    if (coordinator) {
      tools.append(
        button(
          'Give a task',
          'quiet-button',
          () => this.#compose(participant.handle),
          `Give ${laneName(participant)} a task`,
        ),
      )
    }
    const title = element('div', 'row-title')
    title.append(
      lamp(out ? { state: 'out' } : activity),
      element('span', 'row-name', laneName(participant)),
    )
    head.append(title, element('span', 'row-meta', identity), status, tools)
    if (this.#composing === participant.handle) head.append(this.#composer(participant))
    return head
  }

  #card(task, reviews, now) {
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
      element('span', 'card-state', stateLabel(task)),
      element('span', 'card-age', age(task.updatedAt, now)),
    )
    if (task.result) card.append(element('span', 'card-result', task.result))
    item.append(card)
    if (reviews.length > 0) {
      const list = element('ul', 'reviews')
      for (const review of reviews) list.append(element('li', null, reviewLine(review)))
      item.append(list)
    }
    return item
  }

  /**
   * A new task: for the lead by name, for a tier of worker on the team, or an
   * image from the designer; critical work names its purpose. Advice is the
   * lead's alone to ask.
   */
  #openComposer(board) {
    const form = element('form', 'composer')
    const fields = element('div', 'composer-fields')
    const address = element('select')
    address.name = 'address'
    address.setAttribute('aria-label', 'For')
    for (const lane of board.lanes) {
      if (lane.participant.role !== 'lead') continue
      const option = element('option', null, laneName(lane.participant))
      option.value = lane.participant.handle
      address.append(option)
    }
    for (const tier of TIERS) {
      const names = board.lanes
        .filter(
          (lane) =>
            lane.participant.member === null &&
            lane.participant.roles.includes('worker') &&
            lane.participant.tier === tier,
        )
        .map((lane) => lane.participant.handle)
      if (names.length === 0) continue
      const option = element('option', null, `A ${tier} worker (${names.join(', ')})`)
      option.value = `worker:${tier}`
      address.append(option)
    }
    const designers = board.lanes
      .filter(
        (lane) => lane.participant.member === null && lane.participant.roles.includes('designer'),
      )
      .map((lane) => lane.participant.handle)
    if (designers.length > 0) {
      const option = element('option', null, `An image designer (${designers.join(', ')})`)
      option.value = 'designer'
      address.append(option)
    }
    const purpose = element('select')
    purpose.name = 'purpose'
    purpose.setAttribute('aria-label', 'Purpose')
    for (const value of PURPOSES) {
      const option = element('option', null, value)
      option.value = value
      purpose.append(option)
    }
    const labelled = (text, control) => {
      const label = element('label', null, text)
      label.append(control)
      return label
    }
    const purposeLabel = labelled('Purpose', purpose)
    // What the task waits for first: task numbers, checked by the browser.
    const needs = element('input')
    needs.name = 'needs'
    needs.type = 'text'
    needs.placeholder = 'T-3, T-4'
    needs.pattern = String.raw`\s*((T-?)?\d+\s*(,\s*(T-?)?\d+\s*)*)?`
    needs.title = 'Task numbers, like T-3, T-4'
    needs.setAttribute('aria-label', 'Only after')
    const needsLabel = labelled('Only after', needs)
    const tiered = () => address.value.includes(':') || address.value === 'designer'
    const arrange = () => {
      purposeLabel.hidden = !address.value.endsWith(':critical')
      needsLabel.hidden = !tiered()
    }
    address.addEventListener('change', arrange)
    arrange()
    fields.append(labelled('For', address), purposeLabel, needsLabel)
    const field = element('textarea')
    field.name = 'task'
    field.rows = 3
    field.required = true
    field.setAttribute('aria-label', 'Task')
    field.placeholder =
      'What should be done? The member starts from nothing: include every detail it needs and what to return.'
    field.dataset.draft = 'open'
    const submit = element('button', 'primary-button', 'Put on the board')
    submit.type = 'submit'
    const actions = element('div', 'composer-actions')
    actions.append(
      button('Cancel', 'quiet-button', () => {
        this.#composingOpen = false
        this.#actions.onRedraw()
      }),
      submit,
    )
    form.append(fields, field, actions)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const text = field.value.trim()
      if (!text) return
      this.#drafts.delete('open')
      this.#drafts.delete('Only after')
      this.#composingOpen = false
      if (!tiered()) {
        this.#actions.onGiveTask({ handle: address.value }, text)
        return
      }
      const after = [...needs.value.matchAll(/\d+/g)].map((match) => Number(match[0]))
      const waits = after.length === 0 ? {} : { needs: after }
      if (address.value === 'designer') {
        this.#actions.onPutTask({ pool: 'designer', ...waits }, text)
        return
      }
      const [pool, tier] = address.value.split(':')
      this.#actions.onPutTask(
        { pool, tier, ...(tier === 'critical' ? { purpose: purpose.value } : {}), ...waits },
        text,
      )
    })
    requestAnimationFrame(() => field.focus())
    return form
  }

  #composer(participant) {
    const form = element('form', 'composer')
    const field = element('textarea')
    field.name = 'task'
    field.rows = 3
    field.required = true
    field.setAttribute('aria-label', `Task for ${laneName(participant)}`)
    field.placeholder = `What should ${laneName(participant)} do? Include every detail it needs and what to return.`
    field.dataset.draft = participant.handle
    const submit = element('button', 'primary-button', 'Queue task')
    submit.type = 'submit'
    const actions = element('div', 'composer-actions')
    actions.append(
      button('Cancel', 'quiet-button', () => this.#compose(null)),
      submit,
    )
    form.append(field, actions)
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const text = field.value.trim()
      if (!text) return
      this.#drafts.delete(participant.handle)
      this.#composing = null
      this.#actions.onGiveTask(participant, text)
    })
    requestAnimationFrame(() => field.focus())
    return form
  }

  #compose(handle) {
    this.#composing = this.#composing === handle ? null : handle
    this.#actions.onRedraw()
  }

  /**
   * What the human is writing survives a redraw: every text area, and the
   * New task form's choices (who it is for, the purpose, what it waits for).
   * A choice is restored before the fields it shows or hides.
   */
  #saveDrafts() {
    for (const field of this.#root.querySelectorAll(DRAFT_FIELDS)) {
      const key = field.dataset.draft ?? field.getAttribute('aria-label')
      if (field.value) this.#drafts.set(key, field.value)
      else this.#drafts.delete(key)
    }
  }

  #restoreDrafts() {
    for (const field of this.#root.querySelectorAll(DRAFT_FIELDS)) {
      const key = field.dataset.draft ?? field.getAttribute('aria-label')
      if (!this.#drafts.has(key)) continue
      field.value = this.#drafts.get(key)
      if (field.tagName === 'SELECT') field.dispatchEvent(new Event('change'))
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
 * One task: its brief, its result apart from it, its reviews with their
 * findings, the rest of its thread, what its window wrote (ConsensFlow's own
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
      `${who(task.requester)} asked ${task.assignee === null ? `for a ${task.tier} ${task.pool}` : who(task.assignee)} · ${stateLabel(task)} · updated ${age(task.updatedAt, now)} ago${task.needs.length === 0 ? '' : ` · needs ${task.needs.map((need) => `T-${need.number} (${need.state})`).join(', ')}`}`,
    )
    meta.dataset.state = task.state
    const sections = [head, meta]
    const brief = element('section', 'drawer-section')
    brief.append(element('h3', null, 'Brief'), element('p', 'drawer-brief', task.body))
    sections.push(brief)
    const result = task.messages.findLast((message) => message.kind === 'result')
    if (result !== undefined) {
      const block = element('section', 'drawer-section drawer-result-section')
      block.append(element('h3', null, 'Result'), element('p', 'drawer-result', result.body))
      sections.push(block)
    }
    for (const review of task.reviews ?? []) {
      const block = element('section', 'drawer-section drawer-review')
      block.dataset.review = String(review.number)
      block.append(element('h3', 'drawer-review-head', reviewLine(review)))
      if (review.findings !== null) {
        block.append(element('p', 'drawer-review-body', review.findings))
      }
      sections.push(block)
    }
    // The rest of the thread: follow-ups, questions and answers, notes.
    const rest = task.messages.filter(
      (message) => message !== result && !(message.kind === 'task' && message.body === task.body),
    )
    if (rest.length > 0) {
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
      sections.push(thread)
    }
    const actions = element('div', 'drawer-actions')
    if (transcript.items.length > 0) {
      const section = element('section', 'drawer-section')
      section.append(element('h3', null, 'What the agent did'))
      if (transcript.total > transcript.items.length) {
        section.append(
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
      section.append(list)
      sections.push(section)
    }
    if (task.state === 'done') {
      actions.append(button('Accept', 'primary-button', () => this.#actions.onAccept(task)))
      if (task.kind === 'work') {
        actions.append(
          button('Ask for a review', 'quiet-button', () => this.#actions.onReview(task)),
        )
      }
    }
    if (task.state === 'done' || task.state === 'failed') {
      const form = element('form', 'reopen')
      const field = element('textarea')
      field.rows = 3
      field.required = true
      field.setAttribute('aria-label', `Follow-up for T-${task.number}`)
      field.placeholder = `What should ${who(task.assignee)} change?`
      const submit = element('button', 'quiet-button', 'Send back')
      submit.type = 'submit'
      form.append(field, submit)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        if (field.value.trim()) this.#actions.onReopen(task, field.value.trim())
      })
      actions.append(form)
    }
    if (PAUSABLE.includes(task.state) && task.kind === 'work' && task.assignee !== 'lead') {
      actions.append(button('Pause', 'quiet-button', () => this.#actions.onPause(task)))
    }
    if (task.state === 'paused') {
      const form = element('form', 'reopen')
      const field = element('textarea')
      field.rows = 3
      field.required = true
      field.setAttribute('aria-label', `Resume T-${task.number} with`)
      field.placeholder = 'What should happen now? It goes into the same window.'
      const submit = element('button', 'primary-button', 'Resume')
      submit.type = 'submit'
      form.append(field, submit)
      form.addEventListener('submit', (event) => {
        event.preventDefault()
        if (field.value.trim()) this.#actions.onResume(task, field.value.trim())
      })
      actions.append(form)
    }
    if (ACTIVE.includes(task.state)) {
      actions.append(button('Cancel task', 'danger-button', () => this.#actions.onCancel(task)))
    }
    this.#root.setAttribute('aria-label', `Task T-${task.number}`)
    this.#root.dataset.state = task.state
    this.#root.replaceChildren(...sections, actions)
    this.#root.hidden = false
  }

  hide() {
    this.#root.hidden = true
    this.#root.replaceChildren()
  }
}
