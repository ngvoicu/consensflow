/**
 * Which ConsensFlow a built app is, read off its file without starting it, and the one a test
 * may start: the smoke's identity alone. The app writes the identity its build was made with into
 * its own file (`BUILT_IDENTITY`, app/src-tauri/src/lib.rs), which a Windows exe says nowhere
 * else. An app of the release's identity keeps its runtime and its data in the release's folders
 * of the machine's user (`%LOCALAPPDATA%\dev.ngvoicu.consensflow`), which Tauri finds from
 * Windows itself whatever a test sets `LOCALAPPDATA` to: started by a test on a machine someone
 * uses, it writes into that someone's ConsensFlow (T-930 on zeewin2, 2026-10-29, T-951). An exe
 * that says no identity is one built before it did, the shipped ones among them: it may be the
 * release's, and it is not started either.
 */
import { readFileSync } from 'node:fs'

/** The identity of every app a smoke builds and starts. */
export const SMOKE = 'dev.ngvoicu.consensflow.smoke'

/** The words the identity follows in the app's file, and the one it ends with. */
const MARK = 'consensflow-built-identity:'
const END = ';'

/** The identity `bytes` (an app's file) says it was built with, or null where it says none. */
export function builtIdentityIn(bytes) {
  const mark = Buffer.from(MARK)
  const said = new Set()
  for (let at = bytes.indexOf(mark); at !== -1; at = bytes.indexOf(mark, at + 1)) {
    const start = at + mark.length
    const end = bytes.indexOf(END, start)
    // An identity is a short run of a reverse domain's letters; anything else is no identity.
    const identity = end === -1 ? '' : bytes.subarray(start, end).toString('latin1')
    if (!/^[a-z0-9.-]{1,200}$/.test(identity)) return null
    said.add(identity)
  }
  return said.size === 1 ? [...said][0] : null
}

/** Why the app at `exe` is not started by a test, or null where it may be. */
export function refusalOf(exe) {
  const identity = builtIdentityOf(exe)
  if (identity === SMOKE) return null
  const is = identity === null ? 'says no identity, and may be the release' : `is ${identity}`
  return (
    `${exe} is not started: a test starts only an app of the smoke identity (${SMOKE}), and it ${is}. ` +
    'An app of the release keeps its runtime and data in the release folders of the machine user, ' +
    'whatever LOCALAPPDATA says: build one with the smoke identity ' +
    '(node app/scripts/windows-smoke-desktop.mjs --build)'
  )
}

/**
 * What `tauri build --config` is given for an app a smoke builds: the smoke's identity, and
 * with `appAlone` no step before the build (a JSON merge patch's null takes it away).
 */
export function smokeBuildConfig({ appAlone = false } = {}) {
  return appAlone ? { identifier: SMOKE, build: { beforeBuildCommand: null } } : { identifier: SMOKE }
}

/** Reads the file at `exe` for its identity; null where it cannot be read. */
export function builtIdentityOf(exe) {
  try {
    return builtIdentityIn(readFileSync(exe))
  } catch {
    return null
  }
}
