/** What the daemon is made of: where a panic is written down, the engine’s work, the host, the files, the bridge, the engine’s runtime, the processes, `cf`. */
import { bridge, DAEMON, daemon, lines, stop, unit } from './kit.mjs'

export const PLANTS = [
  {
    name: 'errors: a panic is not traced as a daemon error',
    edits: [
      [
        `${DAEMON}/errors.rs`,
        '        self.trace.error(&format!("{what}: {}", panicked.message));',
        '',
      ],
    ],
    runs: [daemon('errors::')],
    meant: 'a_caught_panic_goes_to_the_log_with_its_place_and_to_the_trace_as_a_daemon_error',
  },
  {
    name: 'errors: work that goes on apart is not contained',
    edits: [
      [
        `${DAEMON}/errors.rs`,
        lines(
          '            if let Err(panicked) = contain(work).await {',
          '                errors.caught(what, &panicked);',
          '            }',
        ),
        '            work.await;',
      ],
    ],
    runs: [daemon('errors::')],
    meant: 'work_that_goes_on_apart_is_caught_and_written_down_and_the_rest_goes_on',
  },
  {
    name: 'seams: the engine’s work that panics is not written down',
    edits: [
      [
        `${DAEMON}/seams.rs`,
        lines(
          '            if let Err(panicked) = contain(work).await {',
          '                errors.caught(what, &panicked);',
          '            }',
        ),
        '            work.await;',
      ],
    ],
    runs: [daemon('seams::')],
    meant: 'a_panic_in_work_apart_is_written_down_and_the_work_after_it_runs',
  },
  {
    name: 'seams: a drain runs nothing',
    edits: [
      [`${DAEMON}/seams.rs`, '        self.executor.drain();', '        let _ = &self.executor;'],
    ],
    runs: [daemon('seams::')],
    meant: 'work_spawned_waits_for_a_drain_and_a_drain_runs_it_in_the_order_it_was_woken',
  },
  {
    name: 'seams: a wait is taken by a poll that is not its relay’s',
    edits: [
      [
        `${DAEMON}/seams/boundary.rs`,
        '        if this.relay.is_some() && !this.relayed.get() {',
        '        if false {',
      ],
    ],
    runs: [daemon('contract::callbacks'), daemon('contract::looks')],
    meant: 'two_answers_that_come_between_begin_and_the_executors_first_poll_are_two_callbacks',
  },
  {
    name: 'seams: a wait its relay woke with nothing to say is taken again without its relay',
    edits: [
      [
        `${DAEMON}/seams/boundary.rs`,
        lines(
          '            Poll::Pending => {',
          '                this.relayed.set(false);',
          '                if this.relay.is_none() {',
        ),
        lines('            Poll::Pending => {', '                if this.relay.is_none() {'),
      ],
    ],
    runs: [daemon('seams::')],
    meant: 'a_wait_its_relay_woke_with_nothing_to_say_has_its_answer_from_its_relay_again',
  },
  {
    name: 'host: the daemon’s bridge does not run what the frames woke after a read',
    edits: [
      [
        `${DAEMON}/host.rs`,
        '    BridgeBuilder::new(Role::Daemon).after_read(move || spawn.drain())',
        lines('    drop(spawn);', '    BridgeBuilder::new(Role::Daemon)'),
      ],
    ],
    runs: [daemon('host::')],
    meant: 'a_chain_of_the_engines_turns_a_frame_began_ends_before_the_next_frame_is_handled',
  },
  {
    name: 'seams: a window is not told its participant',
    edits: [
      [
        `${DAEMON}/seams.rs`,
        '"CONSENSFLOW_PARTICIPANT".to_owned()',
        '"CONSENSFLOW_PARTICIPANTS".to_owned()',
      ],
    ],
    runs: [daemon('seams::')],
    meant:
      'a_window_starts_with_its_url_project_participant_runtime_and_the_bundle_first_on_its_path',
  },
  {
    name: 'files: a cause is indented two spaces in the log',
    edits: [[`${DAEMON}/files/log.rs`, 'format!("    {line}")', 'format!("  {line}")']],
    runs: [['-p', 'cf-daemon', '--test', 'files']],
    meant: 'every_log_line_is_what_node_wrote',
  },
  {
    name: 'files: forgetting a project keeps its lines and drops the others',
    edits: [
      [
        `${DAEMON}/files/trace.rs`,
        '        .is_some_and(|named| named == wanted)',
        '        .is_some_and(|named| named != wanted)',
      ],
    ],
    runs: [['-p', 'cf-daemon', '--test', 'files'], daemon('files::')],
    meant: 'forgetting_a_project_drops_its_lines_from_both_files_and_keeps_the_rest',
  },
  {
    name: 'files: a file exactly at its limit is moved aside',
    edits: [['crates/cf-base/src/file/append.rs', 'if size > limit {', 'if size >= limit {']],
    runs: [
      ['-p', 'cf-base', '--lib', 'file::append'],
      ['-p', 'cf-daemon', '--test', 'files'],
    ],
    meant: 'a_file_past_the_limit_is_moved_aside_before_the_append_and_not_at_it',
  },
  {
    name: 'bridge: the end of the input is told as a close',
    edits: [
      [
        'crates/cf-bridge/src/local/state.rs',
        '        self.end(Ended::Input);',
        '        self.end(Ended::Closed);',
      ],
    ],
    runs: [bridge],
    meant: 'the_end_of_the_input_is_the_input',
  },
  {
    name: 'bridge: the hook is told after each frame and not after the read',
    edits: [
      [
        'crates/cf-bridge/src/local/reader.rs',
        lines(
          '            if self.inner.is_closed() {',
          '                return true;',
          '            }',
        ),
        lines(
          '            self.inner.read_handled();',
          '            if self.inner.is_closed() {',
          '                return true;',
          '            }',
        ),
      ],
    ],
    runs: [bridge, daemon('host::')],
    meant: 'it_is_told_once_after_all_the_frames_of_a_read_and_not_between_them',
  },
  {
    name: 'bridge: a read whose frames closed the bridge is not told',
    edits: [
      [
        'crates/cf-bridge/src/local/reader.rs',
        lines(
          '        bridge.inner.read_handled();',
          '        if closed {',
          '            return;',
          '        }',
        ),
        lines(
          '        if closed {',
          '            return;',
          '        }',
          '        bridge.inner.read_handled();',
        ),
      ],
    ],
    runs: [bridge],
    meant: 'a_read_whose_frames_closed_the_bridge_is_told_once_for_the_frames_before_the_close',
  },
  {
    name: 'bridge: the end of the input is told as a read',
    edits: [
      [
        'crates/cf-bridge/src/local/reader.rs',
        '                bridge.inner.eof();',
        lines(
          '                bridge.inner.read_handled();',
          '                bridge.inner.eof();',
        ),
      ],
    ],
    runs: [bridge],
    meant: 'the_end_of_the_input_handled_nothing_and_is_not_told',
  },
  {
    name: 'bridge: a read is not told at all',
    edits: [
      [
        'crates/cf-bridge/src/local/reader.rs',
        lines('        bridge.inner.read_handled();', '        if closed {'),
        '        if closed {',
      ],
    ],
    runs: [bridge],
    meant: 'it_is_told_once_after_all_the_frames_of_a_read_and_not_between_them',
  },
  {
    name: 'engine: a participant is kept held by work that panicked',
    edits: [
      [
        'crates/cf-engine/src/runtime.rs',
        lines('    fn drop(&mut self) {', '        self.held.set(false);'),
        lines(
          '    fn drop(&mut self) {',
          '        if std::thread::panicking() {',
          '            return;',
          '        }',
          '        self.held.set(false);',
        ),
      ],
    ],
    runs: [unit('cf-engine', 'runtime::')],
    meant:
      'work_that_panics_after_a_wait_lets_go_of_its_participant_and_the_next_waiter_takes_its_turn',
  },
  {
    name: 'engine: whoever waits for work that panicked waits for ever',
    edits: [
      [
        'crates/cf-engine/src/runtime.rs',
        '                    answer.fail(&panic_words(panic.as_ref()));',
        '',
      ],
    ],
    runs: [unit('cf-engine', 'runtime::')],
    meant: 'whoever_waits_for_work_that_panicked_ends_with_an_error_and_does_not_wait_for_ever',
  },
  {
    name: 'process: a forced end sends SIGTERM',
    edits: [
      [
        'crates/cf-process/src/terminate.rs',
        '            Ending::Forced => libc::SIGKILL,',
        '            Ending::Forced => libc::SIGTERM,',
      ],
    ],
    runs: [unit('cf-process', 'terminate::')],
    meant: 'ends_a_child_with_the_signal_asked_for',
  },
  {
    name: 'process: a program run to its end does not hand over its end',
    edits: [
      [
        'crates/cf-process/src/capture.rs',
        '    started(Ender::new(pid, &exited));',
        '    let _ = started;',
      ],
    ],
    runs: [unit('cf-process', 'capture::')],
    meant: 'a_program_is_handed_over_to_be_ended_while_it_runs_and_let_go_of_once_it_has_ended',
  },
  {
    name: 'process: a program run to its end (run) is not kept for the way out',
    edits: [
      [
        'crates/cf-harness/src/seams/processes.rs',
        lines(
          '            let kept = |ender| self.keep(ender);',
          '            cf_process::execute(&run, program.cwd.as_deref(), &env, limits, kept).await',
        ),
        lines(
          '            let kept = |_ender| ();',
          '            cf_process::execute(&run, program.cwd.as_deref(), &env, limits, kept).await',
        ),
      ],
    ],
    runs: [unit('cf-harness', 'seams::processes'), daemon('stop::')],
    meant: 'programs_run_to_their_end_are_kept_while_they_run_and_let_go_of_once_they_have_ended',
  },
  {
    name: 'process: a program captured (an update) is not kept for the way out',
    edits: [
      [
        'crates/cf-harness/src/seams/processes.rs',
        lines(
          '            let kept = |ender| self.keep(ender);',
          '            cf_process::capture(&run, program.cwd.as_deref(), &env, limits, kept).await',
        ),
        lines(
          '            let kept = |_ender| ();',
          '            cf_process::capture(&run, program.cwd.as_deref(), &env, limits, kept).await',
        ),
      ],
    ],
    runs: [stop, daemon('stop::')],
    meant: 'a_program_the_daemon_is_waiting_for_is_ended_when_it_stops',
  },
  {
    name: 'process: programs that ended are kept for the way out',
    edits: [
      [
        'crates/cf-harness/src/seams/processes.rs',
        lines('        started.retain(Ender::running);', '        started.push(ender);'),
        '        started.push(ender);',
      ],
    ],
    runs: [unit('cf-harness', 'seams::processes')],
    meant: 'programs_run_to_their_end_are_kept_while_they_run_and_let_go_of_once_they_have_ended',
  },
  {
    name: 'process: a size in megabytes is rounded down',
    edits: [
      [
        'crates/cf-process/src/memory.rs',
        '    (bytes + 524_288) / 1_048_576',
        '    bytes / 1_048_576',
      ],
    ],
    runs: [unit('cf-process', 'memory::')],
    meant: 'megabytes_are_rounded_as_math_round_rounds_them',
  },
  {
    name: 'cf: ui is the native daemon though the home has taken the way back',
    edits: [['crates/cf/src/lib.rs', ' && !way_back::choose(env).node;', ';']],
    runs: [[...stop, 'with_the_way_back']],
    meant: 'with_the_way_back_cf_ui_is_not_the_native_daemon_whatever_the_old_switch_says',
  },
  {
    name: 'cf: a window’s cf ui is the daemon',
    edits: [
      [
        'crates/cf/src/lib.rs',
        'first == "ui" && env.text("CONSENSFLOW_TOKEN").is_none()',
        'first == "ui"',
      ],
    ],
    runs: [[...stop, 'a_window_s_cf_ui']],
    meant: 'a_window_s_cf_ui_is_the_board_and_never_the_daemon',
  },
]
