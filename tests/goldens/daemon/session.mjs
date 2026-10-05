/**
 * What the daemon recorder keeps of the test that is running: the ordered
 * steps it did (the ledger calls it made itself, the HTTP exchanges, page
 * operations and `cf` runs the daemon's surfaces answered, the files it left)
 * and, at the end of the test, the trace those make (`FORMAT.md` says what is
 * in it). Every wrapper in this folder reports here; `hooks.mjs` puts the
 * wrappers in place of the modules the suites import.
 */
import { AsyncLocalStorage } from 'node:async_hooks'
import { mkdirSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { encode, failure } from '../ledger/recording.mjs'
import { mask, tokenName, validate } from './mask.mjs'
import { rootOf, snapshot, wrote } from './world.mjs'

const OUT = process.env.CF_DAEMON_TRACES

/**
 * What the code running now is part of: an exchange the API is answering, a
 * page operation, a call a stand-in of the test is serving. The ledger calls
 * and kicks it makes are the surface's own, and the trace folds them into it;
 * a call made outside every frame is the test's own, and is replayed.
 */
export const frames = new AsyncLocalStorage()

let current = null
const numbers = new Map()
/** What a suite made before its tests ran and every one of them uses: its environments, its UI token. */
const outside = new Set()
let ui = null

/** The test being recorded, or null outside one. */
export const recording = () => current

/** A value as the trace writes it: what JSON cannot hold tagged, as the ledger's traces tag it. */
export const value = (item) => encode(item, { wrap: () => ({ $fn: 0 }) })

/** A thrown value as the trace writes it. */
export const refusal = (cause) => failure(cause).$error

export function beginTest(test) {
  if (current !== null) throw new Error('the daemon recorder takes one test at a time')
  current = {
    test,
    steps: [],
    ids: { exchange: 0, run: 0, operation: 0, token: 0, close: 0, trace: 0 },
    environments: new Set(),
    tokens: new Map(),
    apis: [],
    roots: new Set(),
    ledgers: [],
    open: new Set(),
    finishers: [],
    /** The lines a trace or a log dated with its own clock, which share a folder. */
    dated: new Set(),
    touched: false,
    ui: null,
    world: null,
  }
}

/** A folder the test made, which the trace names «root» wherever it is written. */
export function addRoot(path) {
  if (current !== null) current.roots.add(path)
  if (current !== null && current.roots.size > 1) {
    throw new Error(`one folder per test: ${[...current.roots].join(', ')}`)
  }
}

/** The trace of the test just ended is written, when it did anything the daemon's surfaces answer. */
export function endTest() {
  const finished = current
  current = null
  if (finished === null || !finished.touched) return
  for (const finish of finished.finishers) finish()
  const name = basename(finished.test.file).replace(/(\.test)?\.mjs$/, '')
  const number = (numbers.get(name) ?? 0) + 1
  numbers.set(name, number)
  const trace = {
    format: 1,
    surface: surfaceOf(finished),
    test: finished.test,
    ledger: ledgerOf(finished),
    ...(finished.ui === null && ui === null ? {} : { ui: finished.ui ?? ui }),
    steps: finished.steps,
  }
  if (!OUT) return
  mkdirSync(OUT, { recursive: true })
  const text = mask(JSON.stringify(trace), finished)
  validate(text)
  writeFileSync(join(OUT, `${name}-${String(number).padStart(3, '0')}.json`), `${text}\n`)
}

/** Which landing holds the trace: the surfaces it reached, the narrowest first. */
function surfaceOf({ steps, ui: mounted }) {
  const kinds = new Set(steps.map((step) => step.kind))
  if ([...kinds].some((kind) => kind.startsWith('trace.'))) return 'trace'
  if ([...kinds].some((kind) => kind.startsWith('log.'))) return 'log'
  if (kinds.has('operation')) return 'page'
  if (kinds.has('run')) return 'cf'
  return mounted !== null || ui !== null ? 'screens' : 'api'
}

function ledgerOf({ ledgers }) {
  if (ledgers.length === 0) return null
  if (ledgers.length > 1) throw new Error('a recorded test opens one ledger; this one opened more')
  const { record, given } = ledgers[0]
  return {
    file: record.file,
    options: given,
    ...(record.initial === undefined ? {} : { initial: record.initial }),
    ...(record.openError === undefined ? {} : { openError: record.openError }),
    final: record.final ?? null,
    ...(record.final === undefined ? { unclosed: true } : {}),
  }
}

/** The lines the test's trace and log dated themselves. */
export const dated = () => current.dated

/** The test running now touched a surface of the daemon: its trace is worth writing. */
export function touch() {
  if (current !== null) current.touched = true
}

/** The next number of something this test has made (`exchange`, `run`, ...). */
export const next = (kind) => ++current.ids[kind]

/**
 * A step done. An interval open now (an exchange waiting for its answer, a
 * run) that does not own it is told a step came between its start and end:
 * it is `detached`, and a `settle` step marks where it ended.
 */
export function push(step) {
  for (const interval of current.open) if (!interval.owns(step)) interval.between += 1
  current.steps.push(step)
  return step
}

/** An interval begun: `step` is in the trace where it began, `owns` says which steps are its own. */
export function begin(step, owns = () => false) {
  touch()
  const interval = { step, owns, between: 0 }
  push(step)
  current.open.add(interval)
  return interval
}

/** An interval ended; one that other steps overlapped is marked, and its end placed in the steps. */
export function end(interval, settles) {
  current.open.delete(interval)
  if (interval.between === 0) return
  interval.step.detached = true
  push({ kind: 'settle', ...settles })
}

/** The `cf` runs in flight, each as the interval that began it. */
export const runsInFlight = () =>
  current === null ? [] : [...current.open].filter((interval) => interval.step.kind === 'run')

/** What waits for the end of the test to be written down: a body that was still arriving. */
export function later(finish) {
  if (current !== null) current.finishers.push(finish)
}

/** The ledgers this recorder handed out. */
const ledgers = new WeakSet()
export const recorded = (ledger) => ledgers.add(ledger)
export const isRecorded = (ledger) => ledgers.has(ledger)

/** A kick of the dispatcher: counted in the exchange or operation that asked, a step when none did. */
export function kicked() {
  if (current === null) return
  for (let frame = frames.getStore(); frame !== undefined; frame = frame.parent) {
    if (frame.kind !== 'seam') {
      frame.target.kicks += 1
      return
    }
  }
  push({ kind: 'kick' })
  touch()
}

/** A window's token issued: it is named `T<n>` in the trace, and the window it is for. */
export function issued(real, { participant, project }) {
  if (current === null) return
  const name = `T${next('token')}`
  current.tokens.set(real, name)
  push({
    kind: 'issue',
    token: name,
    project: project.id,
    participant: { id: participant.id, handle: participant.handle },
  })
  touch()
}

/** A token given back: the name it was issued under, or what it was when no `issue` gave it. */
export function revoked(real) {
  if (current === null) return
  push({ kind: 'revoke', token: tokenName(current.tokens, real) ?? real })
  touch()
}

export function apiStarted(url) {
  if (current !== null && !current.apis.includes(url)) current.apis.push(url)
}

export function ledgerOpened(record, given) {
  if (current !== null) current.ledgers.push({ record, given })
}

/** The screens mounted with the UI token the test chose: a suite does so before its tests, so it outlasts one. */
export function uiMounted(token) {
  if (current === null) ui = { token }
  else current.ui = { token }
}

/**
 * An environment the running code reads files by (the roster, the harnesses
 * on `PATH`). Its files are written down before each exchange or operation
 * that may read them, and after, to see what it wrote. One a suite made
 * before its tests is every test's; one a test made is that test's.
 */
export function environment(env) {
  if (current === null) outside.add(env)
  else current.environments.add(env)
}

/** A screen of the running exchange answered it: its trace is the screens'. */
export function screened() {
  for (let frame = frames.getStore(); frame !== undefined; frame = frame.parent) {
    if (frame.kind === 'exchange') frame.target.screens = true
  }
}

/**
 * Before an exchange or an operation: the files and variables that changed
 * since the last world this test wrote down are a `world` step.
 */
export function worldBefore() {
  if (current === null) return null
  const roots = [...outside, ...current.environments]
    .map((env) => ({ env, root: rootOf(env) }))
    .filter((found) => found.root)
  if (roots.length === 0) return null
  if (new Set(roots.map((e) => e.root)).size > 1) throw new Error('one folder of files per test')
  addRoot(roots[0].root)
  const now = snapshot(roots[0].env, roots[0].root, current.ledgers)
  const step = current.world === null ? now.full() : now.since(current.world)
  current.world = now
  if (step !== null) push({ kind: 'world', ...step })
  return now
}

/** After: the files the exchange or operation wrote, each as it was and is. */
export function worldAfter(before, step) {
  if (before === null || current === null) return
  const after = snapshot(before.env, before.root, current.ledgers)
  const changed = wrote(before, after)
  current.world = after
  if (changed !== null) step.wrote = changed
}
