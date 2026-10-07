/**
 * The note that tells a requester its tasks are paused: one note for the
 * tasks that stall together (a pass, the closing of a project's windows),
 * withdrawn when its task is resumed or called off, or narrowed when it names
 * several, however the resume goes. One plant takes one rule out; a test of it
 * fails.
 */
import { lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const ENGINE = 'crates/cf-engine/src'

/** The ledger's tests of the notes: their words, and what a resume and a cancel do to them. */
const ledger = ['-p', 'cf-ledger', '--test', 'pause_notes']
/** The same on each way a resume goes: back on the board, and as the task nobody holds yet. */
const resumes = ['-p', 'cf-ledger', '--test', 'receipt', 'pause_notes']
/** The words of a pause note, and what is taken for one. */
const words = ['-p', 'cf-ledger', '--lib', 'messages::pauses']
/** The engine's tests of a restart, a close and a resume, on the fake windows. */
const engine = ['-p', 'cf-engine', '--test', 'dispatcher', 'pause_notes']
/** The ledger's replay of Node's recordings, with the traces that depart on purpose. */
const replay = ['-p', 'cf-ledger', '--test', 'replay']

/** The withdrawal a resume makes, at the head of `resume_task`. */
const RESUME_WITHDRAWS = lines(
  '        // A refusal below writes nothing, this withdrawal with it.',
  '        leave_pause_notes(store, &task, "resumed")?;',
)
/** The resume's words, put into the window the task has. */
const WORDS =
  '        let words = carrier_body(store, &task, assignee.id, &format!("Resumed: {body}"))?;'

export const PLANTS = [
  {
    name: 'pause notes: a resume leaves the notes of its pause',
    edits: [[`${LEDGER}/tasks/pausing.rs`, RESUME_WITHDRAWS, '']],
    runs: [ledger],
    meant: 'a_resume_withdraws_what_its_requester_was_told_of_the_pause_and_has_not_been_given',
  },
  {
    name: 'pause notes: a cancel leaves a note of several as it was',
    edits: [
      [
        `${LEDGER}/tasks/finishing.rs`,
        lines(
          '    // A note of several that names it, still queued, names it no more.',
          '    leave_pause_notes(store, &task, "cancelled")?;',
        ),
        '',
      ],
    ],
    runs: [ledger],
    meant: 'a_task_called_off_leaves_the_note_of_several_and_a_refused_resume_withdraws_nothing',
  },
  {
    name: 'pause notes: a resume withdraws only when its words go into a window',
    edits: [
      [`${LEDGER}/tasks/pausing.rs`, RESUME_WITHDRAWS, ''],
      [
        `${LEDGER}/tasks/pausing.rs`,
        WORDS,
        lines('        leave_pause_notes(store, &task, "resumed")?;', WORDS),
      ],
    ],
    runs: [resumes],
    meant: 'a_resume_after_its_session_ended_withdraws_the_pause_note_and_keeps_its_own',
  },
  {
    name: 'pause notes: a resume withdraws in a step of its own, though it is refused',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '    model::require_text(body, "body", MAX_BODY)?;',
          '    store.write(|store| {',
          '        let author = by',
        ),
        lines(
          '    model::require_text(body, "body", MAX_BODY)?;',
          '    store.write(|store| {',
          '        let task = store.task_row(project_id, number)?;',
          '        require_task_state(&task, &["paused"], "resume")?;',
          '        leave_pause_notes(store, &task, "resumed")',
          '    })?;',
          '    store.write(|store| {',
          '        let author = by',
        ),
      ],
    ],
    runs: [ledger],
    meant: 'a_task_called_off_leaves_the_note_of_several_and_a_refused_resume_withdraws_nothing',
  },
  {
    name: 'pause notes: a resume withdraws a note the chief wrote',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        "       WHERE task_id = ? AND recipient_id = ? AND sender_id IS NULL AND kind = 'note'",
        "       WHERE task_id = ? AND recipient_id = ? AND kind = 'note'",
      ],
    ],
    runs: [ledger],
    meant: 'a_resume_withdraws_what_its_requester_was_told_of_the_pause_and_has_not_been_given',
  },
  {
    name: 'pause notes: a resume withdraws a note its reader has been given',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        lines(
          `         AND state = 'queued'",`,
          '        params![reason, task.id, task.requester_id],',
        ),
        lines('",', '        params![reason, task.id, task.requester_id],'),
      ],
    ],
    runs: [ledger],
    meant: 'a_resume_withdraws_what_its_requester_was_told_of_the_pause_and_has_not_been_given',
  },
  {
    name: 'pause notes: a resume leaves a note of several naming the task',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        '    for (id, body) in several {',
        '    for (id, body) in several.into_iter().take(0) {',
      ],
    ],
    runs: [ledger],
    meant:
      'a_resume_takes_its_task_out_of_a_note_of_several_and_a_last_one_leaves_it_a_note_of_the_task',
  },
  {
    name: 'pause notes: a note that is no pause note is taken for one',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        '    (render(&paused) == body).then_some(paused)',
        '    Some(paused)',
      ],
    ],
    runs: [words],
    meant: 'no_other_note_is_taken_for_a_pause_note',
  },
  {
    name: 'pause notes: a note takes a task whatever state it is in',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        'note.sender.is_none() && note.state == "queued")',
        'note.sender.is_none())',
      ],
    ],
    runs: [ledger],
    meant: 'a_note_that_was_given_or_withdrawn_or_is_no_pause_note_takes_no_task',
  },
  {
    name: 'pause notes: a note names its tasks out of order',
    edits: [
      [`${LEDGER}/messages/pauses.rs`, '            paused.sort_by_key(|named| named.number);', ''],
    ],
    runs: [ledger],
    meant: 'a_stall_of_the_same_pass_joins_the_note_and_it_names_them_all_in_order',
  },
  {
    name: 'pause notes: a narrowed note is tied to none',
    edits: [
      [
        `${LEDGER}/messages/pauses.rs`,
        '        [one] => Some(store.task_row(project_id, one.number)?.id),',
        '        [_] => None::<i64>,',
      ],
    ],
    runs: [ledger],
    meant:
      'a_resume_takes_its_task_out_of_a_note_of_several_and_a_last_one_leaves_it_a_note_of_the_task',
  },
  {
    name: 'pause notes: a stall never joins the note of its pass',
    edits: [
      [
        `${ENGINE}/stalls.rs`,
        '        let earlier = self.stalls.told.borrow().get(&key).copied();',
        '        let earlier: Option<i64> = None;',
      ],
    ],
    runs: [engine],
    meant: 'tells_a_chief_once_of_all_the_tasks_a_restart_pauses',
  },
  {
    name: 'pause notes: a pass is no burst',
    edits: [
      [
        `${ENGINE}/dispatcher.rs`,
        lines('        let _burst = self.stalls.begin_burst();', '        self.resume_held()?;'),
        '        self.resume_held()?;',
      ],
    ],
    runs: [engine],
    meant: 'tells_a_chief_once_of_all_the_tasks_a_restart_pauses',
  },
  {
    name: 'pause notes: closing windows is no burst',
    edits: [
      [
        `${ENGINE}/dispatcher.rs`,
        lines(
          '        let _burst = self.stalls.begin_burst();',
          '        let mut closing = Vec::new();',
        ),
        '        let mut closing = Vec::new();',
      ],
    ],
    runs: [engine],
    meant: 'tells_a_chief_once_of_all_the_tasks_a_closed_project_pauses',
  },
  {
    name: 'pause notes: what a burst told outlives it',
    edits: [
      [
        `${ENGINE}/stalls.rs`,
        lines(
          '        if left == 0 {',
          '            self.0.told.borrow_mut().clear();',
          '        }',
        ),
        '        let _ = left;',
      ],
    ],
    runs: [engine],
    meant: 'tells_a_stall_after_a_close_in_a_note_of_its_own_though_the_close_left_a_note_queued',
  },
  {
    name: 'pause notes: a departed trace departs at another call',
    edits: [
      [
        'crates/cf-ledger/tests/replay.rs',
        '("core-dispatcher-042", 72, ',
        '("core-dispatcher-042", 73, ',
      ],
    ],
    runs: [replay],
    meant: 'every_departed_trace_is_there_and_still_departs',
  },
]
