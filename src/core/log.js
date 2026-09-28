import { appendFileSync, renameSync, statSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The daemon's own log in the home, `daemon.log`: one line per thing worth
 * knowing afterwards — it started, it stopped and why, a pass that failed or
 * ran long, an error nobody caught — with the error's stack under it. One
 * previous file is kept once it passes `limit` bytes. Never a reason for the
 * daemon to fail: a home that cannot be written loses the line, not the run.
 */
export function daemonLog(home, { limit = 5_000_000, now = () => new Date() } = {}) {
  const file = join(home, 'daemon.log')
  const write = (level, message, error) => {
    const lines = [`${now().toISOString()} ${level} ${message}`]
    if (error !== undefined) {
      const text = error instanceof Error ? (error.stack ?? error.message) : String(error)
      for (const line of text.split('\n')) lines.push(`    ${line}`)
    }
    try {
      let size = 0
      try {
        size = statSync(file).size
      } catch {}
      if (size > limit) renameSync(file, `${file}.1`)
      appendFileSync(file, `${lines.join('\n')}\n`)
    } catch {
      // The home may be read-only or gone mid-run.
    }
  }
  return {
    file,
    info: (message) => write('info', message),
    warn: (message, error) => write('warn', message, error),
    error: (message, error) => write('error', message, error),
  }
}
