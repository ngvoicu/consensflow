/**
 * The chief's report of 2026-10-08, end to end: the daemon the app starts and the
 * native `cf` of a window, built from the sources as a plant leaves them (the
 * suites build the `cf` they run), and the rig's fake agents in real PTYs (the
 * `rig` test of `crates/cf-e2e`). Where the tests of the crates hold each rule
 * apart, these hold that the rule reaches the chief through the pane host: a
 * stale note not pasted into a chief in the middle of a turn, a hold told as it
 * ends, `--help` answered and posted nowhere. And what the black-box suites hold
 * of the daemon as a process (the words its log ends with, the `PATH` a window
 * is given, a result decided on, a Codex window's socket). One plant takes one
 * rule out; a test of it fails.
 */
import { DAEMON_PROCESS, lines, RIG } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const WORDS = 'crates/cf/src/board/words.rs'
const DAEMON_SRC = 'crates/cf-daemon/src'

/** The notes of a task's wait, withdrawn and told. */
const WAITING = RIG('waiting_notes::')
/** Every verb of the board asked for help. */
const HELP = RIG('help::')
/** `cf ui` as a process: its log, and what it opens a window with. */
const CORE_DAEMON = DAEMON_PROCESS('core_daemon::')
/** A result the chief decided on. */
const DECIDED = RIG('decided_result::')
/** The Codex window's supervisor, as a process. */
const CODEX_WINDOW = RIG('codex_session::')

export const PLANTS = [
  {
    // m-1668: "T-357 was taken back from @ullr … waits for another standard
    // worker" reached the chief after another worker had taken T-357.
    name: 'rig: a take-over leaves the note that the task was taken back for the chief to be given',
    edits: [
      [
        `${LEDGER}/tasks/giving.rs`,
        '        leave_pause_notes(store, &task, &format!("was taken by @{}", member.handle))?;\n',
        '',
      ],
    ],
    runs: [WAITING],
    builds: true,
    meant:
      'the_note_that_a_task_was_taken_back_is_withdrawn_when_another_worker_takes_it_before_the_chief_is_given_the_note',
  },
  {
    // m-1670: the hold note reached the chief after the daemon had resumed the task.
    name: 'rig: a resume leaves the note that the task is held for the chief to be given',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '        // A refusal below writes nothing, this withdrawal with it.',
          '        leave_pause_notes(store, &task, "resumed")?;',
        ),
        '',
      ],
    ],
    runs: [WAITING],
    builds: true,
    meant:
      'the_note_that_a_task_is_held_is_withdrawn_when_the_account_is_switched_before_the_chief_is_given_it_and_nothing_is_said_after',
  },
  {
    name: 'rig: the daemon resumes a held task and the chief given the note is told nothing',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '        if by.is_none() {',
          '            tell_goes_on(store, &task)?;',
          '        }',
          '',
        ),
        '',
      ],
    ],
    runs: [WAITING],
    builds: true,
    meant:
      'a_chief_given_the_note_that_a_task_is_held_is_told_it_goes_on_when_the_account_is_switched',
  },
  {
    // m-1542: `cf note --help` posted a note saying "--help".
    name: 'rig: cf --help is text and is posted',
    edits: [[WORDS, '        !self.literal\n', '        false\n            && !self.literal\n']],
    runs: [HELP],
    builds: true,
    meant: 'a_window_asks_every_verb_of_cf_for_help_and_nothing_is_posted_or_read',
  },

  // The daemon as a process, and a window's Codex.
  {
    name: 'rig: the daemon says in its log that its input closed, not ended',
    edits: [
      [
        `${DAEMON_SRC}/stop.rs`,
        'Ended::Input => watching.trip("stdin ended"),',
        'Ended::Input => watching.trip("stdin closed"),',
      ],
    ],
    runs: [CORE_DAEMON],
    builds: true,
    meant: 'starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped_its_input_ending',
  },
  {
    name: 'rig: the daemon stops for a broken bridge and says it broke',
    edits: [
      [
        `${DAEMON_SRC}/start.rs`,
        '                latch.trip("the bridge failed");',
        '                latch.trip("the bridge broke");',
      ],
    ],
    runs: [CORE_DAEMON],
    builds: true,
    meant: 'stops_itself_when_the_apps_end_of_the_bridge_breaks',
  },
  {
    name: 'rig: the bundle’s folder is last on a window’s PATH, not first',
    edits: [
      [
        `${DAEMON_SRC}/seams.rs`,
        'Some(path) => format!("{bin}{delimiter}{}", path.to_string_lossy()),',
        'Some(path) => format!("{}{delimiter}{bin}", path.to_string_lossy()),',
      ],
    ],
    runs: [CORE_DAEMON],
    builds: true,
    meant:
      'opens_a_window_with_the_agents_api_its_project_and_participant_and_the_bundled_cf_first_on_path_and_names_it_no_node',
  },
  {
    name: 'rig: a result the chief accepted is not withdrawn',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        '        withdraw_result(store, &task, "was accepted")?;\n',
        '',
      ],
    ],
    runs: [DECIDED],
    builds: true,
    meant:
      'a_result_the_chief_read_and_decided_on_in_its_own_turn_is_withdrawn_and_the_next_result_reaches_it',
  },
  {
    name: 'rig: a Codex window’s supervisor leaves its socket folder behind',
    edits: [
      [
        'crates/cf-codex-session/src/supervisor.rs',
        '    match std::fs::remove_dir_all(folder) {',
        '    match Ok::<(), io::Error>(()) {',
      ],
    ],
    runs: [CODEX_WINDOW],
    builds: true,
    meant:
      'opens_codex_as_its_server_on_a_private_socket_and_its_tui_through_the_broker_and_leaves_nothing_behind',
  },
]
