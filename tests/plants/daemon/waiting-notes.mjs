/**
 * The notes of a task's wait (the chief's report of 2026-10-08): what ConsensFlow
 * told a requester of a task that waits (it was taken back, it waits for a free
 * member, it is held for its member's quota, it stalled) is withdrawn if the
 * requester has not been given it when the task moves on: taken by a member, or
 * taken back to the board, as it is when resumed or called off. A hold note names
 * the reset as the time the member is expected, and a requester who was given it
 * is told when the daemon resumes the task. One plant takes one rule out; a test
 * of it fails.
 */
import { lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const ENGINE = 'crates/cf-engine/src'

/** The ledger's tests of the notes of a wait, over a ticking clock. */
const ledger = ['-p', 'cf-ledger', '--test', 'receipt', 'waiting_notes']
/** The engine's tests of the same on the fake windows. */
const engine = ['-p', 'cf-engine', '--test', 'dispatcher', 'waiting_notes']
/** The hold note's words, and what is taken for one. */
const words = ['-p', 'cf-ledger', '--lib', 'messages::holds']
/** The ledger's replay of Node's recordings, with the traces that depart on purpose. */
const replay = ['-p', 'cf-ledger', '--test', 'replay']
/** The whole of the engine's dispatcher tests, Node's traces and the departures among them. */
const dispatcher = ['-p', 'cf-engine', '--test', 'dispatcher']

/** The withdrawal a take-over makes, once the session is started. */
const TAKE_OVER_WITHDRAWS =
  '        leave_pause_notes(store, &task, &format!("was taken by @{}", member.handle))?;\n'
/** The withdrawal a release makes, before it writes the note that says where the task is. */
const RELEASE_WITHDRAWS = '        leave_pause_notes(store, &task, "was taken back")?;\n'
/** What the daemon's resume tells a requester who was given the hold note. */
const TELLS_GOES_ON = lines(
  '        if by.is_none() {',
  '            tell_goes_on(store, &task)?;',
  '        }',
  '',
)
/** The filter on the notes a hold note may be among. */
const REACHED =
  "             AND state IN ('delivering', 'delivered', 'read') AND created_at >= ?\","

export const PLANTS = [
  {
    name: 'waiting notes: a take-over leaves the notes of the wait it ends',
    edits: [[`${LEDGER}/tasks/giving.rs`, TAKE_OVER_WITHDRAWS, '']],
    runs: [ledger, engine, dispatcher],
    meant:
      'a_task_another_member_takes_withdraws_the_notes_of_its_wait_its_requester_has_not_been_given',
  },
  {
    name: 'waiting notes: a take-over withdraws a note its reader has been given',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        `         AND state = 'queued' AND body != ?",`,
        `         AND body != ?",`,
      ],
    ],
    runs: [ledger, engine],
    meant:
      'a_task_another_member_takes_leaves_what_was_given_what_others_wrote_and_what_is_about_another',
  },
  {
    name: 'waiting notes: a take-over withdraws a note the chief wrote',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        "       WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'",
        "       WHERE task_id = ? AND recipient_id = ? AND kind = 'note'",
      ],
    ],
    runs: [ledger],
    meant:
      'a_task_another_member_takes_leaves_what_was_given_what_others_wrote_and_what_is_about_another',
  },
  {
    name: 'waiting notes: a pause that comes after the news that the task went on takes it back',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        `         AND state = 'queued' AND body != ?",`,
        `         AND state = 'queued'",`,
      ],
      [
        `${LEDGER}/messages/pauses.rs`,
        '        params![reason, task.id, task.requester_id, goes_on(task.number)],',
        '        params![reason, task.id, task.requester_id],',
      ],
    ],
    runs: [ledger],
    meant: 'the_news_that_a_held_task_went_on_is_not_taken_back_by_a_hold_that_comes_after_it',
  },
  {
    name: 'waiting notes: a task taken or taken back stays in a note of several that named it',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        '    for (id, body) in several {',
        '    for (id, body) in several.into_iter().take(0) {',
      ],
    ],
    runs: [ledger],
    meant: 'a_task_another_member_takes_leaves_a_note_of_several_that_named_it',
  },
  {
    name: 'waiting notes: a release leaves the notes of the wait it ends',
    edits: [[`${LEDGER}/tasks/giving.rs`, RELEASE_WITHDRAWS, '']],
    runs: [ledger, dispatcher],
    meant: 'a_task_taken_back_withdraws_the_notes_of_the_wait_it_leaves_and_keeps_its_own',
  },
  {
    name: 'waiting notes: a release withdraws the note it writes of itself',
    edits: [
      [`${LEDGER}/tasks/giving.rs`, RELEASE_WITHDRAWS, ''],
      [
        `${LEDGER}/tasks/giving.rs`,
        lines('        release_ready(store, project_id)?;', '        Ok(TaskReleased {'),
        lines(
          '        release_ready(store, project_id)?;',
          '        leave_pause_notes(store, &task, "was taken back")?;',
          '        Ok(TaskReleased {',
        ),
      ],
    ],
    runs: [ledger],
    meant: 'a_task_taken_back_withdraws_the_notes_of_the_wait_it_leaves_and_keeps_its_own',
  },
  {
    name: 'waiting notes: the hold note promises the time its harness gave',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        'const TAIL: &str = ", or sooner if its account has quota again; it goes on by itself.";',
        'const TAIL: &str = "; it goes on by itself then.";',
      ],
    ],
    runs: [words, engine],
    meant: 'a_hold_note_names_when_the_member_is_expected_and_that_the_task_may_go_on_sooner',
  },
  {
    name: 'waiting notes: the scheduler holds in words of its own',
    edits: [
      [
        `${ENGINE}/scheduler.rs`,
        lines(
          '                self.seams.ledger.borrow_mut().note_hold(',
          '                    project.id,',
          '                    &task.requester,',
          '                    task.number,',
          '                    &participant.handle,',
          '                    until,',
          '                )?;',
        ),
        lines(
          '                self.seams.ledger.borrow_mut().note(',
          '                    project.id,',
          '                    &NewNote {',
          '                        from: None,',
          '                        to: task.requester.clone(),',
          '                        task: Some(task.number),',
          '                        body: format!(',
          '                            "T-{} waits with @{}: out of quota until {until}; it goes on by itself then.",',
          '                            task.number, participant.handle',
          '                        ),',
          '                    },',
          '                )?;',
        ),
      ],
    ],
    runs: [engine],
    meant:
      'withdraws_the_note_that_says_a_task_is_held_when_the_account_is_switched_and_says_nothing_after',
  },
  {
    name: 'waiting notes: the daemon resumes a held task and tells its requester nothing',
    edits: [[`${LEDGER}/tasks/pausing.rs`, TELLS_GOES_ON, '']],
    runs: [ledger, engine, replay],
    meant: 'a_requester_given_the_note_that_a_task_is_held_is_told_when_the_daemon_resumes_it',
  },
  {
    name: 'waiting notes: the chief resumes a held task and is told the daemon did',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        TELLS_GOES_ON,
        lines('        tell_goes_on(store, &task)?;', ''),
      ],
    ],
    runs: [ledger],
    meant: 'a_resume_of_the_chiefs_own_tells_nothing_more',
  },
  {
    name: 'waiting notes: a requester not given the hold note is told it goes on',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        REACHED,
        "             AND state != 'failed' AND created_at >= ?\",",
      ],
    ],
    runs: [ledger, engine],
    meant: 'a_requester_being_given_the_hold_note_is_told_too_but_one_not_given_it_is_told_nothing',
  },
  {
    name: 'waiting notes: a requester being given the hold note is not told it goes on',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        REACHED,
        "             AND state IN ('delivered', 'read') AND created_at >= ?\",",
      ],
    ],
    runs: [ledger],
    meant: 'a_requester_being_given_the_hold_note_is_told_too_but_one_not_given_it_is_told_nothing',
  },
  {
    name: 'waiting notes: the hold note of an earlier hold counts',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        REACHED,
        "             AND state IN ('delivering', 'delivered', 'read')\",",
      ],
      [
        `${LEDGER}/messages/holds.rs`,
        '        .query_map(params![task.id, task.requester_id, paused_at], |row| {',
        '        .query_map(params![task.id, task.requester_id], |row| {',
      ],
      [
        `${LEDGER}/messages/holds.rs`,
        lines(
          '    let Some(paused_at) = &task.paused_at else {',
          '        return Ok(());',
          '    };',
        ),
        '',
      ],
    ],
    runs: [ledger],
    meant: 'only_the_note_of_the_hold_the_task_is_in_counts',
  },
  {
    name: 'waiting notes: a hold note of an older build is no hold note',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        '        && [TAIL, OLD_TAIL].iter().any(|tail| body.ends_with(tail))',
        '        && [TAIL].iter().any(|tail| body.ends_with(tail))',
      ],
    ],
    runs: [ledger, words],
    meant: 'a_hold_note_of_an_older_build_counts_and_one_that_failed_or_is_not_one_does_not',
  },
  {
    name: 'waiting notes: any note of a task is taken for its hold note',
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        lines(
          '    body.starts_with(&format!("T-{number} waits with @"))',
          '        && [TAIL, OLD_TAIL].iter().any(|tail| body.ends_with(tail))',
        ),
        '    body.starts_with(&format!("T-{number} "))',
      ],
    ],
    runs: [words, ledger],
    meant: 'no_other_note_is_taken_for_the_hold_note_of_a_task',
  },
  {
    name: "waiting notes: the hold note of another task is taken for this task's",
    edits: [
      [
        `${LEDGER}/messages/holds.rs`,
        '    body.starts_with(&format!("T-{number} waits with @"))',
        '    body.contains(" waits with @")',
      ],
    ],
    runs: [words],
    meant: 'no_other_note_is_taken_for_the_hold_note_of_a_task',
  },
  {
    name: 'waiting notes: a task that goes back on the board is told it goes on',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '            let carried = transfer(store, &task, &assignee, &delivery_body(&task))?;',
          '            let at = store.at();',
        ),
        lines(
          '            let carried = transfer(store, &task, &assignee, &delivery_body(&task))?;',
          '            tell_goes_on(store, &task)?;',
          '            let at = store.at();',
        ),
      ],
    ],
    runs: [ledger],
    meant: 'a_task_that_goes_back_on_the_board_when_its_hold_ends_is_not_told_to_go_on',
  },
  {
    name: 'waiting notes: a departed trace departs at another call',
    edits: [
      [
        'crates/cf-ledger/tests/replay.rs',
        '("core-dispatcher-060", 236, ',
        '("core-dispatcher-060", 237, ',
      ],
    ],
    runs: [replay],
    meant: 'every_departed_trace_is_there_and_still_departs',
  },
]
