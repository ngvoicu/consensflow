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

const ACTIVE = ['working', 'waiting', 'queued', 'review', 'open']
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
  task.state === 'failed' || task.state === 'cancelled' ? 'ended' : task.state
const STATE_LABEL = {
  open: 'Open',
  queued: 'Queued',
  working: 'Working',
  waiting: 'Waiting',
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
const POOLS = ['worker', 'advisor']
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

const TEAM_OF_ROLE = { lead: 'lead', worker: 'lead', reviewer: 'lead', pm: 'pm', advisor: 'pm' }
const COORDINATORS = ['human', 'lead', 'pm']

/** Whose team a participant is on: the lead's (with workers and reviewers) or the PM's (with advisors). */
export const teamOf = (participant) => TEAM_OF_ROLE[participant.role] ?? null

/** The human first, then the lead's team and the PM's, each coordinator ahead of its members. */
export function laneOrder(lanes) {
  const rank = ({ participant }) => [
    [null, 'lead', 'pm'].indexOf(teamOf(participant)),
    COORDINATORS.includes(participant.role) ? 0 : 1,
    participant.id,
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
const laneName = (participant) =>
  ({ human: 'You', lead: 'Lead', pm: 'PM' })[participant.handle] ?? `@${participant.handle}`

/** Where a task is going or came from, on its card. */
function route(task) {
  if (task.assignee === null) {
    return `for a ${task.tier} ${task.pool}${task.tags.length === 0 ? '' : ` · ${task.tags.join(', ')}`}`
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

/** A member between tasks: one task per session, so its window is gone until the next. */
const resting = (participant, activity) =>
  ['worker', 'advisor', 'reviewer'].includes(participant.role) &&
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
    // The human's own inbox, then the questions a coordinator has left unanswered too long.
    const waiting = [
      ...inbox.filter((message) => message.state === 'queued'),
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
          'Questions from your agents and the results you asked for appear here.',
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
    const line = element('div', 'strip-line')
    line.append(
      element('span', 'strip-number', `m-${message.id}`),
      element('span', 'strip-title', message.body.split('\n')[0]),
      element(
        'span',
        'strip-route',
        `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)}${message.overdue ? ` to ${who(message.recipient)}` : ''}${message.taskNumber ? ` · T-${message.taskNumber}` : ''}${message.overdue ? ' · unanswered' : ''}`,
      ),
      element('span', 'strip-age', age(message.createdAt, now)),
    )
    if (message.overdue) item.dataset.overdue = 'true'
    item.append(line)
    if (message.body.includes('\n') && !message.questions) {
      item.append(element('p', 'strip-body', message.body))
    }
    if (message.kind === 'question') {
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
    const grouped = board.lanes.some((lane) => lane.participant.role === 'pm')
    let team = null
    for (const lane of laneOrder(board.lanes)) {
      const next = teamOf(lane.participant)
      if (grouped && next !== team) {
        const row = element('tr', 'board-group')
        const cell = element('td', null, next === 'pm' ? "PM's team" : "Lead's team")
        cell.colSpan = COLUMNS.length + 1
        row.append(cell)
        body.append(row)
      }
      team = next
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
    row.append(this.#rowHead(lane, agent, now))
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

  #rowHead(lane, agent, now) {
    const { participant, activity, pane } = lane
    const head = element('th', 'row-head')
    head.setAttribute('scope', 'row')
    if (participant.role === 'human') {
      head.append(element('span', 'row-name', 'You'))
      return head
    }
    const coordinator = participant.role === 'lead' || participant.role === 'pm'
    const identity = [
      coordinator ? null : participant.roles.join('+'),
      participant.tier,
      participant.tags.join(', ') || null,
      participant.harness,
      agent?.model,
    ]
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
              ? 'Free: a window opens with its next task'
              : (ACTIVITY_LABEL[activity?.state] ?? 'No window'),
    )
    status.dataset.state = out ? 'out' : (activity?.state ?? 'closed')
    const tools = element('div', 'row-tools')
    tools.append(
      button(
        'Terminal',
        'quiet-button',
        () => this.#actions.onOpenTerminal(participant),
        `Open ${laneName(participant)}'s terminal`,
      ),
    )
    if (pane === null && !lane.ended) tools.firstChild.disabled = true
    // Only a coordinator takes a task by name; members get theirs from the board by tier.
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
   * A new task: for a coordinator by name, or for a tier of member on the
   * team; tags prefer a member, and critical work names its purpose.
   */
  #openComposer(board) {
    const form = element('form', 'composer')
    const fields = element('div', 'composer-fields')
    const address = element('select')
    address.name = 'address'
    address.setAttribute('aria-label', 'For')
    for (const lane of board.lanes) {
      if (lane.participant.role !== 'lead' && lane.participant.role !== 'pm') continue
      const option = element('option', null, laneName(lane.participant))
      option.value = lane.participant.handle
      address.append(option)
    }
    for (const tier of TIERS) {
      for (const pool of POOLS) {
        const names = board.lanes
          .filter((lane) => lane.participant.roles.includes(pool) && lane.participant.tier === tier)
          .map((lane) => lane.participant.handle)
        if (names.length === 0) continue
        const option = element('option', null, `A ${tier} ${pool} (${names.join(', ')})`)
        option.value = `${pool}:${tier}`
        address.append(option)
      }
    }
    const tags = element('input')
    tags.name = 'tags'
    tags.type = 'text'
    tags.placeholder = 'coding, rust'
    tags.setAttribute('aria-label', 'Tags')
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
    const tagsLabel = labelled('Tags', tags)
    const tiered = () => address.value.includes(':')
    const arrange = () => {
      tagsLabel.hidden = !tiered()
      purposeLabel.hidden = !tiered() || !address.value.endsWith(':critical')
    }
    address.addEventListener('change', arrange)
    arrange()
    fields.append(labelled('For', address), tagsLabel, purposeLabel)
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
      this.#composingOpen = false
      if (!tiered()) {
        this.#actions.onGiveTask({ handle: address.value }, text)
        return
      }
      const [pool, tier] = address.value.split(':')
      this.#actions.onPutTask(
        {
          pool,
          tier,
          tags: tags.value
            .split(',')
            .map((tag) => tag.trim())
            .filter(Boolean),
          ...(tier === 'critical' ? { purpose: purpose.value } : {}),
        },
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

/**
 * One task: its brief, its result apart from it, its reviews with their
 * findings, the rest of its thread, and what the human may do next.
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

  show(task, now = Date.now()) {
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
      `${who(task.requester)} asked ${task.assignee === null ? `for a ${task.tier} ${task.pool}` : who(task.assignee)} · ${stateLabel(task)} · updated ${age(task.updatedAt, now)} ago`,
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
