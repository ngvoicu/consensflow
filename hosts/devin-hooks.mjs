import { createReadStream } from 'node:fs'
import { readFile, stat } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'

const silent = () => ({ code: 0 })

/**
 * Devin's hook for one launch: a session of the conversation the window shows
 * starts with its role text. A subagent's session, another conversation's, or
 * any other event, gets nothing.
 */
export async function runHook(event, { selectedSession, instructions = '' }) {
  const name = event.hook_event_name
  if (
    name !== 'SessionStart' ||
    !instructions ||
    event.agent_id ||
    event.parent_session_id ||
    typeof event.session_id !== 'string' ||
    !/^[A-Za-z0-9_-]{1,200}$/.test(event.session_id) ||
    (await selectedSession()) !== event.session_id
  )
    return silent()
  return {
    code: 0,
    stdout: { hookSpecificOutput: { hookEventName: name, additionalContext: instructions } },
  }
}

/**
 * The conversation one line of Devin's wire log says the window now shows,
 * or null: this stock TUI configures each conversation it opens, at start and
 * at every /new or /resume.
 */
export const shownIn = (event) =>
  event.update?.sessionUpdate === 'config_option_update' &&
  event.update.configOptions?.some((option) => option.id === 'mode') &&
  typeof event.sessionId === 'string'
    ? event.sessionId
    : null

/** The conversation the window shows, as the whole wire log has it so far. */
export async function selectedSession(file) {
  let session = null
  const { size } = await stat(file)
  if (!size) return session
  const input = createReadStream(file, { encoding: 'utf8', end: size - 1 })
  let pending = ''
  try {
    for await (const chunk of input) {
      pending += chunk
      for (let end = pending.indexOf('\n'); end !== -1; end = pending.indexOf('\n')) {
        const line = pending.slice(0, end)
        pending = pending.slice(end + 1)
        if (!line.trim()) continue
        session = shownIn(JSON.parse(line)) ?? session
      }
    }
  } finally {
    input.destroy()
  }
  return session
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const chunks = []
    let bytes = 0
    for await (const chunk of process.stdin) {
      bytes += chunk.length
      if (bytes > 1024 * 1024) throw new Error('hook input exceeds limit')
      chunks.push(chunk)
    }
    const event = JSON.parse(Buffer.concat(chunks).toString('utf8'))
    const result = await runHook(event, {
      selectedSession: () => selectedSession(process.env.CHISEL_PURE_ACP_WIRE_LOG),
      instructions: process.env.CF_DEVIN_ROLE_FILE
        ? await readFile(process.env.CF_DEVIN_ROLE_FILE, 'utf8')
        : '',
    })
    if (result.stdout) process.stdout.write(JSON.stringify(result.stdout))
    process.exitCode = result.code
  } catch {
    // A missing role file or an incomplete native log never stops Devin.
    process.exitCode = 0
  }
}
