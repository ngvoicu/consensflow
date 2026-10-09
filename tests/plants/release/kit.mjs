/**
 * What the tables of plants are written with. A plant is `name`, which says
 * what is wrong with the code once planted; `edits`, the text replaced, each in
 * one file where it is found exactly once; `runs`, the commands that should
 * fail, tried in order until one does; `meant`, the test that was written for
 * it (the driver says when another caught it).
 */

const node = (...args) => [process.execPath, ...args]
const test = (...files) => node('--test', ...files)

/**
 * The tests of the publisher's crate, whose test binaries are named: every one
 * runs, so that the test a plant was meant for is named among the failures.
 */
const publisher = (...binaries) => [
  ...['cargo', 'test', '--offline', '--no-fail-fast', '-p', 'cf-publish'],
  ...binaries.flatMap((name) => ['--test', name]),
]

/** The rule of the feeds and its checks, against GitHub as the simulator has it. */
export const FEEDS = publisher(
  'feeds_rule',
  'feeds_plan',
  'feeds_prerequisites',
  'feeds_check',
  'feeds_cli',
)
/** The publisher, every state a run can be cut short in, and the workflow's steps run as written. */
export const PUBLISH = publisher(
  'publish_release',
  'publish_rerun',
  'publish_cut_short',
  'publish_swap',
  'release_publish',
  'release_steps',
  'release_mac',
)
/** The workflow's own text: who may publish, what holds the token, and the steps that call the publisher. */
export const WORKFLOW = publisher(
  'release_publish',
  'release_steps',
  'release_mac',
  'workflow_scripts',
)
/** The proof of the agents screens against the native daemon, built from the sources as they are. */
export const BUILT_PROOF = ['cargo', 'xtask', 'test', 'agents', '--offline']
/** The app crate's tests of the portable app's collector, built as a worktree can build it. */
export const PORTABLE = ['cargo', 'xtask', 'app', 'test', 'portable::']

/** The updater smoke's readers of evidence (processes, daemon, ledger), and of the terminal command. */
export const EVIDENCE = test('tests/updater-smoke-evidence.test.mjs')
export const LAUNCHERS = test('tests/updater-smoke-launchers.test.mjs')
/** What the smoke is made of apart from the apps: its versions, keys, feed and bundles. */
export const SMOKE_KIT = test('tests/updater-smoke-kit.test.mjs')
/** The smoke on the apps it builds or takes, as `npm run smoke:updater` is. */
export const smoke = (...args) => ['cargo', 'xtask', 'smoke-updater', ...args]

/** The test of a release run again after a later one went out: the feed it left alone must stay. */
export const RERUN = 'leaves_feed_alpha_at_alpha_82_when_alpha_81'

/** The publisher's sources (tools/cf-publish/src): the rule, its checks, the moving, the command line. */
const SRC = 'tools/cf-publish/src'
export const VERSION_RS = `${SRC}/version.rs`
export const RULE_RS = `${SRC}/rule.rs`
export const ASSETS_RS = `${SRC}/assets.rs`
export const READ_RS = `${SRC}/read.rs`
export const CHECKS_RS = `${SRC}/checks.rs`
export const GH_RS = `${SRC}/gh.rs`
export const PUBLISH_RS = `${SRC}/publish.rs`
export const FEED_RS = `${SRC}/publish/feed.rs`
export const CLI_RS = `${SRC}/cli.rs`
export const RELEASE_YML = '.github/workflows/release.yml'
export const PORTABLE_RS = 'app/src-tauri/src/portable.rs'
export const SMOKE_DIR = 'tests/updater-smoke'
