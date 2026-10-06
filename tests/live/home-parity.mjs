#!/usr/bin/env node
/**
 * A restart on a copy of a real home, on both daemons (the flip's soak): each
 * daemon starts on a copy of its own, resumes what was open there, and once
 * nothing moves any more, what each shows is compared: the projects, each
 * project's board and the chief's inbox. The copies' projects work in scratch
 * folders and their windows are the rig's stand-in Claude, so neither daemon
 * reaches the home, a project folder or a real harness. The copies are
 * removed unless `--keep`.
 *
 *   npm run live:home-parity [-- --home <dir>] [--keep]
 *
 * The home defaults to the Candidate's (`~/.consensflow-candidate`); the live
 * app's is the owner's to name.
 */
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { fileURLToPath } from 'node:url'
import { startIntegration } from '../integration/harness.mjs'

const DAEMON = fileURLToPath(new URL('../integration/core-daemon.mjs', import.meta.url))
const FAKE_AGENT = fileURLToPath(new URL('../integration/fake-agent.mjs', import.meta.url))
const NATIVE = fileURLToPath(
  new URL(`../../bin/${process.platform === 'win32' ? 'cf.exe' : 'cf'}`, import.meta.url),
)
/** How long a daemon may take to settle, and how many looks in a row must agree. */
const SETTLE_MS = 180_000
const STILL_LOOKS = 5

const args = process.argv.slice(2)
const homeAt = args.indexOf('--home')
const HOME = homeAt === -1 ? join(homedir(), '.consensflow-candidate') : args[homeAt + 1]
const KEEP = args.includes('--keep')
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
if (!existsSync(join(HOME, 'consensflow.db'))) {
  process.stderr.write(`no ledger in ${HOME}: name a home with --home\n`)
  process.exit(2)
}

const FILES = ['consensflow.db', 'consensflow.db-wal', 'agents.json']

/**
 * The home's ledger and roster as they are now, once: a home in use goes on
 * changing, and both daemons must start from the same one. Plain file copies:
 * the home's ledger is never opened.
 */
function snapshot() {
  const taken = mkdtempSync(join(tmpdir(), 'consensflow-snapshot-'))
  for (const name of FILES) {
    if (existsSync(join(HOME, name))) copyFileSync(join(HOME, name), join(taken, name))
  }
  return taken
}

/** A copy of the snapshot in a root of its own, its projects moved to scratch folders under it. */
function copyHome(taken) {
  const root = mkdtempSync(join(tmpdir(), 'consensflow-parity-'))
  const to = join(root, 'consensflow')
  mkdirSync(to, { recursive: true })
  for (const name of FILES) {
    if (existsSync(join(taken, name))) copyFileSync(join(taken, name), join(to, name))
  }
  const db = new DatabaseSync(join(to, 'consensflow.db'))
  for (const { id } of db.prepare('SELECT id FROM project').all()) {
    const folder = join(root, 'projects', String(id))
    mkdirSync(folder, { recursive: true })
    db.prepare('UPDATE project SET directory = ? WHERE id = ?').run(folder, id)
  }
  const handles = new Set(
    db
      .prepare('SELECT handle FROM participant')
      .all()
      .map((p) => p.handle),
  )
  const { last } = db.prepare('SELECT max(id) AS last FROM message').get()
  db.close()
  return { root, seen: { root, handles, started: 0, last: last ?? 0 } }
}

/** What a daemon shows: its projects, and each one's board and chief's inbox. */
async function look(app) {
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
    }
  }
  return { projects, shown }
}

/** One daemon on its own copy, until what it shows holds still. */
async function run(label, native, taken) {
  const { root, seen } = copyHome(taken)
  if (native)
    process.env.CONSENSFLOW_TEST_DAEMON = JSON.stringify([NATIVE, 'ui', '--json', '--no-open'])
  else delete process.env.CONSENSFLOW_TEST_DAEMON
  const started = Date.now()
  seen.started = started
  const app = await startIntegration({
    daemon: DAEMON,
    fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT },
    existingRoot: root,
  })
  let last = null
  let still = 0
  try {
    while (still < STILL_LOOKS && Date.now() - started < SETTLE_MS) {
      const now = JSON.stringify(mask(await look(app), seen))
      still = now === last ? still + 1 : 0
      last = now
      await sleep(1000)
    }
    const log = readFileSync(join(root, 'consensflow', 'daemon.log'), 'utf8')
    return {
      label,
      root,
      settled: still >= STILL_LOOKS,
      seconds: Math.round((Date.now() - started) / 1000),
      windows: app.openFrames.map((frame) => frame.id).sort(),
      errors: log.split('\n').filter((line) => / error /.test(line)),
      shown: JSON.parse(last),
    }
  } finally {
    await app.close({ preserveRoot: true })
  }
}

/**
 * What differs between runs without being a difference: times written since
 * the start, tokens, ports, the copy's own root, a pane's generation (a
 * time), a receipt's process id, the names of sessions made since (drawn at
 * random), and the ids and order of messages written since: windows that
 * start together answer in whichever order they are ready.
 */
function mask(value, seen, key = null) {
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

/** Each place two values differ, by its path. */
function* differences(a, b, path = '') {
  if (JSON.stringify(a) === JSON.stringify(b)) return
  const isObject = (v) => v !== null && typeof v === 'object'
  if (isObject(a) && isObject(b) && Array.isArray(a) === Array.isArray(b)) {
    for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
      yield* differences(a[key], b[key], `${path}/${key}`)
    }
    return
  }
  yield `${path}: Node ${JSON.stringify(a)?.slice(0, 160)}, native ${JSON.stringify(b)?.slice(0, 160)}`
}

const taken = snapshot()
const node = await run('Node', false, taken)
const native = await run('native', true, taken)
for (const side of [node, native]) {
  process.stdout.write(
    `${side.label}: ${side.settled ? 'settled' : 'NOT settled'} in ${side.seconds} s; ${side.windows.length} windows; ${side.errors.length} errors logged\n`,
  )
  for (const line of side.errors.slice(0, 10)) process.stdout.write(`  ${line}\n`)
}
const windows = [...differences(node.windows, native.windows, '/windows')]
const shown = [...differences(node.shown, native.shown)]
for (const line of [...windows, ...shown].slice(0, 60)) process.stdout.write(`DIFF ${line}\n`)
process.stdout.write(`${windows.length + shown.length} differences\n`)
for (const root of [taken, node.root, native.root]) {
  if (KEEP) process.stdout.write(`kept: ${root}\n`)
  else rmSync(root, { recursive: true, force: true })
}
const ok = node.settled && native.settled && windows.length + shown.length === 0
process.exit(ok ? 0 : 1)
