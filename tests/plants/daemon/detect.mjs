/**
 * Where `crates/cf-harness` finds a harness's CLI (`detect.rs`): on Windows,
 * npm's global folder, `%APPDATA%\npm`, after PATH, the harness's own places
 * and the common ones, and nowhere off Windows. Every name here begins
 * `detect:`, so `npm run plants:daemon -- detect` runs them.
 */
import { lines } from './kit.mjs'

const DETECT = 'crates/cf-harness/src/detect.rs'
const UNIT = ['-p', 'cf-harness', '--lib', 'detect::']
/** Node's recordings of detection, played (`crates/cf-harness/tests/goldens/admin`). */
const GOLDENS = ['-p', 'cf-harness', '--test', 'admin']
const FOLDERS = '        let folders = homed(harness, env).into_iter().chain(npm_global(env));'

/** Each bug: what it is, the text it replaces in a file, who must notice, and with which test. */
export const PLANTS = [
  {
    name: "detect: npm's folder is not looked in",
    edits: [[DETECT, FOLDERS, '        let folders = homed(harness, env).into_iter();']],
    runs: [UNIT, GOLDENS],
    meant: 'a_cli_is_found_in_npm_s_folder_when_path_lacks_it_as_the_cmd_windows_starts',
  },
  {
    name: "detect: npm's folder is looked in before the common places",
    edits: [
      [
        DETECT,
        FOLDERS,
        '        let folders = npm_global(env).into_iter().chain(homed(harness, env));',
      ],
    ],
    runs: [UNIT, GOLDENS],
    meant: 'it_comes_after_path_the_harness_s_own_places_and_the_other_common_ones',
  },
  {
    name: "detect: npm's folder is looked in only where there is a home",
    edits: [[DETECT, FOLDERS, lines('        home(env).ok()?;', FOLDERS)]],
    runs: [UNIT],
    meant: 'it_needs_no_home',
  },
  {
    name: "detect: npm's folder is looked in off Windows too",
    edits: [
      [
        DETECT,
        lines(
          '    if !env.on_windows() {',
          '        return None;',
          '    }',
          '    Some(PathBuf::from(set(env, "APPDATA")?).join("npm"))',
        ),
        '    Some(PathBuf::from(set(env, "APPDATA")?).join("npm"))',
      ],
    ],
    runs: [UNIT, GOLDENS],
    meant: 'nothing_changes_off_windows',
  },
  {
    name: 'detect: an empty APPDATA names a folder',
    edits: [
      [
        DETECT,
        '    Some(PathBuf::from(set(env, "APPDATA")?).join("npm"))',
        '    Some(PathBuf::from(env.os("APPDATA")?).join("npm"))',
      ],
    ],
    runs: [UNIT],
    meant: 'an_appdata_that_is_missing_or_empty_adds_nothing',
  },
]
