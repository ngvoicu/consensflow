/**
 * What the tables of plants are written with. A plant is `name`, which says
 * what is wrong with the code once planted; `edits`, the text replaced, each
 * in one file where it is found exactly once; `runs`, the commands that should
 * fail, tried in order until one does; `meant`, the test that was written for
 * it (the driver says when another caught it). `lines` is the daemon's.
 */
export { lines } from '../daemon/kit.mjs'

/** A `cargo test` of the workspace's crates. */
export const cargo = (...args) => ['cargo', 'test', '--offline', ...args]

/** The recording replayed against the native `cf`, case by case. */
export const GOLDENS = cargo('-p', 'cf', '--test', 'cli_goldens')
/** The parser held to Node's `parseArgs` on three thousand lists of words. */
export const ARGS = cargo('-p', 'cf-base', '--test', 'args')
/** Which words the standalone module answers. */
export const UNITS = cargo('-p', 'cf', '--lib', 'standalone')
/** Which `cf` answers, as a process. */
export const PROCESS = cargo('-p', 'cf', '--test', 'standalone')
/** The checked-in recording held to what Node answers now. */
export const HELD = [process.execPath, '--test', 'tests/cli-goldens.test.mjs']
/** The suites of the CLI against Node's `cf.mjs` and then the native `cf`. */
export const BOTH = [process.execPath, 'tests/clis.mjs', '--offline']

export const STANDALONE = 'crates/cf/src/standalone'
