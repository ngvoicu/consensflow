import { TRANSCRIPT_ITEM_MAX, transcript } from './conversations.js'
import { cut, LedgerError, titleOf } from './model.js'
import { project as readProject } from './projects.js'
import { task as readTask } from './tasks.js'
import { MESSAGE_SELECT, messageView, TASK_SELECT, taskView } from './views.js'

/**
 * What the page reads, each in the one frame the Rust host's bridge carries
 * an answer in: the board, a task with its thread, a participant's messages
 * and what a task's window wrote. What does not fit is cut, marked and
 * counted; `cf` reads the same records whole.
 */

/** How long a coordinator may leave a question before the human sees it too. */
export const OVERDUE_MS = 10 * 60_000
/**
 * How much of a list the page reads at once (the human's notes, what a
 * window wrote), as JSON: half the 1 MiB frame the Rust host's bridge carries
 * each answer in. An item too long for it on its own is cut at
 * TRANSCRIPT_ITEM_MAX characters; at six bytes a character (a control
 * character, escaped) that is 384 KB, so the newest item always fits, and a
 * refresh that reads the notes stays well clear of the frame.
 */
export const PAGE_BYTES = 512 * 1024
/**
 * How much of a message the human's bay on the board carries: two screens
 * of its strip. The page reads the board in one frame of at most 1 MiB, and
 * a message may run to a million characters; its task's thread has it all.
 */
const BAY_EXCERPT = 2_000

/** A result as its card shows it, the way a title reads; null before there is one. */
const firstLine = (body) => (body === undefined ? null : titleOf(body))

/** A message as the bay shows it: a long one cut at BAY_EXCERPT, saying how long it was. */
const bayView = (row) => {
  const message = messageView(row)
  return { ...message, body: cut(message.body, BAY_EXCERPT) }
}

/**
 * The newest of `items` (newest first) that fit in PAGE_BYTES as a JSON
 * list, in that order: what the page reads of a list in one frame. One too
 * long to fit on its own has its text (`field`) cut at TRANSCRIPT_ITEM_MAX
 * characters and goes on; the first that does not fit ends the list.
 */
function newestThatFit(items, field) {
  const fit = []
  // A list of n items takes n + 1 bytes of its own: two brackets, n - 1 commas.
  let used = 1
  for (const item of items) {
    let view = item
    let bytes = Buffer.byteLength(JSON.stringify(view)) + 1
    if (1 + bytes > PAGE_BYTES) {
      view = { ...item, [field]: cut(item[field], TRANSCRIPT_ITEM_MAX) }
      bytes = Buffer.byteLength(JSON.stringify(view)) + 1
    }
    if (used + bytes > PAGE_BYTES) break
    used += bytes
    fit.push(view)
  }
  return fit
}

/**
 * What the window that has a task wrote, as the page reads it in one frame:
 * the last items that fit (`newestThatFit`), no more than `limit` of them,
 * in order, with how many there are (`total`) and how many came (`shown`).
 */
export function latestTranscript(store, projectId, number, { limit } = {}) {
  const { items, total } = transcript(
    store,
    projectId,
    number,
    limit === undefined ? {} : { limit },
  )
  const fit = newestThatFit(items.reverse(), 'text').reverse()
  return { items: fit, total, shown: fit.length }
}

export function board(store, projectId) {
  const project = readProject(store, projectId)
  if (project === null) throw new LedgerError('unknown-project', `no project ${projectId}`, 404)
  const results = new Map(
    store.db
      .prepare(
        `SELECT task_id, body FROM message WHERE project_id = ? AND kind = 'result' ORDER BY id`,
      )
      .all(projectId)
      .map((row) => [row.task_id, row.body]),
  )
  // Each task with the first line of its latest result: what its card shows.
  // Its brief stays out: the drawer reads it with the task, and a long-lived
  // board of briefs would outgrow the frame the page reads it in.
  // A task of a session that has ended sits on its member's lane.
  const rows = store.db
    .prepare(`${TASK_SELECT} WHERE t.project_id = ? ORDER BY t.number`)
    .all(projectId)
  const laneOf = new Map(
    rows.map((row) => [
      row.id,
      row.assignee_left_at !== null && row.assignee_member !== null
        ? row.assignee_member
        : row.assignee,
    ]),
  )
  const tasks = rows.map((row) => {
    const { body: _brief, ...card } = taskView(row)
    return { ...card, result: firstLine(results.get(row.id)) }
  })
  return {
    project,
    // On the board for a member; one given by name waits in its own lane.
    open: tasks.filter((task) => task.state === 'open' && task.assignee === null),
    lanes: project.participants.map((participant) => ({
      participant,
      tasks: tasks.filter((task) => laneOf.get(task.id) === participant.handle),
    })),
    overdue: overdueQuestions(store, projectId),
    gated: gatedMessages(store, projectId),
  }
}

/** What waits for the human's approval, oldest first. */
function gatedMessages(store, projectId) {
  return store.db
    .prepare(`${MESSAGE_SELECT} WHERE m.project_id = ? AND m.state = 'gated' ORDER BY m.id`)
    .all(projectId)
    .map(bayView)
}

/**
 * Questions a coordinator has left unanswered for OVERDUE_MS: the human sees
 * them too. Only one still on its way or in the chief's window counts, from
 * an asker still on the staff, about no task or one still at work (working
 * or waiting); an answer held for the human or declined is no answer yet,
 * as for the task.
 */
function overdueQuestions(store, projectId) {
  const before = new Date(store.now().getTime() - OVERDUE_MS).toISOString()
  return store.db
    .prepare(
      `${MESSAGE_SELECT}
       WHERE m.project_id = ? AND m.kind = 'question' AND r.role = 'chief'
         AND m.state IN ('queued', 'delivering', 'delivered') AND m.created_at <= ?
         AND s.left_at IS NULL AND (m.task_id IS NULL OR t.state IN ('working', 'waiting'))
         AND NOT EXISTS (
           SELECT 1 FROM message a WHERE a.reply_to = m.id AND a.kind = 'answer'
             AND a.state NOT IN ('gated', 'cancelled')
         )
       ORDER BY m.id`,
    )
    .all(projectId, before)
    .map(bayView)
}

/**
 * A task as the page reads it in one frame, null when there is no such
 * task: its brief first, then its thread from the newest message back,
 * each body whole while PAGE_BYTES allows. A body that does not fit is cut
 * at TRANSCRIPT_ITEM_MAX characters, at BAY_EXCERPT, or to the line saying
 * how long it was, whichever fits, and marked `bodyCut`; the earliest
 * messages that do not fit even so are left out, and `messagesLeftOut`
 * says how many. A task message (a brief delivered, a resume, a reopen)
 * always stays, cut to its line at least: there are few, and a window's
 * first one is how the drawer knows the brief it was given. `cf task get`
 * reads it all whole.
 */
export function taskThatFits(store, projectId, number) {
  const task = readTask(store, projectId, number)
  const bytes = (value) => Buffer.byteLength(JSON.stringify(value))
  if (task === null || bytes(task) <= PAGE_BYTES) return task
  const { messages, ...head } = task
  // Each part's forms, from whole to the least it can be.
  const forms = (part) =>
    [
      part,
      ...[TRANSCRIPT_ITEM_MAX, BAY_EXCERPT, 0]
        .filter((max) => part.body.length > max)
        .map((max) => ({ ...part, body: cut(part.body, max), bodyCut: true })),
    ].map((form) => ({ form, size: bytes(form) }))
  const brief = forms(head)
  const thread = messages.map(forms)
  const stays = (at) => messages[at].kind === 'task'
  // The least the answer takes: the brief and every task message cut to
  // its line, and room to say how many messages were left out.
  let used =
    bytes({ ...brief.at(-1).form, messages: [], messagesLeftOut: messages.length }) +
    thread.reduce((sum, options, at) => sum + (stays(at) ? options.at(-1).size + 1 : 0), 0)
  // The first form that fits in the room left, past what is counted for it already.
  const fit = (options, counted, comma) => {
    const chosen = options.find(({ size }) => used + size + comma - counted <= PAGE_BYTES)
    if (chosen !== undefined) used += chosen.size + comma - counted
    return chosen?.form
  }
  const shown = fit(brief, brief.at(-1).size, 0) ?? brief.at(-1).form
  const kept = []
  let leaving = false
  for (let at = messages.length - 1; at >= 0; at -= 1) {
    const options = thread[at]
    if (stays(at)) {
      kept[at] = fit(options, options.at(-1).size + 1, 1) ?? options.at(-1).form
    } else if (!leaving) {
      kept[at] = fit(options, 0, 1)
      leaving = kept[at] === undefined
    }
  }
  const left = messages.filter((_, at) => kept[at] === undefined).length
  return {
    ...shown,
    messages: kept.filter((message) => message !== undefined),
    ...(left === 0 ? {} : { messagesLeftOut: left }),
  }
}

/**
 * A participant's messages as the page reads them in one frame, newest
 * first: those that fit (`newestThatFit`), never what still waits for the
 * human, with how many there are (`total`) and how many came (`shown`).
 * `unread` keeps to the notes still queued: what For you lists for the human.
 */
export function latestMessages(store, participantId, { unread = false } = {}) {
  const which = unread ? `m.kind = 'note' AND m.state = 'queued'` : `m.state != 'gated'`
  const { total } = store.db
    .prepare(`SELECT COUNT(*) AS total FROM message m WHERE m.recipient_id = ? AND ${which}`)
    .get(participantId)
  const rows = store.db
    .prepare(`${MESSAGE_SELECT} WHERE m.recipient_id = ? AND ${which} ORDER BY m.id DESC`)
    .iterate(participantId)
  const fit = newestThatFit(rows.map(messageView), 'body')
  return { messages: fit, total, shown: fit.length }
}
