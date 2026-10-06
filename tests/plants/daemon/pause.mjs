/**
 * The receipt and stop redesign's rules: a pause keeps what a window kept and
 * stops its window until it has paid the stop; a message is received by proof
 * and not by being withdrawn; a door's claim is a write and its answer is
 * read only when the door says it handed it over. One plant puts back what
 * one rule took out, or takes one rule out, and a test of the rule fails.
 */
import { DAEMON, daemon, lines, unit } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const ENGINE = 'crates/cf-engine/src'
const BOARD = 'crates/cf-board/src'
const CODEX = 'crates/cf-codex-session/src'
const CF = 'crates/cf/src'

/** The ledger's tests of the redesign: every sequence of its model. */
const receipt = ['-p', 'cf-ledger', '--test', 'receipt']
/** The engine's dispatcher tests, whole. */
const dispatcher = ['-p', 'cf-engine', '--test', 'dispatcher']
/** What the engine writes a window, and reads of it, as text. */
const text = ['-p', 'cf-engine', '--test', 'text']

export const PLANTS = [
  {
    name: 'pause: a hold withdraws what was on its way',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '        pause(store, &task, detail)?;',
          '        store.db.execute(',
          '            "UPDATE task SET held_until = ? WHERE id = ?",',
        ),
        lines(
          '        pause(store, &task, detail)?;',
          '        store.db.execute(',
          `            "UPDATE message SET state = 'cancelled', reason = 'held' WHERE task_id = ? AND state IN ('queued', 'gated')",`,
          '            [task.id],',
          '        )?;',
          '        store.db.execute(',
          '            "UPDATE task SET held_until = ? WHERE id = ?",',
        ),
      ],
    ],
    runs: [receipt],
    meant:
      'a_hold_ends_in_one_carrier_with_what_was_kept_in_the_order_of_its_ids_and_its_confirm_settles_every_row',
  },
  {
    name: 'pause: a pause cancels an answer',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        `                   AND kind IN ('task', 'note') AND state IN ('queued', 'gated')",`,
        `                   AND kind IN ('task', 'note', 'answer') AND state IN ('queued', 'gated')",`,
      ],
    ],
    runs: [receipt],
    meant: 'the_chiefs_pause_keeps_an_answer_and_takes_back_the_chiefs_words_and_notes',
  },
  {
    name: 'pause: a pause is not counted as a stop',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        '"UPDATE task SET paused_at = updated_at, stop_seq = stop_seq + 1 WHERE id = ?",',
        '"UPDATE task SET paused_at = updated_at WHERE id = ?",',
      ],
    ],
    runs: [receipt],
    meant: 'two_pauses_on_a_clock_that_never_moves_are_two_stops',
  },
  {
    name: 'pause: a door is left open by a pause',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        `AND kind = 'question' AND door_closed_at IS NULL",`,
        `AND kind = 'question' AND door_closed_at IS NULL AND 0",`,
      ],
    ],
    runs: [receipt],
    meant: 'a_pause_voids_the_claim_and_shuts_the_door_and_the_resume_carries_the_answer',
  },
  {
    name: 'pause: a claim stands through a pause',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        '               AND claimed_at IS NOT NULL ORDER BY id",',
        '               AND claimed_at IS NOT NULL AND 0 ORDER BY id",',
      ],
    ],
    runs: [receipt],
    meant: 'a_pause_voids_the_claim_and_shuts_the_door_and_the_resume_carries_the_answer',
  },
  {
    name: 'pause: the task-message bypass is back in the head of a queue',
    edits: [
      [
        `${LEDGER}/messages/delivery.rs`,
        '         AND (?2 = 0 OR m.urgent = 1 OR m.task_id IN (',
        "         AND (?2 = 0 OR m.urgent = 1 OR m.kind = 'task' OR m.task_id IN (",
      ],
    ],
    runs: [receipt],
    meant: 'a_task_paused_before_its_brief_arrived_keeps_the_brief_and_its_window_is_given_nothing',
  },
  {
    name: 'pause: a confirm moves the task past the words that resume it',
    edits: [
      [
        `${LEDGER}/messages/receipt.rs`,
        '            if barrier(store, task.id, window)? || !arrived(store, task.id, window)? {',
        '            if !arrived(store, task.id, window)? {',
      ],
    ],
    runs: [receipt],
    meant:
      'a_confirm_of_what_was_on_its_way_when_the_task_was_paused_leaves_it_queued_behind_the_words',
  },
  {
    name: 'pause: an answer received ignores a second open question',
    edits: [
      [
        `${LEDGER}/messages/receipt.rs`,
        '        "waiting" if !outstanding(store, task.id, window)? => "working",',
        '        "waiting" => "working",',
      ],
    ],
    runs: [receipt],
    meant:
      'one_answer_received_leaves_the_task_waiting_for_the_other_question_and_the_second_makes_it_work',
  },
  {
    name: 'pause: a claim marks the answer read',
    edits: [
      [
        `${LEDGER}/messages/receipt.rs`,
        '"UPDATE message SET claimed_at = ? WHERE id = ?",',
        `"UPDATE message SET claimed_at = ?, state = 'read' WHERE id = ?",`,
      ],
    ],
    runs: [receipt],
    meant: 'a_poll_claims_the_answer_and_the_same_answer_comes_again_to_a_second_poll',
  },
  {
    name: 'pause: a question is born with its door open',
    edits: [[`${LEDGER}/messages/receipt.rs`, '"paused" => true,', '"paused" => false,']],
    runs: [receipt],
    meant:
      'a_question_asked_while_its_task_is_paused_or_queued_behind_the_words_that_resume_it_is_born_with_its_door_shut',
  },
  {
    name: 'pause: a receipt is taken at a door that was shut',
    edits: [
      [
        `${LEDGER}/messages/receipt.rs`,
        'WHERE a.id = ? AND a.carried_by IS NULL AND q.door_closed_at IS NULL",',
        'WHERE a.id = ? AND a.carried_by IS NULL",',
      ],
    ],
    runs: [daemon('routes::answers')],
    meant: 'a_receipt_at_a_door_a_pause_shut_is_refused_and_the_answer_stays_to_come_as_a_message',
  },
  {
    name: 'pause: a fold takes a row held for the human',
    edits: [
      [
        `${LEDGER}/messages/carrying.rs`,
        `     AND kind IN ('task', 'answer', 'note') AND state = 'queued'";`,
        `     AND kind IN ('task', 'answer', 'note') AND state IN ('queued', 'gated')";`,
      ],
    ],
    runs: [receipt],
    meant: 'a_row_still_held_for_the_human_is_not_folded_and_goes_on_its_own_once_passed_on',
  },
  {
    name: 'pause: a carrier stores the text of what it carries',
    edits: [
      [
        `${LEDGER}/messages/carrying.rs`,
        lines(
          '            "UPDATE message SET carried_by = ? WHERE id = ?",',
          '            params![carrier, id],',
          '        )?;',
          '    }',
        ),
        lines(
          '            "UPDATE message SET carried_by = ? WHERE id = ?",',
          '            params![carrier, id],',
          '        )?;',
          '    }',
          '    store.db.execute(',
          '        "UPDATE message SET body = body || char(10) || (SELECT group_concat(body, char(10)) FROM message WHERE carried_by = ?1) WHERE id = ?1 AND EXISTS (SELECT 1 FROM message WHERE carried_by = ?1)",',
          '        [carrier],',
          '    )?;',
        ),
      ],
    ],
    runs: [dispatcher],
    meant: 'a_hold_ends_in_one_paste_with_what_was_kept_and_then_the_words_and_one_marker',
  },
  {
    name: 'pause: a claim is not voided when its window is seen at rest',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        '            Look::Rest => Some("its window is at rest"),',
        '            Look::Rest => None,',
      ],
    ],
    runs: [dispatcher],
    meant:
      'an_unacknowledged_claim_is_voided_at_the_look_that_finds_the_window_at_rest_and_the_answer_is_pasted_once',
  },
  {
    name: 'pause: a claim is not voided when its window exits',
    edits: [
      [
        `${ENGINE}/dispatcher.rs`,
        lines(
          '        self.seams',
          '            .ledger',
          '            .borrow_mut()',
          '            .release_claims(record.id, "its window exited")?;',
        ),
        '',
      ],
    ],
    runs: [dispatcher],
    meant:
      'an_unacknowledged_claim_is_voided_when_the_window_exits_and_the_answer_comes_with_the_words',
  },
  {
    name: 'pause: a claim is not voided when the daemon starts',
    edits: [
      [
        `${ENGINE}/deliveries.rs`,
        '        self.seams.ledger.borrow_mut().release_all_claims()?;',
        '',
      ],
    ],
    runs: [dispatcher],
    meant: 'an_unacknowledged_claim_is_voided_when_the_daemon_starts',
  },
  {
    name: 'pause: payment of a stop is forgiven at launch',
    edits: [
      [
        `${ENGINE}/windows.rs`,
        '            part.stopped = captured.map(|stop| (stop.task_id, stop.seq));',
        lines(
          '            part.stopped = self',
          '                .seams',
          '                .ledger',
          '                .borrow()',
          '                .stop_of(participant.id)',
          '                .ok()',
          '                .flatten()',
          '                .map(|stop| (stop.task_id, stop.seq));',
        ),
      ],
    ],
    runs: [dispatcher],
    meant: 'a_pause_while_the_pane_opens_stays_owed_and_the_first_look_interrupts',
  },
  {
    name: 'pause: a pause while a launch is prepared is not seen',
    edits: [
      [
        `${ENGINE}/windows.rs`,
        '        Ok(on_its_way && ledger.stop_of(participant.id)? == captured)',
        '        Ok(on_its_way)',
      ],
    ],
    runs: [dispatcher],
    meant:
      'a_pause_during_a_launchs_preparation_opens_no_pane_and_the_message_is_given_back_and_carried',
  },
  {
    name: 'pause: a delivery withdrawn while its window got ready is handed over',
    edits: [
      [
        `${ENGINE}/deliveries.rs`,
        lines(
          '        let next = self.seams.ledger.borrow().next_delivery(record.id)?;',
          '        if next.is_none_or(|next| next.id != message.id) {',
          '            return Ok(());',
          '        }',
        ),
        lines(
          '        let next = self.seams.ledger.borrow().next_delivery(record.id)?;',
          '        let _ = next;',
        ),
      ],
    ],
    runs: [dispatcher],
    meant: 'a_pause_while_the_window_gets_ready_hands_it_nothing',
  },
  {
    name: 'pause: what the activity says gates the interrupt again',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        lines(
          '                Look::Work => {',
          '                    self.interrupt_for(project, participant, record, &stop)',
          '                        .await',
          '                }',
        ),
        lines(
          '                Look::Work if self.activity(participant.id).state != crate::ActivityState::Working => Ok(()),',
          '                Look::Work => {',
          '                    self.interrupt_for(project, participant, record, &stop)',
          '                        .await',
          '                }',
        ),
      ],
    ],
    runs: [dispatcher],
    meant:
      'a_window_out_of_quota_and_still_at_work_is_interrupted_while_its_activity_says_it_is_out',
  },
  {
    name: 'pause: a stop is tried once, not in three rounds',
    edits: [[`${ENGINE}/stops.rs`, 'const ROUNDS: u32 = 3;', 'const ROUNDS: u32 = 1;']],
    runs: [dispatcher],
    meant:
      'three_rounds_ignored_exhaust_the_stop_which_is_said_once_tried_again_every_minute_and_paid_at_rest',
  },
  {
    name: 'pause: a launch prepends a brief nobody received',
    edits: [
      [
        `${LEDGER}/messages/delivery.rs`,
        "         AND m.kind = 'task' AND m.state IN ('delivered', 'read') AND m.carried_by IS NULL",
        "         AND m.kind = 'task' AND m.state IN ('queued', 'gated', 'delivering', 'delivered', 'read') AND m.carried_by IS NULL",
      ],
    ],
    runs: [dispatcher, text],
    meant:
      'a_gated_first_launch_gives_its_window_nothing_until_the_human_passes_the_brief_on_and_then_one_paste',
  },
  {
    name: 'pause: the door claims before it asks whether its daemon is stopping or its window gone',
    edits: [
      [
        `${DAEMON}/api/routes/door.rs`,
        lines(
          '        if context.closing.is_set()',
          '            || request.consumer().has_left()',
          '            || context.credentials.resolve(request.bearer()).is_none()',
          '        {',
          '            return unanswered(&asked);',
          '        }',
          '        let claimed = context',
          '            .ledger',
          '            .borrow_mut()',
          '            .claim_answer(asked.id, caller.participant.id)?;',
        ),
        lines(
          '        let claimed = context',
          '            .ledger',
          '            .borrow_mut()',
          '            .claim_answer(asked.id, caller.participant.id)?;',
          '        if context.closing.is_set()',
          '            || request.consumer().has_left()',
          '            || context.credentials.resolve(request.bearer()).is_none()',
          '        {',
          '            return unanswered(&asked);',
          '        }',
        ),
      ],
    ],
    runs: [daemon('routes::door')],
    meant:
      'an_answer_that_came_as_the_daemon_stops_is_not_claimed_and_stays_the_ledgers_to_deliver',
  },
  {
    name: 'pause: a thread shows and receives what waits for the human',
    edits: [
      [
        `${DAEMON}/api/routes/task.rs`,
        '        task.messages.retain(|message| message.state != "gated");',
        '        task.messages.retain(|_| true);',
      ],
    ],
    runs: [daemon('routes::task')],
    meant: 'a_gated_answer_is_left_out_of_the_thread_and_is_not_received',
  },
  {
    name: 'pause: a lane says nothing of a stop its window ignores',
    edits: [
      [
        `${DAEMON}/page/board.rs`,
        lines(
          '        if let Some(ignored) = page.engine.unstopped(id) {',
          '            fields.insert("unstopped".to_owned(), ignored);',
          '        }',
        ),
        '',
      ],
    ],
    runs: [daemon('page::tests::stopping')],
    meant: 'a_lane_has_unstopped_while_its_window_ignores_a_stop_and_in_no_other_lane_or_time',
  },
  {
    name: 'pause: a door shut by the board is wrapped for the model',
    edits: [
      [
        `${BOARD}/door.rs`,
        '        Err(cause) if cause.code() == Some(DOOR_CLOSED) => Reply::Refused(cause.to_string()),',
        '        Err(cause) if cause.code() == Some(DOOR_CLOSED) => Reply::Refused(refusal_reason(&cause)),',
      ],
    ],
    runs: [unit('cf-board', 'door::')],
    meant:
      'a_door_the_board_shut_is_refused_in_the_boards_own_words_and_any_other_refusal_is_wrapped',
  },
  {
    name: 'pause: a hook tells the board before it writes the answer',
    edits: [
      [
        `${CF}/hook.rs`,
        lines(
          '    if let Some(said) = said {',
          '        write!(out, "{said}")?;',
          '        out.flush()?;',
          '    }',
          "    // The answer is the harness's now: the board is told it was handed over.",
          '    if let Some((board, answer)) = handed {',
          '        door::acknowledge(&board, &answer, true);',
          '    }',
        ),
        lines(
          '    if let Some((board, answer)) = &handed {',
          '        door::acknowledge(board, answer, true);',
          '    }',
          '    if let Some(said) = said {',
          '        write!(out, "{said}")?;',
          '        out.flush()?;',
          '    }',
        ),
      ],
    ],
    runs: [unit('cf', 'hook::')],
    meant: 'an_answer_is_written_and_flushed_before_the_board_is_told_it_was_handed_over',
  },
  {
    name: 'pause: a turn that ends leaves the question it asked held at the board',
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        '            let turn_ended = turn_over && thread.is_some() && held.thread.as_deref() == thread;',
        '            let turn_ended = false && thread.is_some() && held.thread.as_deref() == thread;',
      ],
    ],
    runs: [unit('cf-codex-session', 'broker::tests::questions')],
    meant:
      'a_turn_that_completes_ends_the_poll_of_the_question_it_asked_and_the_answer_is_given_back',
  },
  {
    name: 'pause: an answer that outlived its turn is handed to Codex',
    edits: [
      [
        `${CODEX}/broker/pair.rs`,
        '            let ended = ending.is_raised();',
        '            let ended = false;',
      ],
      // The writer's own check of it is a second hold on the answer: both go.
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
    runs: [unit('cf-codex-session', 'broker::tests::questions')],
    meant:
      'a_turn_that_completes_ends_the_poll_of_the_question_it_asked_and_the_answer_is_given_back',
  },
]
