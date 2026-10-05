/**
 * The chief's earlier conversations as the ledger gives them (`chiefHistory`:
 * each conversation's view with its copied items), and the messages a page
 * may name a delivery by. The same inputs serve `lastWords`, `historyPages`
 * and `historyPage`.
 */
import { repeated } from './common.mjs'

/** The first line of a long message, with an emoji across the cut a gist makes at 159 units. */
const STRADDLES = `${'a'.repeat(158)}🙂${'b'.repeat(60)}`

/** An item as `chiefHistory` gives it; `at` and `complete` as the ledger copied them. */
const item = (conversation, index, [role, text, rest = {}]) => ({
  id: `${conversation}-${index}`,
  role,
  text,
  complete: true,
  at: null,
  ...rest,
})

/** A conversation of the chief, which ended after the day it began on. */
export const conversation = (n, harness, items, ended = `2026-10-0${n}T17:00:00.000Z`) => ({
  id: n,
  participantId: 2,
  harness,
  nativeSession: `s-${n}`,
  startedAt: `2026-10-0${n}T09:00:00.000Z`,
  endedAt: ended,
  items: items.map((one, index) => item(n, index, one)),
})

/**
 * The messages a lookup knows, which a delivery line names: every kind, with
 * and without a task, a handoff under each of its titles, and a member's
 * words that look like one.
 */
export const MESSAGES = [
  { id: 5, kind: 'result', sender: 'zeus', taskNumber: 1, body: 'Parser done' },
  { id: 6, kind: 'question', sender: 'zeus', taskNumber: 1, body: 'Which grammar?\nLL or LR' },
  { id: 7, kind: 'note', sender: null, taskNumber: null, body: 'You are the chief now. …' },
  { id: 8, kind: 'note', sender: null, taskNumber: null, body: 'You are the lead now. …' },
  { id: 9, kind: 'task', sender: 'chief', taskNumber: 4, body: 'Build the lexer\nwith tests' },
  { id: 10, kind: 'answer', sender: 'chief', taskNumber: 4, body: 'Use v2' },
  { id: 11, kind: 'note', sender: 'zeus', taskNumber: 2, body: 'A note on the plan' },
  { id: 12, kind: 'note', sender: null, taskNumber: null, body: 'The human changed the staff' },
  { id: 13, kind: 'result', sender: 'zeus', taskNumber: null, body: 'a result with no task' },
  { id: 14, kind: 'question', sender: 'zeus', taskNumber: null, body: STRADDLES },
  { id: 15, kind: 'note', sender: 'diana', taskNumber: 3, body: '\n\n   \n  the first line\nmore' },
  { id: 16, kind: 'question', sender: 'zeus', taskNumber: 2, body: 'See [ConsensFlow m-1 · x]' },
  { id: 17, kind: 'note', sender: 'zeus', taskNumber: null, body: 'You are the chief now' },
  { id: 18, kind: 'status', sender: 'zeus', taskNumber: 2, body: 'a kind the ledger has not' },
  { id: 19, kind: 'task', sender: null, taskNumber: null, body: 'a task with no number' },
  { id: 20, kind: 'note', sender: null, taskNumber: null, body: ' You are the chief now' },
  { id: 21, kind: 'note', sender: null, taskNumber: null, body: '' },
  { id: 9007199254740992, kind: 'note', sender: 'zeus', taskNumber: null, body: 'the largest' },
]

/** A header as a window's record shows it, then its body. */
const header = (id, kind, from, task) =>
  `[ConsensFlow m-${id}${task ? ` · T-${task}` : ''} · ${kind} from ${from}]\n`

/** Everything a record may hold of a delivery, across three harnesses and one the page has no name for. */
function talk() {
  return [
    conversation(1, 'claude-code', [
      ['user', 'Plan the parser.'],
      ['assistant', 'I will start with the lexer.\nThen the grammar.'],
      ['tool', 'ok 12 passed\nall green', { at: '2026-10-01T09:30:00.000Z' }],
      ['custom', 'Hook context: branch main'],
      ['user', `${header(5, 'result', '@zeus', 1)}Parser done\n\nDecide with: cf task accept T-1`],
      ['assistant', 'Accepted T-1.', { at: '2026-10-01T10:00:00.000Z' }],
      ['user', `half a thought${header(7, 'note', 'ConsensFlow')}You are the chief now.`],
      ['custom', `${header(8, 'note', 'ConsensFlow')}You are the lead now.`],
      ['user', `${header(6, 'question', '@zeus', 1)}Which grammar?`],
      ['user', `${header(99, 'note', 'ConsensFlow')}gone`],
      ['assistant', 'I quoted [ConsensFlow m-5 · T-1 · result from @zeus] here'],
      ['assistant', 'Working on it', { complete: false }],
      ['user', `${header(9, 'task', '@chief', 4)}Build the lexer`],
      ['user', `${header(10, 'answer', '@chief', 4)}Use v2`],
      ['user', `${header(11, 'note', '@zeus', 2)}A note on the plan`],
      ['user', `${header(12, 'note', 'ConsensFlow')}The human changed the staff`],
      ['user', `${header(13, 'result', '@zeus')}a result with no task`],
    ]),
    conversation(2, 'codex', [
      ['user', `${header(14, 'question', '@zeus')}${STRADDLES}`],
      ['user', `${header(15, 'note', '@diana', 3)}\n\n  the first line`],
      ['user', `${header(16, 'question', '@zeus', 2)}See [ConsensFlow m-1 · x]`],
      ['user', `${header(17, 'note', '@zeus')}You are the chief now`],
      ['user', `${header(18, 'status', '@zeus', 2)}a kind the ledger has not`],
      ['user', `${header(19, 'task', 'ConsensFlow')}a task with no number`],
      ['user', '[ConsensFlow m-5 '],
      ['user', 'two headers [ConsensFlow m-5 · a] and [ConsensFlow m-6 · b]'],
      ['user', '[ConsensFlow m-00005 · padded id]'],
      ['user', '[ConsensFlow m-99999999999999999999 · too big for an id]'],
      ['user', '[ConsensFlow m-1000000000000000000000 · bigger]'],
      ['user', '[ConsensFlow m-9007199254740993 · rounded]'],
      ['user', '[ConsensFlow m-9007199254740992 · the largest]'],
      ['user', '[ConsensFlow m-20 · a leading space]'],
      ['user', '[ConsensFlow m-21 · empty]'],
      ['user', '[ConsensFlow m-x · no id] and [ConsensFlow m-6 · one'],
      ['assistant', 'Done. [ConsensFlow m-6 · not a delivery of mine'],
      ['tool', 'Tool output with [ConsensFlow m-5 · inside'],
      ['custom', 'A hook said hello'],
    ]),
    conversation(3, 'kimi', [
      ['user', 'On a harness the page has no name for'],
      ['assistant', 'Its own word'],
    ]),
  ]
}

/** The two conversations of the deliveries test, as a codex window's record shows them. */
function deliveries() {
  return [
    conversation(1, 'codex', [
      ['user', `${header(5, 'result', '@zeus', 1)}Parser done\n\nDecide with: cf task accept T-1`],
      ['custom', `${header(6, 'question', '@zeus', 1)}Which grammar?`],
      ['user', `half a thought${header(7, 'note', 'ConsensFlow')}You are the chief now.`],
      ['user', '[ConsensFlow m-99 · note]\ngone'],
      ['assistant', 'I quoted [ConsensFlow m-5 · T-1 · result from @zeus] here'],
    ]),
  ]
}

/** What `cf history` leaves out unless asked, on two harnesses, one conversation to each. */
function tools() {
  return [
    conversation(1, 'pi', [
      ['user', 'run the tests'],
      ['tool', 'ok 12 passed'],
      ['assistant', 'All 12 pass'],
    ]),
    conversation(2, 'claude-code', [['user', 'Ship IT on Friday']]),
  ]
}

/** Pages enough to count: a short exchange many times over, a long line of words, of x, and of text no ASCII holds. */
function long() {
  const items = []
  for (let n = 0; n < 150; n += 1)
    items.push(['user', `question ${n}`], ['assistant', `answer ${n}`])
  items.push(['assistant', repeated(['long ', 1], ['word ', 12_000])])
  items.push(['user', repeated(['one line ', 1], ['x', 30_000])])
  items.push(['tool', repeated(['tool ', 1], ['output\n', 900])])
  items.push(['assistant', repeated(['漢字 résumé 🙂 ', 3_000])])
  items.push(['user', 'the last thing'])
  return [conversation(1, 'claude-code', items)]
}

/** A text of `count` lines. */
const lines = (count, line = (n) => `line ${n}`) =>
  Array.from({ length: count }, (_, n) => line(n)).join('\n')

/** Entries of many lines, of long lines, and of both, across two conversations. */
function manyLines() {
  return [
    conversation(1, 'codex', [
      ['assistant', repeated(['line\n', 600])],
      ['user', lines(192)],
      ['user', lines(193)],
      ['user', lines(194)],
      ['assistant', lines(400, (n) => `${n} ${'w'.repeat(40)}`)],
      ['tool', lines(250)],
      ['user', repeated([`${'a'.repeat(100)}\n`, 90], ['short\n', 30])],
    ]),
    conversation(2, 'pi', [
      ['assistant', `${'y'.repeat(9_000)}\n${lines(120)}\n${'z'.repeat(8_000)}`],
      ['user', '\n'.repeat(250)],
    ]),
  ]
}

/** What a find reads: text that changes in lower case, and text that does not. */
function unicode() {
  return [
    conversation(1, 'claude-code', [
      ['user', 'Ship IT on Friday'],
      ['assistant', 'İstanbul, ΟΔΟΣ and Straße'],
      ['user', 'KELVIN K, ÅNGSTRÖM Å, ǅ ǈ ǋ'],
      ['assistant', 'résumé 🙂 漢字 ΑΣ. ΑΣΑ'],
      ['tool', 'ok 12 passed: TAP version 13'],
      ['user', `${header(5, 'result', '@zeus', 1)}Parser done`],
      ['custom', 'Regex .* [a-z]+ $ ^ (x)'],
    ]),
    conversation(
      2,
      'devin',
      [
        ['user', 'ß ẞ STRASSE strasse'],
        ['assistant', 'SHIP it'],
      ],
      '2026-10-02T18:00:00.000Z',
    ),
  ]
}

/** The histories rows name, by what each is for. */
export function histories() {
  return {
    none: [],
    bare: [conversation(1, 'claude-code', [])],
    talk: talk(),
    deliveries: deliveries(),
    tools: tools(),
    long: long(),
    lines: manyLines(),
    unicode: unicode(),
  }
}

/** A history with its texts as Node is given them. */
export function expanded(history, expand) {
  return history.map((one) => ({
    ...one,
    items: one.items.map((each) => ({ ...each, text: expand(each.text) })),
  }))
}
