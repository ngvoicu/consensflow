import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { openLedger } from '../src/ledger/index.js'

/**
 * What the ledger's tests share (TEST-BDC-01). The ledger is one SQLite file
 * in the home that holds every project, participant, task and inbox message;
 * each test gets a throwaway directory and a clock that moves one second per
 * reading.
 */

export async function withDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-ledger-'))
  try {
    return await fn(dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

export function clock() {
  let at = Date.parse('2026-09-19T10:00:00.000Z')
  return () => {
    at += 1000
    return new Date(at)
  }
}

/** Session names in a fixed order, so a test can say `zeus-amber-pine` and mean the first one. */
export function names() {
  const list = [
    'amber-pine',
    'brisk-birch',
    'calm-brook',
    'coral-canyon',
    'crisp-cedar',
    'dusky-cliff',
  ]
  let at = 0
  return () => list[at++ % list.length]
}

export async function withLedger(fn) {
  return withDir(async (dir) => {
    const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: clock(), names: names() })
    try {
      return await fn(ledger, dir)
    } finally {
      ledger.close()
    }
  })
}

/** A project with a chief and two workers, the shape most tests start from. */
export function staff(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
  })
  ledger.addMember(project.id, {
    agent: 'diana',
    harness: 'codex',
    role: 'worker',
    tier: 'standard',
  })
  const id = (handle) => ledger.project(project.id).participants.find((p) => p.handle === handle).id
  return { project, id }
}

/**
 * A project with a row in every table: T-1 paused in a session of zeus and
 * taken back from it, then held in one of diana's, T-2 needing it, a
 * question and its answer, an urgent tell that reached diana's window, a
 * note, a conversation bound to its native session with a transcript, the
 * chief switched once, and their events. Returns its ids by table.
 */
export function busyProject(ledger, directory) {
  const project = ledger.createProject({
    directory,
    name: path.basename(directory),
    chief: { harness: 'claude-code' },
  })
  const worker = (agent, harness) =>
    ledger.addMember(project.id, { agent, harness, role: 'worker', tier: 'standard' })
  const zeus = worker('zeus', 'claude-code')
  const diana = worker('diana', 'codex')
  const open = (body, needs) =>
    ledger.createTask(project.id, { from: 'chief', pool: 'worker', tier: 'standard', body, needs })
      .task
  const tasks = [open('Parser', []), open('Docs', [1])]
  const first = ledger.assignTask(project.id, 1, zeus.id)
  deliver(ledger, first.message)
  const conversation = ledger.startConversation(first.message.recipientId, {
    harness: 'claude-code',
  })
  ledger.bindConversation(conversation.id, `native-${directory}`)
  ledger.copyTranscript(conversation.id, [
    { id: 'u1', role: 'user', text: 'Parser' },
    { id: 'a1', role: 'assistant', text: 'Which grammar?' },
  ])
  const question = ledger.ask(project.id, {
    from: first.task.assignee,
    to: 'chief',
    task: 1,
    body: 'Which grammar?',
  })
  ledger.answer(question.id, { from: question.recipientId, body: 'The small one' })
  ledger.pauseTask(project.id, 1, { by: 'chief' })
  ledger.releaseTask(project.id, 1, { because: 'ran out of quota' })
  const second = ledger.assignTask(project.id, 1, diana.id)
  deliver(ledger, second.message)
  ledger.holdTask(project.id, 1, { until: '2026-09-20T10:00:00.000Z', because: 'out of quota' })
  const tell = ledger.ask(project.id, {
    from: 'chief',
    to: second.task.assignee,
    task: 1,
    body: 'Where are you?',
    urgent: true,
  })
  deliver(ledger, tell)
  ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus', cut: true })
  const note = ledger.note(project.id, { from: 'chief', to: 'human', body: 'T-1 waits' })
  return {
    project: [project.id],
    participant: ledger.project(project.id).participants.map((p) => p.id),
    conversation: [conversation.id],
    task: tasks.map((task) => task.id),
    message: [...ledger.task(project.id, 1).messages, note].map((message) => message.id),
    event: ledger.events(project.id).map((event) => event.id),
  }
}

/**
 * Empties the event log of a closed ledger file, by hand. The log is a
 * trace: nothing the ledger answers may depend on it.
 */
export function dropTrace(file) {
  const db = new DatabaseSync(file)
  db.exec('DELETE FROM event')
  db.close()
}

/** The id of a session (or any participant) by handle. */
export const sessionId = (ledger, projectId, handle) =>
  ledger.project(projectId).participants.find((p) => p.handle === handle).id

/** Delivers a message the way the dispatcher will: begin, then confirm. */
export function deliver(ledger, message) {
  ledger.beginDelivery(message.id)
  return ledger.confirmDelivery(message.id, { evidence: `native-${message.id}` })
}
