import {
  copyFileSync,
  cpSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  symlinkSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertBuilt, NATIVE_CF } from './choice.mjs'

const REPO = fileURLToPath(new URL('..', import.meta.url))
const WINDOWS = process.platform === 'win32'

/**
 * A bundle laid out as the app's is, made of this checkout: the native `cf` the
 * build put in bin/ where the app's is, the CLI's sources beside it (`cf.mjs`,
 * `src`, `hosts`, `skill`, `package.json`, copied: a link would send the sources'
 * own imports back to the checkout), and this process's Node where the bundle
 * keeps its own. What the native `cf` hands to Node's sources it runs on the
 * Node it finds from its own place in the bundle (`crates/cf/src/node.rs`), and a
 * checkout's bin/ has none, so a run that needs one is run from here:
 *
 *   macOS     <root>/Contents/Resources/cli/bin/{cf, cf.mjs}   <root>/Contents/MacOS/node
 *   Windows   <root>\cli\bin\{cf.exe, cf.mjs}                  <root>\node.exe
 *
 * `cleanup` removes it. The Windows Node is a hard link where the system allows
 * one and a copy where it does not. With `sources: false` the bundle holds the
 * `cf` and the Node only, and the `cf.mjs` beside the `cf` is the caller's to
 * write (a test's stand-in for the CLI); with `node: false` it has no Node.
 */
export function stageBundle({ cf = NATIVE_CF, sources = true, node: withNode = true } = {}) {
  assertBuilt(cf)
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'cf-bundle-')))
  const resources = WINDOWS ? root : join(root, 'Contents', 'Resources')
  const cli = join(resources, 'cli')
  const bin = join(cli, 'bin')
  mkdirSync(bin, { recursive: true })
  const staged = join(bin, WINDOWS ? 'cf.exe' : 'cf')
  copyFileSync(cf, staged)
  if (sources) {
    copyFileSync(join(REPO, 'bin', 'cf.mjs'), join(bin, 'cf.mjs'))
    for (const part of ['src', 'hosts', 'skill']) {
      cpSync(join(REPO, part), join(cli, part), { recursive: true })
    }
    copyFileSync(join(REPO, 'package.json'), join(cli, 'package.json'))
  }
  const node = WINDOWS ? join(root, 'node.exe') : join(root, 'Contents', 'MacOS', 'node')
  if (withNode) {
    mkdirSync(dirname(node), { recursive: true })
    if (WINDOWS) {
      try {
        linkSync(process.execPath, node)
      } catch {
        copyFileSync(process.execPath, node)
      }
    } else {
      symlinkSync(process.execPath, node)
    }
  }
  return {
    root,
    cf: staged,
    cfMjs: join(bin, 'cf.mjs'),
    node,
    // Windows holds a just-ended program's files for a moment: removal retries (EPERM, 2026-10-08).
    cleanup: () => rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }),
  }
}
