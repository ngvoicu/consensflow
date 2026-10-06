/**
 * The receipt and stop redesign's fix round, part B: a question's obligation
 * does not end with its answer's transport, one task is a session's at a time
 * and a window's stop is its own task's, and the way back to Node's daemon and
 * forward again strands nothing. One plant puts back what one rule took out;
 * a test of the rule fails.
 */
import { lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const ENGINE = 'crates/cf-engine/src'

/** The ledger's tests of the redesign: every sequence of its model. */
const receipt = ['-p', 'cf-ledger', '--test', 'receipt']
/** The ledger and the Node daemon on one file: the way back, and forward again. */
const node = ['-p', 'cf-ledger', '--test', 'node']
/** The engine's dispatcher tests, whole. */
const dispatcher = ['-p', 'cf-engine', '--test', 'dispatcher']

/** A window's task as it was: the paused one a session holds first, whatever its window was given. */
const PAUSED_FIRST = [
  `${LEDGER}/tasks/pausing.rs`,
  lines(
    '            "SELECT t.id, t.number, t.stop_seq FROM task t',
    "             WHERE t.assignee_id = ?1 AND t.state IN ('queued', 'working', 'waiting', 'paused')",
    '               AND t.id = (SELECT m.task_id FROM message m',
    "                           WHERE m.recipient_id = ?1 AND m.kind = 'task' AND m.task_id IS NOT NULL",
    "                             AND m.state IN ('delivering', 'delivered', 'read')",
    '                           ORDER BY m.id DESC LIMIT 1)",',
  ),
  lines(
    '            "SELECT id, number, stop_seq FROM task',
    "             WHERE assignee_id = ?1 AND state IN ('queued', 'working', 'waiting', 'paused')",
    "             ORDER BY state = 'paused' DESC, stop_seq DESC, id LIMIT 1\",",
  ),
]

export const PLANTS = [
  {
    name: 'pause: a withdrawn question stops obliging when its answer ends unreceived',
    edits: [
      [
        `${LEDGER}/messages/receipt.rs`,
        lines(
          '                    OR EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id',
          "                               AND a.kind = 'answer'))",
        ),
        lines(
          '                    OR EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id',
          "                               AND a.kind = 'answer'",
          "                               AND a.state IN ('gated', 'queued', 'delivering')))",
        ),
      ],
    ],
    runs: [receipt],
    meant: 'an_answer_whose_delivery_failed_leaves_a_question_withdrawn_at_the_gate_obliging',
  },
  {
    name: 'pause: a task is reopened onto a session that is on another',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '        if found.member_id.is_some() {',
          '            require_free(store, &found)?;',
        ),
        lines(
          '        if found.member_id.is_some() && false {',
          '            require_free(store, &found)?;',
        ),
      ],
    ],
    runs: [receipt],
    meant:
      'a_task_is_not_reopened_onto_a_session_that_has_another_and_it_is_told_what_a_follow_up_is',
  },
  {
    name: 'pause: a window is stopped for the paused task it holds before the one it works on',
    edits: [PAUSED_FIRST],
    runs: [receipt],
    meant: 'a_pause_of_a_task_the_session_holds_but_does_not_work_on_is_not_a_stop_of_its_window',
  },
  {
    name: 'pause: a window at work is pressed for a paused task it holds and does not work on',
    edits: [PAUSED_FIRST],
    runs: [[...dispatcher, 'stops']],
    meant:
      'a_pause_of_a_task_the_window_holds_but_does_not_work_on_presses_no_key_and_its_own_task_is_stopped',
  },
  {
    name: 'pause: a launch reads the stop its window owes before its first message begins',
    edits: [
      [
        `${ENGINE}/windows.rs`,
        lines(
          '        let begun = first',
          '            .map(|first| self.seams.ledger.borrow_mut().begin_delivery(first.id))',
          '            .transpose()?;',
          '        // The stops this window owes are those asked before this look: a pause',
          '        // that comes while it is prepared or opened is asked after it, and is',
          '        // owed. Read in this turn, with the first message begun: the window',
          '        // works on the task that message is about.',
          '        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;',
        ),
        lines(
          '        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;',
          '        let begun = first',
          '            .map(|first| self.seams.ledger.borrow_mut().begin_delivery(first.id))',
          '            .transpose()?;',
        ),
      ],
    ],
    runs: [[...dispatcher, 'launch_stops'], dispatcher],
    meant:
      'a_pause_during_a_launchs_preparation_opens_no_pane_and_the_message_is_given_back_and_carried',
  },
  {
    name: 'pause: the words that first reach a window do not answer a pause asked before them',
    edits: [
      [
        `${ENGINE}/deliveries.rs`,
        lines(
          '        if begun.message.kind == "task" {',
          '            self.pay_with_words(record)?;',
        ),
        lines(
          '        if begun.message.kind == "task" && false {',
          '            self.pay_with_words(record)?;',
        ),
      ],
    ],
    runs: [[...dispatcher, 'stops']],
    meant:
      'the_words_that_first_reach_a_window_for_a_task_paused_before_they_came_answer_that_pause',
  },
  {
    name: 'pause: this ledger opens with rows still under a carrier Node delivered',
    edits: [
      [
        `${LEDGER}/ledger.rs`,
        '    release_stranded(&mut store)?;',
        '    let _ = release_stranded;',
      ],
    ],
    runs: [node, receipt],
    meant:
      'a_carrier_node_delivered_leaves_a_late_answer_that_this_ledger_delivers_when_it_starts_again',
  },
  {
    name: 'pause: this ledger opens leaving a task working that Node moved on for a queued answer',
    edits: [
      [
        `${LEDGER}/messages/carrying.rs`,
        '            reconcile(store, task)?;',
        '            let _ = (task, reconcile);',
      ],
    ],
    runs: [node, receipt],
    meant:
      'a_carrier_node_delivered_leaves_a_late_answer_that_this_ledger_delivers_when_it_starts_again',
  },
  {
    name: 'pause: a carrier that was delivered or is being pasted keeps its rows at the start',
    edits: [
      [
        `${LEDGER}/messages/carrying.rs`,
        "                 WHERE m.state = 'queued' AND c.state NOT IN ('queued', 'gated')",
        "                 WHERE m.state = 'queued' AND c.state IN ('cancelled', 'failed')",
      ],
    ],
    runs: [node, receipt],
    meant:
      'a_carrier_node_delivered_leaves_a_late_answer_that_this_ledger_delivers_when_it_starts_again',
  },
  {
    name: 'pause: a carrier Node began to paste has its rows settled with it at the start',
    edits: [
      [
        `${LEDGER}/messages/carrying.rs`,
        "                 WHERE m.state = 'queued' AND c.state NOT IN ('queued', 'gated')",
        "                 WHERE m.state = 'queued' AND c.state NOT IN ('queued', 'gated', 'delivering')",
      ],
    ],
    runs: [receipt],
    meant: 'a_row_left_under_a_carrier_node_began_to_paste_is_not_taken_for_received_with_it',
  },
]
