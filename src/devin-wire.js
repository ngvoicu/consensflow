/**
 * Devin's wire log, which its adapter reads to learn which conversation the
 * window shows. Devin's SessionStart hook (`cf hook devin-session`) reads
 * it too, in Rust: crates/cf-harness/src/devin/wire.rs.
 */
import { createReadStream } from 'node:fs'
import { stat } from 'node:fs/promises'

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
