import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadLedger } from '../ledger-file.mjs'

/**
 * A home of the Candidate's shape, built here from a ledger recorded while
 * Node's ledger could still make one (tests/fixtures/ledgers/candidate-home.sql,
 * fixed since Node went, see the README beside it): no real home is read by a test,
 * and the shape is what such a home holds by the way the ledger and the agents
 * file are written. Projects open when the app quit, which come back at the
 * next start, each with a chief on an agent of the human's own and a staff, on
 * the harness the rig's stand-in plays (a window opens on nothing else here);
 * and projects closed, with the history that went with them, on harnesses a
 * window cannot open on: sessions, tasks in several states (accepted,
 * cancelled, held, waiting on another), results delivered and accepted, a
 * question and its answer, a switched chief, an urgent tell, notes the human
 * has not read, a conversation bound to its native session with its
 * transcript, a gate with what waits behind it, a member whose agent has left
 * the roster; and an agents file as an older build wrote it, with copies of
 * the catalog's entries beside the human's own.
 *
 * Every project's folder is a real folder with a file in it, outside the home:
 * a run on a copy must leave them, and the home, as they are.
 */

const V1_AGENTS = fileURLToPath(new URL('../fixtures/v1-agents.json', import.meta.url))

/** The folders of the projects the recording holds, by the name the recording gives them. */
const PROJECT_FOLDERS = ['site', 'docs', 'billing', 'legacy']

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
  for (const name of PROJECT_FOLDERS) {
    const path = join(work, name)
    mkdirSync(path, { recursive: true })
    writeFileSync(
      join(path, 'README.md'),
      `The folder of ${name}, which ConsensFlow does not touch.\n`,
    )
  }
  writeAgents(home)
  const ledger = loadLedger(join(home, 'consensflow.db'), 'candidate-home', {
    wal: true,
    replacing: { '@WORK@': work },
  })
  // Open when the app quit, or closed by the human: the state the recording left each project in.
  const ids = (state) =>
    ledger
      .prepare('SELECT id FROM project WHERE state = ? ORDER BY id')
      .all(state)
      .map((row) => row.id)
  const projects = { open: ids('open'), closed: ids('suspended') }
  return { home, work, projects, close: () => ledger.close() }
}
