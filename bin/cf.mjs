#!/usr/bin/env node
/**
 * The CLI's door, until it is deleted. The native `cf` beside this file is the
 * command, every verb of it (`setup`, `doctor` and `ui` too); this hands every
 * command to it with the words as they came, and ends as it ends. There is no
 * other implementation behind it: the Node CLI (`src/cli.js`) runs only where a
 * test starts it by name.
 */
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const native = join(here, process.platform === 'win32' ? 'cf.exe' : 'cf')
const ran = spawnSync(native, process.argv.slice(2), { stdio: 'inherit' })
if (ran.error) {
  process.stderr.write(`cf: ${native} did not start: ${ran.error.message}\n`)
  process.exitCode = 1
} else {
  process.exitCode = ran.status ?? 1
}
