// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
/**
 * Plants in the bundle without Node: that nothing reads what the flip release
 * read (the `use-node` file of a home, the runtime the app named to the daemon),
 * that an npm shim with no Node to run on is refused in words that say what to
 * do, that the daemon the app starts is the bundle's `cf` and nothing is added
 * to it, that the update's check asks for what it should, and that the page's
 * console text, the bundle's packing and the release's checks hold. Each takes
 * one thing out or puts one back, and a test must fail.
 */
import {
  APP,
  BUILD,
  CF_NATIVE,
  CODEX_SESSION,
  CONSOLE_TEXT,
  HOST,
  lines,
  NPM_SHIMS,
  PORTABLE_PACK,
  PROCESS,
  RUNNABLE,
  SEAMS,
  SIGN_MAC,
  STAND_IN,
  UI,
  UPDATE_RELEASE,
} from './kit.mjs'

const CF_LIB = 'crates/cf/src/lib.rs'
const RUNNABLE_RS = 'crates/cf-process/src/runnable.rs'
const SEAMS_RS = 'crates/cf-daemon/src/seams.rs'
const SUPERVISOR_RS = 'crates/cf-codex-session/src/supervisor.rs'
const TESTING_RS = 'crates/cf-harness/src/testing/mod.rs'
/** The line that gives the stand-in of a window its Node. */
const STAND_IN_NODE =
  '    fs::write(file.with_file_name("node.exe"), "").expect("a stand-in\'s node written");\n'
const DAEMON_COMMAND = 'app/src-tauri/src/daemon_command.rs'
const UPDATE_INSTALL = 'app/src-tauri/src/update_install.rs'
const RELEASE_BUNDLE_RS = 'tools/cf-release/src/update/bundle.rs'
const PORTABLE_PACK_RS = 'crates/cf-portable/src/pack.rs'
const CONSOLE_JS = 'app/ui/core/console-text.js'

/** The app's tests of the module they are about. */
const app = (filter) => [...APP, filter]

/** The home's `use-node` file, asked for by a reader that has no business asking. */
const USE_NODE =
  'cf_base::home::config_root(env).is_some_and(|root| root.join("use-node").exists())'

export const PLANTS = [
  // The file the flip release read, read by nothing now.
  {
    name: 'deletion: cf ui is not the daemon in a home that has the use-node file',
    edits: [
      [
        CF_LIB,
        '    let asked = first == "ui" && env.text("CONSENSFLOW_TOKEN").is_none();',
        `    let asked = first == "ui" && env.text("CONSENSFLOW_TOKEN").is_none() && !${USE_NODE};`,
      ],
    ],
    runs: [UI],
    meant: 'cf_ui_is_the_daemon_in_a_home_the_flip_release_sent_to_node',
  },
  {
    name: 'deletion: a verb is refused in a home that has the use-node file',
    edits: [
      [
        CF_LIB,
        '        None => standalone::run(env, args, out, err),',
        lines(
          `        None if ${USE_NODE} => {`,
          '            writeln!(err, "cf: this home is on Node")?;',
          '            Ok(1)',
          '        }',
          '        None => standalone::run(env, args, out, err),',
        ),
      ],
    ],
    runs: [PROCESS],
    meant: 'a_use_node_file_left_in_the_home_changes_no_answer',
  },
  {
    name: 'deletion: setup is refused in a home that has the use-node file',
    edits: [
      [
        CF_LIB,
        '        None => standalone::run(env, args, out, err),',
        lines(
          `        None if ${USE_NODE} && args.first().is_some_and(|word| word == "setup") => {`,
          '            writeln!(err, "cf: this home is on Node")?;',
          '            Ok(1)',
          '        }',
          '        None => standalone::run(env, args, out, err),',
        ),
      ],
    ],
    runs: [PROCESS],
    meant: 'setup_and_doctor_in_a_home_with_a_use_node_file_make_and_read_the_command_here',
  },

  // The runtime the app named to the daemon, passed on to every window.
  {
    name: 'deletion: the window is told a Node again',
    edits: [
      [
        SEAMS_RS,
        '        env.push(("PATH".to_owned(), self.path.clone()));',
        lines(
          '        env.push(("CONSENSFLOW_NODE".to_owned(), "/the/apps/node".to_owned()));',
          '        env.push(("PATH".to_owned(), self.path.clone()));',
        ),
      ],
    ],
    runs: [SEAMS],
    meant: 'a_window_starts_with_its_url_project_participant_and_the_bundle_first_on_its_path',
  },
  {
    name: 'deletion: the app names a Node to the daemon again',
    edits: [
      [
        DAEMON_COMMAND,
        '    command.args(["ui", "--json", "--no-open"]);',
        '    command.args(["ui", "--json", "--no-open"]).env("CONSENSFLOW_NODE", "/the/apps/node");',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_daemon_is_the_bundled_cf_running_ui_and_nothing_of_the_apps_is_added_to_it',
  },
  {
    name: 'deletion: the app opens the daemon without --no-open',
    edits: [
      [
        DAEMON_COMMAND,
        '    command.args(["ui", "--json", "--no-open"]);',
        '    command.args(["ui", "--json"]);',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_daemon_is_the_bundled_cf_running_ui_and_nothing_of_the_apps_is_added_to_it',
  },
  {
    name: 'deletion: the app’s log names another cf than the one it starts',
    edits: [
      [
        DAEMON_COMMAND,
        '    format!("starting the daemon: {} ui --json --no-open", cf.display())',
        '    format!("starting the daemon: {} ui --json --no-open", cf.with_file_name("cf.mjs").display())',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_log_says_which_cf_the_daemon_is',
  },
  {
    name: 'deletion: the bundled cf is looked for beside cf.mjs',
    edits: [
      [
        DAEMON_COMMAND,
        '    root.join("cli").join("bin").join(CF)',
        '    root.join("cli").join("bin").join("cf.mjs")',
      ],
    ],
    runs: [app('daemon_command')],
    meant: 'the_cf_is_the_one_in_the_clis_folder_of_the_bundle_or_the_portable_runtime',
  },

  // An npm shim with no Node to be found.
  {
    name: 'deletion: an npm shim runs on the Node an environment names',
    edits: [
      [
        RUNNABLE_RS,
        '    on_path("node", env)\n}',
        '    on_path("node", env).or_else(|| env.path("CONSENSFLOW_NODE").map(Path::to_path_buf))\n}',
      ],
    ],
    runs: [RUNNABLE],
    meant: 'an_npm_shim_with_no_node_beside_it_or_on_the_path_is_refused_saying_what_to_do',
  },
  {
    name: 'deletion: an npm shim with no Node goes through cmd.exe',
    edits: [
      [
        RUNNABLE_RS,
        '        Shim::NeedsNode => return Err(needs_node(executable)),',
        '        Shim::NeedsNode => {}',
      ],
    ],
    runs: [RUNNABLE],
    meant: 'an_npm_shim_with_no_node_beside_it_or_on_the_path_is_refused_saying_what_to_do',
  },
  {
    name: 'deletion: a window is opened on an npm shim with no Node as on one that cannot be read',
    edits: [
      [
        RUNNABLE_RS,
        '        Shim::NeedsNode => return Err(needs_node(Path::new(executable))),',
        lines(
          '        Shim::NeedsNode => {',
          '            return Err(format!(',
          '                "{executable} is not an npm shim, and only cmd.exe could run it in a window"',
          '            ));',
          '        }',
        ),
      ],
    ],
    runs: [RUNNABLE],
    meant: 'an_npm_shim_with_no_node_beside_it_or_on_the_path_is_refused_saying_what_to_do',
  },
  {
    name: 'deletion: the Node beside an npm shim is not looked for',
    edits: [[RUNNABLE_RS, '    if beside.exists() {\n        return Some(beside);\n    }\n', '']],
    runs: [RUNNABLE],
    meant: 'an_npm_shim_takes_the_node_beside_it_first_as_npm_itself_would',
  },
  {
    name: 'deletion: the refusal does not say the Node can be made visible',
    edits: [
      [
        RUNNABLE_RS,
        "Make the harness's Node \\\n         visible to ConsensFlow, or install",
        'Install \\\n         the harness, or install',
      ],
    ],
    runs: [RUNNABLE, NPM_SHIMS],
    meant: 'an_npm_shim_with_no_node_beside_it_or_on_the_path_is_refused_saying_what_to_do',
  },
  {
    name: 'deletion: a Codex that is an npm shim with no Node is said to be a server that did not start',
    edits: [
      [
        SUPERVISOR_RS,
        lines(
          '        args.extend(plan.endpoint.listen().iter().cloned());',
          '        let (mut command, program) = command(plan, &args).map_err(SessionError::Unrunnable)?;',
        ),
        lines(
          '        args.extend(plan.endpoint.listen().iter().cloned());',
          '        let (mut command, program) = command(plan, &args).map_err(SessionError::Server)?;',
        ),
      ],
    ],
    runs: [CODEX_SESSION],
    meant: 'a_codex_that_is_an_npm_shim_with_no_node_to_run_on_is_refused_saying_what_to_do',
  },

  // The tests' stand-in for a window's program on Windows is an npm shim, and
  // brings the Node it runs on: the app names none to the daemon now.
  {
    name: 'deletion: the Windows stand-in of a window has no Node beside it',
    edits: [[TESTING_RS, STAND_IN_NODE, '']],
    runs: [STAND_IN],
    meant:
      'a_window_opens_on_the_npm_shim_stand_in_in_an_environment_with_only_its_folder_on_the_path',
  },
  {
    name: 'deletion: the Windows stand-in of a window has no Node beside it, seen at the daemon’s host',
    edits: [[TESTING_RS, STAND_IN_NODE, '']],
    runs: [HOST],
    meant: 'a_window_opens_on_an_npm_shim_with_its_node_beside_it_and_is_refused_without_one',
  },

  // The update's check, and the portable app's collector.
  {
    name: 'deletion: the update’s check asks for the package.json of a CLI again',
    edits: [
      [
        UPDATE_INSTALL,
        '    if !app.join("Contents/Resources/cli/bin/cf").is_file() {',
        lines(
          '    if !app.join("Contents/Resources/cli/package.json").is_file() {',
          '        return Err("The archive must include the CLI\'s package.json".into());',
          '    }',
          '    if !app.join("Contents/Resources/cli/bin/cf").is_file() {',
        ),
      ],
    ],
    runs: [app('update_install')],
    meant: 'a_signed_bundle_replaces_the_whole_app_and_leaves_nothing_of_an_older_layout',
  },
  {
    name: 'deletion: the update’s check takes a bundle whose seal is broken',
    edits: [
      [
        UPDATE_INSTALL,
        '    if !signature.status.success() {',
        '    if false && !signature.status.success() {',
      ],
    ],
    runs: [app('update_install')],
    meant: 'a_newer_bundle_modified_after_signing_leaves_the_old_app_intact',
  },
  // (The portable app's collector, which keeps a flip release's node.exe, is plants/release/portable.mjs's.)
  {
    name: 'deletion: the portable exe is packed with a node.exe again',
    edits: [
      [
        PORTABLE_PACK_RS,
        lines('const RUNTIME: [&str; 4] = [', '    "cli",'),
        lines('const RUNTIME: [&str; 5] = [', '    "node.exe",', '    "cli",'),
      ],
    ],
    runs: [PORTABLE_PACK],
    meant:
      'a_packed_exe_is_the_app_then_its_runtime_as_a_gzip_compressed_tar_then_the_length_and_the_tag',
  },
  {
    name: 'deletion: the release does not ask the bundled cf its version',
    edits: [
      [
        RELEASE_BUNDLE_RS,
        '    if version != cf_version {',
        '    if false && version != cf_version {',
      ],
    ],
    runs: [UPDATE_RELEASE],
    meant: 'rejects_a_bundle_whose_plist_and_bundled_cf_disagree_on_the_version',
  },
  {
    name: 'deletion: the release asks for the package.json of a CLI again',
    edits: [
      [
        RELEASE_BUNDLE_RS,
        'for (what, required) in [("native executable", &binary), ("a window\'s cf", &cf)] {',
        'for (what, required) in [("native executable", &binary), ("a window\'s cf", &cf), ("bundled CLI package", &path.join("Contents").join("Resources").join("cli").join("package.json"))] {',
      ],
    ],
    runs: [UPDATE_RELEASE],
    meant: 'takes_a_bundle_that_ships_nothing_of_nodes',
  },

  {
    name: 'deletion: the app is signed without the hardened runtime',
    edits: [
      [
        'tools/cf-release/src/sign_mac/signing.rs',
        '    codesign(tools, signing, app, &["--options", "runtime"])\n}',
        '    codesign(tools, signing, app, &[])\n}',
      ],
    ],
    runs: [SIGN_MAC],
    meant: 'an_ad_hoc_release_signs_the_code_then_the_app_then_makes_the_dmg_again_with_no_notary',
  },
  {
    name: 'deletion: a verb is given the first line of an argument',
    edits: [
      [
        'crates/cf/src/standalone/agent.rs',
        '            input.insert(key.to_owned(), Value::from(text));',
        '            input.insert(key.to_owned(), Value::from(text.lines().next().unwrap_or("")));',
      ],
    ],
    runs: [BUILD, CF_NATIVE],
    meant: 'are given every argument whole: line breaks, quotes, % and & and ^, diacritics',
  },

  // The page's console text.
  {
    name: 'deletion: the page spells an em dash as a hyphen',
    edits: [[CONSOLE_JS, "    '—': '--',", "    '—': '-',"]],
    runs: [CONSOLE_TEXT],
    meant: 'spells every code point the console changes as the table has it',
  },
  {
    name: 'deletion: the page does not compose a text before it spells it',
    edits: [[CONSOLE_JS, "text.normalize('NFC')", 'text']],
    runs: [CONSOLE_TEXT],
    meant:
      'spells the texts that compose or decompose as a whole as the table has them, and null as null',
  },
  {
    name: 'deletion: the page changes a letter',
    edits: [
      [
        CONSOLE_JS,
        'if (code < 0x80 || /\\p{L}/u.test(character)) return character',
        'if (code < 0x80) return character',
      ],
    ],
    runs: [CONSOLE_TEXT],
    meant: 'leaves every other code point as it is',
  },
  {
    name: 'deletion: the page’s module imports something',
    edits: [
      [
        CONSOLE_JS,
        'const CONSOLE_ASCII = new Map(',
        "import { join } from 'node:path'\nconst CONSOLE_ASCII = new Map(",
      ],
    ],
    runs: [CONSOLE_TEXT],
    meant: 'imports nothing, so the page loads it as it is',
  },
]
