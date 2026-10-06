#!/usr/bin/env node
/**
 * The CLI's door, for the flip release. The native `cf` beside this file is the
 * command; this hands every command to it, so one implementation writes each
 * file of the home:
 *
 * - a window's token (`CONSENSFLOW_TOKEN`) makes `cf` the board, which only the
 *   native `cf` is, whatever else is asked;
 * - a home that has taken the way back (the `use-node` file in it, which
 *   `useNode` finds, the same decision the native `cf` and the app make) runs
 *   Node's own CLI, `src/cli.js`, in this process: the app's daemon
 *   (`node cf.mjs ui`) and every command a terminal runs;
 * - everything else goes to the native `cf`, with the words as they came.
 *
 * `setup` and `doctor` are the two verbs the native `cf` still hands to Node's
 * sources, so they stay Node's here too: handed back, they would go round and
 * round between the two. They leave `NODE_ONLY` when the native `cf` answers them.
 *
 * Removed with Node, in the release that deletes it.
 */
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { useNode } from '../src/use-node.js'

/** The verbs the native `cf` does not answer, so which Node's CLI answers whatever the home says. */
const NODE_ONLY = new Set(['setup', 'doctor'])

const [command] = process.argv.slice(2)

if (process.env.CONSENSFLOW_TOKEN || !(useNode(process.env) || NODE_ONLY.has(command))) {
  const here = dirname(fileURLToPath(import.meta.url))
  const native = join(here, process.platform === 'win32' ? 'cf.exe' : 'cf')
  const ran = spawnSync(native, process.argv.slice(2), { stdio: 'inherit' })
  if (ran.error) {
    process.stderr.write(`cf: ${native} did not start: ${ran.error.message}\n`)
    process.exitCode = 1
  } else {
    process.exitCode = ran.status ?? 1
  }
} else {
  await import('../src/cli.js')
}
