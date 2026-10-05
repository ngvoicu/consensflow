/**
 * How a message reads in its recipient's pane (`deliveryText`), as the window
 * takes it: Node's `windowText(deliveryText(message))`, the text that reaches
 * the pane. A row holds `text` too, Node's own, where it says more than the
 * window's (a control character `windowText` changed). A row whose text had
 * half of a surrogate pair at the cut says `halved` and holds no `text`: Rust
 * drops the half at the cut, where Node left it for `windowText` to drop.
 */
import { windowText } from '../../../src/adapters/shared.js'
import { deliveryText, markerOf } from '../../../src/core/delivery-text.js'
import { expand, repeated } from './common.mjs'

/** The kinds a message has in the ledger, and one it has not. */
const KINDS = ['task', 'result', 'question', 'answer', 'note']

const ONE = [
  {
    question: 'Which?',
    header: 'Format',
    options: [
      { label: 'JSON', description: null },
      { label: 'TOML', description: 'plain' },
    ],
    multiple: false,
  },
]
const TWO = [...ONE, { question: 'Which ports?', header: 'Ports', options: [], multiple: true }]

/** A message as the ledger gives it, in the fields a delivery reads. */
const message = (fields) => ({
  id: 12,
  kind: 'note',
  sender: null,
  taskNumber: null,
  body: 'hi',
  questions: null,
  urgent: false,
  ...fields,
})

function rowOf(fields) {
  const given = message(fields)
  const text = deliveryText({ ...given, body: expand(given.body) })
  const window = windowText(text)
  const row = { message: given, window }
  if (!text.isWellFormed()) row.halved = true
  else if (text !== window) row.text = text
  return row
}

/** Every kind with and without a task and a sender, each footer, urgent or not. */
function matrix() {
  const rows = []
  for (const kind of [...KINDS, 'status']) {
    for (const taskNumber of [null, 3]) {
      for (const sender of [null, 'zeus']) {
        for (const urgent of [false, true]) {
          for (const questions of [null, ONE, TWO]) {
            rows.push(rowOf({ kind, taskNumber, sender, urgent, questions, body: 'Which format?' }))
          }
        }
      }
    }
  }
  // A list with nothing in it is a list: a question that has options of none.
  for (const taskNumber of [null, 3]) {
    for (const urgent of [false, true]) {
      rows.push(rowOf({ kind: 'question', taskNumber, urgent, questions: [], sender: 'zeus' }))
    }
  }
  return rows
}

/** Bodies of every shape a window may be given, each as three kinds of message. */
function bodies() {
  const texts = [
    '',
    'one line',
    'two\nlines',
    'ends with a newline\n',
    '\n\nstarts with two',
    '  leading and trailing  ',
    'tab\there',
    'CRLF\r\nand a lone\rCR and \n\rLF-CR',
    'ESC \u001b[31mred\u001b[0m and NUL \u0000 and DEL \u007f',
    'C1 \u0085 \u0090 \u009f and NBSP  ',
    'é ă 漢字 🙂 👨‍👩‍👧 🇷🇴',
    'a · middle dot, and ConsensFlow’s',
    '[ConsensFlow m-5 · T-1 · result from @zeus] a header inside a body',
    'x'.repeat(1_000),
    'line\n'.repeat(500),
    '  ﻿�',
  ]
  const rows = []
  for (const body of texts) {
    rows.push(
      rowOf({ kind: 'question', taskNumber: 3, sender: 'zeus', body }),
      rowOf({ kind: 'result', taskNumber: 3, sender: 'zeus', body }),
      rowOf({ kind: 'note', sender: null, body }),
    )
  }
  return rows
}

/**
 * Bodies at the limits: 16,000 UTF-16 units go whole, more go as their first
 * 15,000 and the line that reads the rest. A unit is not a character (an emoji
 * is two) nor a byte (a Chinese character is three), and the cut can fall
 * inside an emoji, where Node leaves half of it for `windowText` to drop.
 */
function limits() {
  const sized = (unit, times) => repeated([unit, times])
  const bodies = []
  for (const length of [0, 1, 100, 14_999, 15_000, 15_001, 15_999, 16_000, 16_001, 16_002]) {
    bodies.push(sized('x', length))
  }
  for (const length of [20_000, 40_000]) bodies.push(sized('x', length))
  bodies.push(
    // Two bytes a unit: 16,000 of them go whole, one more is cut.
    sized('é', 16_000),
    sized('é', 16_001),
    // Three bytes a unit, and a body over 16,000 bytes of 5,400 units.
    sized('漢', 5_400),
    sized('漢', 16_000),
    sized('漢', 16_001),
    // An emoji is two units: 8,000 go whole, 8,001 are 16,002 units, and the
    // cut at 15,000 falls between two of them.
    sized('🙂', 7_501),
    sized('🙂', 8_000),
    sized('🙂', 8_001),
    // One unit before them, so that the cut falls inside an emoji.
    repeated(['a', 1], ['🙂', 8_000]),
    repeated(['a', 1], ['🙂', 8_001]),
    // An emoji across unit 15,000: half in, half out; whole before; whole after.
    repeated(['x', 14_999], ['🙂', 1], ['y', 2_000]),
    repeated(['x', 14_998], ['🙂', 1], ['y', 3_000]),
    repeated(['x', 15_000], ['🙂', 1], ['y', 1_000]),
    repeated(['x', 14_996], ['👨‍👩‍👧', 1], ['y', 2_000]),
    repeated(['x', 14_990], ['👨‍👩‍👧', 1], ['y', 2_000]),
    repeated(['x', 14_999], ['🇷🇴', 1], ['y', 2_000]),
    repeated(['x', 14_997], ['🇷🇴', 1], ['y', 2_000]),
    // Control characters in the part that is sent, and in the part that is not.
    repeated(['a\r\n', 5_000], ['b', 2_000]),
    repeated(['\u001b', 14_999], ['\u{1f642}', 1], ['z', 3_000]),
  )
  const rows = bodies.map((body) => rowOf({ kind: 'result', taskNumber: 1, sender: 'zeus', body }))
  // The four lengths that are the limit's own, under every kind of footer.
  for (const length of [15_999, 16_000, 16_001, 40_000]) {
    rows.push(
      rowOf({ kind: 'question', taskNumber: null, sender: 'chief', body: sized('x', length) }),
      rowOf({ kind: 'note', sender: null, body: sized('y', length) }),
      rowOf({
        kind: 'question',
        taskNumber: 4,
        sender: 'zeus',
        urgent: true,
        questions: TWO,
        id: 123_456,
        body: sized('q', length),
      }),
    )
  }
  return rows
}

/** `deliveryText` over every shape, and the marker a window's record is searched for. */
export function deliveryTable() {
  return [...matrix(), ...bodies(), ...limits()]
}

export function markerTable() {
  return [0, 1, 12, 123, 123_456, Number.MAX_SAFE_INTEGER].map((id) => ({
    id,
    marker: markerOf(id),
  }))
}
