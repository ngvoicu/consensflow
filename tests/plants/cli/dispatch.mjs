/**
 * Plants in which `cf` answers the standalone verbs: the verbs themselves, `ui`
 * (the daemon's, no verb here), the words as they came, and a reader that went
 * away. The tests of the module, of the library and of the process catch them;
 * three are given to the suites of the CLI alone, run against both CLIs, which
 * must catch them too. Which implementation answers (the home's file, a window's
 * token) is `flip.mjs`'s.
 */
import { BOTH, GOLDENS, LIBRARY, lines, PROCESS, STANDALONE, UNITS } from './kit.mjs'

const MOD = `${STANDALONE}/mod.rs`
const LIB = 'crates/cf/src/lib.rs'

export const PLANTS = [
  {
    // The suites give the native cf no Node beside it, and one that did not
    // answer the verb would say it is no command.
    name: 'dispatch: the catalog is an unknown command, seen by the suites of the CLI',
    edits: [[MOD, '        Some("catalog") => catalog::run(rest, out),\n', '']],
    runs: [BOTH],
    meant: 'runs the native cf',
  },
  {
    name: 'dispatch: setup and doctor are answered as unknown commands',
    edits: [
      [
        MOD,
        lines(
          '        Some("setup") => setup::run(env, rest, out),',
          '        // Whatever words follow it are no matter, as in Node.',
          '        Some("doctor") => doctor::run(env, out),',
          '',
        ),
        '',
      ],
    ],
    runs: [UNITS, PROCESS],
    meant: 'with_no_folder_to_keep_the_agents_in_a_verb_that_needs_it_says_so',
  },
  {
    // As for the catalog.
    name: 'dispatch: setup is an unknown command, seen by the suites of the CLI',
    edits: [[MOD, '        Some("setup") => setup::run(env, rest, out),\n', '']],
    runs: [BOTH],
    meant: 'roster edits, setup and diagnostic reads leave role files and old manifests alone',
  },
  {
    name: 'dispatch: ui is answered by the module as a verb',
    edits: [
      [
        MOD,
        '        Some("doctor") => doctor::run(env, out),\n',
        '        Some("doctor") => doctor::run(env, out),\n        Some("ui") => Ok(()),\n',
      ],
    ],
    runs: [UNITS],
    meant: 'ui_is_the_daemons_and_reaching_this_module_it_is_an_unknown_command',
  },
  {
    // `main` runs a `ui` as the daemon before the words get here (`native_ui`);
    // one that reaches `run` has no verb to answer it, and says so.
    name: 'dispatch: a ui that reaches run is answered as if it were a verb',
    edits: [
      [
        LIB,
        '        None => standalone::run(env, args, out, err),\n',
        lines(
          '        None if args.first().is_some_and(|word| word == "ui") => {',
          '            writeln!(out, "ui")?;',
          '            Ok(0)',
          '        }',
          '        None => standalone::run(env, args, out, err),',
          '',
        ),
      ],
    ],
    runs: [LIBRARY],
    meant: 'a_ui_that_reaches_the_library_is_an_unknown_command_and_starts_nothing',
  },
  {
    name: 'dispatch: the verbs are handed the words with --json taken out',
    edits: [
      [
        LIB,
        'None => standalone::run(env, args, out, err),',
        lines(
          'None => standalone::run(',
          '                env,',
          '                &args.iter().filter(|arg| *arg != "--json").cloned().collect::<Vec<_>>(),',
          '                out,',
          '                err,',
          '            ),',
        ),
      ],
    ],
    runs: [PROCESS],
    meant: 'a_json_word_is_the_verbs_own_and_is_not_taken_out_before_the_verb_reads_it',
  },
  {
    name: 'dispatch: a reader that went away is a failure',
    edits: [
      [
        'crates/cf/src/main.rs',
        'Err(cause) if cause.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,',
        'Err(cause) if cause.kind() == ErrorKind::BrokenPipe => ExitCode::FAILURE,',
      ],
    ],
    runs: [GOLDENS],
    meant: 'every_case_says_writes_and_exits_as_node_did',
  },
  {
    name: 'dispatch: the usage has no doctor, seen by the suites of the CLI',
    edits: [
      [
        `${STANDALONE}/usage.txt`,
        'doctor                                    Inspect runtime, roster and bundled roles',
        '',
      ],
    ],
    runs: [BOTH],
    meant: 'knows the window commands and the ones outside a window',
  },
]
