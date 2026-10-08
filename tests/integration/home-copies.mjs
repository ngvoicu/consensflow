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
import { fileURLToPath } from 'node:url'
import { openLedger, SCHEMA_VERSION } from '../../src/ledger/index.js'
import { errorLines, linesOf } from '../choice.mjs'
import { startIntegration } from './harness.mjs'

/**
 * What a run on a copy of a home is made of, for `npm run live:home-parity` (a
 * copy of the live home) and the suite that holds it to account on a home built
 * by a fixture (tests/integration/home-round-trip.test.mjs). A home is the
 * ledger and the agents file; a copy is made from a snapshot of those, plain
 * file copies, so the home itself is never opened, and its projects are moved
 * to scratch folders under the copy, so no window opens in a project's own
 * folder. Each daemon runs on the copy with the rig's stand-in Claude as every
 * window, and is chosen in the words of tests/choice.mjs, whatever the
 * environment says (`select`).
 *
 * Two runs are made on copies:
 *
 *   parity      each daemon on a copy of its own: what each shows once nothing
 *               moves any more, compared, and the agents file each leaves.
 *   round trip  one copy, Node's daemon, then the native one, then Node's: the
 *               flip's way back. The native one writes, by the restart's
 *               resume and by what a probe project hands out and delivers (the
 *               trip adds it to the copy beside the home's own projects, so
 *               that a home with nothing to deliver is tried the same). Each
 *               start must succeed, and read what the one before left: the
 *               projects, each one's board and the inboxes, as it shows them.
 */

/** What a snapshot takes of a home. */
export const HOME_FILES = ['consensflow.db', 'consensflow.db-wal', 'agents.json']
const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

/** How long a daemon may take to settle, and how many looks in a row must agree. */
export const SETTLE = { limitMs: 180_000, looks: 5, everyMs: 1000 }

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
 * above: what the native daemon is given to hand out and deliver. Node's
 * ledger makes it on the copy, before any daemon starts.
 */
function addProbe(file, directory) {
  const ledger = openLedger(file)
  try {
    const project = ledger.createProject({
      directory,
      name: 'round trip probe',
      chief: { harness: 'claude-code', agent: PROBE.chief },
      staff: [{ agent: PROBE.worker, harness: 'claude-code', roles: ['worker'], tier: PROBE.tier }],
    })
    return project.id
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
 * What a daemon shows: its projects, and each one's board and the inboxes of
 * the chief and the human.
 */
export async function look(app) {
  const { projects } = await app.requestNode('projects.list', {})
  const shown = {}
  for (const project of projects) {
    const asked = async (op, body) => {
      try {
        return await app.requestNode(op, body)
      } catch (cause) {
        return { refused: String(cause?.message ?? cause) }
      }
    }
    shown[project.id] = {
      board: await asked('board.get', { project: project.id }),
      inbox: await asked('inbox.get', { project: project.id, participant: 'chief' }),
      human: await asked('inbox.get', { project: project.id, participant: 'human' }),
    }
  }
  return { projects, shown }
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

/**
 * What the ledger file says of itself once no daemon holds it: its schema
 * version, its integrity, and how many references do not hold. It is read on a
 * copy of the file: reading a ledger in write-ahead mode makes files beside it,
 * and the next daemon is to open what the one before left, nothing else made.
 */
export function ledgerFacts(home) {
  const scratch = mkdtempSync(join(tmpdir(), 'consensflow-facts-'))
  try {
    for (const name of ['consensflow.db', 'consensflow.db-wal']) {
      if (existsSync(join(home, name))) copyFileSync(join(home, name), join(scratch, name))
    }
    const db = new DatabaseSync(join(scratch, 'consensflow.db'))
    try {
      return {
        schema: db.prepare('PRAGMA user_version').get().user_version,
        integrity: db.prepare('PRAGMA integrity_check').get().integrity_check,
        broken: db.prepare('PRAGMA foreign_key_check').all().length,
      }
    } finally {
      db.close()
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

/**
 * The conversation each window opened on, by the window's id: `sessions` has
 * the session its launch names, and `launches` how, a fresh one
 * (`--session-id`) or one the window comes back on (`--resume`).
 */
export function conversationsOf(frames) {
  const sessions = {}
  const launches = {}
  for (const { id, argv } of frames) {
    const at = argv.findIndex((word) => word === '--session-id' || word === '--resume')
    if (at === -1) continue
    sessions[id] = argv[at + 1]
    launches[id] = argv[at]
  }
  return { sessions, launches }
}

/** The agents file as it is, by its hash; null if there is none. */
export function rosterHash(home) {
  const file = join(home, 'agents.json')
  return existsSync(file) ? createHash('sha256').update(readFileSync(file)).digest('hex') : null
}

/**
 * Looks at a daemon until what it shows holds still: `looks` in a row agree,
 * masked, or `limitMs` from `from` has gone. What it shows then, unmasked.
 */
async function settled(app, seen, settle, from) {
  let last = null
  let still = 0
  while (still < settle.looks && Date.now() - from < settle.limitMs) {
    const now = JSON.stringify(mask(await look(app), seen))
    still = now === last ? still + 1 : 0
    last = now
    await sleep(settle.everyMs)
  }
  return { settled: still >= settle.looks, shown: JSON.parse(last ?? 'null') }
}

/**
 * One daemon (`select`: `node`, `native` or a command as a JSON array, the
 * native one's) on a copy, from its start until
 * what it shows holds still, then what `work` does with it and until that
 * holds still too, then its stop. The rig refuses a daemon whose start line
 * says it is not the one asked for. What it logged as an error is what its own
 * lines say, the log being shared with every start on the copy. `entry` is
 * what it showed on settling after its start, `exit` what it showed last, and
 * `windows` the windows it opened by itself, before `work`, with the
 * conversations they opened on (`sessions`, `launches`). What it left of the
 * ledger and the agents file is `facts` and `roster`, with `rosterBefore` the
 * agents file it found. A start that fails after it was up says what it had
 * shown by then, in `ran` on the error it throws: what the next look at it
 * would have been compared with is not lost to the failure.
 */
export async function start({ copy, select, settle = SETTLE, work = null }) {
  const { root, home, seen } = copy
  const began = Date.now()
  const rosterBefore = rosterHash(home)
  const app = await startIntegration({
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
    existingRoot: root,
    select,
  })
  const ran = { select, daemon: app.daemon, pid: app.daemonPid(), rosterBefore }
  let failure = null
  try {
    const entry = await settled(app, seen, settle, began)
    ran.entry = entry.shown
    ran.windows = app.openFrames.map((frame) => frame.id).sort()
    Object.assign(ran, conversationsOf(app.openFrames))
    let exit = entry
    if (work !== null) {
      ran.worked = await work(app, copy)
      exit = await settled(app, seen, settle, Date.now())
    }
    Object.assign(ran, {
      settled: entry.settled && exit.settled,
      seconds: Math.round((Date.now() - began) / 1000),
      exit: exit.shown,
      outside: [...new Set(app.openFrames.map((frame) => frame.cwd))].filter(
        (cwd) => !within(root, cwd),
      ),
    })
  } catch (cause) {
    failure = cause
  }
  try {
    await app.close({ preserveRoot: true })
  } catch (cause) {
    failure ??= cause
  }
  if (failure !== null) {
    throw Object.assign(failure instanceof Error ? failure : new Error(String(failure)), { ran })
  }
  ran.errors = errorLines(linesOf(readFileSync(join(home, 'daemon.log'), 'utf8'), ran.pid))
  ran.roster = rosterHash(home)
  ran.facts = ledgerFacts(home)
  return ran
}

/** What a ledger's facts say is wrong with it, as what a daemon is said to have left. */
function ledgerProblems(facts) {
  const wrong = []
  if (facts.schema !== SCHEMA_VERSION) {
    wrong.push(`the ledger at schema ${facts.schema}, not ${SCHEMA_VERSION}`)
  }
  if (facts.integrity !== 'ok' || facts.broken > 0) {
    wrong.push(`a ledger that fails its checks: ${JSON.stringify(facts)}`)
  }
  return wrong
}

/**
 * Each daemon on a copy of its own, the same snapshot to both (no probe): the
 * result of each (`runs`, by the word that selects it), and what differs
 * between what they show, the windows they opened and the agents file each
 * left. A start that did not succeed, a window that opened outside its copy,
 * and a ledger that fails its checks are problems. `roots` are the copies, for
 * the caller to remove. A test plants a fault with `before` (a function per
 * word that selects, run on that daemon's copy before it starts).
 */
export async function parity(taken, { settle = SETTLE, before = {} } = {}) {
  const runs = {}
  const roots = []
  const problems = []
  for (const select of ['node', 'native']) {
    const copy = copyHome(taken)
    roots.push(copy.root)
    before[select]?.(copy)
    copy.seen.started = Date.now()
    try {
      runs[select] = await start({ copy, select, settle })
    } catch (cause) {
      problems.push(`the ${select} daemon did not succeed: ${cause?.message ?? cause}`)
    }
  }
  const { node, native } = runs
  const found =
    node === undefined || native === undefined
      ? []
      : [
          ...differences(node.windows, native.windows, '/windows'),
          ...differences(node.exit, native.exit),
          ...differences(node.roster, native.roster, '/agents.json'),
        ]
  for (const run of Object.values(runs)) {
    for (const cwd of run.outside) {
      problems.push(`the ${run.select} daemon opened a window in ${cwd}, outside its copy`)
    }
    for (const what of ledgerProblems(run.facts)) {
      problems.push(`the ${run.select} daemon left ${what}`)
    }
  }
  return { runs, roots, differences: found, problems }
}

/**
 * The flip's way back, on one copy: the daemons of `order` (Node's, the native
 * one, Node's) started one after the other, each stopped before the next. What
 * it holds to, each a problem when it fails:
 *
 *  - every start succeeds, settles, and is the daemon asked for;
 *  - what each shows on settling is what the one before showed last (the
 *    projects, each board, the inboxes of the chief and the human), and the
 *    windows each opens by itself are the first one's, each on the same
 *    conversation;
 *  - the ledger's schema is the one Node's build knows after every start, its
 *    integrity holds, and the agents file is as the first start left it;
 *  - no window opens outside the copy.
 *
 * `work` is what the native daemon (the one at `workAt`) is given to do once
 * it has settled: by default the probe's chief hands a task out and its result
 * is delivered. A test plants a fault with `before` (a function per index, run
 * on the copy before that start, as a daemon that left the home in a state it
 * should not have would) and `commands` (a command per index, as a JSON array
 * the rig takes for the native daemon, for a stand-in that is not what it says).
 */
export async function roundTrip(
  taken,
  {
    order = ['node', 'native', 'node'],
    settle = SETTLE,
    probe = true,
    work = deliverProbe,
    workAt = order.indexOf('native'),
    before = {},
    commands = {},
  } = {},
) {
  const copy = copyHome(taken, { probe })
  copy.seen.started = Date.now()
  const starts = []
  const problems = []
  const said = (at, what) => `start ${at + 1} (${order[at]}) ${what}`
  /** What a start read of what the one before left, as far as it was up to read it. */
  const readProblems = (at, ran) => {
    const [first, last] = [starts[0], starts[at - 1]]
    const sides = [`start ${at} (${order[at - 1]}) left`, `start ${at + 1} (${order[at]}) read`]
    return [
      ...[...differences(last.exit, ran.entry, '', sides)].map((found) =>
        said(at, `did not read what the one before left: ${found}`),
      ),
      ...[...differences(first.windows, ran.windows, '/windows', sides)].map((found) =>
        said(at, `opened other windows than the first start: ${found}`),
      ),
      // Each chief comes back on the conversation the first start put it on.
      ...[...differences(first.sessions, ran.sessions, '/sessions', sides)].map((found) =>
        said(at, `opened a window on another conversation than the first start: ${found}`),
      ),
    ]
  }
  for (const [at, select] of order.entries()) {
    before[at]?.(copy)
    let ran
    let failed = null
    try {
      ran = await start({
        copy,
        select: commands[at] ?? select,
        settle,
        work: at === workAt && copy.probe !== null ? work : null,
      })
    } catch (cause) {
      failed = cause
      ran = cause?.ran
      problems.push(said(at, `did not succeed: ${cause?.message ?? cause}`))
    }
    // A start that failed once it was up is held to what it had read by then.
    if (at > 0 && ran?.entry !== undefined) problems.push(...readProblems(at, ran))
    if (failed !== null) break
    starts.push(ran)
    if (!ran.settled) problems.push(said(at, `did not settle in ${settle.limitMs / 1000} s`))
    for (const cwd of ran.outside) {
      problems.push(said(at, `opened a window in ${cwd}, outside its copy`))
    }
    for (const what of ledgerProblems(ran.facts)) problems.push(said(at, `left ${what}`))
    if (at > 0 && ran.roster !== starts[0].roster) {
      problems.push(said(at, 'left the agents file other than the first start did'))
    }
  }
  return { copy, starts, problems }
}

/**
 * The probe's work: its chief is told to hand out a task, and the result of it
 * reaches the chief's window (the task is given to the probe's worker, whose
 * window opens, answers and closes). Answers the result's text, as the chief's
 * inbox has it.
 */
export async function deliverProbe(app, { probe }) {
  const project = probe.project
  await app.tell(project, `DISPATCH --tier ${PROBE.tier} Reply with exactly: ROUND-TRIP`)
  const delivered = async () => {
    const { messages } = await app.requestNode('inbox.get', { project, participant: 'chief' })
    return messages.find((m) => m.kind === 'result' && m.state === 'delivered')
  }
  await app.waitFor(async () => (await delivered()) !== undefined, 90_000)
  return (await delivered()).body
}
