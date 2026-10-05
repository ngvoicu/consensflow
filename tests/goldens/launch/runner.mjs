/**
 * Plays a launch scenario against Node's adapters and writes down what each
 * step did: the oracle `crates/cf-harness` is held to.
 *
 * A scenario is data. Its steps set up a root (stand-in CLIs, files, a
 * harness's own status files), prepare a launch, open its window, look at
 * it, ask whether it is ready and deliver to it, through a pane host that
 * answers what the scenario says and writes down what it was asked. The
 * Rust test plays the same steps against its own adapters.
 *
 * What a step did is written so that it is the same on every run:
 * - every path under the root is `$ROOT/…`;
 * - a value the adapter drew at random (Claude's session id) is named
 *   (`$SESSION`) wherever it shows, and a later step names it so too;
 * - the process this runs as is `$PID`, one long dead `$DEAD`, and a
 *   second live one, which a scenario that names it is given, `$OTHER`;
 * - a step the scenario asks of the adapter writes down what it did to the
 *   tree under the root: each file and folder it made, changed or removed,
 *   with its mode and a file's text.
 *
 * A step may carry `kept`: a difference Rust keeps from Node on purpose,
 * why, and the fields Rust answers instead. Node's answer is recorded as
 * ever; the Rust player holds its own to `kept`, and to Node's elsewhere.
 */
import { spawn } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { claudeCodeAdapter } from '../../../src/adapters/claude-code.js'
import { fakeExecutable } from '../../helpers.mjs'

/** A process id no process has: macOS's pids stop below it, and Windows' are multiples of four. */
export const DEAD = 999_999

const ADAPTERS = { 'claude-code': claudeCodeAdapter }
const WINDOWS = process.platform === 'win32'

/** The process ids a scenario names, by their names. */
function pids(context) {
  const other = context.other === null ? {} : { $OTHER: context.other.pid }
  return { $PID: process.pid, $DEAD: DEAD, ...other }
}

/** `$ROOT/a/b` as a path under `root`, a named process as its id, the named values as drawn. */
function real(context, value) {
  if (Array.isArray(value)) return value.map((item) => real(context, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, real(context, item)]),
    )
  }
  if (typeof value !== 'string') return value
  if (Object.hasOwn(pids(context), value)) return pids(context)[value]
  let text = value
  for (const [name, drawn] of Object.entries(context.named)) text = text.replaceAll(name, drawn)
  if (!text.startsWith('$ROOT')) return text
  return path.join(context.root, ...text.slice('$ROOT'.length).split('/').filter(Boolean))
}

/** A file's text with the named processes and values in it: JSON that no object writes. */
function realText(context, text) {
  let filled = text
  for (const [name, pid] of Object.entries(pids(context)))
    filled = filled.replaceAll(name, `${pid}`)
  for (const [name, drawn] of Object.entries(context.named)) filled = filled.replaceAll(name, drawn)
  return filled
}

/** What a step answered, with the root, the named values and the live processes written as the scenario writes them. */
function written(context, value) {
  if (Array.isArray(value)) return value.map((item) => written(context, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, written(context, item)]),
    )
  }
  if (typeof value === 'number') {
    const live = Object.entries(pids(context)).find(
      ([name, pid]) => name !== '$DEAD' && pid === value,
    )
    return live === undefined ? value : live[0]
  }
  if (typeof value !== 'string') return value
  let text = value
  for (const [name, drawn] of Object.entries(context.named)) text = text.replaceAll(drawn, name)
  const root = context.root
  const slashed = (text) => text.replaceAll('\\', '/')
  if (text.includes(root)) text = text.replaceAll(root, '$ROOT')
  return text.startsWith('$ROOT') ? slashed(text) : text
}

/** Every file and folder under `root`, by its path there, with its mode and a file's text. */
function tree(root) {
  const found = new Map()
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const full = path.join(folder, entry.name)
      const relative = path.relative(root, full).replaceAll('\\', '/')
      const mode = WINDOWS ? null : statSync(full).mode & 0o777
      if (entry.isDirectory()) {
        found.set(relative, { mode })
        walk(full)
      } else {
        found.set(relative, { mode, text: readFileSync(full, 'utf8') })
      }
    }
  }
  if (existsSync(root)) walk(root)
  return found
}

/** What a step did to the tree: each path made, changed or removed, in order. */
function changes(before, after) {
  const made = []
  for (const relative of new Set([...before.keys(), ...after.keys()])) {
    const was = before.get(relative)
    const entry = after.get(relative)
    if (entry === undefined) made.push({ path: `$ROOT/${relative}`, removed: true })
    else if (was === undefined || was.mode !== entry.mode || was.text !== entry.text) {
      made.push({ path: `$ROOT/${relative}`, ...entry })
    }
  }
  return made.sort((left, right) => (left.path < right.path ? -1 : left.path > right.path ? 1 : 0))
}

/**
 * A pane host that answers each request as the scenario says, the next
 * answer for its operation each time (`{throws: message}` for one that
 * fails), and writes down what it was asked. An answer the step left
 * unasked is the scenario's mistake, unless its operation is `optional`.
 */
function scriptedHost(context, answers) {
  const left = new Map(Object.entries(answers ?? {}).map(([op, list]) => [op, [...list]]))
  return {
    async request(op, body) {
      context.requests.push(written(context, { op, body }))
      const answer = left.get(op)?.shift()
      if (answer === undefined) throw new Error(`no answer for ${op}`)
      if (answer?.throws !== undefined) throw Object.assign(new Error(answer.throws), answer)
      return real(context, answer)
    },
    unused(optional = []) {
      return [...left].filter(([op, list]) => list.length > 0 && !optional.includes(op))
    },
  }
}

/** A live process of the scenario's own besides this one: `$OTHER`. */
function startOther() {
  return WINDOWS
    ? spawn('ping', ['-n', '600', '127.0.0.1'], { stdio: 'ignore' })
    : spawn('sleep', ['600'], { stdio: 'ignore' })
}

/** Sets the root up as a step says: false for a step that asks the adapter. */
async function setUp(context, step) {
  if (step.executable !== undefined) {
    fakeExecutable(path.join(context.root, 'bin', step.executable))
    return true
  }
  if (step.write !== undefined) {
    const file = real(context, step.write)
    await fs.mkdir(path.dirname(file), { recursive: true })
    await fs.writeFile(file, real(context, step.text))
    return true
  }
  if (step.remove !== undefined) {
    await fs.rm(real(context, step.remove), { force: true })
    return true
  }
  if (step.status !== undefined || step.statusText !== undefined) {
    // Claude's own status of a live process (`sessions/<pid>.json`), or a
    // file of its by another name, or its text where JSON cannot hold it.
    const folder = path.join(context.env.CLAUDE_CONFIG_DIR, 'sessions')
    await fs.mkdir(folder, { recursive: true })
    if (step.statusText !== undefined) {
      await fs.writeFile(path.join(folder, step.file), realText(context, step.statusText))
      return true
    }
    const { pid, ...fields } = real(context, step.status)
    const file = step.file ?? `${pid}.json`
    await fs.writeFile(path.join(folder, file), JSON.stringify({ pid, ...fields }))
    return true
  }
  if (step.opened !== undefined) {
    // What the engine writes into a window's launch once its pane opened.
    const { pid } = real(context, step.opened)
    if (pid !== undefined) context.launch.pid = pid
    return true
  }
  if (step.follow !== undefined) {
    context.launch.nativeSession = real(context, step.follow)
    return true
  }
  return false
}

/** A step that asks the adapter, played: what it answered. */
async function ask(context, step) {
  if (step.prepare !== undefined) {
    try {
      const plan = await context.adapter.prepare(real(context, step.prepare))
      context.launch = plan.launch
      for (const [name, field] of Object.entries(step.draws ?? {}))
        context.named[name] = plan[field]
      // The launch bag is the window's own state, which its later steps show.
      return {
        argv: plan.argv,
        env: Object.entries(plan.env),
        dropEnv: plan.dropEnv,
        nativeSession: plan.nativeSession ?? null,
      }
    } catch (cause) {
      return { refused: cause.message }
    }
  }
  context.requests = []
  const host = scriptedHost(context, step.answers)
  const pane = { id: 'p1-zeus', generation: 1 }
  const call = async () => {
    if (step.observe !== undefined) {
      return context.adapter.observe({ launch: context.launch, pane, host, conversation: null })
    }
    if (step.ready !== undefined)
      return context.adapter.ready({ launch: context.launch, pane, host })
    if (step.deliver !== undefined) {
      return context.adapter.deliver({ launch: context.launch, pane, host, text: step.deliver })
    }
    throw new Error(`a step of no kind: ${JSON.stringify(step)}`)
  }
  let answer
  try {
    answer = { answer: (await call()) ?? { undefined: true } }
  } catch (cause) {
    answer = { throws: cause.message }
  }
  const unused = host.unused(step.optional)
  if (unused.length > 0) throw new Error(`answers left unasked: ${JSON.stringify(unused)}`)
  return { ...answer, requests: context.requests }
}

/** Plays `scenario` in a root of its own: what each step answered, by its index. */
export async function play(scenario) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-launch-golden-'))
  const context = { root, named: {}, launch: null, requests: [], other: null }
  try {
    if (JSON.stringify(scenario).includes('$OTHER')) context.other = startOther()
    context.env = real(context, scenario.env)
    await fs.mkdir(path.join(root, 'bin'), { recursive: true })
    context.adapter = ADAPTERS[scenario.harness]({ env: context.env })
    const answers = []
    for (const [index, each] of scenario.steps.entries()) {
      if (await setUp(context, each)) continue
      const before = tree(root)
      const answer = await ask(context, each).catch((cause) => {
        throw new Error(`${scenario.name}, step ${index}: ${cause.message}`)
      })
      const after = tree(root)
      answers.push(written(context, { step: index, ...answer, tree: changes(before, after) }))
    }
    return { ...scenario, answers }
  } finally {
    context.other?.kill()
    await fs.rm(root, { recursive: true, force: true })
  }
}
