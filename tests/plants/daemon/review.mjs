/**
 * Calliope's review of the merged fix round: the door's receipt is said again
 * while the board cannot be reached, `cf` says nothing of the answers in an
 * output a harness may cut, the one asked is told to answer again when an
 * answer did not stand, a row Node was pasting is not stranded, and a task
 * sent back to a busy session is told what it can do. One plant takes one fix
 * out, and a test of it fails. A run written `{ node: [...] }` is `node --test`
 * of those files.
 */
import { lines, unit } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const BOARD = 'crates/cf-board/src'
const CF = 'crates/cf/src'

/** The ledger's tests of the redesign: every sequence of its model. */
const receipt = ['-p', 'cf-ledger', '--test', 'receipt']
const door = unit('cf-board', 'door::')
const cfBoard = unit('cf', 'board::')
const opencode = { node: ['tests/opencode-extension.test.mjs'] }

export const PLANTS = [
  {
    name: 'review: a row Node was pasting on its own stays under the carrier it rode in when its delivery goes again',
    edits: [
      [
        `${LEDGER}/messages/delivery.rs`,
        lines(
          '        store.db.execute(',
          '            "UPDATE message SET carried_by = NULL',
          '             WHERE id = ?1',
          "               AND carried_by IN (SELECT id FROM message WHERE state NOT IN ('queued', 'gated'))\",",
          '            [message_id],',
          '        )?;',
          '        adopt(store, message_id)?;',
        ),
        '        adopt(store, message_id)?;',
      ],
    ],
    runs: [receipt],
    meant:
      'a_row_node_was_pasting_on_its_own_under_a_carrier_that_is_over_goes_again_as_a_row_of_its_own',
  },
  {
    name: 'review: a task sent back to a busy session is told it can be opened for its tier',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        lines(
          '            Giving::SentBack(number) => {',
          '                format!(" before sending T-{number} back, or open a new task for its tier")',
          '            }',
        ),
        '            Giving::SentBack(_) => ", or open the task for its tier".to_owned(),',
      ],
    ],
    runs: [receipt],
    meant:
      'a_task_is_not_reopened_onto_a_session_that_has_another_and_is_told_what_a_reopen_can_do',
  },
  {
    name: 'review: an answer that failed to arrive is not told to the one asked',
    edits: [
      [
        `${LEDGER}/messages/delivery.rs`,
        lines(
          '            if message.kind == "answer" {',
          '                tell_failed_answer(store, &message, &task, reason)?;',
          '            }',
        ),
        '',
      ],
    ],
    runs: [receipt],
    meant:
      'an_answer_whose_delivery_failed_for_a_question_withdrawn_at_the_gate_tells_the_chief_to_answer_it_again',
  },
  {
    name: 'review: a task sent back is not told to the one asked for the answer it lacks',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        '        tell_sent_back(store, &task, assignee.id, by)?;',
        '',
      ],
    ],
    runs: [receipt],
    meant:
      'a_task_sent_back_while_an_answer_waited_for_the_human_tells_the_chief_to_answer_it_again',
  },
  {
    name: 'review: the one asked who has left the project fails the delivery that is told to them',
    edits: [
      [
        `${LEDGER}/messages/again.rs`,
        lines('    if asked.left_at.is_some() {', '        return Ok(());', '    }'),
        '',
      ],
    ],
    runs: [receipt],
    meant: 'nobody_is_told_of_an_answer_that_failed_for_a_task_that_is_over_or_whose_asker_left',
  },
  {
    name: 'review: a failed answer is told of a task that is over',
    edits: [
      [
        `${LEDGER}/messages/again.rs`,
        lines(
          '        matches!(',
          '            task.state.as_str(),',
          '            "queued" | "working" | "waiting" | "paused"',
          '        ),',
        ),
        '        true,',
      ],
    ],
    runs: [receipt],
    meant: 'nobody_is_told_of_an_answer_that_failed_for_a_task_that_is_over_or_whose_asker_left',
  },
  {
    name: 'review: a question with an answer on its way is told to be answered again',
    edits: [
      [
        `${LEDGER}/messages/again.rs`,
        lines(
          "               AND NOT EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'",
          "                               AND a.state IN ('gated', 'queued', 'delivering', 'delivered', 'read'))",
        ),
        '               AND 1 = 1',
      ],
    ],
    runs: [receipt],
    meant:
      'a_task_sent_back_tells_nobody_of_a_question_that_never_had_an_answer_or_has_one_on_its_way',
  },
  {
    name: 'review: a question that never had an answer is told to be answered again',
    edits: [
      [
        `${LEDGER}/messages/again.rs`,
        lines(
          "               AND EXISTS (SELECT 1 FROM message a WHERE a.reply_to = q.id AND a.kind = 'answer'",
          "                           AND a.state IN ('cancelled', 'failed'))",
        ),
        '               AND 1 = 1',
      ],
    ],
    runs: [receipt],
    meant:
      'a_task_sent_back_tells_nobody_of_a_question_that_never_had_an_answer_or_has_one_on_its_way',
  },
  {
    name: 'review: the decline of an answer says its own words, not the ones the others use',
    edits: [
      [
        `${LEDGER}/messages/again.rs`,
        '    format!("Answer it again: cf answer m-{question} \\"…\\"")',
        '    format!("Answer again: cf answer m-{question} \\"…\\"")',
      ],
    ],
    runs: [receipt],
    meant: 'declining_an_answer_tells_the_chief_in_the_words_the_others_use',
  },
  {
    name: 'review: the door says a receipt once whatever came of it',
    edits: [
      [
        `${BOARD}/door.rs`,
        '        if !cause.is_unreachable() || lost >= retries.len() {',
        '        if cause.is_unreachable() || lost >= 0 {',
      ],
    ],
    runs: [door],
    meant:
      'a_receipt_whose_reply_was_lost_is_said_again_until_the_board_takes_it_whichever_it_says',
  },
  {
    name: 'review: the door says a receipt the board refused again',
    edits: [
      [
        `${BOARD}/door.rs`,
        '        if !cause.is_unreachable() || lost >= retries.len() {',
        '        if lost >= retries.len() {',
      ],
    ],
    runs: [door],
    meant: 'a_receipt_the_board_refused_is_not_said_again',
  },
  {
    name: 'review: the door says a receipt for ever to a board that stays out of reach',
    edits: [
      [
        `${BOARD}/door.rs`,
        '        if !cause.is_unreachable() || lost >= retries.len() {',
        '        if !cause.is_unreachable() {',
      ],
    ],
    runs: [door],
    meant: 'a_board_that_stays_out_of_reach_is_told_five_times_and_nothing_is_raised',
  },
  {
    name: 'review: OpenCode says a receipt once whatever came of it',
    edits: [
      [
        'hosts/lib/question-door.js',
        '      if (cause?.refused || lost >= retries.length) return\n      await new Promise((resolve) => setTimeout(resolve, retries[lost]))',
        '      return\n      await new Promise((resolve) => setTimeout(resolve, retries[lost]))',
      ],
    ],
    runs: [opencode],
    meant:
      'OpenCode: a receipt whose reply was lost is said again, and the answer is handed over once',
  },
  {
    name: 'review: OpenCode says a receipt the board refused again',
    edits: [
      [
        'hosts/lib/question-door.js',
        '      if (cause?.refused || lost >= retries.length) return\n      await new Promise((resolve) => setTimeout(resolve, retries[lost]))',
        '      if (lost >= retries.length) return\n      await new Promise((resolve) => setTimeout(resolve, retries[lost]))',
      ],
    ],
    runs: [opencode],
    meant:
      'OpenCode: a receipt is said again while the board cannot be reached, then let be; a refusal is final',
  },
  {
    name: 'review: cf says it wrote answers whole of an output a harness may cut',
    edits: [
      [
        `${CF}/board/cut.rs`,
        "    printed.len() < SEEN_WHOLE_BYTES && printed.matches('\\n').count() < SEEN_WHOLE_LINES",
        '    !printed.is_empty() || printed.is_empty()',
      ],
    ],
    runs: [cfBoard],
    meant: 'a_thread_a_harness_may_cut_says_nothing_of_the_answers_in_it_and_they_come_as_text',
  },
  {
    name: 'review: cf counts the characters of an output, not its bytes, against what a harness shows',
    edits: [
      [
        `${CF}/board/cut.rs`,
        "    printed.len() < SEEN_WHOLE_BYTES && printed.matches('\\n').count() < SEEN_WHOLE_LINES",
        "    printed.chars().count() < SEEN_WHOLE_BYTES && printed.matches('\\n').count() < SEEN_WHOLE_LINES",
      ],
    ],
    runs: [cfBoard],
    meant: 'what_is_counted_is_bytes_so_that_characters_of_more_than_one_are_not_missed',
  },
  {
    name: 'review: cf does not count the lines of an output against what a harness shows',
    edits: [
      [
        `${CF}/board/cut.rs`,
        "    printed.len() < SEEN_WHOLE_BYTES && printed.matches('\\n').count() < SEEN_WHOLE_LINES",
        '    printed.len() < SEEN_WHOLE_BYTES',
      ],
    ],
    runs: [cfBoard],
    meant: 'what_is_measured_is_what_was_printed_so_lines_count_in_the_text_and_not_in_the_json',
  },
  {
    name: 'review: cf measures the text of a command even when it printed its JSON',
    edits: [
      [
        `${CF}/board/mod.rs`,
        '            let printed = if json {\n                let data = cf_base::json::js_order(data);\n                format!("{}\\n", serde_json::to_string_pretty(&data)?)\n            } else {\n                format!("{text}\\n")\n            };',
        '            let measured = format!("{text}\\n");\n            let printed = if json {\n                let data = cf_base::json::js_order(data);\n                format!("{}\\n", serde_json::to_string_pretty(&data)?)\n            } else {\n                measured.clone()\n            };',
      ],
      [
        `${CF}/board/mod.rs`,
        'wrote.filter(|_| cut::seen_whole(&printed))',
        'wrote.filter(|_| cut::seen_whole(&measured))',
      ],
    ],
    runs: [cfBoard],
    meant: 'what_is_measured_is_what_was_printed_so_lines_count_in_the_text_and_not_in_the_json',
  },
]
