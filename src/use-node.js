import { statSync } from 'node:fs'
import { join } from 'node:path'
import { configRoot } from './roster.js'

/**
 * The way back to Node, for the flip release alone: which implementation
 * writes a home. The same decision as `choose` in `crates/cf-base/src/way_back.rs`,
 * for `bin/cf.mjs`, held to one answer with it by `crates/cf-base/tests/way-back.json`.
 *
 * The native `cf` and the native daemon are the default. A user whose home
 * trips on them makes a file named `use-node` in ConsensFlow's home, and while
 * it is there everything that writes that home runs on Node, the app's daemon
 * and every `cf` verb a terminal runs; deleting it returns to native. The
 * environment says nothing: a variable cannot be the way back, since the app
 * does not hand a terminal its own.
 *
 * What counts is the name being there. Its content is never read, and neither is
 * the file: whatever `stat` finds at the name, following links, is the user's
 * act (a file, an empty one, a folder, a file nobody may read), and a name
 * nothing can be found at is none (a link to nowhere, a home whose folder cannot
 * be searched). `statSync` and not `existsSync`, which on Windows answers
 * true for a link to nowhere, where Rust's `metadata` follows the link and does
 * not.
 *
 * Removed with Node, in the release that deletes it.
 */
export const FILE = 'use-node'

export function useNode(env) {
  try {
    return statSync(join(configRoot(env), FILE), { throwIfNoEntry: false }) !== undefined
  } catch {
    return false
  }
}
