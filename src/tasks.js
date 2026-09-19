import { randomUUID } from 'node:crypto'

export const TASK_STATES = ['planned', 'active', 'blocked', 'review', 'accepted', 'cancelled']
export const TASK_KINDS = ['general', 'implementation', 'research', 'specification', 'review']
const fields = new Set([
  'title',
  'description',
  'status',
  'kind',
  'conversation',
  'dependsOn',
  'reviewOf',
  'note',
  'question',
])
const belongs = (row, owner) => row?.lead?.startsWith(`tab:${owner}:`) === true
const assignmentId = (conversation) => `assignment:${conversation}`
const MAX_TASK_BYTES = 512 * 1024
const taskBytes = (task) => Buffer.byteLength(JSON.stringify(task))

export class TaskError extends Error {
  constructor(message, status = 400) {
    super(message)
    this.status = status
    this.code = 'task-refused'
  }
}
const refuse = (message, status) => {
  throw new TaskError(message, status)
}
function text(value, field, max, empty = false) {
  if (typeof value !== 'string' || (!empty && !value.trim()) || value.length > max)
    refuse(
      field +
        ' must be text' +
        (empty ? '' : ' and not empty') +
        ' (maximum ' +
        max +
        ' characters)',
    )
  return value.toWellFormed()
}
const ownerOf = (tabs, id) =>
  tabs.find((tab) => tab.id === id) ?? refuse('Task owner not found', 404)
const taskOf = (tasks, id) =>
  tasks.find((task) => task.id === id) ?? refuse('Task not found in your own group', 404)

function history(task, actor, note, now, extra = {}) {
  task.history.push({ actor, note: note.slice(0, 1000), at: now, ...extra })
  task.history = task.history.slice(-64)
  task.updatedAt = now
  task.revision += 1
}
function blank(id, title, now) {
  return {
    id,
    title,
    description: '',
    status: 'planned',
    kind: 'general',
    conversation: null,
    source: 'explicit',
    dependsOn: [],
    reviewOf: null,
    questions: [],
    history: [],
    revision: 0,
    createdAt: now,
    updatedAt: now,
  }
}

/** Called inside the existing sent-record transaction; never a second ledger write. */
export function recordAssignment(
  tab,
  { conversation, task: content, agent, opId, initial = true },
  now = Date.now(),
) {
  if (typeof content !== 'string' || !content.trim()) return
  tab.tasks ??= []
  const id = assignmentId(conversation)
  let task = tab.tasks.find((item) => item.id === id)
  if (task?.history.some((item) => item.opId === opId)) return
  if (!task) {
    // The title is a readable excerpt; the description retains the complete recorded prompt.
    const title = content
      .replace(/^Critical work: [^\n]+\n\n/, '')
      .replace(/\s+/g, ' ')
      .slice(0, 160)
    task = blank(id, initial ? title : conversation, now)
    Object.assign(task, {
      description: initial ? content.toWellFormed().slice(0, 32768) : '',
      descriptionTruncated: initial && content.length > 32768,
      conversation,
      agent,
      source: initial ? 'assignment' : 'historical',
    })
    tab.tasks.push(task)
  }
  task.status = 'active'
  history(task, 'dispatch', content, now, { opId })
  // Keep recent dispatch evidence bounded without failing an already admitted prompt.
  while (taskBytes(task) > MAX_TASK_BYTES && task.history.length > 1) task.history.shift()
}

function allTasks(tab, threads) {
  const tasks = structuredClone(tab.tasks ?? [])
  for (const [conversation, row] of Object.entries(threads)) {
    if (
      !belongs(row, tab.id) ||
      tab.deletedConversations?.includes(conversation) ||
      tasks.some((task) => task.id === assignmentId(conversation))
    )
      continue
    const at = Date.parse(row.createdAt) || 0
    tasks.push({
      ...blank(assignmentId(conversation), conversation, at),
      source: 'historical',
      status: 'unknown',
      conversation,
      agent: row.agent,
    })
  }
  return tasks
}
function summary(task, tab) {
  const { description: _description, history: updates, questions, ...short } = task
  return {
    ...short,
    owner: tab.id,
    role: tab.role ?? 'lead',
    note: updates.at(-1)?.note.slice(0, 160) ?? '',
    questionCount: questions.length,
    unanswered: questions.filter((q) => q.answer === undefined).length,
  }
}
function linksValid(tasks, changed) {
  const seen = new Set()
  const visiting = new Set()
  const visit = (task) => {
    if (visiting.has(task.id)) refuse('Task links would create a cycle')
    if (seen.has(task.id)) return
    visiting.add(task.id)
    for (const id of [...task.dependsOn, task.reviewOf].filter(Boolean)) {
      if (typeof id !== 'string') refuse('Task link must name a task in your own group')
      visit(taskOf(tasks, id))
    }
    visiting.delete(task.id)
    seen.add(task.id)
  }
  visit(changed)
}
function patchTask(task, input, tasks, threads, owner, actor, now) {
  for (const key of Object.keys(input))
    if (!['action', 'id', 'revision'].includes(key) && !fields.has(key))
      refuse(`Unknown task field: ${key}`)
  for (const key of ['title', 'description', 'note', 'question']) {
    if (input[key] !== undefined)
      text(
        input[key],
        key,
        key === 'description' ? 32768 : key === 'title' ? 160 : 1000,
        ['description', 'note'].includes(key),
      )
  }
  if (input.status !== undefined && !TASK_STATES.includes(input.status))
    refuse('Invalid task status')
  if (input.kind !== undefined && !TASK_KINDS.includes(input.kind)) refuse('Invalid task kind')
  if (
    task.source !== 'explicit' &&
    input.conversation !== undefined &&
    input.conversation !== task.conversation
  )
    refuse('An assignment cannot change its conversation')
  if (
    input.conversation !== undefined &&
    input.conversation !== null &&
    !belongs(threads[input.conversation], owner)
  )
    refuse('Conversation must belong to your own group')
  if (
    input.dependsOn !== undefined &&
    (!Array.isArray(input.dependsOn) || input.dependsOn.length > 32)
  )
    refuse('dependsOn must contain at most 32 task IDs')
  for (const key of [
    'title',
    'description',
    'status',
    'kind',
    'conversation',
    'dependsOn',
    'reviewOf',
  ])
    if (input[key] !== undefined) task[key] = structuredClone(input[key])
  if (input.description !== undefined) task.descriptionTruncated = false
  if (input.dependsOn) task.dependsOn = [...new Set(task.dependsOn)]
  if (input.question !== undefined) {
    if (task.questions.length >= 64)
      refuse('This task already has 64 questions; create a separate task')
    task.questions.push({ id: randomUUID(), text: input.question, askedAt: now })
    task.status = 'blocked'
  }
  linksValid(tasks, task)
  history(
    task,
    actor,
    input.note ?? input.question ?? (input.status ? `Status: ${input.status}` : 'Task updated'),
    now,
  )
}

/** Owns task edits and bounded projections; all persistence uses the app's Store queue. */
export class Tasks {
  constructor(store) {
    this.store = store
  }

  async list(owner, { combined = false, offset = 0, limit = 100 } = {}) {
    if (!Number.isSafeInteger(offset) || offset < 0) refuse('Invalid task offset')
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) refuse('Task limit must be 1–100')
    const tabs = await this.store.readTabs()
    const selected = ownerOf(tabs, owner)
    const root = combined && selected.role === 'pm' ? ownerOf(tabs, selected.parentTabId) : selected
    const owners = combined
      ? tabs.filter((tab) => tab.id === root.id || tab.parentTabId === root.id)
      : [selected]
    const rows = []
    for (const tab of owners) {
      const threads = await this.store.readThreads(tab.directory)
      rows.push(...allTasks(tab, threads).map((task) => summary(task, tab)))
    }
    rows.sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id))
    return {
      tasks: rows.slice(offset, offset + limit),
      total: rows.length,
      offset,
      next: offset + limit < rows.length ? offset + limit : null,
      owners: owners.map((tab) => ({
        id: tab.id,
        role: tab.role ?? 'lead',
        name: tab.roleName,
        closed: tab.closed === true,
      })),
      counts: Object.fromEntries(
        [...TASK_STATES, 'unknown'].map((state) => [
          state,
          rows.filter((task) => task.status === state).length,
        ]),
      ),
      questions: rows.reduce((count, task) => count + task.unanswered, 0),
    }
  }

  async get(owner, id) {
    const tab = ownerOf(await this.store.readTabs(), owner)
    const task = taskOf(allTasks(tab, await this.store.readThreads(tab.directory)), id)
    return { ...task, owner, role: tab.role ?? 'lead' }
  }

  async change(owner, input, actor = 'coordinator') {
    if (!['coordinator', 'human'].includes(actor)) refuse('Invalid task actor')
    if (!input || typeof input !== 'object' || Array.isArray(input))
      refuse('Task change must be an object')
    return this.store.mutate(null, `task.${input.action}`, async (io) => {
      const tabs = await io.readTabs()
      const tab = ownerOf(tabs, owner)
      if (tab.deleting) refuse('This task group is being deleted', 409)
      const threads = await io.readThreads(tab.directory)
      const tasks = allTasks(tab, threads)
      const now = Date.now()
      let task
      if (input.action === 'add') {
        task = blank(`task-${randomUUID()}`, text(input.title, 'title', 160), now)
        tasks.push(task)
        patchTask(task, input, tasks, threads, owner, actor, now)
      } else {
        task = taskOf(tasks, input.id)
        if (input.revision !== task.revision) refuse('Task changed; reload it before saving', 409)
        if (input.action === 'answer') {
          if (actor !== 'human') refuse('Only the human can answer this question', 403)
          for (const key of Object.keys(input))
            if (!['action', 'id', 'revision', 'question', 'answer'].includes(key))
              refuse(`Unknown task field: ${key}`)
          const question = task.questions.find((item) => item.id === input.question)
          if (!question) refuse('Question not found', 404)
          if (question.answer !== undefined) refuse('Question already answered', 409)
          question.answer = text(input.answer, 'answer', 1000)
          question.answeredAt = now
          history(task, actor, `Answer: ${question.answer}`, now)
        } else if (input.action === 'update')
          patchTask(task, input, tasks, threads, owner, actor, now)
        else refuse('Unknown task action')
      }
      if (taskBytes(task) > MAX_TASK_BYTES)
        refuse('Task size is too large; shorten its description or start a separate task')
      const referenced = new Set(
        tasks.flatMap((item) => [...item.dependsOn, item.reviewOf].filter(Boolean)),
      )
      // Referenced historical targets must outlive their panes as well as the edited task.
      tab.tasks = tasks.filter(
        (item) =>
          item.source !== 'historical' ||
          item.id === task.id ||
          referenced.has(item.id) ||
          tab.tasks?.some((saved) => saved.id === item.id),
      )
      await io.writeTabs(tabs)
      return { ...structuredClone(task), owner, role: tab.role ?? 'lead' }
    })
  }
}
