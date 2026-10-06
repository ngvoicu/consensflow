/**
 * Plants in the flip: which implementation writes a home. The home's file
 * (`use-node`) is read by every reader and nothing in the environment is: a
 * file one of them ignores, the old `CONSENSFLOW_DAEMON` one of them reads
 * again, a Node that the native `cf` asks the environment for, a `cf.mjs` that
 * runs a verb itself without the file, and a repair that rewrites a launcher of
 * another home or makes one where there was none. The tests of each reader and
 * the table the two deciders share catch them, and the suites that run what
 * the readers started (the door, one writer) catch a few the readers' own
 * tests would, run on the native `cf` built from the planted sources.
 */
import {
  APP,
  BUILD,
  CHOICE,
  DOOR,
  HOLDS,
  LAUNCHER,
  lines,
  NODE_TABLE,
  TABLE,
  UI,
  WAY_BACK,
  WRITER,
} from './kit.mjs'

const LIB = 'crates/cf/src/lib.rs'
const NODE = 'crates/cf/src/node.rs'
const DECIDER = 'crates/cf-base/src/way_back.rs'
const NODE_DECIDER = 'src/use-node.js'
const REPAIR = 'crates/cf-launcher/src/repair.rs'
const COMMAND = 'app/src-tauri/src/daemon_command.rs'
const DOOR_FILE = 'bin/cf.mjs'
const CHOICE_FILE = 'tests/choice.mjs'

/** The app's tests of what it starts, or of the launcher it repairs. */
const app = (filter) => [...APP, filter]

/** What the native `cf` of the first reader does with the file: `ui` is the daemon only without it. */
const UI_ASKS = ' && !way_back::choose(env).node;'
/** What it does with a verb when the home has the file: Node's. */
const VERBS_ASK = lines(
  '            if choice.node {',
  '                return node::run(args, &choice, err);',
  '            }',
)
/** What the app does: the daemon is Node's when the home has the file. */
const APP_ASKS = '(command_for(node, cli, choice.node), choice)'
/** What the door does: a verb is forwarded unless the file is there. */
const DOOR_ASKS = '!(useNode(process.env) || NODE_ONLY.has(command))'
/** What the decider does: the file is there when `stat` finds the name. */
const DECIDER_FINDS = 'let node = file.as_deref().is_some_and(Path::exists);'
/** What `native` asks of Node's place. */
const FIND_NODE = lines(
  '    let tried = candidates(cf, cfg!(windows));',
  '    tried',
  '        .iter()',
  '        .find(|node| node.is_file())',
  '        .cloned()',
  '        .ok_or(tried)',
)

export const PLANTS = [
  // The file, ignored by one reader.
  {
    name: 'flip: the native cf’s ui is the daemon though the home has the file',
    edits: [[LIB, UI_ASKS, ';']],
    runs: [[...UI, 'with_the_way_back']],
    meant: 'with_the_way_back_cf_ui_is_not_the_native_daemon_whatever_the_old_switch_says',
  },
  {
    name: 'flip: the native cf answers a verb itself though the home has the file',
    edits: [[LIB, VERBS_ASK, '']],
    runs: [WAY_BACK],
    meant: 'with_the_file_every_tokenless_verb_runs_on_the_node_beside_cf_with_none_named',
  },
  {
    name: 'flip: the app starts the native daemon though the home has the file',
    edits: [[COMMAND, APP_ASKS, '(command_for(node, cli, false), choice)']],
    runs: [app('daemon_command')],
    meant: 'the_daemon_is_node_running_cf_mjs_when_the_home_has_the_file',
  },
  {
    name: 'flip: cf.mjs forwards a verb though the home has the file',
    edits: [[DOOR_FILE, DOOR_ASKS, '!NODE_ONLY.has(command)']],
    runs: [DOOR],
    meant: 'runs each verb on Node’s own CLI, in its process, when the home has taken the way back',
  },
  {
    name: 'flip: the Rust decider never finds the file',
    edits: [[DECIDER, DECIDER_FINDS, 'let node = false;']],
    runs: [TABLE],
    meant: 'every_case_has_one_answer_whatever_the_environment_adds',
  },
  {
    name: 'flip: the Node decider never finds the file',
    edits: [
      [
        NODE_DECIDER,
        'return statSync(join(configRoot(env), FILE), { throwIfNoEntry: false }) !== undefined',
        'return false',
      ],
    ],
    runs: [NODE_TABLE],
    meant: 'has one answer for every case of the table, whatever the environment adds',
  },
  {
    name: 'flip: the Rust decider counts a folder for nothing',
    edits: [[DECIDER, DECIDER_FINDS, 'let node = file.as_deref().is_some_and(Path::is_file);']],
    runs: [TABLE],
    meant: 'every_case_has_one_answer_whatever_the_environment_adds',
  },
  {
    name: 'flip: the Node decider counts a folder for nothing',
    edits: [
      [
        NODE_DECIDER,
        'return statSync(join(configRoot(env), FILE), { throwIfNoEntry: false }) !== undefined',
        'return statSync(join(configRoot(env), FILE), { throwIfNoEntry: false })?.isFile() === true',
      ],
    ],
    runs: [NODE_TABLE],
    meant: 'has one answer for every case of the table, whatever the environment adds',
  },
  {
    name: 'flip: the Rust decider counts a file nobody may read for nothing',
    edits: [
      [
        DECIDER,
        DECIDER_FINDS,
        'let node = file.as_deref().is_some_and(|file| std::fs::File::open(file).is_ok());',
      ],
    ],
    runs: [TABLE],
    meant: 'every_case_has_one_answer_whatever_the_environment_adds',
  },

  // The old CONSENSFLOW_DAEMON, read again by one reader.
  {
    name: 'flip: the app reads the old switch for Node',
    edits: [
      [
        COMMAND,
        APP_ASKS,
        '(command_for(node, cli, choice.node || env.text("CONSENSFLOW_DAEMON") == Some("node")), choice)',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_environment_says_nothing_of_which_daemon_it_is',
  },
  {
    name: 'flip: the app reads the old switch for native',
    edits: [
      [
        COMMAND,
        APP_ASKS,
        '(command_for(node, cli, choice.node && env.text("CONSENSFLOW_DAEMON") != Some("native")), choice)',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_environment_says_nothing_of_which_daemon_it_is',
  },
  {
    name: 'flip: the native cf’s ui reads the old switch',
    edits: [
      [
        LIB,
        UI_ASKS,
        ' && !way_back::choose(env).node && env.text("CONSENSFLOW_DAEMON") != Some("node");',
      ],
    ],
    runs: [[...UI, 'without_the_way_back']],
    meant: 'without_the_way_back_cf_ui_is_the_native_daemon_whatever_the_old_switch_says',
  },
  {
    name: 'flip: the native cf’s verbs read the old switch',
    edits: [
      [
        LIB,
        'if choice.node {',
        'if choice.node || env.text("CONSENSFLOW_DAEMON") == Some("node") {',
      ],
    ],
    runs: [WAY_BACK],
    meant: 'the_old_switch_in_the_environment_changes_nothing_either_way',
  },
  {
    name: 'flip: cf.mjs reads the old switch',
    edits: [
      [
        DOOR_FILE,
        '!(useNode(process.env) || NODE_ONLY.has(command))',
        "!(useNode(process.env) || process.env.CONSENSFLOW_DAEMON === 'node' || NODE_ONLY.has(command))",
      ],
    ],
    runs: [DOOR],
    meant: 'has the old switch in the environment change nothing, in either home',
  },
  {
    name: 'flip: the Rust decider reads the old switch',
    edits: [
      [
        DECIDER,
        DECIDER_FINDS,
        'let node = file.as_deref().is_some_and(Path::exists) || env.text("CONSENSFLOW_DAEMON") == Some("node");',
      ],
    ],
    runs: [TABLE],
    meant: 'every_case_has_one_answer_whatever_the_environment_adds',
  },
  {
    name: 'flip: the Node decider reads the old switch',
    edits: [
      [
        NODE_DECIDER,
        'export function useNode(env) {\n  try {',
        "export function useNode(env) {\n  if (env.CONSENSFLOW_DAEMON === 'node') return true\n  try {",
      ],
    ],
    runs: [NODE_TABLE],
    meant: 'has one answer for every case of the table, whatever the environment adds',
  },

  // cf.mjs.
  {
    name: 'flip: cf.mjs runs a verb itself without the file',
    edits: [[DOOR_FILE, DOOR_ASKS, 'false']],
    runs: [DOOR],
    meant:
      'hands each verb to the native cf beside it whole, and its stdio and exit code back, when the home has no way back',
  },
  {
    name: 'flip: cf.mjs forwards nothing to the board for a window’s token',
    edits: [[DOOR_FILE, 'if (process.env.CONSENSFLOW_TOKEN || ', 'if (']],
    runs: [DOOR],
    meant:
      'forwards a window’s token to the native cf, which is then the board, whatever the home says',
  },
  {
    name: 'flip: cf.mjs forwards setup and doctor, which the native cf hands back',
    edits: [[DOOR_FILE, "new Set(['setup', 'doctor'])", 'new Set([])']],
    runs: [DOOR],
    meant:
      'keeps setup and doctor on Node’s CLI, which the native cf hands them to, whatever the home says',
  },

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

  // The Node the native cf runs.
  {
    name: 'flip: the native cf runs the Node an environment names',
    edits: [
      [
        NODE,
        FIND_NODE,
        lines(
          '    if let Some(named) = std::env::var_os("CONSENSFLOW_NODE") {',
          '        return Ok(PathBuf::from(named));',
          '    }',
          FIND_NODE,
        ),
      ],
    ],
    runs: [WAY_BACK],
    meant: 'the_node_an_environment_names_is_not_the_one_that_runs',
  },
  {
    name: 'flip: the native cf looks for its Node in the environment alone',
    edits: [
      [
        NODE,
        FIND_NODE,
        lines(
          '    let tried = candidates(cf, cfg!(windows));',
          '    std::env::var_os("CONSENSFLOW_NODE").map(PathBuf::from).ok_or(tried)',
        ),
      ],
    ],
    runs: [WAY_BACK],
    meant: 'with_the_file_every_tokenless_verb_runs_on_the_node_beside_cf_with_none_named',
  },

  // What runs the readers: the native cf built from the planted sources, one writer for a home.
  {
    name: 'flip: the native cf answers a verb itself though the home has the file, seen by one writer',
    edits: [[LIB, VERBS_ASK, '']],
    runs: [BUILD, WRITER],
    meant: 'writes a roster through Node with the file in the home, and through Rust without it',
  },
  {
    name: 'flip: the native cf’s ui is the daemon though the home has the file, seen by one writer',
    edits: [[LIB, UI_ASKS, ';']],
    runs: [BUILD, WRITER],
    meant:
      'has the daemon and the verbs of a home the same implementation, by whichever way the daemon is started',
  },

  // What the tests choose in their own home.
  {
    name: 'flip: the Node leg’s home has no file',
    edits: [[CHOICE_FILE, "    writeFileSync(file, '')", '    rmSync(file, { force: true })']],
    runs: [CHOICE],
    meant:
      'makes the choice in the home it is given: the file for Node’s, none for the native one’s',
  },
  {
    name: 'flip: the native leg’s home keeps the file',
    edits: [[CHOICE_FILE, '    rmSync(file, { force: true })\n  }\n}', '  }\n}']],
    runs: [CHOICE],
    meant:
      'makes the choice in the home it is given: the file for Node’s, none for the native one’s',
  },
  {
    name: 'flip: the app logs the wrong daemon',
    edits: [
      [
        COMMAND,
        lines(
          '        (Some(file), true) => format!(',
          '            "starting Node\'s daemon: {} is there, the way back to Node",',
        ),
        lines(
          '        (Some(file), true) => format!(',
          '            "starting the native daemon: {} is there, the way back to Node",',
        ),
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_log_says_which_daemon_and_what_chose_it',
  },
]
