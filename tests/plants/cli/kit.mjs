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
/** The library as a caller other than `main` has it: what no process can bring to it. */
export const LIBRARY = cargo('-p', 'cf', '--test', 'dispatch')
/** The suites of the CLI against the native `cf`, built from the sources as they are. */
export const CLIS = ['cargo', 'xtask', 'test', 'clis', '--offline']

export const STANDALONE = 'crates/cf/src/standalone'

/** The native `cf` of bin/, built from the sources as they are: what the test scripts below run. */
export const BUILD = ['cargo', 'xtask', 'build-cf', '--offline']
/** `cf ui` as a process: the daemon, in a home that has a leftover `use-node` file or none, and a window's token the board. */
export const UI = cargo('-p', 'cf', '--test', 'daemon_stop')
/** How a program starts here, an npm shim with no Node to run on among them. */
export const RUNNABLE = cargo('-p', 'cf-process', '--lib', 'runnable')
/** An npm shim found in npm's folder on Windows, started as one on PATH is. */
export const NPM_SHIMS = cargo('-p', 'cf-harness', '--lib', 'detect')
/** What a window starts with: no Node named to it. */
export const SEAMS = cargo('-p', 'cf-daemon', '--lib', 'seams')
/** A Codex window's supervisor, which refuses a Codex it has no Node to run. */
export const CODEX_SESSION = cargo('-p', 'cf-codex-session', '--lib', 'supervisor')
/** The stand-in the tests give a window on Windows, written and opened on any system. */
export const STAND_IN = cargo('-p', 'cf-harness', '--lib', 'testing::')
/** The daemon's host: a window opens on an npm shim with a Node to run on, and is refused without. */
export const HOST = cargo('-p', 'cf-daemon', '--lib', 'host::')
/** The page's console text, held to the table the Rust one is held to. */
export const CONSOLE_TEXT = [process.execPath, '--test', 'tests/console-text.test.mjs']
/** The release's check of the bundle it publishes (`cf-release prepare-update`, run on bundles and archives made in the test). */
export const UPDATE_RELEASE = cargo('-p', 'cf-release', '--test', 'prepare_update')
/** The portable exe's packing. */
export const PORTABLE_PACK = [process.execPath, '--test', 'tests/portable.test.mjs']
/** The Developer ID signing of the app (`cf-release sign-mac`): every Mach-O hardened, none entitled. */
export const SIGN_MAC = cargo('-p', 'cf-release', '--lib', 'sign_mac::')
/** A verb of the native `cf` run outside a window, given its arguments whole (the build of bin/ first). */
export const CF_NATIVE = [process.execPath, '--test', 'tests/integration/cf-native.test.mjs']
/** The launcher's repair, which commands it rewrites and which it leaves. */
export const LAUNCHER = cargo('-p', 'cf-launcher')
/** What the repair leaves as it is, and says (cargo stops at the first test binary that fails, so the whole crate may not reach it). */
export const HOLDS = cargo('-p', 'cf-launcher', '--test', 'repair_holds')
/** The app crate's tests, which `npm run test:app` runs where the app cannot be built as it ships. */
export const APP = ['cargo', 'xtask', 'app', 'test']
