/**
 * The receipt and stop redesign's fix round, one task per session at every
 * door: a member session takes one task at a time, whichever door gives it
 * one (a follow-up, a reopen, a task named for it, or what waited on the board
 * for its needs), and what waited goes to the session once it is free. One
 * plant takes one check out; a test of the door fails. The reopen's plant is
 * in `obligations.mjs`, which is where the rule was first made.
 */
import { lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'

/** The ledger's tests of the redesign: every sequence of its model. */
const receipt = ['-p', 'cf-ledger', '--test', 'receipt']
/** The engine's dispatcher tests about a follow-up that waits for its session. */
const followUps = ['-p', 'cf-engine', '--test', 'dispatcher', 'follow_ups']

/** The tests of the release, which a second task waiting for a session is the state of. */
const RELEASE_WAITS = 'a_release_to_a_session_that_has_another_in_hand_waits_and_goes_'

export const PLANTS = [
  {
    name: 'session: a follow-up is given to a session that is on its work',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        lines('    require_free(store, &session)?;', '    bring_back(store, session)'),
        '    bring_back(store, session)',
      ],
    ],
    runs: [receipt],
    meant: 'a_follow_up_is_refused_while_its_session_is_on_its_work_and_goes_once_it_is_free',
  },
  {
    name: 'session: a task named for a session is given to it whatever it has',
    edits: [
      [
        `${LEDGER}/tasks/giving.rs`,
        lines(
          '                let named = store.participant_by_handle(project_id, to)?;',
          '                require_free(store, &named)?;',
        ),
        '                let named = store.participant_by_handle(project_id, to)?;',
      ],
    ],
    runs: [receipt],
    meant: 'a_task_named_for_a_session_is_refused_while_it_is_on_its_work_and_goes_once_it_is_free',
  },
  {
    name: 'session: a follow-up that waits for what it needs does not make its session busy',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        '        &[&HELD_TASK_STATES[..], &["paused", "open"]].concat(),',
        '        &[&HELD_TASK_STATES[..], &["paused"]].concat(),',
      ],
    ],
    runs: [receipt],
    meant:
      'a_follow_up_waiting_for_what_it_needs_makes_its_session_busy_to_every_door_though_its_window_has_nothing_in_hand',
  },
  {
    name: 'session: the rule holds a member that is no session, the chief too',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        '    if taker.member_id.is_some() && holds_work(store, taker.id)? {',
        '    if holds_work(store, taker.id)? {',
      ],
    ],
    runs: [receipt],
    meant: 'the_rule_is_a_member_sessions_and_the_chief_and_a_members_own_lane_are_held_to_none',
  },
  {
    name: 'session: a window is kept open for a follow-up that waits for what it needs',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        '        &[&HELD_TASK_STATES[..], &["paused"]].concat(),',
        '        &[&HELD_TASK_STATES[..], &["paused", "open"]].concat(),',
      ],
    ],
    runs: [followUps],
    meant:
      'a_follow_up_waiting_for_what_it_needs_keeps_no_window_open_and_goes_to_one_opened_again_on_the_conversation',
  },
  {
    name: 'session: a follow-up that waits for what it needs is in hand of its window',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        '        &[&HELD_TASK_STATES[..], &["paused"]].concat(),',
        '        &[&HELD_TASK_STATES[..], &["paused", "open"]].concat(),',
      ],
    ],
    runs: [receipt],
    meant:
      'a_follow_up_waiting_for_what_it_needs_makes_its_session_busy_to_every_door_though_its_window_has_nothing_in_hand',
  },
  {
    name: 'session: a paused task is not in hand of its window',
    edits: [
      [
        `${LEDGER}/staff/sessions.rs`,
        '        &[&HELD_TASK_STATES[..], &["paused"]].concat(),',
        '        &HELD_TASK_STATES[..],',
      ],
    ],
    runs: [receipt],
    meant: 'a_task_waiting_for_a_session_stays_through_what_does_not_free_it',
  },
  {
    name: 'session: a release goes to a session that has a task in hand',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        '        if assignee.member_id.is_some() && has_task_in_hand(store, assignee.id)? {',
        '        if assignee.member_id.is_some() && false && has_task_in_hand(store, assignee.id)? {',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}with_that_tasks_result`,
  },
  {
    name: 'session: a task waiting for its session is not released when its need is accepted',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '        store.move_task(&task, "accepted", json!({ "by": by }))?;',
          '        release_ready(store, project_id)?;',
        ),
        '        store.move_task(&task, "accepted", json!({ "by": by }))?;',
      ],
    ],
    runs: [receipt],
    meant:
      'a_follow_up_waiting_for_what_it_needs_makes_its_session_busy_to_every_door_though_its_window_has_nothing_in_hand',
  },
  {
    name: 'session: a task waiting for its session is not released with its result',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '        store.move_task(&task, "done", json!({ "result": message_id }))?;',
          '        release_ready(store, project_id)?;',
        ),
        '        store.move_task(&task, "done", json!({ "result": message_id }))?;',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}with_that_tasks_result`,
  },
  {
    name: 'session: a task waiting for its session is not released when the task is called off',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '    store.move_task(&task, "cancelled", json!({ "by": by }))?;',
          '    release_ready(store, project_id)?;',
        ),
        '    store.move_task(&task, "cancelled", json!({ "by": by }))?;',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}when_that_task_is_called_off`,
  },
  {
    name: 'session: a task waiting for its session is not released when the task fails',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '        store.move_task(&task, "failed", json!({ "reason": reason }))?;',
          '        release_ready(store, project_id)?;',
        ),
        '        store.move_task(&task, "failed", json!({ "reason": reason }))?;',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}when_that_task_fails`,
  },
  {
    name: 'session: a task waiting for its session is not released when a delivery to it fails for good',
    edits: [
      [
        `${LEDGER}/messages/delivery.rs`,
        lines(
          '                store.move_task(&task, "failed", json!({}))?;',
          '                release_ready(store, task.project_id)?;',
        ),
        '                store.move_task(&task, "failed", json!({}))?;',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}when_that_tasks_delivery_fails_for_good`,
  },
  {
    name: 'session: a task waiting for its session is not released when its task is taken back for its tier',
    edits: [
      [
        `${LEDGER}/tasks/giving.rs`,
        lines('        release_ready(store, project_id)?;', '        Ok(TaskReleased {'),
        '        Ok(TaskReleased {',
      ],
    ],
    runs: [receipt],
    meant: `${RELEASE_WAITS}when_that_task_is_taken_back_for_its_tier`,
  },
  {
    name: 'session: a window is asked to stop for the task its oldest words were about',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        '                           ORDER BY m.id DESC LIMIT 1)",',
        '                           ORDER BY m.id ASC LIMIT 1)",',
      ],
    ],
    runs: [receipt],
    meant: 'a_stop_paid_for_one_task_never_hides_a_later_stop_of_the_task_the_window_goes_on_with',
  },
]
