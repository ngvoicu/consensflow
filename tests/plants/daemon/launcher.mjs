/**
 * The terminal command's plants (`crates/cf-launcher`), with those of what
 * `crates/cf-harness` does with it (`prepare.mjs`): each bug is one a test was
 * written for, and the plant says which. They are the proof that those tests
 * hold what they claim, and run with the rest: `npm run plants:daemon --
 * launcher` (every name here begins with it; the harness's begin `stale` and
 * `prepare`).
 *
 * What no plant can show is what only a system that is not this one reads: the
 * `.cmd` run by cmd.exe (`crates/cf-launcher/tests/run.rs` runs it on Windows),
 * and libuv's attribute test of a writable folder there (`places.rs`).
 */
import { lines } from './kit.mjs'
import { PLANTS as PREPARE } from './prepare.mjs'

const LAUNCHER = 'crates/cf-launcher/src'
const launcher = (...args) => ['-p', 'cf-launcher', ...args]
const UNITS = launcher('--lib')
const TERMINAL = launcher('--test', 'terminal')
const REPAIR = launcher('--test', 'repair')
const HOLDS = launcher('--test', 'repair_holds')
const GOLDENS = launcher('--test', 'goldens')
const RUN = launcher('--test', 'run')
// The one test of what opening the app prepares that a folder's writability breaks.
const INSTALL = ['-p', 'cf-harness', '--test', 'install']

/** Each bug: what it is, the text it replaces in a file, who must notice, and with which test. */
const COMMAND = [
  {
    name: 'launcher: a launcher for cmd ends its lines in LF',
    edits: [[`${LAUNCHER}/text.rs`, '{pin}\\"{}\\" %*\\r\\n",', '{pin}\\"{}\\" %*\\n",']],
    runs: [UNITS, GOLDENS],
    meant:
      'the_launcher_for_cmd_ends_its_lines_in_cr_lf_and_forwards_the_arguments_with_percent_star',
  },
  {
    name: 'launcher: a launcher for sh forwards no arguments',
    edits: [[`${LAUNCHER}/text.rs`, '{pin}exec \\"{}\\" \\"$@\\"\\n",', '{pin}exec \\"{}\\"\\n",']],
    runs: [UNITS, RUN, GOLDENS],
    meant: 'the_launcher_for_sh_runs_the_cf_it_names_with_every_argument',
  },
  {
    name: 'launcher: the mark is spelled in another case',
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        'pub(crate) const MARKER: &str = "Installed by ConsensFlow";',
        'pub(crate) const MARKER: &str = "Installed by consensflow";',
      ],
    ],
    runs: [UNITS, REPAIR],
    meant: 'the_mark_is_what_makes_a_command_ours',
  },
  {
    name: 'launcher: a program that holds the mark in its bytes is a launcher of ours',
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        "text.contains(MARKER) && !text.contains('\\0')",
        'text.contains(MARKER)',
      ],
    ],
    runs: [UNITS, REPAIR],
    meant: 'a_program_that_merely_holds_the_mark_in_its_bytes_is_no_launcher_of_ours',
  },
  {
    name: 'launcher: a dollar sign in a path is left for the shell to read',
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        "matches!(character, '\\\\' | '\"' | '$' | '`')",
        "matches!(character, '\\\\' | '\"' | '`')",
      ],
    ],
    runs: [UNITS, RUN],
    meant: 'what_sh_reads_for_itself_is_escaped_in_a_path_and_nothing_else_is',
  },
  {
    name: 'launcher: a percent sign in a cmd path is not doubled',
    edits: [[`${LAUNCHER}/text.rs`, 'text.replace(\'%\', "%%")', 'text.replace(\'%\', "%")']],
    runs: [UNITS],
    meant: 'a_percent_sign_is_doubled_in_a_cmd_path_and_nothing_else_is',
  },
  {
    name: "launcher: the old shape's white space is Rust's, not JavaScript's",
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        'space.chars().all(js::is_space)',
        'space.chars().all(char::is_whitespace)',
      ],
    ],
    runs: [UNITS, GOLDENS],
    meant: 'the_old_shape_is_the_first_quoted_pair_that_the_pattern_matches_and_no_other',
  },
  {
    name: 'launcher: the old shape need not name a cf.mjs',
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        lines('            && entry.len() > ENTRY.len()', '            && entry.ends_with(ENTRY);'),
        '            && entry.len() > ENTRY.len();',
      ],
    ],
    runs: [UNITS, GOLDENS],
    meant: 'the_old_shape_is_the_first_quoted_pair_that_the_pattern_matches_and_no_other',
  },
  {
    name: 'launcher: the old shape is the last quoted pair that fits, not the first',
    edits: [
      [`${LAUNCHER}/text.rs`, 'quotes.windows(4).find_map(', 'quotes.windows(4).rev().find_map('],
    ],
    runs: [UNITS, GOLDENS],
    meant: 'the_old_shape_is_the_first_quoted_pair_that_the_pattern_matches_and_no_other',
  },
  {
    name: 'launcher: a verbatim path is written as it is',
    edits: [
      [
        `${LAUNCHER}/text.rs`,
        '    if !windows {\n        return text.into_owned();',
        '    if true {\n        return text.into_owned();',
      ],
    ],
    runs: [UNITS, REPAIR],
    meant: 'a_path_is_spelled_plain_for_windows_and_as_it_is_for_anything_else',
  },
  {
    name: 'launcher: a repair asks for the spelling it was given',
    edits: [
      [`${LAUNCHER}/repair.rs`, 'let cf = spelled(cf, windows);', 'let cf = cf.to_string_lossy();'],
    ],
    runs: [REPAIR],
    meant: 'a_cf_named_in_the_verbatim_spelling_is_written_and_known_in_the_plain_one',
  },
  {
    name: 'launcher: a command is known by the spelling it was given',
    edits: [
      [
        `${LAUNCHER}/wiring.rs`,
        'mine: named == spelled(cf, windows),',
        'mine: named == cf.to_string_lossy(),',
      ],
    ],
    runs: [REPAIR],
    meant: 'a_cf_named_in_the_verbatim_spelling_is_written_and_known_in_the_plain_one',
  },
  {
    name: 'launcher: a repair drops the home a command pinned',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        'let script = launcher(windows, cf, pinned_home(&text, windows).as_deref());',
        'let script = launcher(windows, cf, None);',
      ],
    ],
    runs: [REPAIR],
    meant: 'an_old_command_keeps_the_home_it_pinned',
  },
  {
    name: 'launcher: a repair pins a home the command did not',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        'let script = launcher(windows, cf, pinned_home(&text, windows).as_deref());',
        'let script = launcher(windows, cf, Some("/another/home"));',
      ],
    ],
    runs: [REPAIR],
    meant: 'an_old_command_is_rewritten_in_the_new_shape_by_both_its_names',
  },
  {
    name: 'launcher: a repair rewrites a command that already runs this cf',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        lines(
          '    if matches!(runs(&text, windows), Some(Runs::Native { cf: named }) if named == cf) {',
          '        return Repair::Current;',
          '    }',
        ),
        '',
      ],
    ],
    runs: [HOLDS],
    meant: 'a_repair_run_twice_writes_nothing_the_second_time',
  },
  {
    name: 'launcher: a repair points a command at a place macOS takes away',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        lines('    if is_translocated(cf) {', '        return Repair::Transient;', '    }'),
        '',
      ],
    ],
    runs: [HOLDS],
    meant: 'an_app_opened_from_where_it_was_downloaded_leaves_the_command_as_it_is',
  },
  {
    name: 'launcher: a repair takes a command that runs another cf for this ones',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        'Some(Runs::Native { cf: named }) if named == cf)',
        'Some(Runs::Native { cf: _named }) if true)',
      ],
    ],
    runs: [REPAIR],
    meant: 'a_command_that_runs_another_bundles_cf_is_rewritten_to_this_one',
  },
  {
    name: 'launcher: a repair rewrites a command that is not ours',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        lines('    if !is_ours(&text) {', '        return Repair::Unmarked;', '    }'),
        '',
      ],
    ],
    runs: [HOLDS],
    meant: 'an_unmarked_command_is_left_alone_byte_for_byte_and_so_is_its_time',
  },
  {
    name: 'launcher: a repair makes a command where there was none',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        lines('    if !file.exists() {', '        return Repair::Absent;', '    }'),
        lines(
          '    if !file.exists() {',
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
    name: 'launcher: a repair looks at the first name only',
    edits: [
      [
        `${LAUNCHER}/repair.rs`,
        'for name in names(windows) {',
        'for name in names(windows).into_iter().take(1) {',
      ],
    ],
    runs: [HOLDS],
    meant: 'a_command_that_is_there_under_one_name_is_not_made_under_the_other',
  },
  {
    name: 'launcher: a status is asked of the second name',
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        'let name = names(env.on_windows())[0];',
        'let name = names(env.on_windows())[1];',
      ],
    ],
    runs: [TERMINAL, GOLDENS],
    meant: 'a_command_by_the_short_name_alone_is_not_the_command_that_is_reported',
  },
  {
    name: 'launcher: a folder is on PATH when a folder on it begins like it',
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        '.any(|at| at == dir_text)',
        '.any(|at| at.starts_with(&dir_text))',
      ],
    ],
    runs: [GOLDENS],
    meant: 'every_install_of_the_form_of_cmd_leaves_what_node_left_and_says_what_it_said',
  },
  {
    name: "launcher: an install replaces someone else's command",
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        'if file.exists() && !is_ours(&read_text(&file)?) {',
        'if file.exists() && false {',
      ],
    ],
    runs: [GOLDENS],
    meant: 'every_install_of_the_form_of_cmd_leaves_what_node_left_and_says_what_it_said',
  },
  {
    name: 'launcher: an install makes a command that cannot be run',
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        'std::fs::Permissions::from_mode(0o755)',
        'std::fs::Permissions::from_mode(0o644)',
      ],
    ],
    runs: [TERMINAL, GOLDENS],
    meant: 'writes_a_launcher_that_runs_this_very_copy',
  },
  {
    name: 'launcher: the last folder that can be written takes the command',
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        'folders.iter().find(|folder| writable(folder))',
        'folders.iter().rfind(|folder| writable(folder))',
      ],
    ],
    runs: [TERMINAL],
    meant: 'the_first_candidate_that_can_be_written_takes_the_command_and_a_missing_one_is_made',
  },
  {
    name: 'launcher: the refusal is worded another way',
    edits: [
      [
        `${LAUNCHER}/install.rs`,
        '— create one, or add it yourself",',
        '- create one, or add it yourself",',
      ],
    ],
    runs: [TERMINAL, GOLDENS],
    meant: 'explains_itself_when_no_candidate_directory_can_be_written',
  },
  {
    name: 'launcher: a project bin folder takes the command',
    edits: [
      [
        `${LAUNCHER}/places.rs`,
        'let root = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;',
        lines(
          'if let Some(project) = env.path("CONSENSFLOW_BIN_DIR") {',
          '            return Ok(vec![project.to_path_buf()]);',
          '        }',
          '        let root = config_root(env).ok_or_else(|| "missing home in env".to_owned())?;',
        ),
      ],
    ],
    runs: [UNITS, TERMINAL],
    meant: 'by_default_the_only_place_is_the_bin_of_the_home_whatever_a_project_says',
  },
  {
    name: 'launcher: a folder under /opt is made',
    edits: [
      [
        `${LAUNCHER}/places.rs`,
        'folder.starts_with("/usr") || folder.starts_with("/opt")',
        'folder.starts_with("/usr")',
      ],
    ],
    runs: [UNITS],
    meant: 'a_system_folder_is_one_that_begins_with_usr_or_opt_and_nothing_else_is',
  },
  {
    name: 'launcher: a folder is writable only when it is a folder',
    edits: [
      [
        `${LAUNCHER}/places.rs`,
        'access(dir, AccessFlags::W_OK).is_ok()',
        'dir.is_dir() && access(dir, AccessFlags::W_OK).is_ok()',
      ],
    ],
    runs: [UNITS, GOLDENS, INSTALL],
    meant: 'a_file_is_as_writable_as_the_system_says_so_that_the_write_is_where_it_fails',
  },
  {
    name: 'launcher: a native command is never this copys',
    edits: [[`${LAUNCHER}/wiring.rs`, 'mine: named == spelled(cf, windows),', 'mine: false,']],
    runs: [TERMINAL],
    meant: 'says_whether_the_command_runs_this_copy_or_another_consensflow',
  },
  {
    name: 'launcher: an old command is never this copys',
    edits: [
      [
        `${LAUNCHER}/wiring.rs`,
        'mine: entry == spelled(&cf.with_file_name("cf.mjs"), windows),',
        'mine: false,',
      ],
    ],
    runs: [TERMINAL],
    meant: 'a_command_that_runs_the_cf_mjs_beside_this_cf_is_this_copys_though_it_is_the_old_shape',
  },
  {
    name: 'launcher: the bundle of an entry is looked for a level too high',
    edits: [[`${LAUNCHER}/wiring.rs`, '(0..4).fold(', '(0..3).fold(']],
    runs: [TERMINAL],
    meant:
      'says_when_the_command_runs_the_installed_release_which_development_must_never_write_into',
  },
  {
    name: 'launcher: a program that is not there is not said first',
    edits: [[`${LAUNCHER}/wiring.rs`, 'if !self.exists {', 'if !self.exists && !self.mine {']],
    runs: [UNITS],
    meant: 'the_old_shape_is_reported_in_node_s_three_sentences',
  },
]

/** The command's own, and what `crates/cf-harness` does with it. */
export const PLANTS = [...COMMAND, ...PREPARE]
