/**
 * The result of a task the chief decides on before it is given it: accepting
 * the task or sending it back withdraws the result in the decision's own
 * transaction, and the dispatcher has nothing to paste after it (the chief's
 * turn of 2026-10-08, given T-9's result with "Decide with: cf task accept
 * T-9 …" after it had accepted T-9). A result being pasted, or given, stays;
 * a cancel withdraws what an older build left queued. One plant takes one rule
 * out; a test of it fails.
 */
import { lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'

/** The ledger's tests of the result a decision withdraws, and of what stays. */
const ledger = ['-p', 'cf-ledger', '--test', 'receipt', 'decided']
/** The engine's tests of the same on the chief's fake window. */
const engine = ['-p', 'cf-engine', '--test', 'dispatcher', 'decided_results']
/** The ledger's replay of Node's recordings, with the traces that depart on purpose. */
const replay = ['-p', 'cf-ledger', '--test', 'replay']
/** The chief eval's plumbing counts, over a ledger with a result withdrawn. */
const measure = { node: ['tests/evals-measure.test.mjs'] }

/** The withdrawal an acceptance makes, after the gate's. */
const ACCEPT_WITHDRAWS = lines(
  '        withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;',
  '        withdraw_result(store, &task, "was accepted")?;',
)
/** The withdrawal a sending back makes, after the gate's. */
const SEND_BACK_WITHDRAWS = lines(
  '        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;',
  '        withdraw_result(store, &task, "was sent back")?;',
)
/** What the withdrawal finds: a task's result, still queued. */
const FINDS = "       WHERE task_id = ? AND kind = 'result' AND state = 'queued'\","

export const PLANTS = [
  {
    name: 'decided result: an accept leaves the result its requester was not given',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        ACCEPT_WITHDRAWS,
        '        withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;',
      ],
    ],
    runs: [ledger],
    meant:
      'the_chief_that_read_a_result_and_accepted_its_task_in_its_turn_is_not_given_the_result_after',
  },
  {
    name: 'decided result: an accept leaves it, and the dispatcher pastes it into the chief',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        ACCEPT_WITHDRAWS,
        '        withdraw_gated(store, task.id, &format!("accepted by @{by}"))?;',
      ],
    ],
    runs: [engine],
    meant:
      'pastes_no_result_into_a_chief_that_accepted_its_task_in_the_turn_the_result_waited_behind',
  },
  {
    name: 'decided result: a sending back leaves the result its requester was not given',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        SEND_BACK_WITHDRAWS,
        '        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;',
      ],
    ],
    runs: [ledger],
    meant:
      'the_chief_that_read_a_result_and_sent_its_task_back_in_its_turn_is_not_given_the_result_after',
  },
  {
    name: 'decided result: a sending back leaves it, and the dispatcher pastes it into the chief',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        SEND_BACK_WITHDRAWS,
        '        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;',
      ],
    ],
    runs: [engine],
    meant:
      'pastes_no_result_into_a_chief_that_sent_its_task_back_in_the_turn_the_result_waited_behind',
  },
  {
    name: 'decided result: a cancel leaves a result queued that an older build left',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        "       WHERE task_id = ? AND state IN ('queued', 'delivering', 'gated')\",",
        "       WHERE task_id = ? AND state IN ('delivering', 'gated')\",",
      ],
    ],
    runs: [ledger],
    meant: 'a_cancel_withdraws_a_result_that_an_older_build_left_queued_for_a_task_sent_back',
  },
  {
    name: 'decided result: a decision withdraws a result that is being pasted',
    edits: [
      [
        `${LEDGER}/queue.rs`,
        FINDS,
        "       WHERE task_id = ? AND kind = 'result' AND state IN ('queued', 'delivering')\",",
      ],
    ],
    runs: [ledger],
    meant: 'a_result_being_pasted_when_the_decision_comes_stays_and_arrives',
  },
  {
    name: 'decided result: a decision withdraws a result its requester was given',
    edits: [
      [
        `${LEDGER}/queue.rs`,
        FINDS,
        "       WHERE task_id = ? AND kind = 'result' AND state IN ('queued', 'delivered')\",",
      ],
    ],
    runs: [ledger],
    meant: 'a_result_the_chief_was_given_already_stays_when_it_decides',
  },
  {
    name: 'decided result: a decision withdraws whatever else is queued for the task',
    edits: [[`${LEDGER}/queue.rs`, FINDS, "       WHERE task_id = ? AND state = 'queued'\","]],
    runs: [ledger],
    meant: 'a_decision_withdraws_the_result_of_its_task_and_nothing_else_the_chief_is_waiting_for',
  },
  {
    name: 'decided result: a decision withdraws the results of the other tasks',
    edits: [
      [
        `${LEDGER}/queue.rs`,
        FINDS,
        "       WHERE task_id != ? AND kind = 'result' AND state = 'queued'\",",
      ],
    ],
    runs: [ledger],
    meant: 'a_decision_withdraws_the_result_of_its_task_and_nothing_else_the_chief_is_waiting_for',
  },
  {
    name: 'decided result: the reason a result is withdrawn names no decision',
    edits: [
      [
        `${LEDGER}/queue.rs`,
        'params![format!("T-{} {why}", task.number), task.id],',
        'params![why, task.id],',
      ],
    ],
    runs: [ledger],
    meant:
      'the_chief_that_read_a_result_and_accepted_its_task_in_its_turn_is_not_given_the_result_after',
  },
  {
    name: 'decided result: a sending back withdraws in a step of its own, though it is refused',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        SEND_BACK_WITHDRAWS,
        '        withdraw_gated(store, task.id, &format!("sent back by @{by}"))?;',
      ],
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '    model::require_text(body, "body", MAX_BODY)?;',
          '    store.write(|store| {',
          '        let author = store.participant_by_handle(project_id, by)?;',
        ),
        lines(
          '    model::require_text(body, "body", MAX_BODY)?;',
          '    store.write(|store| {',
          '        let task = store.task_row(project_id, number)?;',
          '        require_task_state(&task, &["done", "failed"], "reopen")?;',
          '        withdraw_result(store, &task, "was sent back")',
          '    })?;',
          '    store.write(|store| {',
          '        let author = store.participant_by_handle(project_id, by)?;',
        ),
      ],
    ],
    runs: [ledger],
    meant: 'a_refused_decision_withdraws_nothing',
  },
  {
    name: 'decided result: a departed trace departs at another call',
    edits: [
      [
        'crates/cf-ledger/tests/replay.rs',
        '("core-api-003", 19, RESULT_ACCEPTED),',
        '("core-api-003", 20, RESULT_ACCEPTED),',
      ],
    ],
    runs: [replay],
    meant: 'every_departed_trace_is_there_and_still_departs',
  },
  {
    name: 'decided result: the eval counts a result the chief decided on as one that never reached it',
    edits: [
      [
        'evals/measure.mjs',
        "WHERE kind = 'result' AND recipient_id = ? AND state != 'cancelled'\",",
        "WHERE kind = 'result' AND recipient_id = ?\",",
      ],
    ],
    runs: [measure],
    meant:
      'does not count a result the chief decided on before it was given it as one that never reached the chief',
  },
]
