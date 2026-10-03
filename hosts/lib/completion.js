/**
 * Lossless completion extraction from each harness's native, read-only store.
 *
 * A record is read on from where the previous look at it stopped: a JSONL
 * transcript from the byte after its last whole line, OpenCode's store from
 * the conversation's last event, Devin's from its last message row and each
 * wire log from where it was left. What a look returns is what a reading of
 * the whole record would: a record that shrank or was replaced is read again
 * from its start. A final unterminated JSONL append may be incomplete;
 * malformed newline-terminated records fail closed. SQLite is read under one
 * transaction. Nothing here uses the bounded display normaliser.
 *
 * Each harness's reader is its own module in `completion/`; what they share,
 * a reading's shape and the reading on of a JSONL file, is in
 * `completion/shared.js`.
 */
import { claudeReader, claudeTranscript } from './completion/claude-code.js'
import { codexReader, codexTranscript } from './completion/codex.js'
import { devinReader } from './completion/devin.js'
import { opencodeReader } from './completion/opencode.js'
import { piReader, piTranscript } from './completion/pi.js'

/** What a harness's own record of a conversation says, read whole. */
export async function answers(kind, sessionId, env, options = {}) {
  if (!sessionId) return { unknown: true, reason: 'missing session id' }
  if (env === null || typeof env !== 'object') {
    return { unknown: true, reason: 'missing explicit env argument' }
  }
  const read = recordReader(kind, sessionId, env)
  return read === null ? { unknown: true, reason: `unknown kind: ${kind}` } : read(options)
}

/**
 * `answers` for a caller that looks at the same conversations every second
 * (the delivery watcher; the live chief's transcript reached 135 MB). Each
 * conversation keeps its reader, so a look reads only what its harness wrote
 * since the last one, and a record that did not change returns the previous
 * result. Looks at one conversation take turns: each reads on from where the
 * one before stopped. Results are shared: callers must not mutate them.
 * A conversation nobody has asked about for `idleMs` (its window closed) is
 * forgotten, so a daemon that runs for weeks keeps only what it still reads.
 */
export function cachedAnswers({ idleMs = 10 * 60_000, now = Date.now } = {}) {
  const known = new Map()
  let swept = now()
  return (kind, sessionId, env, options = {}) => {
    const at = now()
    if (at - swept >= idleMs) {
      swept = at
      for (const [key, entry] of known) if (at - entry.readAt >= idleMs) known.delete(key)
    }
    const key = `${kind}\n${sessionId}`
    let entry = known.get(key)
    if (entry === undefined) {
      const read =
        sessionId && env !== null && typeof env === 'object'
          ? recordReader(kind, sessionId, env)
          : null
      if (read === null) return answers(kind, sessionId, env, options)
      entry = { read, looked: Promise.resolve() }
      known.set(key, entry)
    }
    entry.readAt = at
    const look = entry.looked.then(() => entry.read(options))
    // A look that failed must not hold up the ones after it.
    entry.looked = look.catch(() => {})
    return look
  }
}

/** Whether the harness has kept a record of the conversation at all. */
export async function hasTranscript(kind, sessionId, env) {
  return (await locateTranscript(kind, sessionId, env)) !== null
}

/**
 * A reader of one conversation's record: each call reads on from where the
 * last stopped, and never throws (an unreadable record is an unknown answer).
 * Null for a harness this module does not read.
 */
function recordReader(kind, sessionId, env) {
  switch (kind) {
    case 'codex':
      return codexReader(sessionId, env)
    case 'claude-code':
      return claudeReader(sessionId, env)
    case 'pi':
      return piReader(sessionId, env)
    case 'opencode':
      return opencodeReader(sessionId, env)
    case 'devin':
      return devinReader(sessionId, env)
    default:
      return null
  }
}

/** Where a JSONL harness keeps one session's transcript, or null. */
async function locateTranscript(kind, sessionId, env) {
  switch (kind) {
    case 'claude-code':
      return claudeTranscript(sessionId, env)
    case 'codex':
      return codexTranscript(sessionId, env)
    case 'pi':
      return piTranscript(sessionId, env)
    default:
      return null
  }
}
