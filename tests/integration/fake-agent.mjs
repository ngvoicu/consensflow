import { spawn } from 'node:child_process'
import { appendFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * A stand-in for the `claude` CLI in the new core's integration tests. It
 * speaks Claude's launch arguments, writes Claude's transcript records and its
 * live `sessions/<pid>.json` status, and reads its terminal raw, the way a real
 * TUI does: a bracketed paste followed by Enter is one message.
 *
 * Every message is one turn. `Reply with exactly: X` answers X. A line
 * `DISPATCH @agent <task>` runs `cf task add @agent <task>` with this window's
 * own token, the way a lead hands out work. Anything else is acknowledged.
 */

const CF = fileURLToPath(new URL('../../bin/cf.mjs', import.meta.url))
const VALUE_FLAGS = new Set([
  '--settings',
  '--add-dir',
  '--append-system-prompt-file',
  '--system-prompt-snapshot',
  '--permission-mode',
  '--model',
  '--effort',
])
const PASTE_START = '\u001b[200~'
const PASTE_END = '\u001b[201~'

let sessionId = null
let resuming = false
let seed = null
const args = process.argv.slice(2)
for (let at = 0; at < args.length; at += 1) {
  const arg = args[at]
  if (VALUE_FLAGS.has(arg)) at += 1
  else if (arg === '--session-id' || arg === '--resume') {
    sessionId = args[at + 1]
    resuming = arg === '--resume'
    at += 1
  } else seed = arg
}
if (!sessionId) throw new Error('the fake agent needs --session-id or --resume')

const config = process.env.CLAUDE_CONFIG_DIR
const transcript = join(config, 'projects', 'integration', `${sessionId}.jsonl`)
const statusFile = join(config, 'sessions', `${process.pid}.json`)
mkdirSync(dirname(transcript), { recursive: true })
mkdirSync(dirname(statusFile), { recursive: true })
appendFileSync(join(dirname(process.env.CONSENSFLOW_HOME), 'pids.jsonl'), `${process.pid}\n`)
appendFileSync(
  join(dirname(process.env.CONSENSFLOW_HOME), 'processes.jsonl'),
  `${process.pid}\t${sessionId}\n`,
)
process.on('exit', () => rmSync(statusFile, { force: true }))

let ordinal = 0
const append = (fields) => {
  ordinal += 1
  appendFileSync(
    transcript,
    `${JSON.stringify({
      sessionId,
      version: '2.1.277',
      timestamp: new Date().toISOString(),
      uuid: `${sessionId}-${process.pid}-${ordinal}`,
      ...fields,
    })}\n`,
  )
}
const status = (value) =>
  writeFileSync(
    statusFile,
    JSON.stringify({ pid: process.pid, sessionId, kind: 'interactive', status: value }),
  )

function runCf(words) {
  return new Promise((resolve) => {
    const child = spawn(process.env.CONSENSFLOW_NODE ?? process.execPath, [CF, ...words], {
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let output = ''
    child.stdout.on('data', (chunk) => {
      output += chunk
    })
    child.stderr.on('data', (chunk) => {
      output += chunk
    })
    child.on('close', () => resolve(output.trim()))
  })
}

async function replyTo(text) {
  const dispatch = /^DISPATCH @(\S+) (.+)$/m.exec(text)
  if (dispatch) return `dispatched: ${await runCf(['task', 'add', `@${dispatch[1]}`, dispatch[2]])}`
  const exact = /Reply with exactly: (\S+)/.exec(text)
  if (exact) return exact[1]
  return `noted: ${text.split('\n')[0]}`
}

let turns = Promise.resolve()
function turn(text) {
  turns = turns.then(async () => {
    status('busy')
    append({ type: 'user', message: { role: 'user', content: text } })
    const reply = await replyTo(text)
    append({
      type: 'assistant',
      message: {
        id: `${sessionId}-message-${process.pid}-${ordinal}`,
        role: 'assistant',
        content: [{ type: 'text', text: reply }],
        stop_reason: 'end_turn',
      },
    })
    append({
      type: 'system',
      subtype: 'stop_hook_summary',
      preventedContinuation: false,
      hookCount: 1,
    })
    status('idle')
  })
  return turns
}

status('idle')
process.stdout.write(`fake agent ${resuming ? 'resumed' : 'started'} on ${sessionId}\n`)
if (seed !== null) turn(seed)

// Raw input, like a real TUI: a bracketed paste is text, Enter outside one submits.
if (process.stdin.isTTY) process.stdin.setRawMode(true)
let buffer = ''
let pasting = false
let pending = ''
process.stdin.setEncoding('utf8')
process.stdin.on('data', (chunk) => {
  pending += chunk
  while (pending.length > 0) {
    if (pending.startsWith(PASTE_START)) {
      pasting = true
      pending = pending.slice(PASTE_START.length)
    } else if (pending.startsWith(PASTE_END)) {
      pasting = false
      pending = pending.slice(PASTE_END.length)
    } else if (pending.startsWith('\u001b') && pending.length < PASTE_START.length) {
      return
    } else {
      const char = pending[0]
      pending = pending.slice(1)
      if (char === '\r' && !pasting) {
        if (buffer.length > 0) turn(buffer)
        buffer = ''
      } else if (char === '\u0003') {
        process.exit(0)
      } else {
        buffer += char === '\r' ? '\n' : char
      }
    }
  }
})
process.stdin.on('end', () => process.exit(0))
process.on('SIGHUP', () => process.exit(0))
process.on('SIGTERM', () => process.exit(0))
