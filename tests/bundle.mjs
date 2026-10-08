import { copyFileSync, mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertBuilt, NATIVE_CF } from './choice.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const WINDOWS = process.platform === 'win32'

/**
 * A bundle laid out as the app's is, made of this checkout: the native `cf` the
 * build put in bin/ where the app's is, and the door, `bin/cf.mjs`, beside it
 * (which the app does not ship; the tests of the door run it there):
 *
 *   macOS     <root>/Contents/Resources/cli/bin/{cf, cf.mjs}
 *   Windows   <root>\cli\bin\{cf.exe, cf.mjs}
 *
 * `cleanup` removes it. Nothing else is in it: the app ships no Node and no
 * sources.
 */
export function stageBundle({ cf = NATIVE_CF } = {}) {
  assertBuilt(cf)
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'cf-bundle-')))
  const resources = WINDOWS ? root : join(root, 'Contents', 'Resources')
  const bin = join(resources, 'cli', 'bin')
  mkdirSync(bin, { recursive: true })
  const staged = join(bin, WINDOWS ? 'cf.exe' : 'cf')
  copyFileSync(cf, staged)
  copyFileSync(join(REPO, 'bin', 'cf.mjs'), join(bin, 'cf.mjs'))
  return {
    root,
    cf: staged,
    cfMjs: join(bin, 'cf.mjs'),
    // Windows holds a just-ended program's files for a moment: removal retries (EPERM, 2026-10-08).
    cleanup: () => rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }),
  }
}
