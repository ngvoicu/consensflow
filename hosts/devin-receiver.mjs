import { createReadStream } from 'node:fs'
import { appendFile, readFile, stat } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'
import { receiverRequest } from './lib/receiver.js'

const silent = () => ({ code: 0 })
const events = new Set(['SessionStart', 'UserPromptSubmit', 'Stop', 'SessionEnd'])
const selected = (receiver, event) =>
  receiver?.session === event.session_id && receiver.retiredAt === undefined

/** Native callbacks fetch once, then return. History verification owns receipt. */
export async function runHook(event, { request, selectedSession, instructions = '' }) {
  const name = event.hook_event_name
  if (
    !events.has(name) ||
    event.agent_id ||
    event.parent_session_id ||
    typeof event.session_id !== 'string' ||
    !/^[A-Za-z0-9_-]{1,200}$/.test(event.session_id) ||
    (await selectedSession()) !== event.session_id
  )
    return silent()

  const role =
    name === 'SessionStart' && instructions
      ? {
          code: 0,
          stdout: {
            hookSpecificOutput: {
              hookEventName: name,
              additionalContext: `${instructions}\n\nDevin reply collection: available worker or advisor replies are fetched at prompt and stop boundaries. A reply arriving after you become idle stays pending until the next human prompt. Use cf results and cf read to check available reports; do not ask the owner to paste a report that is already stored.`,
            },
          },
        }
      : silent()
  if (!request) return role
  if (name === 'SessionStart') {
    try {
      const receiver = await request('state', {})
      await request('register', {
        session: event.session_id,
        previous: receiver?.lease ?? null,
        source: event.source,
      })
    } catch {
      // Role restrictions must remain present even when registration is unavailable.
    }
    return role
  }
  let receiver = await request('state', {})
  if (!selected(receiver, event)) return silent()
  if (name === 'SessionEnd') {
    await request('retire', { lease: receiver.lease })
    return silent()
  }
  const claim = await request('claim', { lease: receiver.lease })
  if (!claim) return silent()
  const identity = { result: claim.result, claim: claim.id, lease: receiver.lease }
  await request('begin', identity)
  receiver = await request('state', {})
  if (
    !selected(receiver, event) ||
    receiver.lease !== identity.lease ||
    (await selectedSession()) !== event.session_id
  ) {
    await request('release', {
      ...identity,
      admitted: false,
      bytesWritten: 0,
      reason: 'native selection changed before hook output',
    })
    return silent()
  }
  return {
    code: 0,
    stdout:
      name === 'Stop'
        ? { decision: 'block', reason: claim.text }
        : { hookSpecificOutput: { hookEventName: name, additionalContext: claim.text } },
  }
}

/** Selection comes from this stock TUI's own native session configuration events. */
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
        const event = JSON.parse(line)
        if (
          event.update?.sessionUpdate === 'config_option_update' &&
          event.update.configOptions?.some((option) => option.id === 'mode') &&
          typeof event.sessionId === 'string'
        )
          session = event.sessionId
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
    const current = () => selectedSession(process.env.CHISEL_PURE_ACP_WIRE_LOG)
    if (
      (await current()) === event.session_id &&
      events.has(event.hook_event_name) &&
      !event.agent_id &&
      !event.parent_session_id
    )
      await appendFile(
        process.env.CF_DEVIN_EVENTS,
        `${JSON.stringify({ ...event, at: Date.now() })}\n`,
        { mode: 0o600 },
      )
    const result = await runHook(event, {
      selectedSession: current,
      instructions: process.env.CF_DEVIN_ROLE_FILE
        ? await readFile(process.env.CF_DEVIN_ROLE_FILE, 'utf8')
        : '',
      request: process.env.CF_RESULT_RECEIVER
        ? receiverRequest(process.env.CF_RESULT_RECEIVER)
        : undefined,
    })
    if (result.stdout) process.stdout.write(JSON.stringify(result.stdout))
    process.exitCode = result.code
  } catch {
    // A missing app or incomplete native log never becomes a wake or a receipt.
    process.exitCode = 0
  }
}
