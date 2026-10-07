import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { openLedger } from '../../src/ledger/index.js'
import { busyProject, clock, deliver, names } from '../ledger-fixtures.mjs'

/**
 * A home of the Candidate's shape, built here with Node's ledger: no real home
 * is read by a test, and the shape is what such a home holds by the way the
 * ledger and the agents file are written. Projects open when the app quit,
 * which come back at the next start, each with a chief on an agent of the
 * human's own and a staff, on the harness the rig's stand-in plays (a window
 * opens on nothing else here); and projects closed, with the history that went
 * with them, on harnesses a window cannot open on: sessions, tasks in several
 * states (accepted, cancelled, held, waiting on another), results delivered
 * and accepted, a question and its answer, a switched chief, an urgent tell,
 * notes the human has not read, a conversation bound to its
 * native session with its transcript, a gate with what waits behind it, a
 * member whose agent has left the roster; and an agents file as an older build
 * wrote it, with copies of the catalog's entries beside the human's own.
 *
 * Every project's folder is a real folder with a file in it, outside the home:
 * a run on a copy must leave them, and the home, as they are.
 */

const V1_AGENTS = fileURLToPath(new URL('../fixtures/v1-agents.json', import.meta.url))

/** The agents of the human's own that the open projects run on: the harness the rig's stand-in plays. */
const OWN_AGENTS = [
  { id: 'lead', name: 'Lead', kind: 'claude-code', model: 'fake-chief' },
  { id: 'builder', name: 'Builder', kind: 'claude-code', model: 'fake', workTier: 'standard' },
]

/** The agents file an older build wrote, with the human's own agents added to it. */
function writeAgents(home) {
  const document = JSON.parse(readFileSync(V1_AGENTS, 'utf8'))
  const stamp = '2026-08-30T12:00:00.000Z'
  document.agents.push(
    ...OWN_AGENTS.map((agent) => ({ ...agent, createdAt: stamp, updatedAt: stamp })),
  )
  writeFileSync(join(home, 'agents.json'), `${JSON.stringify(document, null, 2)}\n`)
}

/**
 * The home built under `dir`: its ledger (`home`) and the folders of its
 * projects (`work`), which `projects` names by the project's id. The ledger is
 * left open, as a home in use has it, so that a snapshot of it carries the
 * write-ahead file; `close` ends it.
 */
export function buildHome(dir) {
  const home = join(dir, 'consensflow')
  const work = join(dir, 'work')
  mkdirSync(home, { recursive: true })
  const folder = (name) => {
    const path = join(work, name)
    mkdirSync(path, { recursive: true })
    writeFileSync(
      join(path, 'README.md'),
      `The folder of ${name}, which ConsensFlow does not touch.\n`,
    )
    return path
  }
  writeAgents(home)
  const ledger = openLedger(join(home, 'consensflow.db'), { now: clock(), names: names() })
  const projects = { open: [], closed: [] }
  const handle = (project, name) => project.participants.find((p) => p.handle === name)

  // Open when the app quit: a project with a finished task, a cancelled one, a
  // note for the human and the chief's conversation; and a project just begun.
  const site = ledger.createProject({
    directory: folder('site'),
    name: 'site',
    chief: { harness: 'claude-code', agent: 'lead' },
    staff: [{ agent: 'builder', harness: 'claude-code', roles: ['worker'], tier: 'standard' }],
  })
  const chiefConversation = ledger.startConversation(handle(site, 'chief').id, {
    harness: 'claude-code',
  })
  ledger.bindConversation(chiefConversation.id, 'site-chief-session')
  ledger.copyTranscript(chiefConversation.id, [
    { id: 'u1', role: 'user', text: 'Build the parser, then the changelog.' },
    { id: 'a1', role: 'assistant', text: 'T-1 goes to the builder.' },
  ])
  ledger.createTask(site.id, {
    from: 'chief',
    pool: 'worker',
    tier: 'standard',
    body: 'Build the parser\n\nThe grammar is in docs/grammar.md.',
  })
  const first = ledger.assignTask(site.id, 1, handle(site, 'builder').id)
  deliver(ledger, first.message)
  const session = ledger.startConversation(first.message.recipientId, { harness: 'claude-code' })
  ledger.bindConversation(session.id, 'site-builder-session')
  ledger.copyTranscript(session.id, [
    { id: 'u1', role: 'user', text: 'Build the parser' },
    { id: 'a1', role: 'assistant', text: 'Parser done: 14 tests pass' },
  ])
  deliver(ledger, ledger.recordResult(site.id, 1, { body: 'Parser done: 14 tests pass' }).message)
  ledger.acceptTask(site.id, 1, { by: 'chief' })
  ledger.createTask(site.id, {
    from: 'chief',
    pool: 'worker',
    tier: 'standard',
    body: 'Draft the changelog',
  })
  ledger.cancelTask(site.id, 2, { by: 'chief' })
  ledger.note(site.id, { from: 'chief', to: 'human', body: 'T-1 is accepted: the parser is in.' })

  const docs = ledger.createProject({
    directory: folder('docs'),
    name: 'docs',
    chief: { harness: 'claude-code', agent: 'lead' },
  })
  ledger.note(docs.id, { from: 'chief', to: 'human', body: 'The docs project is open.' })
  projects.open.push(site.id, docs.id)

  // Closed by the human, with all that went with them: a project with a row in
  // every table and a chief switched to Codex, and one whose member's agent has
  // left the roster, with its gate holding a brief and a question.
  const billing = busyProject(ledger, folder('billing'))
  ledger.setProjectState(billing.project[0], 'suspended')
  const legacy = ledger.createProject({
    directory: folder('legacy'),
    name: 'legacy',
    chief: { harness: 'codex', agent: 'hyperion' },
    gate: true,
  })
  ledger.addMember(legacy.id, {
    agent: 'retired-one',
    harness: 'pi',
    role: 'worker',
    tier: 'light',
  })
  ledger.createTask(legacy.id, { from: 'chief', to: 'retired-one', body: 'Tidy the changelog' })
  ledger.note(legacy.id, { from: 'retired-one', to: 'chief', body: 'Which entries are old?' })
  ledger.setProjectState(legacy.id, 'suspended')
  projects.closed.push(billing.project[0], legacy.id)

  return { home, work, projects, close: () => ledger.close() }
}
