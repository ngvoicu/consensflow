/** The daemon’s run: the pass loop and the throttles, the stop, the start. */
import { DAEMON, daemon, lines, stop } from './kit.mjs'

export const PLANTS = [
  {
    name: 'pass: a kick during a pass is lost',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        lines('    if *state.running.borrow() {', '        state.again.set(true);'),
        '    if *state.running.borrow() {',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'one_pass_at_a_time_and_kicks_during_a_pass_run_one_more_after_it',
  },
  {
    name: 'pass: a kick runs the pass inside the call',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        '        schedule(&self.state);',
        '        let _ = futures_util::FutureExt::now_or_never(run(&self.state));',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_kick_runs_its_pass_from_the_turn_after_the_work_begun_just_after_it',
  },
  {
    name: 'pass: what a pass’s first part woke is not run where the pass was begun',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        lines('    drop(begun);', '    state.spawn.drain();'),
        '    drop(begun);',
      ],
    ],
    runs: [daemon('pass::')],
    meant:
      'a_pass_a_kick_begins_does_its_first_part_there_and_what_it_woke_is_run_before_the_next_callback',
  },
  {
    name: 'pass: a pass of exactly five seconds is slow',
    edits: [[`${DAEMON}/pass.rs`, 'if took > SLOW_PASS {', 'if took >= SLOW_PASS {']],
    runs: [daemon('pass::')],
    meant: 'a_pass_longer_than_five_seconds_is_written_down_and_one_of_five_is_not',
  },
  {
    name: 'pass: a panic in a pass is not contained',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        '        let outcome = contain((pass.work)()).await;',
        '        let outcome: Result<Result<(), String>, crate::errors::Panicked> = Ok((pass.work)().await);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_pass_that_panics_is_written_down_traced_and_said_and_the_next_one_runs',
  },
  {
    name: 'throttle: every call begins a wait of its own',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        lines('        if pending.replace(true) {', '            return;', '        }'),
        '        pending.replace(true);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'the_throttle_tells_once_a_wait_after_the_first_call_whatever_came_between',
  },
  {
    name: 'stop: the last reason wins',
    edits: [
      [
        `${DAEMON}/stop.rs`,
        lines(
          '            if reason.is_some() {',
          '                return false;',
          '            }',
          '',
        ),
        '',
      ],
    ],
    runs: [daemon('stop::')],
    meant: 'the_first_reason_wins_and_the_daemon_stops_for_that_one_alone',
  },
  {
    name: 'stop: the process exits 1',
    edits: [[`${DAEMON}/stop.rs`, '        (self.exit)(0);', '        (self.exit)(1);']],
    runs: [daemon('stop::'), stop],
    meant: 'a_stop_says_why_and_how_big_the_daemon_is_and_ends_the_log_with_exit_0',
  },
  {
    name: 'stop: the children are left running',
    edits: [[`${DAEMON}/stop.rs`, 'contain_now(|| (self.ends_children)())', 'contain_now(|| ())']],
    runs: [daemon('stop::')],
    meant: 'the_children_still_running_are_ended_on_the_way_out',
  },
  {
    name: 'stop: a panic in the tail ends the tail',
    edits: [
      [
        `${DAEMON}/stop.rs`,
        lines(
          '        if let Err(panicked) = contain_now(|| (self.ends_children)()) {',
          '            self.errors.caught("the children did not end", &panicked);',
          '        }',
        ),
        '        (self.ends_children)();',
      ],
    ],
    runs: [daemon('stop::')],
    meant: 'children_that_cannot_be_ended_are_written_down_and_the_ledger_is_still_closed',
  },
  {
    name: 'stop: the ledger is not closed in place',
    edits: [
      [
        'crates/cf-ledger/src/ledger.rs',
        lines(
          '        let open = std::mem::replace(&mut self.store.db, Connection::open_in_memory()?);',
          '        open.close().map_err(|(_, cause)| cause.into())',
        ),
        '        Ok(())',
      ],
    ],
    runs: [['-p', 'cf-ledger', '--test', 'opening'], daemon('stop::')],
    meant: 'closes_in_place_where_it_is_shared_and_frees_the_file_with_its_data_kept',
  },
  {
    name: 'start: SIGTERM and SIGINT are not listened for',
    edits: [
      [
        `${DAEMON}/stop.rs`,
        lines('    if signals {', '        listen_for_signals(latch, errors);', '    }'),
        '    let _ = signals;',
      ],
    ],
    // Only a process has signals of its own: the daemon of a test is told to stop.
    runs: [stop],
    meant: 'a_quiet_daemon_starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped',
  },
  {
    name: 'start: a bridge that failed does not stop the daemon',
    edits: [
      [
        `${DAEMON}/start.rs`,
        lines(
          '                log.error("the bridge failed", Some(&cause.to_string()));',
          '                latch.trip("the bridge failed");',
        ),
        '                log.error("the bridge failed", Some(&cause.to_string()));',
      ],
      [
        `${DAEMON}/stop.rs`,
        '            Ended::Failed(_) => watching.trip("the bridge failed"),',
        '            Ended::Failed(_) => {}',
      ],
    ],
    runs: [daemon('start::'), stop],
    meant: 'an_output_nobody_reads_stops_it_as_the_bridge_failing_while_its_input_stays_open',
  },
  {
    name: 'start: the executor’s driver is not spawned',
    edits: [[`${DAEMON}/start.rs`, '    spawn.drive();', '']],
    runs: [daemon('start::')],
    meant: 'the_engine_s_work_is_driven_from_the_start_so_what_is_woken_outside_a_drain_runs',
  },
  {
    name: 'start: what was open is not suspended for the restart',
    edits: [[`${DAEMON}/start.rs`, '    ledger.borrow_mut().suspend_for_restart()?;', '']],
    runs: [daemon('start::')],
    meant: 'what_was_open_comes_back_and_its_window_opens_with_the_environment_the_app_expects',
  },
  {
    name: 'start: an agents file that cannot be used stops the start',
    edits: [
      [
        `${DAEMON}/start.rs`,
        '        log.error("the agents file could not be used", Some(&cause));',
        '        return Err(StartError::System(cause));',
      ],
    ],
    runs: [daemon('start::')],
    meant: 'an_agents_file_that_cannot_be_used_stops_no_start_and_the_log_says_why',
  },
]
