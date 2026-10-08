/**
 * What the tables of plants are written with. A plant is `name`, which says
 * what is wrong with the code once planted; `edits`, the text replaced, each in
 * one file where it is found exactly once; `runs`, the commands that should
 * fail, tried in order until one does; `meant`, the test that was written for
 * it (the driver says when another caught it).
 */

const node = (...args) => [process.execPath, ...args]
const test = (...files) => node('--test', ...files)

/** The rule of the feeds and its checks, against GitHub as the tests have it. */
export const FEEDS = test('tests/feeds.test.mjs')
/** The publisher, every state a run can be cut short in, and the workflow's steps run as written. */
export const PUBLISH = test('tests/publish.test.mjs', 'tests/release-publish.test.mjs')
/** The workflow's own text: who may publish, and the steps that call the scripts. */
export const WORKFLOW = test('tests/release-publish.test.mjs', 'tests/workflow-scripts.test.mjs')
/** The proof of the agents screens against the native daemon, built from the sources as they are. */
export const BUILT_PROOF = node('tests/agents-daemons.mjs', '--offline')
/** The app crate's tests of the portable app's collector, built as a worktree can build it. */
export const PORTABLE = node('tests/app-tests.mjs', 'portable::')

/** The updater smoke's readers of evidence (processes, daemon, ledger), and of the terminal command. */
export const EVIDENCE = test('tests/updater-smoke-evidence.test.mjs')
export const LAUNCHERS = test('tests/updater-smoke-launchers.test.mjs')
/** What the smoke is made of apart from the apps: its versions, keys, feed and bundles. */
export const SMOKE_KIT = test('tests/updater-smoke-kit.test.mjs')
/** The smoke on the apps it builds or takes, as `npm run smoke:updater` is. */
export const smoke = (...args) => node('tests/smoke-updater.mjs', ...args)

/** The test of a release run again after a later one went out: the feed it left alone must stay. */
export const RERUN = 'leaves feed-alpha at alpha.82 when alpha.81'

export const FEEDS_JS = 'app/scripts/feeds.mjs'
export const PUBLISH_JS = 'app/scripts/publish.mjs'
export const RELEASE_YML = '.github/workflows/release.yml'
export const PORTABLE_RS = 'app/src-tauri/src/portable.rs'
export const SMOKE_DIR = 'tests/updater-smoke'
