/**
 * Plants bugs in the Rust daemon, one at a time, and checks that a test catches
 * each. A plant is a few pieces of text replaced in the sources; the tests that
 * should notice are run (never in parallel: the sources are changed under them)
 * and each plant is reported caught or missed. Every file a plant touches is
 * first copied outside the repository and is put back from that copy, byte for
 * byte, whatever the run came to, on Ctrl-C and on being terminated too; a run
 * killed past that leaves the copies in the folder it says first.
 *
 *   npm run plants:daemon                  # every plant
 *   npm run plants:daemon -- stop door     # the plants whose names hold a word
 *   npm run plants:daemon -- --check       # only that every plant still applies
 *
 * A plant that stops applying (the text it replaces was changed) is an error to
 * mend here, not a pass. One that does not compile, and one that makes a test
 * wait for ever (its run is ended after five minutes: a test that hangs where
 * it should fail), count as missed. Exit code 1 if any plant is missed or does
 * not apply.
 */
import { spawn } from 'node:child_process'
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
/** The longest one run of tests may take, in milliseconds: the slowest takes a minute. */
const RUN_LIMIT = 5 * 60 * 1000

const lines = (...text) => text.join('\n')
const DAEMON = 'crates/cf-daemon/src'
const unit = (crate, ...filter) => ['-p', crate, '--lib', ...filter]
const daemon = (...filter) => unit('cf-daemon', ...filter)
const stop = ['-p', 'cf', '--test', 'daemon_stop']

/**
 * `name`, which says what is wrong with the code once planted; `edits`, the
 * text replaced, each in one file where it is found exactly once; `runs`, the
 * arguments of the `cargo test` that should fail, tried in order until one
 * does; `meant`, the test that was written for it.
 */
const PLANTS = [
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
        '        drop(tokio::task::spawn_local(async move { run(&state) }));',
        '        run(&state);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_kick_runs_its_pass_from_the_turn_after_the_work_begun_just_after_it',
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
        '        let outcome = contain((state.work)()).await;',
        '        let outcome: Result<Result<(), String>, crate::errors::Panicked> = Ok((state.work)().await);',
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
        `${DAEMON}/start.rs`,
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
        `${DAEMON}/start.rs`,
        '            Ended::Failed(_) => watching.trip("the bridge failed"),',
        '            Ended::Failed(_) => {}',
      ],
    ],
    runs: [daemon('start::'), stop],
    meant: 'an_output_nobody_reads_stops_it_as_the_bridge_failing_while_its_input_stays_open',
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
  {
    name: 'door: it does not hear the daemon stopping',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        '    while answer.is_none() && Instant::now() < until && !context.closing.is_set() {',
        '    while answer.is_none() && Instant::now() < until {',
      ],
      [
        `${DAEMON}/api/routes/door.rs`,
        '            () = context.closing.wait() => {}',
        '            () = std::future::pending::<()>() => {}',
      ],
    ],
    runs: [daemon('door::')],
    meant: 'the_daemon_stopping_answers_a_waiting_door_at_once_with_what_there_is',
  },
  {
    name: 'door: it does not look again after a wait',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        lines(
          '        }',
          '        answer = context.ledger.borrow().answer_to(asked.id)?;',
          '    }',
        ),
        lines('        }', '    }'),
      ],
    ],
    runs: [daemon('door::')],
    meant: 'an_answer_that_comes_while_it_waits_is_found_at_the_next_poll',
  },
  {
    name: 'door: it waits as long as it is asked',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        'const MAX_WAIT_MS: f64 = 25_000.0;',
        'const MAX_WAIT_MS: f64 = 250_000.0;',
      ],
    ],
    runs: [daemon('door::')],
    meant: 'it_holds_no_more_than_25_seconds_however_much_is_asked',
  },
  {
    name: 'credentials: a token revoked still acts',
    edits: [
      [
        `${DAEMON}/api/credentials.rs`,
        '        self.by_digest.borrow_mut().remove(&digest(token));',
        '        let _ = token;',
      ],
    ],
    runs: [daemon('credentials::')],
    meant: 'a_token_names_one_participant_of_one_project_until_it_is_revoked',
  },
  {
    name: 'credentials: any word is the UI token',
    edits: [
      [
        `${DAEMON}/api/credentials.rs`,
        '    bool::from(presented.ct_eq(&token))',
        '    bool::from(presented.ct_eq(&presented))',
      ],
    ],
    runs: [daemon('credentials::')],
    meant: 'the_ui_token_matches_itself_only',
  },
  {
    name: 'body: two mebibytes exactly is too large',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        'if bytes.len() + chunk.len() > MAX_JSON_BYTES {',
        'if bytes.len() + chunk.len() >= MAX_JSON_BYTES {',
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_body_of_exactly_two_mebibytes_is_read_and_one_byte_more_is_too_large',
  },
  {
    name: 'body: a screen’s text is counted in bytes',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        '        units += read.encode_utf16().count();',
        '        units += read.len();',
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_screens_body_is_counted_in_utf16_units_not_in_bytes',
  },
  {
    name: 'server: a panic in a request is not contained',
    edits: [
      [
        `${DAEMON}/api/server.rs`,
        '        let outcome = contain(async {',
        '        let outcome = Ok::<_, crate::errors::Panicked>(async {',
      ],
      [
        `${DAEMON}/api/server.rs`,
        lines('        })', '        .await;', '        match outcome {'),
        lines('        }.await);', '        match outcome {'),
      ],
    ],
    runs: [daemon('server::')],
    meant: 'a_handler_that_panics_is_a_500_internal_and_the_front_goes_on',
  },
  {
    name: 'server: a client that leaves drops its handler',
    edits: [
      [
        `${DAEMON}/api/server.rs`,
        '    let handler = tokio::task::spawn_local(async move {',
        '    let handler = async move {',
      ],
      [
        `${DAEMON}/api/server.rs`,
        lines(
          '    });',
          '    let answer = handler',
          '        .await',
          '        .unwrap_or_else(|_| Failure::Internal("the request was cut short".to_owned()).answer());',
        ),
        lines('    };', '    let answer = handler.await;'),
      ],
    ],
    runs: [daemon('server::')],
    meant: 'a_client_that_leaves_does_not_drop_its_handler',
  },
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
    name: 'page: an operation that panics is not contained',
    edits: [
      [
        `${DAEMON}/page/mod.rs`,
        '    match contain(async move { serve(page, operation, body).await }).await {',
        '    match Ok::<_, crate::errors::Panicked>(serve(page, operation, body).await) {',
      ],
    ],
    runs: [daemon('page::')],
    meant: 'an_operation_that_panics_at_its_first_poll_answers_not_ok_and_the_bridge_goes_on',
  },
  {
    name: 'page: an operation that only reads wakes the dispatcher',
    edits: [['crates/cf-proto/src/page.rs', '                | Self::BoardGet\n', '']],
    runs: [['-p', 'cf-proto', 'page::'], daemon('page::')],
    meant: 'the_operations_that_only_read_wake_nothing',
  },
  {
    name: 'page: an operation is named otherwise than the page names it',
    edits: [
      [
        'crates/cf-proto/src/page.rs',
        '            Self::StaffLast => "staff.last",',
        '            Self::StaffLast => "staff.latest",',
      ],
    ],
    runs: [['-p', 'cf-proto', 'page::']],
    meant: 'the_operations_are_the_twenty_eight_the_app_forwards',
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
    name: 'cli: --json reads as --no-open',
    edits: [
      [
        `${DAEMON}/cli.rs`,
        lines(
          '                    if name == "json" {',
          '                        flags.json = true;',
        ),
        lines(
          '                    if name == "json" {',
          '                        flags.no_open = true;',
        ),
      ],
    ],
    runs: [daemon('cli::')],
    meant: 'the_two_options_and_positionals_are_read_as_parse_args_reads_them',
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
    runs: [['-p', 'cf-bridge', '--features', 'local', '--lib', 'local::']],
    meant: 'the_end_of_the_input_is_the_input',
  },
  {
    name: 'engine: a participant is kept held by work that panicked',
    edits: [
      [
        'crates/cf-engine/src/runtime.rs',
        lines('    fn drop(&mut self) {', '        if self.acting {'),
        lines(
          '    fn drop(&mut self) {',
          '        if std::thread::panicking() {',
          '            return;',
          '        }',
          '        if self.acting {',
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
    name: 'cf: any value of CONSENSFLOW_DAEMON is the native daemon',
    edits: [
      [
        'crates/cf/src/lib.rs',
        '        && env.text("CONSENSFLOW_DAEMON") == Some("native")',
        '        && env.text("CONSENSFLOW_DAEMON").is_some()',
      ],
    ],
    runs: [[...stop, 'without_the_switch']],
    meant: 'without_the_switch_cf_ui_is_not_the_native_daemon',
  },
  {
    name: 'cf: a window’s cf ui is the daemon',
    edits: [
      [
        'crates/cf/src/lib.rs',
        lines(
          '        && env.text("CONSENSFLOW_DAEMON") == Some("native")',
          '        && env.text("CONSENSFLOW_TOKEN").is_none();',
        ),
        '        && env.text("CONSENSFLOW_DAEMON") == Some("native");',
      ],
    ],
    runs: [[...stop, 'a_window_s_cf_ui']],
    meant: 'a_window_s_cf_ui_is_the_board_and_never_the_daemon',
  },
  // The numbers Node has, each changed in turn: a test that says them in terms
  // of the constant itself holds nothing.
  {
    name: 'constant: a pass runs every two seconds',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        'pub const PASS: Duration = Duration::from_millis(1000);',
        'pub const PASS: Duration = Duration::from_millis(2000);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_pass_runs_a_second_after_the_start_and_then_every_second',
  },
  {
    name: 'constant: a pass is slow at ten seconds',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        'pub const SLOW_PASS: Duration = Duration::from_millis(5_000);',
        'pub const SLOW_PASS: Duration = Duration::from_millis(10_000);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_pass_longer_than_five_seconds_is_written_down_and_one_of_five_is_not',
  },
  {
    name: 'constant: the daemon says it is alive every hour',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        'pub const HEARTBEAT: Duration = Duration::from_secs(10 * 60);',
        'pub const HEARTBEAT: Duration = Duration::from_secs(60 * 60);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'every_ten_minutes_a_line_says_it_is_alive_how_its_passes_went_and_how_big_it_is',
  },
  {
    name: 'constant: a stop waits half a minute for a pass',
    edits: [
      [
        `${DAEMON}/pass.rs`,
        'pub const STOP_WAIT: Duration = Duration::from_millis(1_000);',
        'pub const STOP_WAIT: Duration = Duration::from_millis(30_000);',
      ],
    ],
    runs: [daemon('pass::')],
    meant: 'a_stop_waits_only_a_moment_for_a_pass_held_up_by_a_slow_window',
  },
  {
    name: 'constant: a door polls every two and a half seconds',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        'const POLL: Duration = Duration::from_millis(250);',
        'const POLL: Duration = Duration::from_millis(2500);',
      ],
    ],
    runs: [daemon('door::')],
    meant: 'an_answer_that_comes_while_it_waits_is_found_at_the_next_poll',
  },
  {
    name: 'constant: a connection idles for a minute',
    edits: [
      [
        `${DAEMON}/api/server.rs`,
        'const IDLE: Duration = Duration::from_secs(5);',
        'const IDLE: Duration = Duration::from_secs(60);',
      ],
    ],
    runs: [daemon('server::')],
    meant: 'a_connection_waits_five_seconds_for_its_next_request_as_node_s_keep_alive_did',
  },
  {
    name: 'constant: the page is told a second after a change',
    edits: [
      [
        `${DAEMON}/start.rs`,
        'const STATE_EVENT: Duration = Duration::from_millis(100);',
        'const STATE_EVENT: Duration = Duration::from_millis(1000);',
      ],
    ],
    runs: [daemon('start::')],
    meant:
      'the_page_is_told_the_board_moved_a_hundred_milliseconds_after_the_first_change_and_once',
  },
  {
    name: 'constant: a preview is sixteen hundred units',
    edits: [
      [
        `${DAEMON}/api/views.rs`,
        'const PREVIEW_UNITS: usize = 160;',
        'const PREVIEW_UNITS: usize = 1600;',
      ],
    ],
    runs: [daemon('api::')],
    meant: 'the_preview_is_the_first_line_cut_at_160_utf16_units',
  },
  {
    name: 'constant: the log and the trace keep fifty megabytes',
    edits: [
      [
        `${DAEMON}/files/log.rs`,
        'pub const LIMIT: u64 = 5_000_000;',
        'pub const LIMIT: u64 = 50_000_000;',
      ],
      [
        `${DAEMON}/files/trace.rs`,
        'pub const LIMIT: u64 = 5_000_000;',
        'pub const LIMIT: u64 = 50_000_000;',
      ],
    ],
    runs: [daemon('files::')],
    meant: 'the_log_of_a_home_is_moved_aside_past_five_million_bytes_and_not_at_them',
  },
  {
    name: 'constant: an agents’ request may hold four mebibytes',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        'pub const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;',
        'pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;',
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_body_of_exactly_two_mebibytes_is_read_and_one_byte_more_is_too_large',
  },
  {
    name: 'constant: a screen’s request may hold 128 K units',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        'pub const MAX_TEXT_UNITS: usize = 64 * 1024;',
        'pub const MAX_TEXT_UNITS: usize = 128 * 1024;',
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_screens_body_counts_the_chunks_together_and_stops_at_the_limit',
  },
  {
    name: 'constant: the open deadline is ten minutes',
    edits: [
      [
        `${DAEMON}/host.rs`,
        'pub const OPEN_DEADLINE: Duration = Duration::from_secs(60);',
        'pub const OPEN_DEADLINE: Duration = Duration::from_secs(600);',
      ],
    ],
    runs: [daemon('host::')],
    meant: 'an_open_the_host_never_answers_is_refused_as_the_deadline_after_sixty_seconds',
  },
  {
    name: 'constant: the stop waits half a minute',
    edits: [
      [
        `${DAEMON}/stop.rs`,
        'pub const DEADLINE: Duration = Duration::from_secs(1);',
        'pub const DEADLINE: Duration = Duration::from_secs(30);',
      ],
    ],
    runs: [daemon('stop::')],
    meant:
      'a_pass_that_never_ends_a_door_an_idle_connection_and_a_body_half_sent_cost_one_deadline',
  },
]

const args = process.argv.slice(2)
const checkOnly = args.includes('--check')
const words = args.filter((arg) => !arg.startsWith('--'))
const chosen = PLANTS.filter(
  (plant) => words.length === 0 || words.some((word) => plant.name.includes(word)),
)

/** `text` with `from` replaced by `to` where it is found exactly once. */
function replaced(plant, file, text, from, to) {
  const found = text.split(from).length - 1
  if (found !== 1) {
    throw new Error(`${plant.name}: ${file} holds ${JSON.stringify(from)} ${found} times, not once`)
  }
  return text.replace(from, () => to)
}

/** The text of each file `plant` touches once planted, by file. */
function planted(plant) {
  const files = new Map()
  for (const [file, from, to] of plant.edits) {
    const text = files.get(file) ?? readFileSync(join(REPO, file), 'utf8')
    files.set(file, replaced(plant, file, text, from, to))
  }
  return files
}

if (checkOnly) {
  for (const plant of chosen) planted(plant)
  process.stdout.write(`${chosen.length} plants apply\n`)
  process.exit(0)
}

const saved = mkdtempSync(join(tmpdir(), 'cf-plants-'))
process.stdout.write(`copies of what is planted are kept in ${saved}\n`)
/** What is planted now: its files, with the copies they go back from. */
let planting = null
let running = null

/** Puts every file of the plant in hand back from its copy, and says if one was not. */
function restore() {
  if (planting === null) return
  const { copies } = planting
  planting = null
  for (const { path, copy, original } of copies) {
    copyFileSync(copy, path)
    if (!readFileSync(path).equals(original)) {
      throw new Error(`${path} is not as it was: its copy is ${copy}`)
    }
  }
}

/** Ends the run of tests in hand, with every process it started. */
function endRun() {
  if (running === null) return
  try {
    process.kill(-running.pid, 'SIGKILL')
  } catch {}
}

function leave(code) {
  endRun()
  restore()
  rmSync(saved, { recursive: true, force: true })
  process.exit(code)
}
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(signal, () => leave(130))

/**
 * One `cargo test`: its output, and what caught the plant if anything did: the
 * tests it says failed, or the run itself where a test binary died with no
 * test left to say so (a signal nothing was ready for).
 */
function cargoTest(runArgs) {
  return new Promise((resolve) => {
    const child = spawn('cargo', ['test', '--offline', ...runArgs], {
      cwd: REPO,
      detached: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    running = child
    let output = ''
    child.stdout.on('data', (data) => {
      output += data
    })
    child.stderr.on('data', (data) => {
      output += data
    })
    let hung = false
    const late = setTimeout(() => {
      hung = true
      endRun()
    }, RUN_LIMIT)
    child.on('close', (code) => {
      clearTimeout(late)
      running = null
      const compiled = !output.includes('could not compile')
      const caught = [...output.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((hit) => hit[1])
      const died = output.match(/process didn't exit successfully: `[^`]*` \(([^)]*)\)/)
      if (compiled && !hung && code !== 0 && caught.length === 0) {
        caught.push(`${runArgs.join(' ')}: the test binary died${died ? ` (${died[1]})` : ''}`)
      }
      resolve({ output, caught, compiled, hung, runArgs })
    })
  })
}

async function trial(plant) {
  const copies = []
  const edited = planted(plant)
  for (const [file, text] of edited) {
    const path = join(REPO, file)
    const copy = join(saved, `${copies.length}-${file.replaceAll('/', '_')}`)
    copyFileSync(path, copy)
    copies.push({ path, copy, original: readFileSync(path) })
    planting = { copies }
    writeFileSync(path, text)
  }
  try {
    let ran = null
    for (const runArgs of plant.runs) {
      ran = await cargoTest(runArgs)
      if (!ran.compiled) return { verdict: 'does not compile', ran }
      if (ran.hung) return { verdict: 'hung', ran }
      if (ran.caught.length > 0) return { verdict: 'caught', ran }
    }
    return { verdict: 'missed', ran }
  } finally {
    restore()
  }
}

let wrong = 0
for (const plant of chosen) {
  const started = Date.now()
  const { verdict, ran } = await trial(plant)
  const seconds = ((Date.now() - started) / 1000).toFixed(0)
  const by =
    verdict === 'caught'
      ? ` by ${ran.caught[0]}${ran.caught.length > 1 ? ` (+${ran.caught.length - 1})` : ''}${
          ran.caught.some((name) => name.endsWith(plant.meant)) ? '' : `, not by ${plant.meant}`
        }`
      : ''
  if (verdict !== 'caught') wrong += 1
  process.stdout.write(`${verdict.toUpperCase().padEnd(16)} ${plant.name} (${seconds} s)${by}\n`)
  if (verdict === 'does not compile') process.stdout.write(`${ran.output.slice(-1500)}\n`)
  if (verdict === 'hung') {
    process.stdout.write(`    ${ran.runArgs.join(' ')} did not end in ${RUN_LIMIT / 1000} s\n`)
  }
  if (verdict === 'missed') {
    // What the last run ran, to tell a test that passed from none that ran.
    const summary = ran.output
      .split('\n')
      .filter((line) => /^(running \d+ test|test result:)/.test(line))
    process.stdout.write(`${summary.map((line) => `    ${line}`).join('\n')}\n`)
  }
}
rmSync(saved, { recursive: true, force: true })
process.stdout.write(`${chosen.length - wrong} of ${chosen.length} plants caught\n`)
process.exit(wrong === 0 ? 0 : 1)
