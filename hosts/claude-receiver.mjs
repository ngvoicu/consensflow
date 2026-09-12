import { pathToFileURL } from 'node:url'
import { receiverRequest } from './lib/receiver.js'

const silent = () => ({ code: 0 })
const selected = (receiver, event) =>
  receiver?.session === event.session_id && receiver.retiredAt === undefined

/** Async hooks carry only a notice. Native synchronous prompt hooks carry answer bodies. */
export async function runHook(event, { request, signal }) {
  if (event.agent_id || typeof event.session_id !== 'string') return silent()
  const name = event.hook_event_name
  if (name === 'FileChanged' && event.file_path !== signal) return silent()
  let receiver = await request('state', {})
  if (name === 'SessionStart') {
    receiver = await request('register', {
      session: event.session_id,
      previous: receiver?.lease ?? null,
      source: event.source,
    })
    return {
      code: 0,
      stdout: { hookSpecificOutput: { hookEventName: name, watchPaths: [signal] } },
    }
  }
  if (!selected(receiver, event)) return silent()
  if (name === 'FileChanged' || name === 'Stop') {
    const result = await request('wake', { lease: receiver.lease })
    return result.wake
      ? { code: 2, stderr: 'ConsensFlow has a completed result available for this conversation.' }
      : silent()
  }
  if (name === 'SessionEnd') {
    await request('retire', { lease: receiver.lease })
    return silent()
  }
  if (name !== 'UserPromptSubmit') return silent()
  const claim = await request('claim', { lease: receiver.lease })
  if (!claim) return silent()
  const identity = { result: claim.result, claim: claim.id, lease: receiver.lease }
  await request('begin', identity)
  const current = await request('state', {})
  if (!selected(current, event) || current.lease !== receiver.lease || claim.text.length >= 9500) {
    await request('release', {
      ...identity,
      admitted: false,
      bytesWritten: 0,
      reason: 'native selection changed or hook output exceeds budget',
    })
    return silent()
  }
  // Cancellation belongs to this synchronous native hook. Exit is never a receipt;
  // the app checks the exact hook_additional_context attachment in native history.
  return {
    code: 0,
    stdout: { hookSpecificOutput: { hookEventName: name, additionalContext: claim.text } },
  }
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
      request: receiverRequest(process.env.CF_RESULT_RECEIVER),
      signal: process.env.CF_RESULT_SIGNAL,
    })
    if (result.stdout) process.stdout.write(JSON.stringify(result.stdout))
    if (result.stderr) process.stderr.write(result.stderr)
    process.exitCode = result.code
  } catch {
    // An unavailable app never turns a failed hook into a model wake or claimed receipt.
    process.exitCode = 0
  }
}
