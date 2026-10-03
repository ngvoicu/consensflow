/**
 * Watch a live project from outside: what its tasks and windows are doing,
 * read from copies of the app's ledger and trace (the live files are never
 * opened), the way poker-lab was watched on 2026-10-03.
 *
 *   npm run watch -- --project poker-lab
 *   npm run watch -- --project 5 --minutes 30 --tokens
 *
 * It says: the tasks not finished, and who holds each; the messages still on
 * their way; each window's changes of state in the last minutes (one that
 * flips between working and idle is flagged), the last one and why; the
 * windows that stopped on a prompt; deliveries held, retried or failed;
 * members out of quota. With --tokens, what each Claude window's own record
 * says it used: its first turn's context, what it read from the cache, wrote
 * and output.
 */
import { copyFileSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { parseArgs } from 'node:util'

const { values } = parseArgs({
  options: {
    project: { type: 'string' },
    minutes: { type: 'string', default: '60' },
    tokens: { type: 'boolean', default: false },
    home: { type: 'string' },
  },
})
if (!values.project)
  throw new Error('usage: npm run watch -- --project <name or id> [--minutes 60] [--tokens]')
const HOME = values.home ?? process.env.CONSENSFLOW_HOME ?? join(homedir(), '.consensflow')
/**
 * A window flips when its state keeps changing within seconds, most of its
 * changes so: a chief that takes a message the moment its turn ends changes
 * quickly too, now and then, a misread window nearly every time.
 */
const QUICK_MS = 5_000
const FLIPPING = 10

const copy = mkdtempSync(join(tmpdir(), 'cf-watch-'))
try {
  for (const name of ['consensflow.db', 'consensflow.db-wal', 'events.jsonl']) {
    if (existsSync(join(HOME, name))) copyFileSync(join(HOME, name), join(copy, name))
  }
  const db = new DatabaseSync(join(copy, 'consensflow.db'))
  const project = db
    .prepare('SELECT id, name, state FROM project WHERE id = ? OR name = ?')
    .get(Number(values.project) || -1, values.project)
  if (project === undefined) throw new Error(`no project ${values.project}`)
  const since = new Date(Date.now() - Number(values.minutes) * 60_000).toISOString()
  const say = (line = '') => process.stdout.write(`${line}\n`)
  const clock = (iso) => (iso ? new Date(iso).toTimeString().slice(0, 8) : '-')
  say(
    `${project.name} (project ${project.id}, ${project.state}); the last ${values.minutes} minutes`,
  )

  say('\nTasks not finished')
  for (const task of db
    .prepare(
      `SELECT t.number, t.state, a.handle AS assignee, a.harness, t.updated_at AS at, t.title
       FROM task t LEFT JOIN participant a ON a.id = t.assignee_id
       WHERE t.project_id = ? AND t.state NOT IN ('accepted', 'cancelled') ORDER BY t.number`,
    )
    .all(project.id)) {
    say(
      `  T-${task.number} ${task.state.padEnd(8)} ${String(task.assignee ?? '-').padEnd(26)} ${String(task.harness ?? '').padEnd(12)} since ${clock(task.at)}  ${task.title.slice(0, 50)}`,
    )
  }

  say('\nMessages on their way')
  for (const message of db
    .prepare(
      `SELECT m.id, m.kind, m.state, s.handle AS sender, r.handle AS recipient, t.number, m.created_at AS at, m.body
       FROM message m LEFT JOIN participant s ON s.id = m.sender_id
       LEFT JOIN participant r ON r.id = m.recipient_id LEFT JOIN task t ON t.id = m.task_id
       WHERE m.project_id = ? AND m.state IN ('queued', 'delivering', 'gated') ORDER BY m.id`,
    )
    .all(project.id)) {
    say(
      `  m-${message.id} ${message.kind} ${message.state} ${message.sender ?? 'ConsensFlow'} -> ${message.recipient}${message.number ? ` T-${message.number}` : ''} since ${clock(message.at)}: ${message.body.replace(/\s+/g, ' ').slice(0, 60)}`,
    )
  }

  const events = existsSync(join(copy, 'events.jsonl'))
    ? readFileSync(join(copy, 'events.jsonl'), 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((line) => {
          try {
            return JSON.parse(line)
          } catch {
            return null
          }
        })
        .filter((event) => event?.project === project.id && event.at >= since)
    : []
  const windows = new Map()
  for (const event of events.filter((event) => event.kind === 'window.activity')) {
    const window = windows.get(event.participant) ?? {
      changes: 0,
      quick: 0,
      last: null,
      prompts: [],
    }
    window.changes += 1
    if (window.last !== null && Date.parse(event.at) - Date.parse(window.last.at) < QUICK_MS) {
      window.quick += 1
    }
    window.last = event
    if (event.state === 'waiting') window.prompts.push(event)
    windows.set(event.participant, window)
  }
  say('\nWindows')
  for (const [handle, window] of windows) {
    const { state, reason, at } = window.last
    say(
      `  ${handle.padEnd(26)} ${String(window.changes).padStart(4)} changes (${window.quick} within ${QUICK_MS / 1000} s), now ${state}${reason ? ` (${reason})` : ''} since ${clock(at)}${window.quick >= FLIPPING && window.quick * 2 >= window.changes ? '  FLIPPING' : ''}`,
    )
  }
  const prompts = [...windows].flatMap(([handle, window]) =>
    window.prompts.map((event) => `  ${clock(event.at)} ${handle}: ${event.reason ?? 'waiting'}`),
  )
  if (prompts.length > 0) {
    say('\nStopped on a prompt')
    for (const line of prompts) say(line)
  }
  const deliveries = events.filter((event) =>
    ['delivery.held', 'delivery.retried', 'delivery.failed'].includes(event.kind),
  )
  if (deliveries.length > 0) {
    say('\nDeliveries held, retried or failed')
    for (const event of deliveries) {
      say(
        `  ${clock(event.at)} ${event.kind.slice(9).padEnd(7)} m-${event.message ?? event.data?.message} ${event.participant ?? ''} ${event.reason ?? event.data?.reason ?? ''}`,
      )
    }
  }
  const out = db
    .prepare(
      `SELECT handle, out_until FROM participant WHERE project_id = ? AND out_until IS NOT NULL AND out_until > ?`,
    )
    .all(project.id, new Date().toISOString())
  if (out.length > 0) {
    say('\nOut of quota')
    for (const member of out) say(`  ${member.handle} until ${clock(member.out_until)}`)
  }

  if (values.tokens) {
    say('\nClaude windows, as their records count tokens')
    say(`  ${'session'.padEnd(26)} turns  first turn  cache read  written  output`)
    const projects = join(homedir(), '.claude', 'projects')
    for (const { handle, native } of db
      .prepare(
        `SELECT p.handle, c.native_session AS native FROM conversation c
         JOIN participant p ON p.id = c.participant_id
         WHERE p.project_id = ? AND p.harness = 'claude-code' ORDER BY c.id`,
      )
      .all(project.id)) {
      const file = existsSync(projects)
        ? readdirSync(projects)
            .map((folder) => join(projects, folder, `${native}.jsonl`))
            .find((path) => existsSync(path))
        : undefined
      if (file === undefined) continue
      const seen = new Set()
      const used = { turns: 0, first: null, read: 0, written: 0, output: 0 }
      for (const line of readFileSync(file, 'utf8').split('\n')) {
        if (!line.includes('"usage"')) continue
        const record = JSON.parse(line)
        const message = record.message
        if (record.type !== 'assistant' || seen.has(message?.id)) continue
        seen.add(message.id)
        const usage = message.usage ?? {}
        const read = usage.cache_read_input_tokens ?? 0
        const written = (usage.cache_creation_input_tokens ?? 0) + (usage.input_tokens ?? 0)
        used.turns += 1
        used.first ??= read + written
        used.read += read
        used.written += written
        used.output += usage.output_tokens ?? 0
      }
      const n = (count) => count.toLocaleString('en-US').padStart(10)
      say(
        `  ${handle.padEnd(26)} ${String(used.turns).padStart(5)} ${n(used.first ?? 0)} ${n(used.read)} ${n(used.written)} ${n(used.output)}`,
      )
    }
  }
  db.close()
} finally {
  rmSync(copy, { recursive: true, force: true })
}
