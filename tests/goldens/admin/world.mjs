/**
 * Plays a scenario against Node's harness admin and writes down what it did:
 * the oracle `crates/cf-harness` is held to, step by step, by the Rust player
 * (`crates/cf-harness/tests/admin`).
 *
 * A scenario is data: `{ name, env, files, effects, steps, kept }`.
 * - `env`: the environment the admin is given, over a default of
 *   `tempEnv`'s; `$ROOT` in any text is the scenario's own folder, made
 *   afresh, under the name the system gives it (so a link resolved lies
 *   under it too).
 * - `files`: what the folder holds before the first step: `{ path, text,
 *   executable, mode }`, `{ path, link }` or `{ dir }`.
 * - `effects`: what the world answers, never the machine's own. `run`: the
 *   programs the admin runs (a version probe, an update), by the program's
 *   name and its arguments (`codex --version`), each a list of answers in
 *   the order asked: `{ stdout, stderr }`, `{ error: { message, killed,
 *   stdout, stderr } }` as `execFile` rejects, or `{ held: true }`.
 *   `latest`: the release each harness's feed says (`{ value }`, `{ error }`
 *   or `{ held }`), where the admin is given a `latest` as its tests give
 *   one. `fetch`: where it is not, and the admin asks Node's own release
 *   feed, the answers of the network, in the order asked: `{ status, chunks
 *   }` (a redirect is a status `fetch` refuses, as it is told to), or `{
 *   failure }` (`refused`, `cut`, `stall`, `stall-body`).
 * - `steps`: `check` (`id`, `refresh`), `update` (`id`), `source` (`id`,
 *   `executable`), `detect`; each begun under its `name` (its place by
 *   default) and run until it waits on something a step controls; `release`
 *   (`kind`, `call`, `answer`) answers a held call; `advance` (`ms`) moves
 *   the clock, firing the timeouts due on the way; `write`, `remove` change
 *   the files.
 * - `kept`: a difference Rust keeps from Node on purpose: `{ step, name,
 *   why, rust }`, `rust` the answer Rust gives where Node's is recorded.
 *
 * Nothing waits on the machine: the clock (`Date.now`, `AbortSignal.timeout`)
 * starts at 2026-09-19T12:00:00Z and moves only when a step advances it;
 * `execFile` and `fetch` answer as scripted, in a turn of the loop, and what
 * is asked of them is written down. A step's record lists what settled
 * (`settled`, in the order begun: by name, with its result or its `error`),
 * what still waits, and the calls it made, by program (`capture`), by
 * harness (`latest`) and by address (`fetch`), each in its own order: which
 * of two harnesses asks first is up to the runtime.
 *
 * A path under the folder is `$ROOT/…` in what is recorded, and the hash that
 * names a bundle of an extension is `$HASH`.
 */
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs'
import { createRequire, syncBuiltinESMExports } from 'node:module'
import { tmpdir } from 'node:os'
import { basename, dirname, join } from 'node:path'
import { promisify } from 'node:util'

const require = createRequire(import.meta.url)
const childProcess = require('node:child_process')
const realExecFile = childProcess.execFile

/** The scenario being played, or null: `execFile` is scripted only while one is. */
let playing = null

// The admin binds `promisify(execFile)` when it is loaded, so what it binds is
// put in place first, and is this one for good: it answers as the machine
// does when no scenario is being played.
function scriptedExecFile(file, ...rest) {
  return realExecFile(file, ...rest)
}
scriptedExecFile[promisify.custom] = (file, args = [], options = {}) =>
  playing === null
    ? promisify(realExecFile)(file, args, options)
    : playing.execute(file, args, options)
childProcess.execFile = scriptedExecFile
syncBuiltinESMExports()

const { HarnessAdmin, releaseSource } = await import('../../../src/harness-admin.js')
const { detectHarnesses, harnessPath, knownHarnesses, missingHarnesses } = await import(
  '../../../src/harnesses.js'
)

/** When a scenario's clock starts, as the launch recorder's does. */
const EPOCH = Date.parse('2026-09-19T12:00:00Z')

/** The words a timeout signal aborts a request with. */
const TIMEOUT_WORDS = 'The operation was aborted due to timeout'

/** A turn of the loop: everything that waits on a promise has gone on by then. */
const turn = () => new Promise((resolve) => setImmediate(resolve))

/** The environment every scenario starts from: `tempEnv`'s (tests/helpers.mjs). */
export const ENV = {
  HOME: '$ROOT/home',
  CONSENSFLOW_HOME: '$ROOT/consensflow',
  CLAUDE_CONFIG_DIR: '$ROOT/home/.claude',
  CODEX_HOME: '$ROOT/home/.codex',
  XDG_CONFIG_HOME: '$ROOT/home/.config',
  PATH: '$ROOT/bin',
  CONSENSFLOW_BIN_DIR: '$ROOT/consensflow/bin',
}

/** `value` with every text of it mapped through `change`. */
function mapTexts(value, change) {
  if (typeof value === 'string') return change(value)
  if (Array.isArray(value)) return value.map((each) => mapTexts(each, change))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, each]) => [key, mapTexts(each, change)]),
    )
  }
  return value
}

/** A recorded text with the folder written `$ROOT` and a bundle's hash `$HASH`. */
function normalize(value, root) {
  return mapTexts(value, (text) =>
    text
      .replaceAll(root, '$ROOT')
      .replace(/([\\/]extensions[\\/](?:pi|opencode)[\\/])[0-9a-f]{64}/g, '$1$$HASH'),
  )
}

/** A program's name as a test names it: its file's name, an extension a stand-in has taken off. */
function nameOf(file) {
  return basename(file).replace(/\.(cmd|bat|exe|mjs)$/i, '')
}

/** An error as `execFile` rejects with one. */
function failure({ message, killed = false, stdout = '', stderr = '' }) {
  const error = new Error(message)
  error.killed = killed
  error.stdout = stdout
  error.stderr = stderr
  return error
}

/** An error of `fetch`: `TypeError: fetch failed`, with the cause of it. */
function fetchFailed(cause) {
  return new TypeError('fetch failed', { cause: new Error(cause) })
}

/** The bytes of a chunk a scenario writes: text, bytes, or text repeated. */
function bytesOf(chunk) {
  if (typeof chunk === 'string') return new TextEncoder().encode(chunk)
  if (chunk.bytes !== undefined) return Uint8Array.from(chunk.bytes)
  return new TextEncoder().encode((chunk.text ?? '').repeat(chunk.count))
}

/** The clock of a scenario: the time, and the timeout signals that fire as it moves. */
function clockOf() {
  let now = EPOCH
  let order = 0
  const timers = []
  const asked = new Map()
  return {
    now: () => now,
    timeout(ms) {
      const controller = new AbortController()
      asked.set(controller.signal, ms)
      timers.push({
        due: now + ms,
        order: order++,
        fire: () => controller.abort(new DOMException(TIMEOUT_WORDS, 'TimeoutError')),
      })
      return controller.signal
    },
    askedFor: (signal) => asked.get(signal),
    async advance(by) {
      const until = now + by
      for (;;) {
        const due = timers
          .filter((timer) => timer.due <= until)
          .sort((a, b) => a.due - b.due || a.order - b.order)[0]
        if (due === undefined) break
        timers.splice(timers.indexOf(due), 1)
        now = Math.max(now, due.due)
        due.fire()
        await turn()
      }
      now = until
    },
  }
}

/** A body that arrives as scripted: whole, cut off, or never ending. */
function streamOf(script, signal) {
  return new ReadableStream({
    start(controller) {
      if (script.failure === 'stall-body') {
        signal.addEventListener('abort', () => controller.error(signal.reason))
        return
      }
      if (script.failure === 'cut') {
        controller.error(new TypeError('terminated'))
        return
      }
      for (const chunk of script.chunks ?? []) controller.enqueue(bytesOf(chunk))
      controller.close()
    },
  })
}

/**
 * The statuses `fetch` refuses where it is told to (`redirect: 'error'`),
 * whatever headers come with them: probed on Node v26.8.1, the same with a
 * `Location` and without (tests/admin-shapes.test.mjs).
 */
const REDIRECTS = new Set([301, 302, 303, 307, 308])

/** What the scenario's `fetch` does for one request. */
function answerFetch(script, init) {
  if (script.failure === 'refused') {
    return Promise.reject(fetchFailed('connect ECONNREFUSED 127.0.0.1:1'))
  }
  if (script.failure === 'stall') {
    return new Promise((_, reject) =>
      init.signal.addEventListener('abort', () => reject(init.signal.reason)),
    )
  }
  const status = script.status ?? 200
  if (REDIRECTS.has(status) && init.redirect === 'error') {
    return Promise.reject(fetchFailed('unexpected redirect'))
  }
  const body = status === 204 || status === 304 ? null : streamOf(script, init.signal)
  return Promise.resolve(new Response(body, { status }))
}

/** Writes `call` down in the list `name` of `calls` has, which it starts when it has none. */
function note(calls, name, call) {
  if (calls[name] === undefined) calls[name] = []
  calls[name].push(call)
}

/** What the world is, for one scenario: its answers, and what it was asked. */
function worldOf(scenario, clock, root) {
  const effects = structuredClone(scenario.effects ?? {})
  const held = []
  const problems = []
  const world = { env: null, problems }
  let calls = { capture: {}, latest: {}, fetch: {} }
  const next = (kind, key) => {
    const queue = effects[kind]?.[key]
    if (queue === undefined || queue.length === 0) {
      problems.push(`nothing scripted for ${kind} ${key}`)
      return null
    }
    return queue.shift()
  }
  const hold = (kind, key) =>
    new Promise((resolve, reject) => held.push({ kind, key, resolve, reject }))
  const said = (answer) => ({ stdout: answer.stdout ?? '', stderr: answer.stderr ?? '' })
  return Object.assign(world, {
    /** `value` with `$ROOT` in its texts the folder. */
    sub: (value) => mapTexts(value, (text) => text.replaceAll('$ROOT', () => root)),
    async execute(file, args, options) {
      const key = [nameOf(file), ...args].join(' ')
      if (options.windowsHide !== true) problems.push(`${key} may show a window`)
      note(calls.capture, key, {
        program: file,
        args,
        cwd: options.cwd,
        env: options.env === world.env ? 'same' : options.env,
        limits: { timeoutMs: options.timeout ?? 0, maxBuffer: options.maxBuffer ?? 1024 * 1024 },
      })
      const answer = next('run', key)
      if (answer === null) throw failure({ message: `spawn ${key} ENOENT` })
      if (answer.held) return hold('run', key)
      if (answer.error) throw failure(answer.error)
      return said(answer)
    },
    async latest(id, source) {
      note(calls.latest, id, { source })
      const answer = next('latest', id)
      if (answer === null) throw new Error(`no release scripted for ${id}`)
      if (answer.held) return hold('latest', id)
      if (answer.error !== undefined) throw new Error(answer.error)
      return answer.value
    },
    fetch(input, init = {}) {
      const url = String(input)
      note(calls.fetch, url, {
        timeoutMs: clock.askedFor(init.signal) ?? null,
        redirect: init.redirect ?? 'follow',
      })
      const script = effects.fetch?.shift()
      if (script === undefined) {
        problems.push(`nothing scripted for fetch ${url}`)
        return Promise.reject(fetchFailed('nothing scripted'))
      }
      return answerFetch(script, init)
    },
    /** Answers the call of `kind` to `key` that was held longest. */
    release(kind, key, answer) {
      const at = held.findIndex((call) => call.kind === kind && call.key === key)
      if (at < 0) {
        problems.push(`nothing held for ${kind} ${key}`)
        return
      }
      const [call] = held.splice(at, 1)
      if (kind === 'run' && answer.error) call.reject(failure(answer.error))
      else if (kind === 'run') call.resolve(said(answer))
      else if (answer.error !== undefined) call.reject(new Error(answer.error))
      else call.resolve(answer.value)
    },
    /** The calls since the last time they were taken. */
    takeCalls() {
      const taken = calls
      calls = { capture: {}, latest: {}, fetch: {} }
      return taken
    },
    /** What was scripted and never asked. */
    unused() {
      const left = []
      for (const [kind, queues] of Object.entries(effects)) {
        if (kind === 'fetch') {
          if (queues.length > 0) left.push('fetch')
          continue
        }
        for (const [key, queue] of Object.entries(queues)) {
          if (queue.length > 0) left.push(`${kind} ${key}`)
        }
      }
      return left
    },
  })
}

/** The calls of a step as they are recorded: each kind's by name, the names in order. */
function recordedCalls(taken, root) {
  const calls = {}
  for (const [kind, byKey] of Object.entries(taken)) {
    const keys = Object.keys(byKey).sort()
    if (keys.length > 0) calls[kind] = Object.fromEntries(keys.map((key) => [key, byKey[key]]))
  }
  return normalize(JSON.parse(JSON.stringify(calls)), root)
}

/** `value` as JSON text reads, so that what Node leaves out is left out. */
const json = (value) => JSON.parse(JSON.stringify(value))

/** Makes the files a scenario starts with. */
function build(files, sub) {
  for (const file of files ?? []) {
    if (file.dir !== undefined) {
      mkdirSync(sub(file.dir), { recursive: true })
      continue
    }
    const target = sub(file.path)
    mkdirSync(dirname(target), { recursive: true })
    if (file.link !== undefined) {
      symlinkSync(sub(file.link), target)
      continue
    }
    writeFileSync(target, file.text ?? '')
    if (file.executable) chmodSync(target, 0o755)
    else if (file.mode !== undefined) chmodSync(target, file.mode)
  }
}

/** What detection says of the environment. */
function detection(env) {
  const known = knownHarnesses().map((harness) => harness.id)
  return {
    known,
    missing: missingHarnesses(env),
    detected: detectHarnesses(env),
    paths: Object.fromEntries(known.map((id) => [id, harnessPath(id, env)])),
  }
}

/** What a step that changes the files does. */
function change(step, sub) {
  const target = sub(step.path)
  if (step.op === 'remove') {
    unlinkSync(target)
    return
  }
  mkdirSync(dirname(target), { recursive: true })
  writeFileSync(target, step.text ?? '')
  if (step.executable) chmodSync(target, 0o755)
}

/**
 * How `releaseSource` sees the CLI of `id` at `executable`, an environment
 * with no Claude settings in it, as JSON.
 */
export function sourceOf(id, executable) {
  return json(releaseSource(id, executable, { HOME: '/no/such/home' }))
}

/**
 * Plays `scenario` and answers it as it is recorded: its inputs, and what
 * each step did. Throws where the scenario scripted too little or too much.
 */
export async function play(scenario) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'cfadmin-')))
  const clock = clockOf()
  const world = worldOf(scenario, clock, root)
  const saved = { now: Date.now, timeout: AbortSignal.timeout, fetch: globalThis.fetch }
  try {
    build(scenario.files, world.sub)
    const env = world.sub({ ...ENV, ...scenario.env })
    world.env = env
    Date.now = clock.now
    AbortSignal.timeout = clock.timeout
    globalThis.fetch = world.fetch
    playing = world
    const own = scenario.effects?.fetch === undefined ? { latest: world.latest } : {}
    const admin = new HarnessAdmin(env, { ...own, run: world.execute })
    const begun = []
    const unrecorded = []
    const records = []
    for (const [index, step] of scenario.steps.entries()) {
      const name = step.name ?? String(index)
      const start = (work) => {
        const operation = { name, order: begun.length, done: false }
        begun.push(operation)
        Promise.resolve()
          .then(work)
          .then(
            (result) => unrecorded.push({ operation, result: json(result) }),
            (error) => unrecorded.push({ operation, error: error.message }),
          )
          .then(() => {
            operation.done = true
          })
      }
      if (step.op === 'check') {
        start(() => admin.check(step.id ?? null, { refresh: step.refresh === true }))
      } else if (step.op === 'update') start(() => admin.update(step.id))
      else if (step.op === 'source') {
        start(() => releaseSource(step.id, world.sub(step.executable), env))
      } else if (step.op === 'detect') start(() => detection(env))
      else if (step.op === 'release') world.release(step.kind, step.call, step.answer)
      else if (step.op === 'advance') await clock.advance(step.ms)
      else if (step.op === 'write' || step.op === 'remove') change(step, world.sub)
      else throw new Error(`${scenario.name}: no step ${step.op}`)
      for (let waits = 0; waits < 3; waits += 1) await turn()
      const settled = unrecorded.splice(0).sort((a, b) => a.operation.order - b.operation.order)
      const record = {
        settled: settled.map(({ operation, result, error }) =>
          error === undefined ? { name: operation.name, result } : { name: operation.name, error },
        ),
      }
      const waiting = begun
        .filter((operation) => !operation.done)
        .map((operation) => operation.name)
      if (waiting.length > 0) record.waiting = waiting
      const calls = recordedCalls(world.takeCalls(), root)
      if (Object.keys(calls).length > 0) record.calls = calls
      records.push(record)
    }
    const left = world.unused()
    if (left.length > 0) throw new Error(`${scenario.name}: scripted and never asked: ${left}`)
    if (world.problems.length > 0) throw new Error(`${scenario.name}: ${world.problems}`)
    const kept = (scenario.kept ?? []).map(({ step, name, why, rust }) => {
      const found = records[step].settled.find((each) => each.name === (name ?? String(step)))
      if (found === undefined) throw new Error(`${scenario.name}: nothing settled at ${step}`)
      return {
        step,
        name: found.name,
        why,
        rust: rust(structuredClone(found.result ?? found.error)),
      }
    })
    return normalize(
      {
        name: scenario.name,
        env: { ...ENV, ...scenario.env },
        files: scenario.files ?? [],
        effects: scenario.effects ?? {},
        steps: scenario.steps,
        recorded: records,
        ...(kept.length > 0 ? { kept } : {}),
      },
      root,
    )
  } finally {
    playing = null
    Date.now = saved.now
    AbortSignal.timeout = saved.timeout
    globalThis.fetch = saved.fetch
    rmSync(root, { recursive: true, force: true })
  }
}
