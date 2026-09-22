import { appendFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The event file in the home, `events.jsonl`: one JSON line per ledger event
 * and per change of a window's activity, appended as it happens, for whoever
 * watches the daemon from outside (a tail, a reviewer reading along). The
 * ledger's own log stays the record; this is its running copy. Append-only,
 * and never a reason for the daemon to fail.
 */
export function eventTrace(home) {
  const file = join(home, 'events.jsonl')
  return (entry) => {
    try {
      appendFileSync(file, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`)
    } catch {
      // The home may be read-only or gone mid-run; the ledger has the event.
    }
  }
}
