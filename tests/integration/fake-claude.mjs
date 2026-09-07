import { spawn } from 'node:child_process'
import { appendFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'

const CF_CLI = fileURLToPath(new URL('../../bin/cf.mjs', import.meta.url))

const args = process.argv.slice(2)
let sessionId = null
let awaitingSession = false
let resuming = false
let seed = ''
for (const arg of args) {
  if (awaitingSession) {
    sessionId = arg
    awaitingSession = false
  } else if (arg === '--session-id') {
    awaitingSession = true
  } else if (arg === '--resume') {
    awaitingSession = true
    resuming = true
  } else if (arg !== '--dangerously-skip-permissions') {
    seed = arg
  }
}
if (!sessionId) throw new Error('fake Claude needs --session-id')

const directory = join(process.env.CLAUDE_CONFIG_DIR, 'projects', 'integration')
const transcript = join(directory, `${sessionId}.jsonl`)
const pidFile = join(dirname(process.env.CONSENSFLOW_HOME), 'pids.jsonl')
const processFile = join(dirname(process.env.CONSENSFLOW_HOME), 'processes.jsonl')
const PASTE_START = '\u001b[200~'
const PASTE_END = '\u001b[201~'
const readFault = process.env.CF_INTEGRATION_READ_FAULT ?? ''
const bridgeFault = process.env.CF_INTEGRATION_BRIDGE_FAULT ?? ''
mkdirSync(directory, { recursive: true })
appendFileSync(pidFile, `${process.pid}\n`)
appendFileSync(processFile, `${process.pid}\t${sessionId}\n`)

let ordinal = 0
let lastTimestamp = Date.now() - 1
const now = () => {
  lastTimestamp = Math.max(Date.now(), lastTimestamp + 1)
  ordinal += 1
  return new Date(lastTimestamp).toISOString()
}
const common = () => ({ sessionId, version: '2.1.247', timestamp: now() })
const append = (record) => appendFileSync(transcript, `${JSON.stringify(record)}\n`)
const user = (text) =>
  append({
    ...common(),
    type: 'user',
    uuid: `${sessionId}-user-${ordinal}`,
    message: { role: 'user', content: text },
  })
const assistant = (text) =>
  append({
    ...common(),
    type: 'assistant',
    uuid: `${sessionId}-assistant-${ordinal}`,
    message: {
      id: `${sessionId}-message-${ordinal}`,
      role: 'assistant',
      content: [{ type: 'text', text }],
      stop_reason: 'end_turn',
    },
  })
const assistantTool = (id) =>
  append({
    ...common(),
    type: 'assistant',
    uuid: `${sessionId}-assistant-tool-${ordinal}`,
    message: {
      id: `${sessionId}-message-tool-${ordinal}`,
      role: 'assistant',
      content: [{ type: 'tool_use', id, name: 'Bash', input: { command: 'cf read' } }],
      stop_reason: 'tool_use',
    },
  })
const toolResult = (id, text) =>
  append({
    ...common(),
    type: 'user',
    uuid: `${sessionId}-tool-result-${ordinal}`,
    message: {
      role: 'user',
      content: [{ type: 'tool_result', tool_use_id: id, content: [{ type: 'text', text }] }],
    },
  })
const readPart = (deliveryId, part) =>
  new Promise((resolveRead, rejectRead) => {
    const child = spawn(
      process.env.CONSENSFLOW_NODE ?? process.execPath,
      [CF_CLI, 'read', deliveryId, '--part', String(part)],
      { cwd: process.cwd(), env: process.env, stdio: ['ignore', 'pipe', 'pipe'] },
    )
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', (chunk) => {
      stdout += String(chunk)
    })
    child.stdout.on('error', () => {})
    child.stderr.on('data', (chunk) => {
      stderr += String(chunk)
    })
    child.on('error', rejectRead)
    child.on('close', (code, signal) => {
      const result = {
        deliveryId,
        part,
        code,
        signal,
        stdout,
        stderr,
        receiverClosedBeforeOutput: readFault === 'early-close' && part === 1,
      }
      if (readFault === 'early-close' && part === 1) {
        appendFileSync(
          join(process.env.CONSENSFLOW_HOME, 'cf-read-fault.jsonl'),
          `${JSON.stringify(result)}\n`,
        )
      }
      resolveRead(result)
    })
    if (readFault === 'early-close' && part === 1) child.stdout.destroy()
  })

const readAllParts = async (deliveryId) => {
  const parts = []
  for (let part = 1; part <= 1000; part++) {
    const result = await readPart(deliveryId, part)
    if (result.receiverClosedBeforeOutput) {
      throw new Error(`cf read ${deliveryId} --part ${part} receiver closed stdout early (EPIPE)`)
    }
    if (result.code !== 0) {
      if (parts.length === 0) {
        throw new Error(`cf read ${deliveryId} --part ${part} failed: ${result.stderr}`)
      }
      break
    }
    parts.push(result.stdout)
  }
  if (parts.length === 1000) throw new Error(`cf read ${deliveryId} has too many parts`)
  return parts
}

const tailOnly = (text) => String(text).slice(-256)
const withoutBody = (text) => {
  const lines = String(text).split('\n')
  const close = lines.findIndex((line, index) => index > 0 && line.startsWith('[end of part '))
  if (close === -1) return text
  return [lines[0], ...lines.slice(close)].join('\n')
}
const settle = () =>
  append({
    ...common(),
    type: 'system',
    subtype: 'stop_hook_summary',
    preventedContinuation: false,
    hookCount: 0,
    uuid: `${sessionId}-stop-${ordinal}`,
  })

const worker = process.env.CONSENSFLOW_CHILD === '1'
if (worker) {
  user(seed || 'worker task')
  if (seed.includes('CF_DELAYED')) {
    const readyFile = join(process.env.CONSENSFLOW_HOME, 'worker-ready')
    const releaseFile = join(process.env.CONSENSFLOW_HOME, 'worker-release')
    writeFileSync(readyFile, `${sessionId}\n`)
    const deadline = Date.now() + 10_000
    while (!existsSync(releaseFile)) {
      if (Date.now() >= deadline) throw new Error('fake worker release timed out')
      await new Promise((resolve) => setTimeout(resolve, 25))
    }
  }
  const answer = seed.includes('CF_LARGE')
    ? 'L'.repeat(60_000)
    : (process.env.CF_TEST_WORKER_ANSWER ?? 'worker answer')
  assistant(answer)
  settle()
  process.stdout.write(`${answer}\n`)
  if (!seed.includes('CF_HOLD')) process.exit(0)

  const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
  input.on('line', (line) => {
    user(line)
    assistant('worker follow-up')
    settle()
  })
  input.on('close', () => process.exit(0))
}

if (!worker) {
  if (!resuming) {
    writeFileSync(transcript, '')
    user(seed || 'lead seed')
    assistant('lead ready')
    settle()
    process.stdout.write('fake lead ready\n')
  } else {
    process.stdout.write('fake lead resumed\n')
  }

  const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
  let pasted = []
  let bridgeFaulted = false
  let rawMode = false
  let rawModeError = null
  let crSeenBeforeKill = false
  const observationFile = join(process.env.CONSENSFLOW_HOME, 'paste-observed')
  const recordObservation = () =>
    writeFileSync(
      observationFile,
      `${JSON.stringify({ sessionId, rawMode, rawModeError, crSeenBeforeKill })}\n`,
    )
  const observeBridgePaste = (chunk) => {
    if (bridgeFault !== 'after-paste-before-cr') {
      return
    }
    const text = String(chunk)
    if (text.includes('\r')) crSeenBeforeKill = true
    if (!bridgeFaulted && text.includes(PASTE_START)) {
      bridgeFaulted = true
      recordObservation()
    } else if (bridgeFaulted && crSeenBeforeKill) {
      recordObservation()
    }
  }
  if (bridgeFault === 'after-paste-before-cr') {
    if (typeof process.stdin.setRawMode !== 'function') {
      rawModeError = 'stdin.setRawMode is unavailable'
      recordObservation()
    } else {
      try {
        process.stdin.setRawMode(true)
        rawMode = true
      } catch (cause) {
        rawModeError = cause instanceof Error ? cause.message : String(cause)
        recordObservation()
      }
    }
    process.stdin.on('data', observeBridgePaste)
  }
  input.on('line', (line) => {
    observeBridgePaste(line)
    if (bridgeFaulted) return
    const withoutStart = line.startsWith(PASTE_START) ? line.slice(PASTE_START.length) : line
    const clean = withoutStart.endsWith(PASTE_END)
      ? withoutStart.slice(0, -PASTE_END.length)
      : withoutStart
    const pointer = /^@[^\n]+ — run: cf read (d-\d+)/.exec(clean)
    if (pointer) {
      pasted = []
      void readAllParts(pointer[1]).then(
        (parts) => {
          const captured =
            readFault === 'tail-only'
              ? parts.map(tailOnly)
              : readFault === 'body-loss'
                ? parts.map(withoutBody)
                : parts
          for (const body of captured) {
            const toolId = `${sessionId}-cf-read-${ordinal}`
            assistantTool(toolId)
            toolResult(toolId, body)
          }
          assistant('lead receipt')
          settle()
        },
        (cause) => {
          assistant(`cf read failed: ${cause instanceof Error ? cause.message : String(cause)}`)
          settle()
        },
      )
      return
    }
    pasted.push(clean)
    if (!clean.startsWith('[end of delivery ')) return
    const body = pasted.join('\n')
    pasted = []
    user(body)
    assistant('lead receipt')
    settle()
  })
  input.on('close', () => process.exit(0))
}
