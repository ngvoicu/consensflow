import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { configRoot } from '../../src/roster.js'
import { terminalRuntime } from '../../src/terminal.js'

/** The mark that says a launcher is ours (`MARKER`, `src/terminal.js`). */
const MARK = 'Installed by ConsensFlow'

/**
 * The CLI that the command on PATH runs, as `app/scripts/sync-cli.mjs` needs to
 * know it: `{ entry, live }`, where `entry` is the `cf.mjs` of the bundle the
 * command belongs to and `live` says that bundle has the installed release's
 * identity (the live app, which a development command must never write into);
 * null when there is no command of ours, or it says nothing of the kind.
 *
 * The launcher has two shapes. The one alpha.78 and the builds before it wrote
 * names the runtime and the `cf.mjs` it runs, which `terminalRuntime` reads. The
 * one every build after the flip writes, and the app rewrites an older one to at
 * its start (`cf_launcher::repair`), names the bundle's native `cf` alone, and the
 * `cf.mjs` of that bundle is beside it. Once an app has started, the command is
 * the second, and a reader of the first would find nothing and stop syncing.
 *
 *   exec "<cf>" "$@"        (sh)           "<cf>" %*        (cmd.exe)
 */
export function launcherEntry(env) {
  const old = terminalRuntime(env)
  if (old !== null) return { entry: old.entry, live: old.live }
  // The command a status asks of first, in the home's own bin, and only if it is ours.
  const windows = process.platform === 'win32' || (env.OS ?? '').toLowerCase().includes('windows')
  const file = join(configRoot(env), 'bin', windows ? 'consensflow.cmd' : 'consensflow')
  if (!existsSync(file)) return null
  const text = readFileSync(file, 'utf8')
  const cf = text.includes(MARK) ? nativeCf(text) : null
  if (cf === null) return null
  const entry = join(dirname(cf), 'cf.mjs')
  return { entry, live: insideLiveBundle(entry) }
}

/**
 * The `cf` the first line of the new shape names, as `sh` or cmd.exe read it: a
 * double-quoted path with the four characters `sh` reads for itself (`\`, `"`, `$`
 * and the backquote) behind a backslash, or one with `%` doubled; null when no
 * line has that form.
 */
export function nativeCf(text) {
  const sh = /^exec "((?:[^"\\]|\\[\\"$`])*)" "\$@"\r?$/m.exec(text)
  if (sh !== null) return sh[1].replace(/\\([\\"$`])/g, '$1')
  const cmd = /^"([^"]*)" %\*\r?$/m.exec(text)
  return cmd === null ? null : cmd[1].replaceAll('%%', '%')
}

/**
 * Whether a CLI entry sits inside a bundle with the installed release's
 * identity: `.../X.app/Contents/Resources/cli/bin/cf.mjs` →
 * `.../X.app/Contents/Info.plist`.
 */
function insideLiveBundle(entry) {
  const plist = join(dirname(dirname(dirname(dirname(entry)))), 'Info.plist')
  try {
    return /<key>CFBundleIdentifier<\/key>\s*<string>dev\.ngvoicu\.consensflow<\/string>/.test(
      readFileSync(plist, 'utf8'),
    )
  } catch {
    return false
  }
}
