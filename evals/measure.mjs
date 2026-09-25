import { DatabaseSync } from 'node:sqlite'
import { openLedger } from '../src/ledger/index.js'

/**
 * What a chief did, read from the ledger of a finished eval run: the tasks it
 * put on the board and how many ran side by side, what it asked and told the
 * human, the advice and reviews it asked for, the edits it made with its own
 * hands (the transcript copy's Edit and Write results), and its last words.
 * The daemon must have closed the ledger first: the ledger holds its file.
 */
export function measure(file) {
  const ledger = openLedger(file)
  let metrics
  try {
    const [project] = ledger.projects()
    if (project === undefined) throw new Error('the eval ledger holds no project')
    const full = ledger.project(project.id)
    const human = full.participants.find((p) => p.role === 'human')
    const chief = full.participants.find((p) => p.role === 'chief')
    const tasks = ledger
      .board(project.id)
      .lanes.flatMap((lane) => lane.tasks)
      .concat(ledger.board(project.id).open)
      .map((task) => ledger.task(project.id, task.number))
    // The inbox reads newest first; a report reads in order.
    const toHuman = ledger
      .inbox(human.id, { limit: 500 })
      .filter((message) => message.sender === chief.handle)
      .sort((a, b) => a.id - b.id)
    const spans = tasks
      .filter((task) => task.requester === chief.handle)
      .map((task) => {
        const brief = task.messages.find((m) => m.kind === 'task')
        const result = task.messages.find((m) => m.kind === 'result')
        return {
          number: task.number,
          from: brief?.deliveredAt ?? brief?.createdAt ?? task.createdAt,
          to: result?.createdAt ?? null,
        }
      })
    metrics = {
      project: { id: project.id, name: project.name },
      chief: chief.handle,
      tasks: tasks
        .filter((task) => task.requester === chief.handle)
        .map((task) => ({
          number: task.number,
          title: task.title,
          pool: task.pool,
          tier: task.tier,
          state: task.state,
          assignee: task.assignee,
        })),
      parallel: mostAtOnce(spans),
      advice: tasks.filter((t) => t.requester === chief.handle && t.pool === 'advisor').length,
      reviews: tasks.filter((t) => t.requester === chief.handle && t.pool === 'reviewer').length,
      questionsToHuman: toHuman
        .filter((m) => m.kind === 'question')
        .map((m) => ({ id: m.id, options: m.questions !== null, body: m.body.slice(0, 200) })),
      notesToHuman: toHuman
        .filter((m) => m.kind === 'note')
        .map((m) => ({ id: m.id, body: m.body.slice(0, 200) })),
      chiefId: chief.id,
    }
  } finally {
    ledger.close()
  }
  return { ...metrics, ...chiefWindow(file, metrics.chiefId) }
}

/** The chief's own turns and edits, from the daemon's copy of its conversation. */
function chiefWindow(file, chiefId) {
  const db = new DatabaseSync(file, { readOnly: true })
  try {
    const items = db
      .prepare(
        `SELECT t.role, t.text FROM transcript t
         JOIN conversation c ON c.id = t.conversation_id
         WHERE c.participant_id = ? ORDER BY t.conversation_id, t.seq`,
      )
      .all(chiefId)
    const tools = items.filter((i) => i.role === 'tool')
    const assistant = items.filter((i) => i.role === 'assistant')
    return {
      chiefTurns: assistant.length,
      chiefEdits: tools.filter((i) => /has been updated|File created successfully/.test(i.text))
        .length,
      chiefLastWords: assistant.at(-1)?.text.slice(0, 1500) ?? '',
    }
  } finally {
    db.close()
  }
}

/** The most tasks whose windows had the brief and no result yet at one moment. */
function mostAtOnce(spans) {
  const points = spans
    .filter((span) => span.from !== null)
    .flatMap((span) => [
      { at: span.from, delta: 1 },
      { at: span.to ?? '9999', delta: -1 },
    ])
    .sort((a, b) => (a.at < b.at ? -1 : a.at > b.at ? 1 : a.delta - b.delta))
  let open = 0
  let most = 0
  for (const point of points) {
    open += point.delta
    most = Math.max(most, open)
  }
  return most
}

/** Each of a scenario's expectations against the metrics: what held, what did not. */
export function verdict(scenario, metrics) {
  return scenario.expectations.map(({ name, holds }) => ({ name, ok: Boolean(holds(metrics)) }))
}
