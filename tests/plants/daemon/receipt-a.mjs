/**
 * The receipt and stop redesign's first fix round, part A: a read is no
 * receipt (`cf` says what it wrote whole, once it has), a native answer is
 * received only after the handoff happened, a claim does not outlive its
 * request, and the gate holds the brief back from the agents' API. One plant
 * takes one fix out, and a test of it fails. A run written `{ node: [...] }`
 * is `node --test` of those files.
 */
import { DAEMON, daemon, lines, unit } from './kit.mjs'

const ENGINE = 'crates/cf-engine/src'
const CODEX = 'crates/cf-codex-session/src'
const BOARD = 'crates/cf-board/src'
const CF = 'crates/cf/src'

/** The engine's dispatcher tests, whole. */
const dispatcher = ['-p', 'cf-engine', '--test', 'dispatcher']
const cfBoard = unit('cf', 'board::')
const opencode = { node: ['tests/opencode-extension.test.mjs'] }

export const PLANTS = [
  {
    name: 'read: a thread read receives the answers it shows',
    edits: [
      [
        `${DAEMON}/api/routes/task.rs`,
        '        return Ok(Answer::ok(json!({ "task": value(&task)? })));',
        lines(
          '        let ids: Vec<i64> = task',
          '            .messages',
          '            .iter()',
          '            .filter(|message| message.kind == "answer" && message.state == "queued")',
          '            .filter(|message| message.recipient_id == caller.participant.id)',
          '            .map(|message| message.id)',
          '            .collect();',
          '        let mut ledger = context.ledger.borrow_mut();',
          '        ledger.receive_read(caller.participant.id, &ids, cf_ledger::Read::Task)?;',
          '        drop(ledger);',
          '        return Ok(Answer::ok(json!({ "task": value(&task)? })));',
        ),
      ],
    ],
    runs: [daemon('routes::task')],
    meant:
      'a_worker_that_reads_its_task_and_is_refused_its_transcript_still_has_its_answer_to_be_pasted',
  },
  {
    name: 'read: a list receives the answers it shows',
    edits: [
      [
        `${DAEMON}/api/routes/inbox.rs`,
        '    Ok(Answer::ok(json!({ "messages": summaries })))',
        lines(
          '    let ids: Vec<i64> = messages',
          '        .iter()',
          '        .filter(|message| message.kind == "answer" && message.state == "queued")',
          '        .filter(|message| message.recipient_id == caller.participant.id)',
          '        .map(|message| message.id)',
          '        .collect();',
          '    let mut ledger = context.ledger.borrow_mut();',
          '    ledger.receive_read(caller.participant.id, &ids, cf_ledger::Read::Inbox)?;',
          '    drop(ledger);',
          '    Ok(Answer::ok(json!({ "messages": summaries })))',
        ),
      ],
    ],
    runs: [daemon('routes::inbox')],
    meant: 'a_list_receives_nothing_whatever_it_shows_and_the_answers_are_still_to_be_delivered',
  },
  {
    name: 'read: a message read receives the answer it shows',
    edits: [
      [
        `${DAEMON}/api/routes/message.rs`,
        '        Some(message) => Ok(Answer::ok(json!({ "message": value(&message)? }))),',
        lines(
          '        Some(message) => {',
          '            if message.kind == "answer" && message.recipient_id == caller.participant.id {',
          '                let mut ledger = context.ledger.borrow_mut();',
          '                ledger.receive_read(caller.participant.id, &[message.id], cf_ledger::Read::Inbox)?;',
          '            }',
          '            Ok(Answer::ok(json!({ "message": value(&message)? })))',
          '        }',
        ),
      ],
    ],
    runs: [daemon('routes::message')],
    meant:
      'an_answer_read_whole_by_the_one_it_is_for_is_served_and_nothing_is_received_by_serving_it',
  },
  {
    name: 'read: the board takes the word of anyone for any answer',
    edits: [
      [
        `${DAEMON}/api/routes/answers.rs`,
        lines('                && message.recipient_id == caller.participant.id', '        }) {'),
        '        }) {',
      ],
    ],
    runs: [daemon('routes::answers')],
    meant: 'what_is_not_a_queued_answer_for_the_caller_is_left_as_it_is_and_wakes_nothing',
  },
  {
    name: 'read: cf says what it wrote before it has written it',
    edits: [
      [
        `${CF}/board/mod.rs`,
        lines('        Ok(Said { data, text, wrote }) => {', '            let printed = if json {'),
        lines(
          '        Ok(Said { data, text, wrote }) => {',
          '            if let Some(wrote) = &wrote {',
          '                wrote.acknowledge(board);',
          '            }',
          '            let printed = if json {',
        ),
      ],
    ],
    runs: [cfBoard],
    meant:
      'a_task_thread_printed_whole_is_acknowledged_once_after_the_output_is_flushed_in_text_and_in_json',
  },
  {
    name: 'read: cf asks for the task before it checks what it was asked',
    edits: [
      [
        `${CF}/board/task.rs`,
        lines(
          '    let last = if words.on("--transcript") {',
          '        Some(items_asked(&words)?)',
          '    } else {',
          '        None',
          '    };',
          '    let path = format!("/api/tasks/{number}");',
          '    let mut answer = board.get(&path)?;',
          '    let task = answer.take("task")?;',
        ),
        lines(
          '    let path = format!("/api/tasks/{number}");',
          '    let mut answer = board.get(&path)?;',
          '    let task = answer.take("task")?;',
          '    let last = if words.on("--transcript") {',
          '        Some(items_asked(&words)?)',
          '    } else {',
          '        None',
          '    };',
        ),
      ],
    ],
    runs: [cfBoard],
    meant: 'a_bad_last_asks_the_board_nothing_and_so_receives_nothing',
  },
  {
    name: 'read: cf says of a transcript what it did not write',
    edits: [
      [
        `${CF}/board/task.rs`,
        '    Ok(Said::new(Value::Object(data), text))\n}',
        lines(
          '    let answers = waiting_answers(&[json!({ "id": 12, "kind": "answer", "state": "queued" })]);',
          '    Ok(Said::new(Value::Object(data), text).having_written("task", answers))',
          '}',
        ),
      ],
    ],
    runs: [cfBoard],
    meant: 'a_transcript_says_nothing_of_any_answer_whether_it_prints_the_thread_or_not',
  },
  {
    name: 'read: cf says of a list of previews that it wrote the answers whole',
    edits: [
      [
        `${CF}/board/mod.rs`,
        '    Ok(Said::new(messages, text))\n}',
        lines(
          '    let answers = lines::waiting_answers(messages.as_array().map_or(&[], Vec::as_slice));',
          '    Ok(Said::new(messages, text).having_written("inbox", answers))',
          '}',
        ),
      ],
    ],
    runs: [cfBoard],
    meant: 'a_list_of_the_inbox_prints_first_lines_and_says_nothing_of_the_answers_in_it',
  },
  {
    name: 'native: an answer is told to the board as received when it is queued for Codex',
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        lines(
          '            let handed = tokio::select! {',
          '                written = written => written.unwrap_or(false),',
          '                () = ending.wait() => false,',
          '            };',
        ),
        '            let handed = !matches!(written.try_recv(), Err(oneshot::error::TryRecvError::Closed));',
      ],
      [
        `${CODEX}/broker/pair.rs`,
        'use tokio::sync::Notify;',
        'use tokio::sync::{oneshot, Notify};',
      ],
      [
        `${CODEX}/broker/pair.rs`,
        '        let written = match self.native.send_unless(frame, Arc::clone(&ending.raised)) {',
        '        let mut written = match self.native.send_unless(frame, Arc::clone(&ending.raised)) {',
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::handoff')],
    meant:
      'an_answer_waiting_to_be_written_is_not_a_receipt_and_the_turn_ending_gives_it_back_at_once',
  },
  {
    name: 'native: a question is let go as its answer is queued, so its turn ending is not heard',
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        lines(
          '        let this = Rc::clone(self);',
          '        self.shared.spawn(async move {',
          '            let handed = tokio::select! {',
        ),
        lines(
          '        let this = Rc::clone(self);',
          '        self.release(&ending);',
          '        self.shared.spawn(async move {',
          '            let handed = tokio::select! {',
        ),
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::handoff')],
    meant:
      'an_answer_waiting_to_be_written_is_not_a_receipt_and_the_turn_ending_gives_it_back_at_once',
  },
  {
    name: 'native: the writer sends a frame nobody wants any more',
    edits: [
      [
        `${CODEX}/broker/transport.rs`,
        lines(
          '        let wanted = watch',
          '            .as_ref()',
          '            .is_none_or(|watch| !watch.ended.load(Ordering::SeqCst));',
        ),
        '        let wanted = true;',
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::handoff')],
    meant: 'a_frame_not_wanted_any_more_when_its_turn_comes_is_not_sent_and_is_reported_so',
  },
  {
    name: "native: Codex's resolving the request does not end its question",
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        '        let resolved = if method == Some("serverRequest/resolved") {',
        '        let resolved = if false {',
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::handoff')],
    meant:
      'a_request_codex_resolved_ends_the_poll_of_the_question_it_was_and_the_answer_is_given_back',
  },
  {
    name: 'native: a pair that ends takes the board answer with it unacknowledged',
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        lines(
          "        // A task of the broker's, not the pair's: a pair that ends while the",
          '        // board is still polled for the question has its poll end at the next',
          '        // request, and an answer the board claimed meanwhile is still given back.',
          '        self.shared.spawn(async move {',
          '            let (asking, polling, raised) =',
        ),
        lines('        self.spawn(async move {', '            let (asking, polling, raised) ='),
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::handoff')],
    meant:
      'a_pair_that_ends_while_the_board_is_still_asked_gives_back_the_answer_the_poll_in_hand_gets',
  },
  {
    name: 'native: OpenCode takes any reply that resolved for received',
    edits: [
      [
        'hosts/opencode-extension/consensflow-session.mjs',
        lines(
          '    if (result === true) return true',
          '    return result != null && !result.error && (result.response?.ok === true || result.data === true)',
        ),
        '    return true',
      ],
    ],
    runs: [opencode],
    meant:
      "OpenCode: the answer is received only when the SDK's reply took: an error or a status that is no success comes back as a result, not a throw",
  },
  {
    name: 'claim: a window at its own dialog keeps the claim on its answer',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        '            _ if observed.waiting.is_some() => Some("its window shows a dialog of its own"),',
        '            _ if observed.waiting.is_some() => None,',
      ],
    ],
    runs: [dispatcher],
    meant:
      'a_claim_whose_door_gave_up_for_the_windows_own_dialog_is_voided_and_the_answer_is_pasted_once_the_dialog_is_gone',
  },
  {
    name: 'claim: a poll takes no notice of its client having left',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        lines('            || request.consumer().has_left()', '            || context.credentials'),
        '            || context.credentials',
      ],
      // With the check gone a wait the client's leaving ends would spin: both go.
      [
        `${DAEMON}/api/routes/door.rs`,
        '            () = request.consumer().left() => {}',
        '            () = std::future::pending::<()>() => {}',
      ],
    ],
    runs: [daemon('routes::door')],
    meant: 'a_poll_whose_client_has_gone_before_it_looks_claims_nothing_though_the_answer_is_there',
  },
  {
    name: 'claim: a poll goes on waiting after its client has left',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        '            () = request.consumer().left() => {}',
        '            () = std::future::pending::<()>() => {}',
      ],
    ],
    runs: [daemon('routes::door')],
    meant:
      'a_client_that_leaves_ends_the_poll_at_once_and_an_answer_that_comes_after_is_not_claimed_for_it',
  },
  {
    name: 'claim: the server does not tell a handler its client left',
    edits: [
      [
        `${DAEMON}/api/server.rs`,
        lines('    fn drop(&mut self) {', '        self.0.leave();', '    }'),
        lines('    fn drop(&mut self) {', '    }'),
      ],
    ],
    runs: [daemon('api::server')],
    meant: 'a_handler_is_told_when_its_client_leaves_and_not_before',
  },
  {
    name: 'claim: a door gives up at the first poll that gets no answer',
    edits: [
      [
        `${BOARD}/door.rs`,
        '            Err(cause) if cause.is_unreachable() && lost < retries.len() => {',
        '            Err(cause) if cause.is_unreachable() && lost < 0 => {',
      ],
    ],
    runs: [unit('cf-board', 'door::')],
    meant:
      'a_poll_whose_reply_was_lost_is_asked_again_and_gets_the_answer_the_board_claimed_for_it',
  },
  {
    name: "claim: OpenCode's door gives up at the first poll that gets no answer",
    edits: [
      [
        'hosts/lib/question-door.js',
        '      if (cause?.refused || lost >= retries.length) throw cause',
        '      throw cause',
      ],
    ],
    runs: [opencode],
    meant:
      'OpenCode: a poll whose reply was lost is asked again and the answer is handed over once, then acknowledged',
  },
  {
    name: 'gate: the brief is given to an agent while it waits at the gate',
    edits: [
      [
        `${DAEMON}/api/routes/task.rs`,
        lines('        if held_back {', '            task.task.body.clear();', '        }'),
        lines(
          '        if held_back && false {',
          '            task.task.body.clear();',
          '        }',
        ),
      ],
    ],
    runs: [daemon('routes::task')],
    meant: 'a_brief_that_waits_at_the_gate_is_in_no_agents_view_of_the_task',
  },
  {
    name: 'gate: a later message held at the gate takes the received brief back',
    edits: [
      [
        `${DAEMON}/api/routes/task.rs`,
        '            "delivered" | "read" => return false,',
        '            "delivered" | "read" => {}',
      ],
    ],
    runs: [daemon('routes::task')],
    meant:
      'a_brief_is_given_once_it_has_passed_the_gate_and_a_later_message_held_there_does_not_take_it_back',
  },
]
