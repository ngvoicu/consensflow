/**
 * Live: what does Codex's app-server say when a turn is interrupted while its
 * question (`item/tool/requestUserInput`) waits for an answer?
 *
 * The broker holds a member's question until the board answers it, and drops
 * an answer whose turn has already ended: it learns that from `turn/completed`
 * or an idle `thread/status/changed` for the question's thread (the receipt
 * and stop design, §2.7). Without either, an answer given after the human's
 * own interrupt would be taken and lost. This starts the installed Codex's
 * app-server on a cheap model, with the question tool on as a member's window
 * has it, asks for a question, interrupts the turn once it arrives, and says
 * what Codex sent in the next 20 seconds: the turn's end and its status, the
 * thread's status, and whether the pending question was resolved
 * (`serverRequest/resolved`). The thread is ephemeral, the folder the check's
 * own, and every MCP server is switched off for the run, as the evals do.
 *
 *   npm run live:codex-interrupt
 *
 * The exit code is 1 when Codex says neither that the turn ended nor that
 * the thread went idle, and 2 when the model never asked its question (run
 * it again).
 */
import { execFileSync, spawn } from 'node:child_process'
import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createInterface } from 'node:readline'
import { codexIsolation } from '../../evals/plan.mjs'
import { harnessPath, runnable } from './harnesses.mjs'

const MODEL = 'gpt-5.6-luna'
const ASKED_WITHIN_MS = 180_000
const WATCHED_FOR_MS = 20_000
const QUESTION_TOOL = ['--enable', 'default_mode_request_user_input']

const executable = harnessPath('codex', process.env)
if (executable === null) {
  process.stdout.write('left out: codex is not installed on this machine\n')
  process.exit(0)
}

const version = (() => {
  const run = runnable(executable, ['--version'])
  return execFileSync(run.file, run.args, { ...run.options, encoding: 'utf8' }).trim()
})()
const servers = (() => {
  const run = runnable(executable, ['mcp', 'list', '--json'])
  return JSON.parse(
    execFileSync(run.file, run.args, { ...run.options, encoding: 'utf8', timeout: 30_000 }),
  )
})()
const folder = realpathSync(mkdtempSync(path.join(os.tmpdir(), 'cf-codex-interrupt-')))
const server = runnable(executable, [
  'app-server',
  ...QUESTION_TOOL,
  '-c',
  'suppress_unstable_features_warning=true',
  '-c',
  `model=${JSON.stringify(MODEL)}`,
  ...codexIsolation(servers),
])
const child = spawn(server.file, server.args, {
  ...server.options,
  cwd: folder,
  stdio: ['pipe', 'pipe', 'inherit'],
})

const started = Date.now()
const waiting = new Map()
let nextId = 1
let onMessage = () => {}

const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`)
const request = (method, params) =>
  new Promise((resolve, reject) => {
    const id = nextId++
    waiting.set(id, { resolve, reject, method })
    send({ id, method, params })
  })

createInterface({ input: child.stdout }).on('line', (line) => {
  let message
  try {
    message = JSON.parse(line)
  } catch {
    return
  }
  if (message.method === undefined && waiting.has(message.id)) {
    const { resolve, reject, method } = waiting.get(message.id)
    waiting.delete(message.id)
    if (message.error) reject(new Error(`${method}: ${JSON.stringify(message.error)}`))
    else resolve(message.result)
    return
  }
  if (message.method !== undefined) {
    onMessage(message)
  }
})

/** The first message `matches` accepts, from now, or null after `ms`. */
const next = (matches, ms) =>
  new Promise((resolve) => {
    const timer = setTimeout(() => {
      onMessage = () => {}
      resolve(null)
    }, ms)
    onMessage = (message) => {
      if (!matches(message)) return
      clearTimeout(timer)
      onMessage = () => {}
      resolve(message)
    }
  })

const finish = (code, lines) => {
  for (const line of lines) process.stdout.write(`${line}\n`)
  child.kill()
  rmSync(folder, { recursive: true, force: true })
  process.exit(code)
}

try {
  await request('initialize', { clientInfo: { name: 'consensflow-live', version: '0' } })
  send({ method: 'initialized' })
  const { thread } = await request('thread/start', {
    cwd: folder,
    approvalPolicy: 'never',
    sandbox: 'danger-full-access',
    ephemeral: true,
  })
  const asked = next(
    (message) =>
      message.method === 'item/tool/requestUserInput' && message.params?.threadId === thread.id,
    ASKED_WITHIN_MS,
  )
  const { turn } = await request('turn/start', {
    threadId: thread.id,
    input: [
      {
        type: 'text',
        text: 'Use your request_user_input tool to ask me which colour to use, with the options red and blue. Do nothing else until it is answered.',
      },
    ],
  })
  const question = await asked
  if (question === null) {
    finish(2, [`left out: ${version} on ${MODEL} never asked its question; run it again`])
  }
  const interruptedAt = Date.now() - started
  const after = []
  onMessage = (message) => after.push({ at: Date.now() - started, message })
  await request('turn/interrupt', { threadId: thread.id, turnId: turn.id })
  await new Promise((resolve) => setTimeout(resolve, WATCHED_FOR_MS))
  onMessage = () => {}

  const ofThread = after.filter(({ message }) => message.params?.threadId === thread.id)
  const ended = ofThread.find(({ message }) => message.method === 'turn/completed')
  const idle = ofThread.find(
    ({ message }) =>
      message.method === 'thread/status/changed' && message.params.status?.type === 'idle',
  )
  const resolved = ofThread.find(
    ({ message }) =>
      message.method === 'serverRequest/resolved' && message.params.requestId === question.id,
  )
  const when = (found) => (found ? `${found.at - interruptedAt} ms after the interrupt` : '')
  const lines = [
    `${version} on ${MODEL}: question ${JSON.stringify(question.id)} of turn ${turn.id}, interrupted`,
    ended
      ? `ok   turn/completed, status ${ended.message.params.turn?.status}, ${when(ended)}`
      : 'MISS turn/completed never came',
    idle ? `ok   thread/status/changed to idle, ${when(idle)}` : 'MISS the thread never went idle',
    resolved
      ? `ok   serverRequest/resolved for the question, ${when(resolved)}`
      : 'note the question was never resolved (serverRequest/resolved)',
    `     the thread's messages after the interrupt: ${ofThread.map(({ message }) => message.method).join(', ') || 'none'}`,
  ]
  finish(ended || idle ? 0 : 1, lines)
} catch (error) {
  finish(1, [`FAIL ${error.message}`])
}
