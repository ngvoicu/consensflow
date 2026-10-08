/**
 * The chief's report of 2026-10-08, end to end: the daemon the app starts and the
 * native `cf` of a window, built from the sources as a plant leaves them, and
 * the rig's fake agents in real PTYs (`tests/integration/`). Where the tests of
 * the crates hold each rule apart, these hold that the rule reaches the chief
 * through the pane host: a stale note not pasted into a chief in the middle of a
 * turn, a hold told as it ends, `--help` answered and posted nowhere. One plant
 * takes one rule out; a test of it fails.
 */
import { BUILD, lines } from './kit.mjs'

const LEDGER = 'crates/cf-ledger/src'
const WORDS = 'crates/cf/src/board/words.rs'

/** The notes of a task's wait, withdrawn and told. */
const WAITING = [
  process.execPath,
  '--test',
  '--test-concurrency=1',
  'tests/integration/waiting-notes.test.mjs',
]
/** Every verb of the board asked for help. */
const HELP = [process.execPath, '--test', '--test-concurrency=1', 'tests/integration/help.test.mjs']

export const PLANTS = [
  {
    // m-1668: "T-357 was taken back from @ullr … waits for another standard
    // worker" reached the chief after another worker had taken T-357.
    name: 'rig: a take-over leaves the note that the task was taken back for the chief to be given',
    edits: [
      [
        `${LEDGER}/tasks/giving.rs`,
        '        leave_pause_notes(store, &task, &format!("was taken by @{}", member.handle))?;\n',
        '',
      ],
    ],
    runs: [BUILD, WAITING],
    meant:
      'the note that a task was taken back is withdrawn when another worker takes it before the chief is given the note',
  },
  {
    // m-1670: the hold note reached the chief after the daemon had resumed the task.
    name: 'rig: a resume leaves the note that the task is held for the chief to be given',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '        // A refusal below writes nothing, this withdrawal with it.',
          '        leave_pause_notes(store, &task, "resumed")?;',
        ),
        '',
      ],
    ],
    runs: [BUILD, WAITING],
    meant:
      'the note that a task is held is withdrawn when the account is switched before the chief is given it, and nothing is said after',
  },
  {
    name: 'rig: the daemon resumes a held task and the chief given the note is told nothing',
    edits: [
      [
        `${LEDGER}/tasks/pausing.rs`,
        lines(
          '        if by.is_none() {',
          '            tell_goes_on(store, &task)?;',
          '        }',
          '',
        ),
        '',
      ],
    ],
    runs: [BUILD, WAITING],
    meant:
      'a chief given the note that a task is held is told it goes on when the account is switched',
  },
  {
    // m-1542: `cf note --help` posted a note saying "--help".
    name: 'rig: cf --help is text and is posted',
    edits: [[WORDS, '        !self.literal\n', '        false\n            && !self.literal\n']],
    runs: [BUILD, HELP],
    meant: 'a window asks every verb of cf for help, and nothing is posted or read',
  },
]
