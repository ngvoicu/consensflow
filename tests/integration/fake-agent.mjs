import { spawn } from 'node:child_process'
import { appendFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

/**
 * A stand-in for the `claude` CLI in the integration tests. It
 * speaks Claude's launch arguments, writes Claude's transcript records and its
 * live `sessions/<pid>.json` status, and reads its terminal raw, the way a real
 * TUI does: a bracketed paste followed by Enter is one message.
 *
 * Every message is one turn. `Reply with exactly: X` answers X. A line
 * `DISPATCH --tier standard <task>` (or `DISPATCH --review --tier standard
 * <task>`) runs `cf task add` with those words and
 * the task (`\n` in it becomes a line break), with this window's own token,
 * the way a chief hands out work. A line `CF <words>` runs any other `cf` with
 * those words as they stand (`CF task cancel T-3`), and `CF <words> :: <text>`
 * with the text, spaces and all, as one last word (`CF tell T-1 :: Stop now`):
 * the chief's own verbs, as it would type them. A line `SLEEP <seconds> <words>`
 * is a turn that works that long before it does what the words say: a window
 * a chief can still tell, or a chief nothing is delivered to. A key pressed in
 * a turn (the Escape that stops it) is ignored: this window is never stopped. A task
 * saying `QUOTA-OUT` is refused with a 429, Claude's way, by the window whose
 * participant `CF_TEST_QUOTA_OUT` names. A line `ASK <questions JSON>` asks
 * through Claude's question tool: the PreToolUse hook of the settings file
 * this window was launched with runs on a synthetic AskUserQuestion event, and
 * the turn answers with what the hook handed back. A question whose text says
 * `REPLY <words>` is answered with `cf answer`. Anything else is acknowledged.
 */

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
let settingsFile = null
const args = process.argv.slice(2)
for (let at = 0; at < args.length; at += 1) {
  const arg = args[at]
  if (arg === '--settings') settingsFile = args[at + 1]
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

/** `cf` as an agent runs it: the one first on the window's PATH, whichever ConsensFlow installed there. */
function runCf(words) {
  return new Promise((resolve) => {
    const child = spawn('cf', words, {
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

/** Claude's question tool: the settings' PreToolUse hook answers, or the window's dialog would. */
function askThroughHook(questions) {
  const settings = JSON.parse(readFileSync(settingsFile, 'utf8'))
  const command = settings.hooks.PreToolUse.find((h) => h.matcher === 'AskUserQuestion').hooks[0]
    .command
  const event = {
    session_id: sessionId,
    cwd: process.cwd(),
    permission_mode: 'bypassPermissions',
    hook_event_name: 'PreToolUse',
    tool_name: 'AskUserQuestion',
    tool_input: { questions },
  }
  return new Promise((resolve) => {
    // The platform's shell, as Claude runs a hook: /bin/sh here, cmd.exe on Windows.
    const child = spawn(command, {
      shell: true,
      env: process.env,
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    let output = ''
    let errors = ''
    child.stdout.on('data', (chunk) => {
      output += chunk
    })
    child.stderr.on('data', (chunk) => {
      errors += chunk
    })
    child.on('close', (code) => {
      try {
        const answers = JSON.parse(output).hookSpecificOutput.updatedInput.answers
        resolve(`answered: ${questions.map((q) => answers[q.question]).join(' | ')}`)
      } catch {
        resolve(`unanswered (exit ${code}): ${errors.trim() || 'no output'}`.slice(0, 600))
      }
    })
    child.stdin.end(JSON.stringify(event))
  })
}

async function replyTo(text) {
  const slow = /^SLEEP (\d+) (.+)$/m.exec(text)
  if (slow) {
    await new Promise((resolve) => setTimeout(resolve, Number(slow[1]) * 1000))
    return replyTo(slow[2])
  }
  const ask = /^ASK (.+)$/m.exec(text)
  if (ask) return askThroughHook(JSON.parse(ask[1]))
  const asked = /^\[ConsensFlow m-(\d+)[^\]]*question from @/m.exec(text)
  const reply = /REPLY (.+)$/m.exec(text)
  if (asked && reply) return `replied: ${await runCf(['answer', `m-${asked[1]}`, reply[1]])}`
  const dispatch = /^DISPATCH ((?:(?:--(?:advice|review|design|self)|--\S+ \S+) )+)(.+)$/m.exec(
    text,
  )
  if (dispatch) {
    const words = dispatch[1].trim().split(' ')
    const task = dispatch[2].replaceAll('\\n', '\n')
    return `dispatched: ${await runCf(['task', 'add', ...words, task])}`
  }
  const verb = /^CF (.+)$/m.exec(text)
  if (verb) {
    const [words, ...said] = verb[1].split(' :: ')
    return `ran cf: ${await runCf([...words.split(' '), ...(said.length === 0 ? [] : [said.join(' :: ')])])}`
  }
  // The refusing window is a session of the member the test names.
  const me = process.env.CONSENSFLOW_PARTICIPANT ?? ''
  const refusing = process.env.CF_TEST_QUOTA_OUT
  if (/QUOTA-OUT/.test(text) && refusing && (me === refusing || me.startsWith(`${refusing}-`))) {
    return null
  }
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
    if (reply === null) {
      // The provider refused: Claude writes the refusal as an assistant record.
      append({
        type: 'assistant',
        isApiErrorMessage: true,
        apiErrorStatus: 429,
        error: 'rate_limit',
        message: {
          id: `${sessionId}-message-${process.pid}-${ordinal}`,
          role: 'assistant',
          content: [{ type: 'text', text: "You've hit your limit. Resets in 2 hours." }],
        },
      })
      status('idle')
      return
    }
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
      // A real TUI draws what was pasted, and a paste's Enter waits for that.
      process.stdout.write(`[pasted, ${buffer.length} characters]\n`)
    } else if (pending.startsWith('\u001b')) {
      // The start of a paste marker still arriving, or a key: Escape, ignored.
      if ([PASTE_START, PASTE_END].some((marker) => marker.startsWith(pending))) return
      pending = pending.slice(1)
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
