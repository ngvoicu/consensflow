import { orderedPanes, paneLabel, updateActivity } from './sidebar.js'

const states = {
  planned: 'Planned',
  active: 'In progress',
  blocked: 'Blocked',
  review: 'In review',
  accepted: 'Accepted',
  cancelled: 'Cancelled',
  unknown: 'Not recorded',
}
const kinds = ['general', 'implementation', 'research', 'specification', 'review']
const pending = (result) => ['waiting', 'collecting', 'uncertain'].includes(result.state)
const roleName = (owner) => (owner.role === 'pm' ? 'PM' : 'Lead')
const time = (value) => (value ? new Date(value).toLocaleString() : 'Time not recorded')
function element(tag, className = '', text) {
  const node = document.createElement(tag)
  node.className = className
  if (text !== undefined) node.textContent = text
  return node
}
function button(text, action, className = 'quiet-button') {
  const node = element('button', className, text)
  node.type = 'button'
  node.addEventListener('click', action)
  return node
}
function select(label, choices, value) {
  const field = element('label', 'task-field', label)
  const input = element('select')
  input.setAttribute('aria-label', label)
  for (const [key, text] of choices) {
    const option = element('option', '', text)
    option.value = key
    input.append(option)
  }
  input.value = value
  field.append(input)
  return { field, input }
}
function textField(label, value = '', maxLength = 1000, multiline = false) {
  const field = element('label', 'task-field', label)
  const input = element(multiline ? 'textarea' : 'input')
  input.setAttribute('aria-label', label)
  input.value = value
  input.maxLength = maxLength
  if (multiline) input.rows = maxLength > 1000 ? 5 : 2
  field.append(input)
  return { field, input }
}

/** Human session overview. Task progress never changes result receipt or terminal input. */
export class TaskView {
  constructor(container, { invoke, openPane, openResults }) {
    Object.assign(this, { container, invoke, openPane, openResults })
    this.mode = 'board'
    this.zoom = 1
    this.rows = []
    this.root = null
    this.epoch = 0
    this.detailEpoch = 0
    this.timer = null
    this.dialog = element('dialog', 'task-dialog')
    document.body.append(this.dialog)
    this.dialog.addEventListener('close', () => {
      this.detailEpoch += 1
    })
    const top = element('header', 'task-heading')
    const title = element('div')
    title.append(
      element('span', 'task-eyebrow', 'SESSION OVERVIEW'),
      element('h1', '', 'Session tasks'),
    )
    const modes = element('div', 'task-modes')
    this.boardButton = button('Board', () => this.setMode('board'))
    this.graphButton = button('Graph', () => this.setMode('graph'))
    modes.append(
      this.boardButton,
      this.graphButton,
      button('New task', () => this.edit(null), 'primary-button'),
    )
    top.append(title, modes)
    this.overview = element('p', 'task-overview')
    const filters = element('div', 'task-filters')
    const group = select(
      'Task group',
      [
        ['all', 'PM + Lead'],
        ['pm', 'PM + advisors'],
        ['lead', 'Lead + workers'],
      ],
      'all',
    )
    const status = select(
      'Task status',
      [['all', 'All progress'], ...Object.entries(states)],
      'all',
    )
    const search = textField('Search tasks', '', 160)
    search.input.type = 'search'
    this.filters = { group: group.input, status: status.input, search: search.input }
    for (const item of [group, status, search]) {
      filters.append(item.field)
      item.input.addEventListener('input', () => this.draw())
    }
    filters.append(button('Refresh tasks', () => void this.load()))
    this.message = element('p', 'task-message')
    this.message.setAttribute('role', 'status')
    this.people = element('div', 'task-people')
    this.content = element('div', 'task-content')
    this.more = button('Load more tasks', () => void this.load(true))
    this.more.hidden = true
    container.append(
      top,
      this.overview,
      filters,
      this.message,
      this.people,
      this.content,
      this.more,
    )
  }

  async request(command, args) {
    const result = await this.invoke(command, args)
    if (!result || result.ok === false)
      throw new Error(result?.reason ?? result?.error ?? 'Task data unavailable')
    return result
  }

  show(root, state) {
    this.container.hidden = false
    const changed = this.root !== root.id
    const previousState = this.state
    this.state = state
    this.owners = state.tabs
      .filter((tab) => tab.id === root.id || tab.parentTabId === root.id)
      .sort((a, b) => Number(b.role === 'pm') - Number(a.role === 'pm'))
    if (changed) {
      this.root = root.id
      this.rows = []
      this.page = null
      this.epoch += 1
      this.detailEpoch += 1
      this.dialog.close()
      this.filters.group.value = 'all'
      this.filters.status.value = 'all'
      this.filters.search.value = ''
    }
    if (changed || !this.timer) {
      void this.load()
      this.timer ??= setInterval(() => void this.load(), 10000)
    } else if (previousState !== state) this.draw()
  }

  hide() {
    this.container.hidden = true
    clearInterval(this.timer)
    this.timer = null
    this.epoch += 1
    this.detailEpoch += 1
    this.dialog.close()
  }

  async load(more = false) {
    const epoch = ++this.epoch
    const root = this.root
    this.more.disabled = true
    try {
      // Refresh all pages already visible; additions never replace a stale whole ledger.
      const rows = more ? [...this.rows] : []
      let offset = more ? this.page?.next : 0
      const wanted = more ? rows.length + 100 : Math.max(100, this.rows.length)
      if (offset == null) return
      let page
      do {
        page = await this.request('task_list', { tab: root, offset, limit: 100 })
        if (epoch !== this.epoch) return
        if (
          !Array.isArray(page.tasks) ||
          (page.next !== null && (!Number.isSafeInteger(page.next) || page.next <= offset))
        )
          throw new Error('Invalid task page')
        rows.push(...page.tasks)
        offset = page.next
      } while (offset !== null && rows.length < wanted)
      this.rows = [...new Map(rows.map((row) => [`${row.owner}/${row.id}`, row])).values()]
      this.page = page
      this.message.textContent = ''
      this.draw()
    } catch (error) {
      if (epoch === this.epoch)
        this.message.textContent = `${error.message}. Refresh tasks to retry.`
    } finally {
      if (epoch === this.epoch) this.more.disabled = false
    }
  }

  results(owner, conversation) {
    return this.state.results.filter(
      (r) => r.tab === owner && (!conversation || r.conversation === conversation),
    )
  }

  agent(owner, pane, graph = false) {
    const row = element('div', 'task-agent')
    if (!graph) row.dataset.agentPane = pane.id
    const label = paneLabel(owner, pane)
    const focus = button(label, () => this.openPane(owner, pane), 'task-agent-name')
    focus.dataset.focusKey = `${owner.id}/${pane.id}`
    focus.disabled = owner.closed === true || pane.alive === false || pane.closed === true
    const activity = element('span')
    updateActivity(activity, owner, pane)
    const profile = this.state.agents.find((agent) => agent.name === pane.agent)
    const detail = [
      pane.kind === 'lead' ? roleName(owner) : owner.role === 'pm' ? 'Advisor' : 'Worker',
      pane.harness ?? profile?.harness ?? owner.lead?.harness,
    ]
      .filter(Boolean)
      .join(' · ')
    row.append(focus, activity, element('small', 'task-muted', detail))
    const replies = this.results(owner.id, pane.kind === 'lead' ? null : pane.conversation)
    const count = replies.filter(pending).length
    if (replies.length)
      row.append(
        button(
          `${replies.length} replies · ${count} unconfirmed`,
          () => this.openResults(owner, pane.kind === 'lead' ? null : pane.conversation),
          'task-replies',
        ),
      )
    if (pane.failure)
      row.append(
        element(
          'small',
          'task-failure',
          String(pane.failure.reason ?? pane.failure.message ?? 'The latest task failed').slice(
            0,
            300,
          ),
        ),
      )
    return row
  }

  card(task) {
    const card = button('', () => void this.open(task.owner, task.id), 'task-card')
    card.dataset.task = task.id
    card.dataset.testid = `task-card-${task.id}`
    card.dataset.progress = task.status
    card.dataset.focusKey = `${task.owner}/${task.id}`
    const top = element('div', 'task-card-meta')
    top.append(
      element('span', 'task-kind', task.kind),
      element('span', 'task-progress', states[task.status] ?? 'Not recorded'),
    )
    card.append(top, element('strong', '', task.title))
    if (task.conversation) card.append(element('small', 'task-conversation', task.conversation))
    if (task.unanswered)
      card.append(
        element(
          'span',
          'task-attention',
          `${task.unanswered} unanswered question${task.unanswered === 1 ? '' : 's'}`,
        ),
      )
    if (task.source === 'historical')
      card.append(element('small', 'task-muted', 'Original assignment not recorded'))
    if (task.conversation) {
      const replies = this.results(task.owner, task.conversation)
      card.append(
        element(
          'small',
          'task-muted',
          `${replies.length} replies · ${replies.filter(pending).length} unconfirmed`,
        ),
      )
    }
    card.title = `${task.title} · Updated ${time(task.updatedAt)}`
    return card
  }

  setMode(mode) {
    this.mode = mode
    this.draw()
  }

  draw() {
    if (this.container.hidden) return
    const focused = document.activeElement?.dataset.focusKey
    const oldViewport = this.content.querySelector('.task-graph-viewport')
    const scroll = oldViewport && { top: oldViewport.scrollTop, left: oldViewport.scrollLeft }
    const pageTop = this.container.scrollTop
    const results = this.state.results.filter((r) => this.owners.some((o) => o.id === r.tab))
    this.overview.textContent = `${this.page?.total ?? 0} tasks · ${this.page?.questions ?? 0} unanswered questions · ${results.length} replies · ${results.filter(pending).length} unconfirmed`
    this.boardButton.setAttribute('aria-pressed', String(this.mode === 'board'))
    this.graphButton.setAttribute('aria-pressed', String(this.mode === 'graph'))
    const owners = this.owners.filter(
      (o) => this.filters.group.value === 'all' || (o.role ?? 'lead') === this.filters.group.value,
    )
    const search = this.filters.search.value.toLowerCase()
    const rows = this.rows.filter(
      (task) =>
        owners.some((o) => o.id === task.owner) &&
        (this.filters.status.value === 'all' || task.status === this.filters.status.value) &&
        [task.title, task.conversation, task.note].some((text) =>
          String(text ?? '')
            .toLowerCase()
            .includes(search),
        ),
    )
    this.people.hidden = this.mode === 'graph'
    this.people.replaceChildren()
    this.content.replaceChildren()
    if (this.mode === 'graph') this.graph(owners, rows)
    else {
      const board = element('div', 'task-board')
      for (const owner of owners) {
        const people = element('section', 'task-team')
        people.dataset.role = owner.role ?? 'lead'
        people.append(element('h2', '', owner.role === 'pm' ? 'PM + advisors' : 'Lead + workers'))
        for (const pane of orderedPanes(owner).filter((p) => p.kind !== 'shell'))
          people.append(this.agent(owner, pane))
        this.people.append(people)
        const lane = element('section', 'task-lane')
        lane.dataset.role = owner.role ?? 'lead'
        lane.append(element('h2', '', `${roleName(owner)} tasks`))
        const columns = element('div', 'task-columns')
        for (const [title, statuses] of [
          ['To do', ['planned', 'unknown']],
          ['In progress', ['active', 'blocked', 'review']],
          ['Finished', ['accepted', 'cancelled']],
        ]) {
          const column = element('section', 'task-column')
          const tasks = rows.filter(
            (task) => task.owner === owner.id && statuses.includes(task.status),
          )
          column.append(element('h3', '', `${title} · ${tasks.length}`))
          for (const task of tasks) column.append(this.card(task))
          if (!tasks.length) column.append(element('p', 'task-empty', 'No tasks'))
          columns.append(column)
        }
        lane.append(columns)
        board.append(lane)
      }
      this.content.append(board)
    }
    this.more.hidden = this.page?.next == null
    if (this.page?.next != null)
      this.message.textContent = `Showing ${this.rows.length} of ${this.page.total} tasks. Filters and graph apply to loaded tasks.`
    else if (!this.rows.length)
      this.message.textContent =
        'Assignments appear here when you delegate. Add a task for planning or coordinator work.'
    const viewport = this.content.querySelector('.task-graph-viewport')
    if (viewport && scroll) viewport.scrollTo(scroll.left, scroll.top)
    if (focused)
      [...this.container.querySelectorAll('[data-focus-key]')]
        .find((node) => node.dataset.focusKey === focused)
        ?.focus({ preventScroll: true })
    this.container.scrollTop = pageTop
  }

  graph(owners, tasks) {
    const controls = element('div', 'task-graph-controls')
    const viewport = element('div', 'task-graph-viewport')
    const space = element('div', 'task-graph-space')
    const graph = element('div', 'task-graph')
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('aria-label', 'Task relationships')
    const marker = document.createElementNS(svg.namespaceURI, 'marker')
    marker.id = 'task-arrow'
    for (const [key, value] of Object.entries({
      viewBox: '0 0 10 10',
      refX: '9',
      refY: '5',
      markerWidth: '6',
      markerHeight: '6',
      orient: 'auto-start-reverse',
    }))
      marker.setAttribute(key, value)
    const arrow = document.createElementNS(svg.namespaceURI, 'path')
    arrow.setAttribute('d', 'M 0 0 L 10 5 L 0 10 z')
    arrow.style.fill = 'var(--muted)'
    marker.append(arrow)
    const defs = document.createElementNS(svg.namespaceURI, 'defs')
    defs.append(marker)
    svg.append(defs)
    graph.append(svg)
    space.append(graph)
    viewport.append(space)
    let height = 24
    const width = 1160
    const positions = new Map()
    const edges = []
    const place = (node, key, x, y, w = 270, h = 144) => {
      node.classList.add('task-graph-node')
      Object.assign(node.style, {
        left: `${x}px`,
        top: `${y}px`,
        width: `${w}px`,
        height: `${h}px`,
      })
      graph.append(node)
      positions.set(key, { x, y, w, h })
    }
    for (const owner of owners) {
      const own = tasks.filter((t) => t.owner === owner.id)
      const workers = orderedPanes(owner).filter((p) => p.kind === 'worker')
      const lead = owner.panes.find((p) => p.kind === 'lead')
      const laneHeight = Math.max(1, workers.length, own.length) * 170 + 72
      const label = element(
        'h2',
        'task-graph-lane',
        `${roleName(owner)} · ${owner.roleName ?? (owner.role === 'pm' ? 'Planning and advice' : 'Implementation and review')}`,
      )
      Object.assign(label.style, { top: `${height}px`, height: `${laneHeight}px` })
      label.dataset.role = owner.role ?? 'lead'
      graph.append(label)
      if (lead) place(this.agent(owner, lead, true), owner.id, 20, height + 55)
      workers.forEach((p, index) => {
        place(this.agent(owner, p, true), `${owner.id}/${p.id}`, 350, height + 55 + index * 170)
        edges.push([owner.id, `${owner.id}/${p.id}`, 'owns'])
      })
      own.forEach((t, index) => {
        const key = `${owner.id}/${t.id}`
        place(this.card(t), key, 750, height + 55 + index * 170)
        const worker = workers.find((p) => p.conversation === t.conversation)
        edges.push([
          worker ? `${owner.id}/${worker.id}` : owner.id,
          key,
          worker ? 'assigned' : 'owns',
        ])
        for (const id of t.dependsOn ?? []) edges.push([key, `${owner.id}/${id}`, 'depends'])
        if (t.reviewOf) edges.push([key, `${owner.id}/${t.reviewOf}`, 'reviews'])
      })
      height += laneHeight + 28
    }
    for (const [from, to, relation] of edges) {
      const a = positions.get(from),
        b = positions.get(to)
      if (!a || !b) continue
      const group = document.createElementNS(svg.namespaceURI, 'g')
      group.dataset.relation = relation
      const line = document.createElementNS(svg.namespaceURI, 'path')
      const label = document.createElementNS(svg.namespaceURI, 'text')
      const side = a.x === b.x
      const x1 = a.x + a.w,
        y1 = a.y + (relation === 'reviews' ? a.h * 0.72 : a.h / 2)
      const x2 = side ? b.x + b.w : b.x,
        y2 = b.y + b.h / 2
      const middle = side ? x1 + (relation === 'reviews' ? 104 : 58) : (x1 + x2) / 2
      line.setAttribute('d', `M ${x1} ${y1} C ${middle} ${y1}, ${middle} ${y2}, ${x2} ${y2}`)
      line.setAttribute('marker-end', 'url(#task-arrow)')
      label.setAttribute('x', String(middle + (side ? 4 : 0)))
      label.setAttribute('y', String((y1 + y2) / 2 - 6))
      label.textContent = relation
      const description = document.createElementNS(svg.namespaceURI, 'title')
      description.textContent = `${from} ${relation} ${to}`
      group.append(description, line, label)
      svg.append(group)
    }
    Object.assign(graph.style, { width: `${width}px`, height: `${height}px` })
    svg.setAttribute('width', String(width))
    svg.setAttribute('height', String(height))
    const zoom = (value) => {
      this.zoom = Math.max(0.2, Math.min(1.6, value))
      graph.dataset.zoom = String(Number(this.zoom.toFixed(2)))
      graph.style.transform = `scale(${this.zoom})`
      Object.assign(space.style, {
        width: `${width * this.zoom}px`,
        height: `${height * this.zoom}px`,
      })
    }
    controls.append(
      element(
        'p',
        'task-muted',
        'Coordinator → advisors / workers → tasks. Curved links show prerequisites and reviews.',
      ),
      button('Zoom out', () => zoom(this.zoom - 0.15)),
      button('Zoom in', () => zoom(this.zoom + 0.15)),
      button('Fit graph', () => zoom((viewport.clientWidth - 24) / width)),
    )
    zoom(this.zoom)
    this.content.append(controls, viewport)
  }

  async open(owner, id) {
    const epoch = ++this.detailEpoch
    try {
      const task = await this.request('task_get', { tab: owner, id })
      if (epoch === this.detailEpoch && !this.container.hidden) this.edit(task)
    } catch (error) {
      if (epoch === this.detailEpoch) this.message.textContent = error.message
    }
  }

  edit(task) {
    const dialog = this.dialog
    dialog.replaceChildren()
    dialog.setAttribute('aria-label', task ? 'Task details' : 'New task')
    const header = element('header', 'task-heading')
    header.append(
      element('h2', '', task ? task.title : 'New task'),
      button('Close', () => dialog.close()),
    )
    header.lastChild.setAttribute('aria-label', task ? 'Close task details' : 'Close new task')
    const error = element('p', 'task-error')
    error.setAttribute('role', 'alert')
    const form = element('form', 'task-editor')
    const owner = select(
      'Owner',
      this.owners.map((o) => [o.id, `${roleName(o)}${o.roleName ? ` · ${o.roleName}` : ''}`]),
      task?.owner ?? this.owners.find((o) => o.role !== 'pm')?.id,
    )
    owner.input.disabled = !!task
    const title = textField('Task title', task?.title, 160)
    title.input.required = true
    const description = textField('Assignment / description', task?.description, 32768, true)
    const progress = select('Progress', Object.entries(states), task?.status ?? 'planned')
    // Historical unknown may be retained; it cannot be newly assigned.
    progress.input.querySelector('[value="unknown"]').disabled = true
    const kind = select(
      'Task kind',
      kinds.map((k) => [k, k]),
      task?.kind ?? 'general',
    )
    const note = textField('Progress note', '', 1000, true)
    const conversation = select(
      'Conversation',
      [['', 'Coordinator work']],
      task?.conversation ?? '',
    )
    conversation.input.disabled = !!task && task.source !== 'explicit'
    const review = select('Reviews task', [['', 'No review link']], task?.reviewOf ?? '')
    const dependencies = select('Depends on', [], '')
    dependencies.input.multiple = true
    const links = () => {
      const group = this.owners.find((o) => o.id === owner.input.value)
      const options = (input, entries, selected) => {
        input.replaceChildren()
        for (const [value, label] of entries) {
          const option = element('option', '', label)
          option.value = value
          option.selected = selected.includes(value)
          input.append(option)
        }
      }
      const conversations = new Set(
        [task?.conversation, ...(group?.panes ?? []).map((p) => p.conversation)].filter(Boolean),
      )
      options(
        conversation.input,
        [['', 'Coordinator work'], ...[...conversations].map((c) => [c, c])],
        [task?.conversation ?? ''],
      )
      const available = new Map(
        this.rows
          .filter((t) => t.owner === group?.id && t.id !== task?.id)
          .map((t) => [t.id, t.title]),
      )
      for (const id of [...(task?.dependsOn ?? []), task?.reviewOf].filter(Boolean))
        if (!available.has(id)) available.set(id, id)
      options(review.input, [['', 'No review link'], ...available], [task?.reviewOf ?? ''])
      options(dependencies.input, [...available], task?.dependsOn ?? [])
    }
    links()
    owner.input.addEventListener('change', links)
    for (const input of [
      owner,
      title,
      description,
      progress,
      kind,
      conversation,
      review,
      dependencies,
      note,
    ])
      form.append(input.field)
    const save = element('button', 'primary-button', task ? 'Save task' : 'Create task')
    save.type = 'submit'
    let saving = false
    const commit = async (change, _control) => {
      if (saving) return
      saving = true
      const controls = [...dialog.querySelectorAll('input,select,textarea,button')]
        .filter((node) => node !== header.lastChild)
        .map((node) => ({ node, disabled: node.disabled }))
      for (const { node } of controls) node.disabled = true
      error.textContent = ''
      const epoch = this.detailEpoch
      try {
        const saved = await this.request('task_change', { tab: owner.input.value, change })
        if (epoch !== this.detailEpoch) return
        if (task) this.edit(saved)
        else dialog.close()
        await this.load()
      } catch (cause) {
        if (epoch === this.detailEpoch) error.textContent = cause.message
      } finally {
        saving = false
        for (const { node, disabled } of controls) node.disabled = disabled
      }
    }
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      const change = {
        action: task ? 'update' : 'add',
        title: title.input.value,
        description: description.input.value,
        kind: kind.input.value,
        conversation: conversation.input.value || null,
        reviewOf: review.input.value || null,
        dependsOn: [...dependencies.input.selectedOptions].map((o) => o.value),
      }
      if (progress.input.value !== 'unknown') change.status = progress.input.value
      if (task) Object.assign(change, { id: task.id, revision: task.revision })
      if (note.input.value) change.note = note.input.value
      void commit(change, save)
    })
    form.append(save)
    dialog.append(header, error)
    if (task) {
      dialog.append(
        element(
          'p',
          'task-muted',
          `${roleName(task)} · Updated ${time(task.updatedAt)} · Revision ${task.revision}`,
        ),
      )
      if (task.source === 'historical')
        dialog.append(
          element(
            'p',
            'task-muted',
            'Original assignment not recorded. You can add a description.',
          ),
        )
      if (task.descriptionTruncated)
        dialog.append(
          element(
            'p',
            'task-muted',
            'Original assignment exceeds the saved 32,768-character excerpt.',
          ),
        )
      if (task.conversation)
        dialog.append(
          button('View all conversation replies', () =>
            this.openResults(
              this.owners.find((o) => o.id === task.owner),
              task.conversation,
            ),
          ),
        )
      const questions = element('section', 'task-questions')
      if (task.questions?.length)
        questions.append(
          element('h3', '', 'Questions and decisions'),
          element(
            'p',
            'task-muted',
            'Answers are saved here for the coordinator to read when it continues.',
          ),
        )
      for (const question of task.questions ?? []) {
        const row = element('article')
        row.append(element('strong', '', question.text))
        if (question.answer !== undefined) row.append(element('p', '', question.answer))
        else {
          const answer = textField('Answer', '', 1000, true)
          const record = button(
            'Record answer',
            () =>
              void commit(
                {
                  action: 'answer',
                  id: task.id,
                  revision: task.revision,
                  question: question.id,
                  answer: answer.input.value,
                },
                record,
              ),
          )
          row.append(answer.field, record)
        }
        questions.append(row)
      }
      dialog.append(questions)
    }
    dialog.append(form)
    if (task) {
      dialog.append(
        button('Reload task (discard edits)', () => void this.open(task.owner, task.id)),
      )
      const history = element('details', 'task-history')
      history.append(element('summary', '', `Recent updates · ${task.history?.length ?? 0}`))
      for (const item of task.history ?? [])
        history.append(element('p', '', `${time(item.at)} · ${item.actor}\n${item.note}`))
      dialog.append(history)
    }
    if (!dialog.open) dialog.showModal()
  }
}
