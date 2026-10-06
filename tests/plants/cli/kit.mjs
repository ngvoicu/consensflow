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

/** The native `cf` of bin/, built from the sources as they are: what the Node suites below run. */
export const BUILD = [process.execPath, 'app/scripts/build-cf.mjs', '--offline']
/** The way back to Node as a process: `cf` finds the Node beside it, and sends every tokenless command there. */
export const WAY_BACK = cargo('-p', 'cf', '--test', 'way_back')
/** `cf ui` as a process: the daemon the home chooses, and a window's token the board. */
export const UI = cargo('-p', 'cf', '--test', 'daemon_stop')
/** The Rust decider and the table it shares with the Node one. */
export const TABLE = cargo('-p', 'cf-base', '--test', 'way_back')
/** The Node decider, held to the same table. */
export const NODE_TABLE = [process.execPath, '--test', 'tests/way-back.test.mjs']
/** The launcher's repair, which commands it rewrites and which it leaves. */
export const LAUNCHER = cargo('-p', 'cf-launcher')
/** What the repair leaves as it is, and says (cargo stops at the first test binary that fails, so the whole crate may not reach it). */
export const HOLDS = cargo('-p', 'cf-launcher', '--test', 'repair_holds')
/** The app crate's tests, which `npm run test:app` runs where the app cannot be built as it ships. */
export const APP = [process.execPath, 'tests/app-tests.mjs']
/** `bin/cf.mjs`, the door: forwards to the native `cf`, or runs Node's CLI for a home with the file. */
export const DOOR = [process.execPath, '--test', 'tests/cf-door.test.mjs']
/** One writer for a home: which implementation wrote, by what ran. */
export const WRITER = [process.execPath, '--test', 'tests/one-writer.test.mjs']
/** What the tests choose in their own home (`chooseHome`, `daemonCommand`). */
export const CHOICE = [process.execPath, '--test', 'tests/choice.test.mjs']
