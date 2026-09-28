import { appendFileSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The event file in the home, `events.jsonl`: one JSON line per ledger event
 * and per change of a window's activity, appended as it happens, for whoever
 * watches the daemon from outside (a tail, a reviewer reading along). The
 * ledger's own log stays the record; this is its running copy. Append-only,
 * and never a reason for the daemon to fail. `forget(project)` drops a
 * deleted project's lines: a project deleted leaves no trace but the line
 * that says it was.
 */
export function eventTrace(home) {
  const file = join(home, 'events.jsonl')
  const trace = (entry) => {
    try {
      appendFileSync(file, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`)
    } catch {
      // The home may be read-only or gone mid-run; the ledger has the event.
    }
  }
  trace.forget = (project) => {
    try {
      const kept = readFileSync(file, 'utf8')
        .split('\n')
        .filter((line) => line.length > 0)
        .filter((line) => {
          try {
            return JSON.parse(line).project !== project
          } catch {
            return true
          }
        })
      writeFileSync(`${file}.tmp`, kept.map((line) => `${line}\n`).join(''))
      renameSync(`${file}.tmp`, file)
    } catch {
      // No file yet, or one the daemon cannot rewrite: nothing to forget.
    }
  }
  return trace
}
