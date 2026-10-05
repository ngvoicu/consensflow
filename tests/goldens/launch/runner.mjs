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
 * - the process this runs as is `$PID`, and one long dead `$DEAD`;
 * - a prepare writes down the tree it left under the root: each file and
 *   folder it made or changed, its mode, and a file's text.
 */
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

/** `$ROOT/a/b` as a path under `root`, `$PID` and `$DEAD` as process ids, the named values as drawn. */
function real(context, value) {
  if (Array.isArray(value)) return value.map((item) => real(context, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, real(context, item)]),
    )
  }
  if (value === '$PID') return process.pid
  if (value === '$DEAD') return DEAD
  if (typeof value !== 'string') return value
  let text = value
  for (const [name, drawn] of Object.entries(context.named)) text = text.replaceAll(name, drawn)
  if (!text.startsWith('$ROOT')) return text
  return path.join(context.root, ...text.slice('$ROOT'.length).split('/').filter(Boolean))
}

/** What a step answered, with the root, the named values and this process written as the scenario writes them. */
function written(context, value) {
  if (Array.isArray(value)) return value.map((item) => written(context, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, written(context, item)]),
    )
  }
  if (value === process.pid) return '$PID'
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

/** What a step changed of the tree: each path made or changed, in order. */
function changes(before, after) {
  const made = []
  for (const [relative, entry] of after) {
    const was = before.get(relative)
    if (was === undefined || was.mode !== entry.mode || was.text !== entry.text) {
      made.push({ path: `$ROOT/${relative}`, ...entry })
    }
  }
  return made.sort((left, right) => (left.path < right.path ? -1 : left.path > right.path ? 1 : 0))
}

/**
 * A pane host that answers each request as the scenario says, the next
 * answer for its operation each time (`{throws: message}` for one that
 * fails), and writes down what it was asked.
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
  }
}

/** One step, played: what it answered, written as the scenario writes it. */
async function step(context, step) {
  if (step.executable !== undefined) {
    fakeExecutable(path.join(context.root, 'bin', step.executable))
    return null
  }
  if (step.write !== undefined) {
    const file = real(context, step.write)
    await fs.mkdir(path.dirname(file), { recursive: true })
    await fs.writeFile(file, real(context, step.text))
    return null
  }
  if (step.status !== undefined) {
    // Claude's own status of a live process (`sessions/<pid>.json`).
    const { pid, ...fields } = real(context, step.status)
    const folder = path.join(context.env.CLAUDE_CONFIG_DIR, 'sessions')
    await fs.mkdir(folder, { recursive: true })
    await fs.writeFile(path.join(folder, `${pid}.json`), JSON.stringify({ pid, ...fields }))
    return null
  }
  if (step.prepare !== undefined) {
    const before = tree(context.root)
    let answer
    try {
      const plan = await context.adapter.prepare(real(context, step.prepare))
      context.launch = plan.launch
      for (const [name, field] of Object.entries(step.draws ?? {}))
        context.named[name] = plan[field]
      // The launch bag is the window's own state, which its later steps show.
      answer = {
        argv: plan.argv,
        env: plan.env,
        dropEnv: plan.dropEnv,
        nativeSession: plan.nativeSession ?? null,
      }
    } catch (cause) {
      answer = { refused: cause.message }
    }
    return written(context, { ...answer, tree: changes(before, tree(context.root)) })
  }
  if (step.opened !== undefined) {
    // What the engine writes into a window's launch once its pane opened.
    const { pid } = real(context, step.opened)
    if (pid !== undefined) context.launch.pid = pid
    return null
  }
  if (step.follow !== undefined) {
    context.launch.nativeSession = real(context, step.follow)
    return null
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
  return written(context, { ...answer, requests: context.requests })
}

/** Plays `scenario` in a root of its own: what each step answered, by its index. */
export async function play(scenario) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-launch-golden-'))
  try {
    const context = { root, named: {}, launch: null, requests: [] }
    context.env = real(context, scenario.env)
    await fs.mkdir(path.join(root, 'bin'), { recursive: true })
    context.adapter = ADAPTERS[scenario.harness]({ env: context.env })
    const answers = []
    for (const [index, each] of scenario.steps.entries()) {
      const answer = await step(context, each)
      if (answer !== null) answers.push({ step: index, ...answer })
    }
    return { ...scenario, answers }
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
}
