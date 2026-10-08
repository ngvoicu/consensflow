import { appendFileSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'

/**
 * A stand-in for a daemon that is not the one it was asked to be: it writes the
 * start line of Node's daemon in the log and prints a handle line, the two
 * things a rig that was asked for the native daemon looks at, and then waits to
 * be ended. tests/integration/daemon-seam.test.mjs has the rig refuse it.
 */
const home = process.env.CONSENSFLOW_HOME
mkdirSync(home, { recursive: true })
appendFileSync(
  join(home, 'daemon.log'),
  `${new Date().toISOString()} info start pid ${process.pid} node v0.0.0 home ${home}\n`,
)
process.stdout.write(`${JSON.stringify({ url: 'http://127.0.0.1:1/', token: 'none' })}\n`)
process.stdin.on('end', () => process.exit(0))
process.stdin.resume()
