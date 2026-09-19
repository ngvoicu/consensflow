/**
 * The board: one bay per participant, each task a strip in its assignee's bay,
 * the way a control room keeps one strip per flight in each controller's bay.
 * The human's bay comes first and holds what waits for them: questions to
 * answer, results and notes to read, tasks given to them.
 *
 * Everything here is drawn from the core's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the core and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'review', 'open']
const OPEN = [...ACTIVE, 'done', 'failed']
const STATE_ORDER = [
  'review',
  'waiting',
  'working',
  'queued',
  'done',
  'failed',
  'accepted',
  'cancelled',
]
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

/** Where a task is going or came from, on its strip. */
function route(task) {
  if (task.kind === 'review') return `review of T-${task.reviewOf} for ${who(task.requester)}`
  if (task.assignee === null) {
    return `for a ${task.tier} ${task.pool}${task.tags.length === 0 ? '' : ` · ${task.tags.join(', ')}`}`
  }
  return `from ${who(task.requester)}`
}

const stateLabel = (task) =>
  task.state === 'review' ? `In review · round ${task.round + 1}` : STATE_LABEL[task.state]

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

  /**
   * Redraw from the core's state, keeping an open composer and its text. With
   * a PM in the project, each team's bays sit under its own label.
   */
  render({ board, inbox, agents = [], now = Date.now() }) {
    this.#saveDrafts()
    const models = new Map(agents.map((agent) => [agent.name, agent]))
    const grouped = board.lanes.some((lane) => lane.participant.role === 'pm')
    const nodes = []
    let team = null
    for (const lane of laneOrder(board.lanes)) {
      const next = teamOf(lane.participant)
      if (grouped && next !== team) {
        nodes.push(element('p', 'board-group', next === 'pm' ? "PM's team" : "Lead's team"))
      }
      team = next
      nodes.push(
        lane.participant.role === 'human'
          ? this.#humanBay(lane, inbox, now)
          : this.#agentBay(lane, models.get(lane.participant.agent), now),
      )
      if (lane.participant.role === 'human') nodes.push(this.#backlogBay(board, now))
    }
    this.#root.replaceChildren(...nodes)
    this.#restoreDrafts()
  }

  #humanBay(lane, inbox, now) {
    const waiting = inbox.filter((message) => message.state === 'queued')
    const tasks = lane.tasks.filter((task) => ACTIVE.includes(task.state))
    const bay = element('article', 'bay bay-human')
    bay.dataset.handle = 'human'
    const head = element('header', 'bay-head')
    const count = waiting.length + tasks.length
    head.append(
      element('h2', 'bay-name', 'You'),
      element('span', 'bay-meta', 'Questions, results and notes for you'),
      element('span', 'bay-status', count === 0 ? 'Nothing waiting' : `${count} waiting`),
    )
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', 'Waiting for you')
    for (const message of waiting) strips.append(this.#messageStrip(message, now))
    for (const task of tasks) strips.append(this.#taskStrip(task, now))
    if (count === 0) {
      strips.append(
        element(
          'li',
          'bay-empty',
          'Questions from your agents and the results you asked for appear here.',
        ),
      )
    }
    bay.append(head, strips)
    return bay
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
        `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)}${message.taskNumber ? ` · T-${message.taskNumber}` : ''}`,
      ),
      element('span', 'strip-age', age(message.createdAt, now)),
    )
    item.append(line)
    if (message.body.includes('\n')) item.append(element('p', 'strip-body', message.body))
    if (message.kind === 'question') {
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
      item.append(form)
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

  #agentBay(lane, agent, now) {
    const { participant, activity, pane } = lane
    const bay = element('article', 'bay')
    bay.dataset.handle = participant.handle
    bay.dataset.role = participant.role
    const head = element('header', 'bay-head')
    const coordinator = participant.role === 'lead' || participant.role === 'pm'
    const identity = [
      coordinator ? null : participant.role,
      participant.tier,
      participant.tags.join(', ') || null,
      participant.harness,
      agent?.model,
    ]
      .filter(Boolean)
      .join(' · ')
    const out = outOfQuota(participant, now)
    const status = element(
      'span',
      'bay-status',
      out
        ? `Out of quota until ${clock(participant.outUntil)}`
        : activity?.state === 'waiting' && activity.reason
          ? `Waiting: ${activity.reason}`
          : (ACTIVITY_LABEL[activity?.state] ?? 'No window'),
    )
    status.dataset.state = out ? 'out' : (activity?.state ?? 'closed')
    const tools = element('div', 'bay-tools')
    tools.append(
      button(
        'Terminal',
        'quiet-button',
        () => this.#actions.onOpenTerminal(participant),
        `Open ${laneName(participant)}'s terminal`,
      ),
    )
    if (pane === null) tools.firstChild.disabled = true
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
    head.append(
      lamp(out ? { state: 'out' } : activity),
      element('h2', 'bay-name', laneName(participant)),
      element('span', 'bay-meta', identity),
      status,
      tools,
    )
    bay.append(head)
    if (this.#composing === participant.handle) bay.append(this.#composer(participant))

    const tasks = [...lane.tasks].sort(
      (a, b) => STATE_ORDER.indexOf(a.state) - STATE_ORDER.indexOf(b.state) || b.number - a.number,
    )
    const open = tasks.filter((task) => OPEN.includes(task.state))
    const cleared = tasks.length - open.length
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', `${laneName(participant)}'s tasks`)
    for (const task of open) strips.append(this.#taskStrip(task, now))
    if (open.length === 0) strips.append(element('li', 'bay-empty', 'No tasks.'))
    bay.append(strips)
    if (cleared > 0)
      bay.append(element('p', 'bay-cleared', `${cleared} cleared (accepted or cancelled)`))
    return bay
  }

  #taskStrip(task, now) {
    const item = element('li')
    const strip = button('', 'strip', () => this.#actions.onOpenTask(task.number))
    strip.dataset.task = String(task.number)
    strip.dataset.state = task.state
    strip.setAttribute(
      'aria-label',
      `T-${task.number}, ${task.title}, ${STATE_LABEL[task.state]}, from ${who(task.requester)}`,
    )
    strip.append(
      element('span', 'strip-number', `T-${task.number}`),
      element('span', 'strip-title', task.title),
      element('span', 'strip-route', route(task)),
      element('span', 'strip-state', stateLabel(task)),
      element('span', 'strip-age', age(task.updatedAt, now)),
    )
    item.append(strip)
    return item
  }

  /** The tasks waiting for a member of their tier, and where the human puts a new one. */
  #backlogBay(board, now) {
    const bay = element('section', 'backlog')
    bay.setAttribute('role', 'region')
    bay.setAttribute('aria-label', 'Open tasks')
    const head = element('header', 'bay-head')
    const waiting = board.open.length
    head.append(
      element('h2', 'bay-name', 'Open tasks'),
      element('span', 'bay-meta', 'Waiting for a member of their tier'),
      element(
        'span',
        'bay-status',
        waiting === 0 ? 'Nothing waiting' : `${waiting} waiting for a member`,
      ),
      button('New task', 'quiet-button', () => {
        this.#composingOpen = !this.#composingOpen
        this.#actions.onRedraw()
      }),
    )
    bay.append(head)
    if (this.#composingOpen) bay.append(this.#openComposer(board))
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', 'Tasks waiting for a member')
    for (const task of board.open) strips.append(this.#taskStrip(task, now))
    if (waiting === 0) {
      strips.append(
        element(
          'li',
          'bay-empty',
          'A task for a tier waits here until a member of that tier is free.',
        ),
      )
    }
    bay.append(strips)
    return bay
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
          .filter((lane) => lane.participant.role === pool && lane.participant.tier === tier)
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
    field.placeholder = 'What should be done? Include the context it needs and what to return.'
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
    field.placeholder = `What should ${laneName(participant)} do? Include the context it needs and what to return.`
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

/** One task and its whole thread, with what the human may do next. */
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
      element('span', 'strip-number', `T-${task.number}`),
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
    const thread = element('ol', 'thread')
    thread.setAttribute('aria-label', `T-${task.number}'s thread`)
    for (const message of task.messages) {
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
    this.#root.replaceChildren(head, meta, thread, actions)
    this.#root.hidden = false
  }

  hide() {
    this.#root.hidden = true
    this.#root.replaceChildren()
  }
}
