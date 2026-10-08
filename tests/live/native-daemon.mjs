/**
 * The native daemon for a live driver, and what it writes as it goes: a driver
 * chooses the daemon itself (never the command line), by naming it where the
 * integration rig reads it.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = fileURLToPath(new URL('../..', import.meta.url))
/** The native `cf` (`npm run build:cf`): the daemon is `cf ui`. */
export const CF = join(REPO, 'bin', process.platform === 'win32' ? 'cf.exe' : 'cf')

/** Chooses the native daemon for the integration rig this process starts. */
export function useNativeDaemon() {
  process.env.CONSENSFLOW_TEST_DAEMON = JSON.stringify([CF, 'ui', '--json', '--no-open'])
}

/**
 * What a daemon wrote as it went, from its event file (`events.jsonl` in its
 * home): one line for each event the ledger logged (`at`, `project`, `kind`,
 * `data`) and each change of a window's activity (`kind` `window.activity`,
 * with the `participant` and its `state`). The daemon holds the ledger itself
 * exclusively while it runs, so this is the log's running copy: a function that
 * reads it again at each call, oldest first, each line numbered (`n`) in the
 * order it was written.
 */
export function traceOf(home) {
  const file = join(home, 'events.jsonl')
  return () => {
    let text = ''
    try {
      text = readFileSync(file, 'utf8')
    } catch {}
    const lines = []
    for (const line of text.split('\n')) {
      if (line === '') continue
      try {
        lines.push({ n: lines.length + 1, ...JSON.parse(line) })
      } catch {}
    }
    return lines
  }
}
