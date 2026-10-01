import { appendFileSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The event file in the home, `events.jsonl`: one JSON line per ledger event
 * and per change of a window's activity, appended as it happens, for whoever
 * watches the daemon from outside (a tail, a reviewer reading along). The
 * ledger's own log stays the record; this is its running copy. Append-only,
 * one previous file kept once it passes `limit` bytes, as the daemon's log
 * does, and never a reason for the daemon to fail. `forget(project)` drops a
 * deleted project's lines from both: a project deleted leaves no trace but
 * the line that says it was.
 */
export function eventTrace(home, { limit = 5_000_000 } = {}) {
  const file = join(home, 'events.jsonl')
  const trace = (entry) => {
    try {
      let size = 0
      try {
        size = statSync(file).size
      } catch {}
      if (size > limit) renameSync(file, `${file}.1`)
      appendFileSync(file, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`)
    } catch {
      // The home may be read-only or gone mid-run; the ledger has the event.
    }
  }
  // Each file is about `limit` bytes at most, so forgetting reads little,
  // and a file with nothing of the project is not written at all.
  trace.forget = (project) => {
    for (const name of [file, `${file}.1`]) {
      try {
        const lines = readFileSync(name, 'utf8')
          .split('\n')
          .filter((line) => line.length > 0)
        const kept = lines.filter((line) => {
          try {
            return JSON.parse(line).project !== project
          } catch {
            return true
          }
        })
        if (kept.length === lines.length) continue
        writeFileSync(`${name}.tmp`, kept.map((line) => `${line}\n`).join(''))
        renameSync(`${name}.tmp`, name)
      } catch {
        // No file yet, or one the daemon cannot rewrite: nothing to forget.
      }
    }
  }
  return trace
}
