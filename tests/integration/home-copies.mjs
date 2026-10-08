import { createHash } from 'node:crypto'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { isAbsolute, join, relative } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { addProject, openLedgerFile } from '../ledger-file.mjs'

/**
 * What a copy of a home is made of, for the suite that holds it
 * (tests/home-copies.test.mjs). A home is the ledger and the agents file; a
 * copy is made from a snapshot of those, plain file copies, so the home itself
 * is never opened, and its projects are moved to scratch folders under the
 * copy, so no window opens in a project's own folder. What differs between two
 * looks at a copy without being a difference is masked, and where two differ
 * is named. (The runs that started the flip release's two daemons on a copy,
 * Node's and the native one, one after the other, went with the way back to
 * Node.)
 */

/** What a snapshot takes of a home. */
export const HOME_FILES = ['consensflow.db', 'consensflow.db-wal', 'agents.json']

/**
 * The home's ledger and roster as they are now, once: a home in use goes on
 * changing, and every daemon must start from the same one. Plain file copies:
 * the home's ledger is never opened.
 */
export function snapshot(home) {
  const taken = mkdtempSync(join(tmpdir(), 'consensflow-snapshot-'))
  for (const name of HOME_FILES) {
    if (existsSync(join(home, name))) copyFileSync(join(home, name), join(taken, name))
  }
  return taken
}

/**
 * The agents the probe project runs on, added to the copy's agents file: a
 * chief and one worker of the tier its tasks ask for, both on the harness the
 * rig's stand-in plays. The names are no agent's of the home's own: a home that
 * has one is refused.
 */
const PROBE = { chief: 'probe-chief', worker: 'probe', tier: 'standard' }

function addProbeAgents(file) {
  let document = { schemaVersion: 1, agents: [] }
  if (existsSync(file)) {
    try {
      document = JSON.parse(readFileSync(file, 'utf8'))
    } catch {
      throw new Error(`${file} is not JSON: the trip cannot add its probe's agents to it`)
    }
  }
  const agents = Array.isArray(document.agents) ? document.agents : []
  for (const id of [PROBE.chief, PROBE.worker]) {
    if (agents.some((agent) => agent?.id === id)) {
      throw new Error(`the home has an agent named ${id}, which the trip's probe runs on`)
    }
  }
  agents.push(
    { id: PROBE.chief, kind: 'claude-code', model: 'fake-chief' },
    { id: PROBE.worker, kind: 'claude-code', model: 'fake', workTier: PROBE.tier },
  )
  writeFileSync(file, `${JSON.stringify({ ...document, agents }, null, 2)}\n`)
}

/**
 * A project of the trip's own, open, with its chief and a worker on the agents
 * above: what the native daemon is given to hand out and deliver. It is
 * written into the copy's ledger (brought to the schema this build knows
 * first) before any daemon starts. The id of the project.
 */
function addProbe(file, directory) {
  const ledger = openLedgerFile(file)
  try {
    return addProject(ledger, {
      directory,
      name: 'round trip probe',
      chief: { harness: 'claude-code', agent: PROBE.chief },
      staff: [{ agent: PROBE.worker, harness: 'claude-code', roles: ['worker'], tier: PROBE.tier }],
    })
  } finally {
    ledger.close()
  }
}

/**
 * A copy of the snapshot in a root of its own, its projects moved to scratch
 * folders under it, and the probe added if `probe` says so; `roster` is the
 * hash of the agents file it starts with. What the masks need to know of the
 * copy is in `seen`: its root, the handles of the participants it holds, the id
 * of its last message, and when the run began.
 */
export function copyHome(taken, { probe = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'consensflow-parity-'))
  try {
    const home = join(root, 'consensflow')
    mkdirSync(home, { recursive: true })
    for (const name of HOME_FILES) {
      if (existsSync(join(taken, name))) copyFileSync(join(taken, name), join(home, name))
    }
    const file = join(home, 'consensflow.db')
    const db = new DatabaseSync(file)
    for (const { id } of db.prepare('SELECT id FROM project').all()) {
      const folder = join(root, 'projects', String(id))
      mkdirSync(folder, { recursive: true })
      db.prepare('UPDATE project SET directory = ? WHERE id = ?').run(folder, id)
    }
    db.close()
    let probed = null
    if (probe) {
      addProbeAgents(join(home, 'agents.json'))
      const directory = join(root, 'projects', 'probe')
      mkdirSync(directory, { recursive: true })
      probed = { project: addProbe(file, directory), directory }
    }
    const again = new DatabaseSync(file)
    const handles = new Set(
      again
        .prepare('SELECT handle FROM participant')
        .all()
        .map((p) => p.handle),
    )
    const { last } = again.prepare('SELECT max(id) AS last FROM message').get()
    again.close()
    return {
      root,
      home,
      probe: probed,
      roster: rosterHash(home),
      seen: { root, handles, started: 0, last: last ?? 0 },
    }
  } catch (cause) {
    // A copy that could not be made leaves nothing behind.
    rmSync(root, { recursive: true, force: true })
    throw cause
  }
}

/**
 * What differs between runs without being a difference: times written since
 * the start, tokens, ports, the copy's own root, a pane's generation (a
 * time), a receipt's process id, the names of sessions made since (drawn at
 * random), and the ids and order of messages written since: windows that
 * start together answer in whichever order they are ready.
 */
export function mask(value, seen, key = null) {
  if (typeof value === 'string') {
    let text = value.replaceAll(seen.root, '«root»')
    text = text.replace(/\b[0-9a-f]{48,64}\b/g, '«token»')
    text = text.replace(/127\.0\.0\.1:\d+/g, '127.0.0.1:«port»')
    text = text.replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z/g, (at) =>
      Date.parse(at) >= seen.started ? '«now»' : at,
    )
    if (key === 'item') text = text.replace(/-\d+-(\d+)$/, '-«pid»-$1')
    return text.replace(/\b[a-z]+(-[a-z]+){2}\b/g, (name) =>
      seen.handles.has(name) || !/^[a-z]+-[a-z]+-[a-z]+$/.test(name) ? name : '«new session»',
    )
  }
  if (key === 'generation' && typeof value === 'number') return '«generation»'
  if ((key === 'id' || key === 'replyTo') && typeof value === 'number' && value > seen.last) {
    return '«new»'
  }
  if (Array.isArray(value)) {
    const masked = value.map((item) => mask(item, seen))
    if (key !== 'messages') return masked
    const fresh = (message) => message?.id === '«new»'
    const order = (a, b) => (JSON.stringify(a) < JSON.stringify(b) ? -1 : 1)
    return [...masked.filter(fresh).sort(order), ...masked.filter((m) => !fresh(m))]
  }
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([name, item]) => [name, mask(item, seen, name)]),
    )
  }
  return value
}

/** Each place two values differ, by its path, `sides` naming whose each is. */
export function* differences(a, b, path = '', sides = ['Node', 'native']) {
  if (JSON.stringify(a) === JSON.stringify(b)) return
  const isObject = (v) => v !== null && typeof v === 'object'
  if (isObject(a) && isObject(b) && Array.isArray(a) === Array.isArray(b)) {
    for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
      yield* differences(a[key], b[key], `${path}/${key}`, sides)
    }
    return
  }
  yield `${path}: ${sides[0]} ${JSON.stringify(a)?.slice(0, 160)}, ${sides[1]} ${JSON.stringify(b)?.slice(0, 160)}`
}

/**
 * What a folder holds, as one hash: each file's path, size and bytes, in path
 * order. For the folders a run must leave as it found them.
 */
export function digestTree(folder) {
  const hash = createHash('sha256')
  const walk = (dir) => {
    const entries = readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : 1,
    )
    for (const entry of entries) {
      const path = join(dir, entry.name)
      hash.update(`${entry.isDirectory() ? 'd' : 'f'} ${relative(folder, path)}\n`)
      if (entry.isDirectory()) walk(path)
      else hash.update(`${statSync(path).size}\n`).update(readFileSync(path))
    }
  }
  walk(folder)
  return hash.digest('hex')
}

/** Whether `path` is `root` or under it. */
export function within(root, path) {
  const way = relative(root, path)
  return way === '' || (!way.startsWith('..') && !isAbsolute(way))
}

/** The agents file as it is, by its hash; null if there is none. */
export function rosterHash(home) {
  const file = join(home, 'agents.json')
  return existsSync(file) ? createHash('sha256').update(readFileSync(file)).digest('hex') : null
}
