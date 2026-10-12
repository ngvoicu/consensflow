import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * The native `cf` this checkout builds into bin/ (`npm run build:cf`), which the
 * page's Playwright suite starts as the daemon (`cf ui --json --no-open`, in
 * `app/tests/agents-server.mjs`) and asks for the roster. It is the only
 * implementation there is: the product chooses nothing.
 */

const REPO = fileURLToPath(new URL('..', import.meta.url))

/** The native `cf` of this checkout, where `npm run build:cf` puts it. */
export const NATIVE_CF = join(REPO, 'bin', process.platform === 'win32' ? 'cf.exe' : 'cf')

/**
 * Says the native `cf` of this checkout is built, which every suite that runs it
 * needs: the page's Playwright suite runs it, so it needs `npm run build:cf`
 * first, as CI's prepare-sidecar step has it. A missing one is this sentence and
 * not an ENOENT from wherever it was first started.
 */
export function assertBuilt(file = NATIVE_CF) {
  assert.ok(existsSync(file), `missing built cf: ${file}; build it with npm run build:cf`)
}
