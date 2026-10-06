/**
 * Plants in which `cf` answers the standalone verbs: the switch, a window's
 * token, the verbs left to others, the words as they came, and a reader that
 * went away. The tests of the module and of the process catch them; three are
 * given to the suites of the CLI alone, run against both CLIs, which must catch
 * them too.
 */
import { BOTH, GOLDENS, lines, PROCESS, STANDALONE, UNITS } from './kit.mjs'

const MOD = `${STANDALONE}/mod.rs`
const SWITCH =
  'if env.text("CONSENSFLOW_DAEMON") != Some("native") || env.text("CONSENSFLOW_TOKEN").is_some() {'

export const PLANTS = [
  {
    name: 'dispatch: the switch is not looked at',
    edits: [[MOD, SWITCH, 'if env.text("CONSENSFLOW_TOKEN").is_some() {']],
    runs: [UNITS],
    meant: 'it_answers_nothing_while_the_switch_is_off_whatever_the_verb',
  },
  {
    // The suites give the native cf its switch, so one that ignored it would pass
    // them: what they see is a verb handed back to Node's sources, with no runtime.
    name: 'dispatch: the catalog is handed to Node though the switch is on, seen by the suites of the CLI',
    edits: [
      [MOD, 'Some("catalog") => catalog::run(rest, out),', 'Some("catalog") => return Ok(None),'],
    ],
    runs: [BOTH],
    meant: 'runs the native cf',
  },
  {
    name: 'dispatch: a window token does not make cf the board',
    edits: [[MOD, SWITCH, 'if env.text("CONSENSFLOW_DAEMON") != Some("native") {']],
    runs: [UNITS],
    meant: 'a_window_token_makes_cf_the_board_which_this_module_does_not_answer',
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
    meant: 'ui_is_the_one_verb_left_to_the_daemon_with_the_switch_on',
  },
  {
    // As for the catalog: the suites give the native cf its switch and no runtime,
    // so a setup handed back to Node's sources is a setup that fails.
    name: 'dispatch: setup is handed to Node though the switch is on, seen by the suites of the CLI',
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
        'None => match standalone::run(env, args, out, err)? {',
        lines(
          'None => match standalone::run(',
          '            env,',
          '            &args.iter().filter(|arg| *arg != "--json").cloned().collect::<Vec<_>>(),',
          '            out,',
          '            err,',
          '        )? {',
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
