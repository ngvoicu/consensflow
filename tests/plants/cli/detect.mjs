/**
 * Plants in Node's detection of a harness (`src/harnesses.js`), the oracle the
 * Rust one is held to: on Windows, npm's global folder, `%APPDATA%\npm`, after
 * PATH, the harness's own places and the common ones, and nowhere off Windows.
 * Its tests, and the recording of detection that the Rust one replays
 * (`tests/admin-goldens.test.mjs`), must catch each. Every name here begins
 * `detection in Node:`, so `npm run plants:cli -- detection` runs them; the
 * Rust ones are `npm run plants:daemon -- detect`.
 */
import { lines } from './kit.mjs'

const HARNESSES = 'src/harnesses.js'
/** Node's tests of where a harness is found, and the recording held to what Node answers now. */
const DETECTION = [
  process.execPath,
  '--test',
  'tests/harnesses.test.mjs',
  'tests/admin-goldens.test.mjs',
]
const LOCATION = "(onWindows(env) && env.APPDATA ? join(env.APPDATA, 'npm') : null)"
const COMMON = lines("  HOMED(['.volta', 'bin']),", '  NPM_GLOBAL,', ']')

const plant = (name, edits, meant) => ({
  name: `detection in Node: ${name}`,
  edits: edits.map(([from, to]) => [HARNESSES, from, to]),
  runs: [DETECTION],
  meant,
})

export const PLANTS = [
  plant(
    "npm's folder is not looked in",
    [[COMMON, lines("  HOMED(['.volta', 'bin']),", ']')]],
    'is where a harness is found when PATH lacks it',
  ),
  plant(
    "npm's folder is looked in before the common places",
    [
      [COMMON, lines("  HOMED(['.volta', 'bin']),", ']')],
      ['const COMMON = [\n', 'const COMMON = [\n  NPM_GLOBAL,\n'],
    ],
    "comes after PATH, the harness's own places and the other common ones",
  ),
  plant(
    "npm's folder is looked in off Windows too",
    [['(onWindows(env) && env.APPDATA ?', '(env.APPDATA ?']],
    'changes nothing off Windows',
  ),
  plant(
    "a missing APPDATA is Windows' own place for it under the home",
    [
      [
        LOCATION,
        "(onWindows(env) ? join(env.APPDATA || join(home(env), 'AppData', 'Roaming'), 'npm') : null)",
      ],
    ],
    'adds nothing when APPDATA is missing or empty',
  ),
  plant(
    'an empty APPDATA names a folder',
    [['(onWindows(env) && env.APPDATA ?', '(onWindows(env) && env.APPDATA !== undefined ?']],
    'adds nothing when APPDATA is missing or empty',
  ),
]
