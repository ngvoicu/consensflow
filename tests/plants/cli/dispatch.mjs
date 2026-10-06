/**
 * Plants in which `cf` answers the standalone verbs: the one verb left to the
 * daemon, the words as they came, and a reader that went away. The tests of the
 * module and of the process catch them; three are given to the suites of the
 * CLI alone, run against both CLIs, which must catch them too. Which
 * implementation answers (the home's file, a window's token) is `flip.mjs`'s.
 */
import { BOTH, GOLDENS, lines, PROCESS, STANDALONE, UNITS } from './kit.mjs'

const MOD = `${STANDALONE}/mod.rs`

export const PLANTS = [
  {
    // The suites give the native cf no Node beside it, so one that handed the
    // verb on would be refused: what they see is `none is bundled`.
    name: 'dispatch: the catalog is handed to Node, seen by the suites of the CLI',
    edits: [
      [MOD, 'Some("catalog") => catalog::run(rest, out),', 'Some("catalog") => return Ok(None),'],
    ],
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
    meant: 'ui_is_the_one_verb_left_to_the_daemon',
  },
  {
    // As for the catalog: the suites give the native cf no Node beside it, so a
    // setup handed back to Node's sources is a setup that fails.
    name: 'dispatch: setup is handed to Node, seen by the suites of the CLI',
    edits: [
      [MOD, 'Some("setup") => setup::run(env, rest, out),', 'Some("setup") => return Ok(None),'],
    ],
    runs: [BOTH],
    meant: 'roster edits, setup and diagnostic reads leave role files and old manifests alone',
  },
  {
    name: 'dispatch: the verbs are handed the words with --json taken out',
    edits: [
      [
        'crates/cf/src/lib.rs',
        'match standalone::run(env, args, out, err)? {',
        lines(
          'match standalone::run(',
          '                env,',
          '                &args.iter().filter(|arg| *arg != "--json").cloned().collect::<Vec<_>>(),',
          '                out,',
          '                err,',
          '            )? {',
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
