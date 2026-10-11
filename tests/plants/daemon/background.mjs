/**
 * What a worker's turn leaves running, and how a window closes (the report of
 * 2026-10-09: a recording run left in the background died, killed outright, with
 * the window its worker's turn had ended). One plant takes one rule out; a test
 * of it fails. The first plants run the tests of the crates, which hold each rule
 * apart; the `rig:` plants after them hold that the rule reaches the chief through
 * the real daemon and pane host, and run only the case of the rig
 * (`crates/cf-e2e/tests/rig/background_work.rs`) that holds it. The rig's cases
 * build the `cf` and the pane host they run from the sources as planted, in
 * release, which makes these the slowest plants.
 */
import { lines } from './kit.mjs'

const ENGINE = 'crates/cf-engine/src'
const PANES = 'crates/cf-panes/src'

/** The engine's tests of the hold, the settle time, the marks and the notes, on the fake windows. */
const engine = ['-p', 'cf-engine', '--test', 'dispatcher', 'background']
/** The pane host's tests of what a tree had before a turn, and of how a window closes. */
const baseline = ['-p', 'cf-panes', '--lib', 'background']
const closing = ['-p', 'cf-panes', '--lib', 'close']
/** The same over the wire, with the helper the daemon runs. */
const wire = ['-p', 'cf-panes', '--test', 'headless', 'a_turn_marked']
/** The app's quit, which closes the windows within the grace they are given. */
const quit = { app: ['quitting'] }

/** The case of the rig whose name starts `stem`: the daemon and the pane host in real terminals. */
const rigCase = (stem) => ['-p', 'cf-e2e', '--test', 'rig', `background_work::${stem}`]
const HOLD = rigCase('a_turn_that_ends_with_the_job')
const SETTLE = rigCase('a_job_that_ends_with_no_turn')
const BEFORE_THE_TURN = rigCase('work_the_window_had_before')
const CANCEL = rigCase('cancelling_a_task')
const PAUSE = rigCase('pausing_a_task')
const GRACE = rigCase('a_job_that_ignores')

/** What the rig's cases are called (`meant`: the case written for the plant). */
const HOLD_MEANT =
  'a_turn_that_ends_with_the_job_it_started_running_is_not_the_result_the_job_ends_and_the_turn_it_wakes_is'
const SETTLE_MEANT =
  'a_job_that_ends_with_no_turn_to_wake_leaves_the_last_turns_end_as_the_result_once_the_agent_has_had_a_moment'
const BEFORE_THE_TURN_MEANT =
  'work_the_window_had_before_its_turn_is_not_the_turns_a_server_the_harness_started_with_it_holds_nothing'
const CANCEL_MEANT =
  'cancelling_a_task_whose_job_still_runs_asks_the_job_to_end_first_and_tells_whoever_gave_the_task'
const PAUSE_MEANT = 'pausing_a_task_whose_job_still_runs_leaves_its_window_and_the_job_as_they_are'
const GRACE_MEANT =
  'a_job_that_ignores_the_signals_that_ask_it_to_end_is_killed_once_the_grace_of_ten_seconds_is_over'

/** The edits a rule is taken out by, for the plant of the crates and the plant of the rig alike. */
const TURN_END_IS_THE_RESULT = [
  `${ENGINE}/dispatcher.rs`,
  '            } else if left == Left::Nothing {',
  '            } else {',
]
const LAUNCH_NEVER_MARKED = [
  `${ENGINE}/deliveries.rs`,
  '            if let Some(pane) = pane.filter(|_| launching) {',
  '            if let Some(pane) = pane.filter(|_| launching && false) {',
]
const CANCEL_SAYS_NOTHING = [
  `${ENGINE}/dispatcher.rs`,
  '                self.tell_left_running(&project, &participant, left)?;',
  '                self.tell_left_running(&project, &participant, &[])?;',
]
const BEFORE_THE_TURN_IS_THE_TURNS = [
  `${PANES}/background.rs`,
  '            .filter(|member| !before.contains(&member.key))',
  '            .filter(|member| !before.contains(&member.key) || true)',
]
const EXIT_LEAVES_THE_WORK_OUT = [
  `${PANES}/pane_handlers.rs`,
  lines('        tail: Some(screen.tail(TAIL_LINES)),', '        background,', '    };'),
  lines(
    '        tail: Some(screen.tail(TAIL_LINES)),',
    '        background: {',
    '            let _ = background;',
    '            Vec::new()',
    '        },',
    '    };',
  ),
]
const KILLED_WITHOUT_BEING_ASKED = [
  `${PANES}/pty/close.rs`,
  lines(
    '    let asked = pane.ask_to_exit();',
    '    wait_until(pane, &asked, Instant::now() + grace);',
    '    terminate_asked(pane)',
  ),
  lines('    let _ = grace;', '    terminate_asked(pane)'),
]

export const PLANTS = [
  {
    name: 'background: a turn that ends with its work still running is its task’s result',
    edits: [TURN_END_IS_THE_RESULT],
    runs: [engine],
    meant: 'holds_the_end_of_a_turn_while_the_work_it_started_runs',
  },
  {
    name: 'background: the agent is given no time to go on once its work has ended',
    edits: [
      [
        `${ENGINE}/background.rs`,
        '        if now - ended_at < self.seams.limits.background_settle_ms {',
        '        if now - ended_at > self.seams.limits.background_settle_ms {',
      ],
    ],
    runs: [engine],
    meant:
      'work_that_ends_with_no_turn_to_wake_leaves_the_last_turns_end_as_the_result_after_the_settle_time',
  },
  {
    name: 'background: a message that comes while the task is held marks the window again',
    edits: [
      [
        `${ENGINE}/background.rs`,
        lines(
          '        if record.background.borrow().held.is_some() {',
          '            return;',
          '        }',
        ),
        '',
      ],
    ],
    runs: [engine],
    meant: 'marks_the_window_where_a_turn_begins_and_not_while_its_task_is_held',
  },
  {
    name: 'background: a message goes into a window with no mark of where its turn begins',
    edits: [
      [
        `${ENGINE}/deliveries.rs`,
        lines(
          "        // A turn begins with this message: what the window's tree runs now came before it.",
          '        self.mark_turn(record, &pane).await;',
        ),
        '',
      ],
    ],
    runs: [engine],
    meant: 'marks_the_window_before_a_message_goes_into_it_when_nothing_is_held',
  },
  {
    name: 'background: a launch’s first message is never marked when it shows in the record',
    edits: [LAUNCH_NEVER_MARKED],
    runs: [engine],
    meant: 'marks_the_window_where_a_turn_begins_and_not_while_its_task_is_held',
  },
  {
    name: 'background: a cancelled task’s window closes and says nothing of the work it ended',
    edits: [CANCEL_SAYS_NOTHING],
    runs: [engine],
    meant:
      'a_cancelled_task_closes_its_window_with_the_work_still_running_and_whoever_gave_the_task_is_told',
  },
  {
    name: 'background: the pause note of a window that went leaves out the work it ended',
    edits: [
      [
        `${ENGINE}/dispatcher.rs`,
        '                let because = if left.is_empty() {',
        '                let because = if true {',
      ],
    ],
    runs: [engine],
    meant: 'a_window_that_goes_by_itself_with_the_work_running_says_so_in_the_pause_note',
  },
  {
    name: 'background: a note quotes a key or a token of a command as it is',
    edits: [
      [
        `${ENGINE}/background.rs`,
        '            let hidden = mask(&process.command);',
        lines(
          '            let hidden = crate::shown::mask::Masked {',
          '                text: process.command.clone(),',
          '                count: 0,',
          '            };',
        ),
      ],
    ],
    runs: [engine],
    meant: 'what_a_note_quotes_of_the_work_has_its_secrets_hidden_and_is_held_to_a_line',
  },
  {
    name: 'background: what a tree had before the turn is the turn’s',
    edits: [BEFORE_THE_TURN_IS_THE_TURNS],
    runs: [baseline, ['-p', 'cf-panes', '--lib', 'turn']],
    meant: 'what_the_tree_had_when_the_turn_began_is_not_the_turns',
  },
  {
    name: 'background: a window’s exit leaves out the work it left running',
    edits: [EXIT_LEAVES_THE_WORK_OUT],
    runs: [wire],
    meant:
      'a_turn_marked_over_the_wire_lists_what_it_left_and_the_exit_of_the_window_closed_says_so',
  },
  {
    name: 'close: a window is killed without being asked to end or given a grace',
    edits: [KILLED_WITHOUT_BEING_ASKED],
    runs: [closing],
    meant:
      'a_program_that_traps_the_request_to_end_runs_its_handler_and_the_close_does_not_wait_out_the_grace',
  },
  {
    name: 'close: a close waits the whole grace out though everything has ended',
    edits: [
      [
        `${PANES}/pty/close.rs`,
        lines('        if pane.is_gone(asked) {', '            return;', '        }'),
        lines('        if pane.is_gone(asked) && false {', '            return;', '        }'),
      ],
    ],
    runs: [closing],
    meant:
      'a_program_that_traps_the_request_to_end_runs_its_handler_and_the_close_does_not_wait_out_the_grace',
  },
  {
    name: 'close: a table that goes waits on each window in turn',
    edits: [
      [
        `${PANES}/pty.rs`,
        lines(
          '        for (pane, _) in ask_all_to_exit(panes.values_mut(), self.close_grace) {',
          '            let _ = terminate_asked(pane);',
          '        }',
        ),
        lines(
          '        for pane in panes.values_mut() {',
          '            let _ = terminate(pane, self.close_grace);',
          '        }',
        ),
      ],
    ],
    runs: [closing],
    meant: 'a_table_that_goes_asks_every_window_to_end_before_it_waits_on_any',
  },
  {
    name: 'close: a shutdown closes the windows one after the other',
    edits: [
      [
        `${PANES}/headless.rs`,
        lines(
          '        std::thread::scope(|scope| {',
          '            for pane in open {',
          '                scope.spawn(move || {',
          '                    let _ = panes.kill(&PaneKey::new(pane.id, pane.generation));',
          '                });',
          '            }',
          '        });',
        ),
        lines(
          '        for pane in open {',
          '            let _ = panes.kill(&PaneKey::new(pane.id, pane.generation));',
          '        }',
        ),
      ],
    ],
    runs: [closing],
    meant: 'every_window_of_a_table_a_shutdown_closes_gets_its_grace_at_once',
  },
  {
    name: 'close: a quit gives the windows no longer than the drain, so their programs are killed in it',
    edits: [
      [
        'app/src-tauri/src/runtime.rs',
        'finish_before_deadline(SHUTDOWN_REAP, move || reap_all(&panes))',
        'finish_before_deadline(SHUTDOWN_DRAIN, move || reap_all(&panes))',
      ],
    ],
    runs: [quit],
    meant: 'quitting_gives_the_windows_programs_their_grace_before_it_kills_them',
  },
  {
    name: 'background: the board does not say why a window that works does',
    edits: [
      [
        'app/ui/core/board.js',
        "if (state === 'working' && activity.reason) return [state,",
        'if (false) return [state,',
      ],
    ],
    runs: [{ node: ['tests/board-ui.test.mjs'] }],
    meant: 'says that the window works, and on what',
  },

  // The same rules end to end, through the real daemon and pane host: each of
  // these runs only the case of the rig that holds the rule.
  {
    name: 'rig: a turn that ends with its job still running is its task’s result',
    edits: [TURN_END_IS_THE_RESULT],
    runs: [HOLD],
    meant: HOLD_MEANT,
  },
  {
    name: 'rig: a launch’s first message is never marked when it shows in the record',
    edits: [LAUNCH_NEVER_MARKED],
    runs: [HOLD],
    meant: HOLD_MEANT,
  },
  {
    name: 'rig: a job that has ended holds its task for ever',
    edits: [
      [
        `${ENGINE}/background.rs`,
        lines('        part.held = None;', '        Ok(Left::Nothing)'),
        '        Ok(Left::Ended)',
      ],
    ],
    runs: [SETTLE],
    meant: SETTLE_MEANT,
  },
  {
    name: 'rig: what the window had before its turn is the turn’s',
    edits: [BEFORE_THE_TURN_IS_THE_TURNS],
    runs: [BEFORE_THE_TURN],
    meant: BEFORE_THE_TURN_MEANT,
  },
  {
    name: 'rig: a cancelled task’s window closes and says nothing of the work it ended',
    edits: [CANCEL_SAYS_NOTHING],
    runs: [CANCEL],
    meant: CANCEL_MEANT,
  },
  {
    name: 'rig: a window’s exit leaves out the work it left running',
    edits: [EXIT_LEAVES_THE_WORK_OUT],
    runs: [CANCEL],
    meant: CANCEL_MEANT,
  },
  {
    name: 'rig: a paused task’s window is closed with it',
    edits: [
      [
        'crates/cf-ledger/src/staff/sessions.rs',
        '        &[&HELD_TASK_STATES[..], &["paused"]].concat(),',
        '        &HELD_TASK_STATES[..],',
      ],
    ],
    runs: [PAUSE],
    meant: PAUSE_MEANT,
  },
  {
    name: 'rig: a window is killed without being asked to end or given a grace',
    edits: [KILLED_WITHOUT_BEING_ASKED],
    runs: [GRACE],
    meant: GRACE_MEANT,
  },
]
