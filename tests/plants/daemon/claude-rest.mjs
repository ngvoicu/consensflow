/**
 * A Claude window that stopped is at rest: the interrupt record that names no
 * message ends its turn, a turn the daemon interrupted before a word of it is
 * read at rest by the daemon's own press (and no turn just pasted is), and the
 * text Claude puts back in its input box is cleared before the next paste. One
 * plant takes one rule out; a test of it fails.
 */
import { lines } from './kit.mjs'

const HARNESS = 'crates/cf-harness/src/claude'
const ENGINE = 'crates/cf-engine/src'

/** The Claude reader's own tests: what each record says of a turn. */
const reader = ['-p', 'cf-harness', '--lib', 'claude::record']
/** The rule of a stopped turn, on readings. */
const rule = ['-p', 'cf-harness', '--lib', 'claude::stopped']
/** A Claude window on the records of two live runs: its looks and its pastes. */
const window = ['-p', 'cf-harness', '--test', 'launch', 'claude::stops']
/** The engine's dispatcher tests, whole. */
const dispatcher = ['-p', 'cf-engine', '--test', 'dispatcher']

const LOOK = '                    let by_press = idle && self.stopped(&reading);'
/** The engine's press, and the window told once the host took the keys. */
const PRESS =
  '            press_interrupt(&*self.seams.host, &*self.seams.time, &pane, keys).await;'
const TOLD = lines(
  '            if press_interrupt(&*self.seams.host, &*self.seams.time, &pane, keys).await {',
  '                window.interrupted();',
  '            }',
)

export const PLANTS = [
  {
    name: 'rest: an interrupt record must name the message it interrupted, as Node asked',
    edits: [
      [
        `${HARNESS}/record/text.rs`,
        lines(
          '    let message = record.get("message");',
          '    if field(message, "role").and_then(Value::as_str) != Some("user") {',
          '        return false;',
          '    }',
        ),
        lines(
          '    let message = record.get("message");',
          '    let names_message = record',
          '        .get("interruptedMessageId")',
          '        .and_then(Value::as_str)',
          '        .is_some_and(|id| !id.is_empty());',
          '    if field(message, "role").and_then(Value::as_str) != Some("user") || !names_message {',
          '        return false;',
          '    }',
        ),
      ],
      [
        `${HARNESS}/record/keep.rs`,
        '    ("error", &Keep::Scalar),\n',
        '    ("error", &Keep::Scalar),\n    ("interruptedMessageId", &Keep::Scalar),\n',
      ],
    ],
    runs: [reader],
    meant:
      'only_a_marker_alone_in_a_list_on_a_user_record_ends_the_turn_whether_or_not_it_names_the_message',
  },
  {
    name: 'rest: a window idle with its turn in flight is at rest with no press for it',
    edits: [
      [
        `${HARNESS}/adapter.rs`,
        lines(
          '        self.pressed',
          '            .borrow()',
          '            .as_ref()',
          '            .is_some_and(|pressed| pressed.stopped(record, self.time.wall_ms()))',
        ),
        lines(
          '        self.pressed',
          '            .borrow()',
          '            .as_ref()',
          '            .map_or(true, |pressed| pressed.stopped(record, self.time.wall_ms()))',
        ),
      ],
    ],
    runs: [window],
    meant:
      'a_turn_the_daemon_interrupted_before_claude_wrote_a_word_is_read_at_rest_a_moment_after_the_press',
  },
  {
    name: 'rest: the press counts at once, with no moment for a turn about to begin to show',
    edits: [[`${HARNESS}/stopped.rs`, 'const HOLD_MS: i64 = 1_000;', 'const HOLD_MS: i64 = 0;']],
    runs: [rule, window],
    meant:
      'a_press_over_the_turn_in_flight_stopped_it_once_a_moment_has_passed_with_nothing_written',
  },
  {
    name: 'rest: a press is for any turn, not only the one it was pressed over',
    edits: [[`${HARNESS}/stopped.rs`, '    &*record.items[at].id == over\n', '    true\n']],
    runs: [rule, window],
    meant: 'a_turn_just_pasted_is_not_stopped_by_the_press_for_the_turn_before_it',
  },
  {
    name: 'rest: a turn Claude has begun to answer is one it stopped before it began',
    edits: [
      [
        `${HARNESS}/stopped.rs`,
        '            .all(|item| item.role == Role::Custom)',
        '            .all(|_| true)',
      ],
    ],
    runs: [rule],
    meant:
      'a_turn_with_a_word_of_the_assistants_or_a_tools_output_is_not_one_stopped_before_it_began',
  },
  {
    name: 'rest: a window that is busy or waiting is read at rest by a press',
    edits: [
      [`${HARNESS}/adapter.rs`, LOOK, '                    let by_press = self.stopped(&reading);'],
    ],
    runs: [window],
    meant: 'a_window_that_is_busy_or_waiting_is_not_read_at_rest_by_a_press',
  },
  {
    name: 'rest: a window that shows another conversation keeps the press',
    edits: [
      [
        `${HARNESS}/adapter.rs`,
        lines(
          '        *self.asked.borrow_mut() = None;',
          '        *self.pressed.borrow_mut() = None;',
          '        self.restored.set(false);',
          '    }',
        ),
        '    }',
      ],
    ],
    runs: [window],
    meant: 'a_window_that_shows_another_conversation_forgets_the_press',
  },
  {
    name: 'rest: a paste leaves the press for the turn it begins',
    edits: [
      [
        `${HARNESS}/adapter.rs`,
        lines(
          '                *self.pressed.borrow_mut() = None;',
          '                self.restored.set(false);',
        ),
        '                self.restored.set(false);',
      ],
    ],
    runs: [window],
    meant: 'the_text_claude_put_back_in_its_input_box_is_cleared_before_the_next_message_is_pasted',
  },
  {
    name: 'rest: the text Claude put back in its input box is pasted after',
    edits: [
      [
        `${HARNESS}/adapter.rs`,
        '            if self.restored.get() {',
        '            if self.restored.get() && false {',
      ],
    ],
    runs: [window],
    meant: 'the_text_claude_put_back_in_its_input_box_is_cleared_before_the_next_message_is_pasted',
  },
  {
    name: 'rest: an input box that could not be cleared is pasted into all the same',
    edits: [
      [
        `${HARNESS}/adapter.rs`,
        lines(
          '                    let reason = format!("the window\'s input box could not be cleared: {cause}");',
          '                    return Ok(Admission::Refused { reason });',
        ),
        '                    let _ = cause;',
      ],
    ],
    runs: [window],
    meant:
      'an_input_box_that_could_not_be_cleared_is_not_pasted_into_and_is_cleared_again_next_time',
  },
  {
    name: 'rest: the key that clears the input box is Ctrl+U, which clears a line',
    edits: [
      [`${HARNESS}/stopped.rs`, 'CLEAR_INPUT: [u8; 1] = [0x03];', 'CLEAR_INPUT: [u8; 1] = [0x15];'],
    ],
    runs: [window],
    meant: 'the_text_claude_put_back_in_its_input_box_is_cleared_before_the_next_message_is_pasted',
  },
  {
    name: 'rest: the engine presses the interrupt keys and does not tell the window',
    edits: [[`${ENGINE}/stops.rs`, TOLD, lines(PRESS, '            let _ = &window;')]],
    runs: [dispatcher],
    meant:
      'a_turn_stopped_before_its_first_word_is_paid_at_rest_and_the_resume_goes_in_once_and_ends_with_its_result',
  },
  {
    name: 'rest: the window is told of keys the host refused',
    edits: [[`${ENGINE}/stops.rs`, TOLD, lines(PRESS, '            window.interrupted();')]],
    runs: [dispatcher],
    meant: 'keys_the_host_refused_are_no_press_and_the_window_is_not_told_until_a_round_goes_in',
  },
]
