/**
 * What `crates/cf-harness` does with the terminal command and with Claude's
 * settings: the stale hooks it reports (`claude/stale_hooks.rs`) and what
 * opening the app prepares (`prepare.rs`). `launcher.mjs` holds the command
 * itself, and passes these on with its own; every name here begins as the
 * area does, so `npm run plants:daemon -- stale` and `-- prepare` run them.
 */
import { lines } from './kit.mjs'

const HARNESS = 'crates/cf-harness/src'
const HOOKS = ['-p', 'cf-harness', '--test', 'host_payloads']
const INSTALL = ['-p', 'cf-harness', '--test', 'install']

/** Each bug: what it is, the text it replaces in a file, who must notice, and with which test. */
export const PLANTS = [
  {
    name: 'stale hooks: a hook is named in any case',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        'js::stringify(entry).contains("consensflow")',
        'js::stringify(entry).to_lowercase().contains("consensflow")',
      ],
    ],
    runs: [HOOKS],
    meant: 'every_settings_file_node_was_played_is_read_as_node_read_it',
  },
  {
    name: 'stale hooks: the events are in the order of their names',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        lines('        .map(|(event, _)| event)', '        .collect()', '}'),
        lines(
          '        .map(|(event, _)| event)',
          '        .collect::<std::collections::BTreeSet<_>>()',
          '        .into_iter()',
          '        .collect()',
          '}',
        ),
      ],
    ],
    runs: [HOOKS],
    meant: 'every_settings_file_node_was_played_is_read_as_node_read_it',
  },
  {
    name: 'stale hooks: an event that is no list counts',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        lines(
          '            entries.as_array().is_some_and(|list| {',
          '                list.iter()',
          '                    .any(|entry| js::stringify(entry).contains("consensflow"))',
          '            })',
        ),
        '            js::stringify(entries).contains("consensflow")',
      ],
    ],
    runs: [HOOKS],
    meant: 'every_settings_file_node_was_played_is_read_as_node_read_it',
  },
  {
    name: 'stale hooks: hooks that is a list names no event',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        'Some(Value::Array(hooks)) => hooks',
        'Some(Value::Array(hooks)) if false => hooks',
      ],
    ],
    runs: [HOOKS],
    meant: 'every_settings_file_node_was_played_is_read_as_node_read_it',
  },
  {
    name: 'stale hooks: an empty config folder is no folder',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        'let folder = env.os("CLAUDE_CONFIG_DIR").map_or_else(',
        'let folder = env.os("CLAUDE_CONFIG_DIR").filter(|folder| !folder.is_empty()).map_or_else(',
      ],
    ],
    runs: [HOOKS],
    meant:
      'an_empty_claude_config_dir_is_the_working_folder_as_node_kept_it_and_no_home_is_no_failure',
  },
  {
    name: 'stale hooks: USERPROFILE is asked before HOME',
    edits: [
      [
        `${HARNESS}/claude/stale_hooks.rs`,
        lines('    env.os("HOME")', '        .or_else(|| env.os("USERPROFILE"))'),
        lines('    env.os("USERPROFILE")', '        .or_else(|| env.os("HOME"))'),
      ],
    ],
    runs: [HOOKS],
    meant: 'without_a_claude_config_dir_the_settings_are_in_dot_claude_of_the_home',
  },
  {
    name: 'prepare: a launcher that cannot be installed stops the integrations',
    edits: [
      [
        `${HARNESS}/prepare.rs`,
        lines(
          '        report.push(format!("The cf launcher could not be installed: {message}"));',
          '    }',
        ),
        lines(
          '        report.push(format!("The cf launcher could not be installed: {message}"));',
          '        return Prepared {',
          '            report,',
          '            pi_extension: pi::Extension::NotInstalled,',
          '            opencode_extension: opencode::Extension::NotInstalled,',
          '        };',
          '    }',
        ),
      ],
    ],
    runs: [INSTALL],
    meant:
      'app_preparation_says_why_its_launcher_could_not_be_installed_and_prepares_the_integrations_all_the_same',
  },
  {
    name: 'prepare: the launcher is not installed',
    edits: [
      [
        `${HARNESS}/prepare.rs`,
        'cf_launcher::install(env, cf, &Places::default())',
        'Ok::<Option<cf_launcher::Installed>, String>(None)',
      ],
    ],
    runs: [INSTALL],
    meant: 'app_preparation_owns_its_launcher_and_integrations_not_role_documents_or_global_skills',
  },
  {
    name: "prepare: the launcher's failure is worded another way",
    edits: [
      [
        `${HARNESS}/prepare.rs`,
        '"The cf launcher could not be installed: {message}"',
        '"The launcher could not be installed: {message}"',
      ],
    ],
    runs: [INSTALL],
    meant:
      'app_preparation_says_why_its_launcher_could_not_be_installed_and_prepares_the_integrations_all_the_same',
  },
]
