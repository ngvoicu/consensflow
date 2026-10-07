/**
 * A window that stopped a turn before a word of its answer took the turn's
 * message back out of its conversation: its look says so, the stop is paid at
 * rest and, in the same step, the ledger keeps the message for the window
 * again, so the words that resume the task carry it. A stop that leaves the
 * message in the conversation changes nothing. One plant takes one rule out;
 * a test of it fails.
 */
import { lines } from './kit.mjs'

const ENGINE = 'crates/cf-engine/src'
const LEDGER = 'crates/cf-ledger/src/messages/delivery.rs'
const CLAUDE = 'crates/cf-harness/src/claude/adapter.rs'

/** The engine's tests of the rule, on the fake window. */
const engine = ['-p', 'cf-engine', '--test', 'dispatcher', 'taken_back']
/** The ledger's tests of the rule: what is kept, and what carries it. */
const ledger = ['-p', 'cf-ledger', '--test', 'receipt', 'taken_back']
/** The Claude window's looks, on the records of two live runs. */
const window = ['-p', 'cf-harness', '--test', 'launch', 'claude::stops']

/** The step that pays a stop at rest: what the ledger is told, then the payment. */
const TELLS = lines(
  '                    if observed.took_back {',
  '                        self.take_back(participant, &stop, observed)?;',
  '                    }',
  '                    self.pay(record, &stop);',
)

export const PLANTS = [
  {
    name: 'taken back: the stop is paid at rest and the ledger is not told',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        '                    if observed.took_back {',
        '                    if observed.took_back && false {',
      ],
    ],
    runs: [engine],
    meant:
      'a_stop_paid_at_rest_by_a_window_that_took_its_brief_back_keeps_the_brief_for_the_words_that_resume_the_task',
  },
  {
    name: 'taken back: every stop paid at rest takes its message back',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        '                    if observed.took_back {',
        '                    if true {',
      ],
    ],
    runs: [engine],
    meant:
      'a_stop_that_leaves_the_message_in_the_conversation_leaves_it_received_and_the_resume_carries_no_brief',
  },
  {
    name: 'taken back: a turn that began with another message than the task`s is taken for it',
    edits: [
      [
        `${ENGINE}/taken_back.rs`,
        '                && turn.is_some_and(|turn| turn.text.contains(&marker_of(message.id)))',
        '                && turn.is_some()',
      ],
    ],
    runs: [engine],
    meant:
      'a_stop_that_leaves_the_message_in_the_conversation_leaves_it_received_and_the_resume_carries_no_brief',
  },
  {
    name: 'taken back: the stop is paid before the ledger is told, and a failed write loses it',
    edits: [
      [
        `${ENGINE}/stops.rs`,
        TELLS,
        lines(
          '                    self.pay(record, &stop);',
          '                    if observed.took_back {',
          '                        self.take_back(participant, &stop, observed)?;',
          '                    }',
        ),
      ],
    ],
    runs: [engine],
    meant:
      'a_ledger_that_could_not_be_written_leaves_the_stop_owed_and_the_next_look_at_rest_takes_it_back',
  },
  {
    name: 'taken back: the message is cancelled, not kept for the window again',
    edits: [
      [
        LEDGER,
        `"UPDATE message SET state = 'queued', reason = ?, delivered_at = NULL, receipt = NULL`,
        `"UPDATE message SET state = 'cancelled', reason = ?, delivered_at = NULL, receipt = NULL`,
      ],
    ],
    runs: [ledger],
    meant: 'a_brief_its_window_took_back_is_kept_again_and_the_words_that_resume_the_task_carry_it',
  },
  {
    name: 'taken back: it does not join the words that already wait for the window',
    edits: [
      [
        LEDGER,
        lines(
          '        adopt(store, message_id)?;',
          '        known_message(store, message_id)',
          '    })',
          '}',
          '',
          '/// A delivery given up',
        ),
        lines(
          '        known_message(store, message_id)',
          '    })',
          '}',
          '',
          '/// A delivery given up',
        ),
      ],
    ],
    runs: [ledger, engine],
    meant:
      'words_that_resumed_the_task_before_the_stop_was_paid_take_the_message_in_when_it_is_taken_back',
  },
  {
    name: 'taken back: what its paste carried is left received, and lost with it',
    edits: [
      [
        LEDGER,
        lines(
          '        store.db.execute(',
          `            "UPDATE message SET state = 'queued', delivered_at = NULL, receipt = NULL`,
          `         WHERE carried_by = ? AND state = 'delivered'",`,
          '            [message_id],',
          '        )?;',
        ),
        '',
      ],
    ],
    runs: [ledger],
    meant:
      'a_resume_taken_back_with_what_it_carried_is_kept_again_with_it_and_the_next_words_carry_each_once',
  },
  {
    name: 'taken back: the look at a Claude window does not say it took its message back',
    edits: [
      [
        CLAUDE,
        'observed(idle && (settled || empty) || by_press, waiting, by_press)',
        'observed(idle && (settled || empty) || by_press, waiting, false)',
      ],
    ],
    runs: [window],
    meant:
      'a_turn_the_daemon_interrupted_before_claude_wrote_a_word_is_read_at_rest_a_moment_after_the_press',
  },
]
