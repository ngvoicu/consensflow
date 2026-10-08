/**
 * Plants in what the flip release left: the repair of the terminal's command (a
 * launcher of another home rewritten, one made where there was none), which the
 * tests of the launcher's repair and the app's catch. (The flip's own decider,
 * which implementation writes a home, went with Node, and so did the choice the
 * tests made in their own home; its plants are `deletion.mjs`'s now: that
 * nothing reads what it read.)
 */
import { APP, HOLDS, LAUNCHER, lines } from './kit.mjs'

const REPAIR = 'crates/cf-launcher/src/repair.rs'

/** The app's tests of the launcher it repairs. */
const app = (filter) => [...APP, filter]

export const PLANTS = [
  // The repair.
  {
    name: 'flip: the repair rewrites a launcher of another home',
    edits: [
      [
        REPAIR,
        lines(
          '    if !serves(&text, windows, env) {',
          '        return Repair::Elsewhere;',
          '    }',
        ),
        '',
      ],
    ],
    runs: [LAUNCHER],
    meant: 'the_live_app_repairs_its_command_and_leaves_the_candidates_byte_for_byte',
  },
  {
    name: 'flip: the repair rewrites a launcher of another home, seen by the app’s tests',
    edits: [
      [
        REPAIR,
        lines(
          '    if !serves(&text, windows, env) {',
          '        return Repair::Elsewhere;',
          '    }',
        ),
        '',
      ],
    ],
    runs: [app('launcher')],
    meant: 'a_command_pinned_to_another_home_is_left_byte_for_byte_in_both_directions',
  },
  {
    name: 'flip: the repair serves a launcher that pins no home to any home',
    edits: [
      [
        REPAIR,
        'None => default_root(env).is_some_and(|default| default.to_string_lossy() == home),',
        'None => true,',
      ],
    ],
    runs: [LAUNCHER],
    meant: 'a_command_that_pins_no_home_is_the_default_homes_and_the_candidate_leaves_it',
  },
  {
    name: 'flip: the repair makes a command where there was none',
    edits: [
      [
        REPAIR,
        lines('    if !file.exists() {', '        return Repair::Absent;', '    }'),
        lines(
          '    if !file.exists() {',
          '        let _ = std::fs::create_dir_all(file.parent().unwrap_or(file));',
          '        let _ = write_launcher(file, &launcher(windows, cf, None), windows);',
          '        return Repair::Rewritten;',
          '    }',
        ),
      ],
    ],
    runs: [HOLDS],
    meant: 'a_missing_command_is_never_made_and_nor_is_the_folder_it_would_be_in',
  },
  {
    name: 'flip: the repair makes a command where there was none, seen by the app’s tests',
    edits: [
      [
        REPAIR,
        lines('    if !file.exists() {', '        return Repair::Absent;', '    }'),
        lines(
          '    if !file.exists() {',
          '        let _ = std::fs::create_dir_all(file.parent().unwrap_or(file));',
          '        let _ = write_launcher(file, &launcher(windows, cf, None), windows);',
          '        return Repair::Rewritten;',
          '    }',
        ),
      ],
    ],
    runs: [app('launcher')],
    meant: 'no_command_is_created_where_there_is_none',
  },
]
