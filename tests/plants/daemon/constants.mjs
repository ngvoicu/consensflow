/** The numbers Node has, each changed in turn: a test that says them in terms of the constant itself holds nothing. */
import { DAEMON, daemon } from './kit.mjs'

export const PLANTS = [
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
