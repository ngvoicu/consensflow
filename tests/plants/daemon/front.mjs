/** The HTTP front and the page: the door, the tokens, the bodies, the server, the page’s operations, the verb. */
import { DAEMON, daemon, lines } from './kit.mjs'

export const PLANTS = [
  {
    name: 'door: it does not hear the daemon stopping',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        lines(
          '        if context.closing.is_set()',
          '            || request.consumer().has_left()',
          '            || context.credentials.resolve(request.bearer()).is_none()',
          '        {',
        ),
        lines(
          '        if request.consumer().has_left()',
          '            || context.credentials.resolve(request.bearer()).is_none()',
          '        {',
        ),
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
        lines('            () = request.consumer().left() => {}', '        }', '    }'),
        lines(
          '            () = request.consumer().left() => {}',
          '        }',
          '        return unanswered(&asked);',
          '    }',
        ),
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
    name: 'body: the end of a body is not a callback of its own',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        lines('            drop(sender);', '            arrived();'),
        '            drop(sender);',
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_handler_goes_on_from_the_end_of_its_body_before_a_task_that_was_runnable_already',
  },
  {
    name: 'body: a result whose body ends as its window exits is refused',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        lines('            drop(sender);', '            arrived();'),
        '            drop(sender);',
      ],
    ],
    runs: [daemon('contract::requests')],
    meant: 'a_result_whose_body_ends_as_its_window_exits_is_recorded_before_the_exit',
  },
  {
    name: 'body: a body that broke is not closed and drained',
    edits: [
      [
        `${DAEMON}/api/body.rs`,
        lines('                if broke {', '                    break;'),
        lines('                if broke {', '                    return;'),
      ],
    ],
    runs: [daemon('body::')],
    meant: 'a_handler_goes_on_from_the_end_of_its_body_before_a_task_that_was_runnable_already',
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
        '    let begun = begin(&*server.spawn, async move {',
        '    let begun = (async move {',
      ],
      [
        `${DAEMON}/api/server.rs`,
        lines('    })', '    .await;', '    server.spawn.drain();'),
        lines('    });', '    server.spawn.drain();'),
      ],
    ],
    runs: [daemon('server::')],
    meant: 'a_client_that_leaves_does_not_drop_its_handler',
  },
  {
    name: 'server: what a request’s first part woke is not run before the next callback',
    edits: [
      [
        `${DAEMON}/api/server.rs`,
        lines('    server.spawn.drain();', '    response(&begun.await)'),
        '    response(&begun.await)',
      ],
    ],
    runs: [daemon('server::')],
    meant:
      'a_requests_engine_work_is_begun_in_its_first_part_and_its_chain_ends_before_the_next_callback',
  },
  {
    name: 'page: an operation that panics is not contained',
    edits: [
      [
        `${DAEMON}/page/mod.rs`,
        '        contain(async move { serve(page, operation, body).await }),',
        '        async move { Ok::<_, crate::errors::Panicked>(serve(page, operation, body).await) },',
      ],
    ],
    runs: [daemon('page::')],
    meant: 'an_operation_that_panics_at_its_first_poll_answers_not_ok_and_the_bridge_goes_on',
  },
  {
    name: 'page: an operation is not begun on the executor',
    edits: [
      [
        `${DAEMON}/page/mod.rs`,
        lines(
          '    let begun = begin(',
          '        spawn,',
          '        contain(async move { serve(page, operation, body).await }),',
          '    )',
          '    .await;',
          '    match begun.await {',
        ),
        '    match contain(async move { serve(page, operation, body).await }).await {',
      ],
    ],
    runs: [daemon('page::')],
    meant: 'an_operation_is_begun_where_its_frame_is_read_and_its_turns_end_before_the_next_frame',
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
]
