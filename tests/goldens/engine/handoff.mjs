/**
 * What passes to a chief the human switched in: its first message
 * (`handoffText`), the human's last words (`lastWords`), the pages of
 * `cf history` (`historyPage`, `historyPages`) and what is a handoff
 * (`isHandoff`).
 *
 * A text that holds half of a surrogate pair (`firstLine`'s cut through an
 * emoji) is written as the readers had it, U+FFFD there, with `halved`
 * (`common.mjs`).
 */
import {
  handoffText,
  historyPage,
  historyPages,
  isHandoff,
  lastWords,
} from '../../../src/core/handoff.js'
import { expand, numberOf, numeral, repeated, said } from './common.mjs'
import { conversation, expanded, histories, MESSAGES } from './histories.mjs'

const known = new Map(MESSAGES.map((message) => [message.id, message]))
/** The lookup the API gives `historyPage`: a message by its id, none for a number it never held. */
const message = (id) => known.get(id) ?? null

/** Conversations as rows hold them (texts maybe repeated) and as Node is given them. */
const given = (conversations) => expanded(conversations, expand)

const HISTORIES = histories()

/** Conversations that put one entry at the edge of a page, and two at the edge of one. */
function edges() {
  const alone = (role, text) => [conversation(1, 'claude-code', [[role, text]])]
  const pair = (first, second) => [conversation(1, 'claude-code', [first, second])]
  const user = (text) => ['user', text]
  return [
    // An entry is `Claude Code chief: ` (19 bytes) and the text: 7,384 bytes is a whole one.
    ...[7_364, 7_365, 7_366].map((n) => alone('assistant', repeated(['x', n]))),
    ...[3_682, 3_683].map((n) => alone('assistant', repeated(['é', n]))),
    ...[2_455, 2_456].map((n) => alone('assistant', repeated(['漢', n]))),
    ...[1_841, 1_842].map((n) => alone('assistant', repeated(['🙂', n]))),
    // `Human: ` is 7 bytes, and each entry costs two more: 7,400 fill a page.
    pair(user(repeated(['a', 3_691])), user(repeated(['b', 3_691]))),
    pair(user(repeated(['a', 3_692])), user(repeated(['b', 3_691]))),
    pair(user(repeated(['a', 3_691])), user(repeated(['b', 3_692]))),
    // And 194 lines: each entry costs its lines and one.
    pair(user(Array.from({ length: 96 }, () => 'l').join('\n')), user('m\n'.repeat(95))),
    pair(user(Array.from({ length: 97 }, () => 'l').join('\n')), user('m\n'.repeat(95))),
    pair(user(Array.from({ length: 96 }, () => 'l').join('\n')), user('m\n'.repeat(96))),
    // One line past 16 pages' worth of bytes, of text that is all pairs, and of three-byte units.
    alone('assistant', repeated(['🙂', 20_000])),
    alone('user', repeated(['漢', 9_000], ['\n', 1], ['é', 9_000])),
  ]
}

/** What a row names its conversations by: one of the shared histories, or its own. */
function source(row) {
  return 'history' in row ? HISTORIES[row.history] : row.conversations
}

// ───────────────────────────── lastWords, isHandoff ─────────────────────────────

const turn = (role, text, rest) => [role, text, rest]

function lastWordsRows() {
  const own = [
    [],
    [conversation(1, 'codex', [turn('assistant', 'only me')])],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('assistant', 'hi')])],
    [
      conversation(1, 'codex', [
        turn('user', 'hello'),
        turn('assistant', 'hi', { complete: false }),
      ]),
    ],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('assistant', '')])],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('assistant', ' \n\t ')])],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('assistant', ' ﻿ ')])],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('assistant', '\u0085')])],
    [conversation(1, 'codex', [turn('user', 'hello'), turn('tool', 'ok'), turn('custom', 'hook')])],
    [
      conversation(1, 'codex', [
        turn('user', 'hello'),
        turn('custom', 'x'),
        turn('assistant', 'later'),
      ]),
    ],
    [
      conversation(1, 'codex', [
        turn('assistant', 'early'),
        turn('user', 'hello'),
        turn('assistant', 'a'),
      ]),
    ],
    [
      conversation(1, 'codex', [
        turn('user', 'first'),
        turn('assistant', 'a'),
        turn('user', 'second'),
      ]),
    ],
    [conversation(1, 'codex', [turn('user', 'first'), turn('user', '  \n '), turn('user', '')])],
    [conversation(1, 'codex', [turn('user', '    padded \n ')])],
    [conversation(1, 'codex', [turn('user', '\u0085kept\u0085')])],
    [conversation(1, 'codex', [turn('user', '﻿text﻿')])],
    [
      conversation(1, 'codex', [
        turn('user', 'before[ConsensFlow m-5 · T-1 · result from @zeus]\nbody'),
      ]),
    ],
    [conversation(1, 'codex', [turn('user', ' \n [ConsensFlow m-5 · result')])],
    [
      conversation(1, 'codex', [turn('user', 'earlier words'), turn('assistant', 'ok')]),
      conversation(2, 'claude-code', [
        turn('user', '[ConsensFlow m-5 · result'),
        turn('assistant', 'x'),
      ]),
    ],
    [
      conversation(1, 'codex', [turn('user', 'earlier words')]),
      conversation(2, 'pi', [turn('assistant', 'nothing from the human here')]),
      conversation(3, 'devin', []),
    ],
    [conversation(1, 'codex', [turn('user', 'a [ConsensFlow m-x · b] and [ConsensFlow m-12 · c')])],
    [conversation(1, 'codex', [turn('user', 'a [ConsensFlow m-12· b][ConsensFlow m-٣ · c')])],
    [
      conversation(1, 'codex', [
        turn('user', 'one [ConsensFlow m-7 · a] two [ConsensFlow m-8 · b]'),
      ]),
    ],
    [conversation(1, 'codex', [turn('custom', 'not the human'), turn('tool', 'nor a tool')])],
    [conversation(1, 'codex', [turn('user', '🙂'.repeat(200))])],
  ]
  return [
    ...Object.keys(HISTORIES).map((history) => ({
      history,
      last: lastWords(given(HISTORIES[history])),
    })),
    ...own.map((conversations) => ({ conversations, last: lastWords(given(conversations)) })),
  ]
}

function isHandoffRows() {
  const notes = []
  for (const body of [
    'You are the chief now',
    'You are the chief now. The human switched this project’s chief…',
    'You are the lead now',
    "You are the lead now. The human switched this project's lead…",
    'You are the chief nowhere',
    ' You are the chief now',
    'you are the chief now',
    'The human changed the staff; it is now:',
    'You are the',
    '',
  ]) {
    for (const [kind, sender] of [
      ['note', null],
      ['note', 'zeus'],
      ['task', null],
      ['result', null],
      ['question', null],
      ['answer', null],
    ]) {
      notes.push({ kind, sender, body })
    }
  }
  return notes.map((note) => ({ ...note, handoff: isHandoff(note) }))
}

// ──────────────────────────────── handoffText ────────────────────────────────

const NO_WORK = { questions: [], results: [], own: [] }
const question = (id, sender, taskNumber, body) => ({ id, sender, taskNumber, body })
const task = (number, title, assignee, state) => ({ number, title, assignee, state })

function handoffRows() {
  const rows = []
  const add = (args) => rows.push({ ...args, ...said(handoffText(args)) })
  const seat = (harness, agent) => ({ harness, agent })
  const busy = {
    questions: [
      question(6, 'zeus', 1, 'Which grammar?\nLL or LR'),
      question(14, 'diana', null, `${'a'.repeat(158)}🙂${'b'.repeat(40)}`),
      question(16, 'zeus', 2, '\n  \n[ConsensFlow m-1 · x] defused'),
      question(22, 'zeus', 3, ''),
    ],
    results: [
      task(2, 'Lexer', 'diana', 'done'),
      task(5, 'Plan [ConsensFlow m-1 · x]', 'zeus', 'done'),
      task(7, '', 'a|b', 'done'),
    ],
    own: [
      task(3, 'Plan [ConsensFlow m-1 · x]', 'chief', 'working'),
      task(8, 'Wait for T-3', null, 'waiting'),
      task(9, '漢字 🙂', 'chief', 'queued'),
    ],
  }
  const lasts = [
    null,
    { text: 'Keep the API as it is.\nThanks', answered: false },
    { text: 'Ship it.', answered: true },
    { text: `${'a'.repeat(298)}🙂 tail`, answered: true },
    { text: `${'a'.repeat(299)}🙂 tail`, answered: false },
    { text: `${'a'.repeat(300)} tail`, answered: false },
    { text: `${'a'.repeat(301)} tail`, answered: false },
    { text: '[ConsensFlow m-9 · x] defused\nsecond', answered: false },
    { text: '\n\n  \nthe first line that says something', answered: true },
    { text: '', answered: false },
  ]
  const switches = [
    [seat('claude-code', null), seat('codex', 'astraeus')],
    [seat('pi', 'selene'), seat('claude-code', null)],
    [seat('codex', 'astraeus'), seat('devin', 'zeus')],
    [seat('opencode', ''), seat('pi', '')],
    [seat('kimi', null), seat('claude-code', 'calliope')],
    [seat('devin', 'a (b)'), seat('opencode', null)],
    [seat('claude-code', null), seat(null, null)],
  ]
  for (const [from, to] of switches) {
    add({ from, to, open: NO_WORK, last: null, cut: false, pages: 1 })
  }
  for (const last of lasts) {
    add({ from: switches[0][0], to: switches[0][1], open: NO_WORK, last, cut: false, pages: 2 })
  }
  for (const pages of [0, 1, 2, 3, 12]) {
    for (const cut of [false, true]) {
      add({ from: switches[1][0], to: switches[1][1], open: NO_WORK, last: null, cut, pages })
    }
  }
  for (const open of [
    NO_WORK,
    { ...NO_WORK, questions: busy.questions.slice(0, 1) },
    { ...NO_WORK, questions: busy.questions },
    { ...NO_WORK, results: busy.results },
    { ...NO_WORK, own: busy.own },
    busy,
  ]) {
    add({ from: switches[0][0], to: switches[0][1], open, last: lasts[1], cut: true, pages: 3 })
    add({ from: switches[2][0], to: switches[2][1], open, last: lasts[2], cut: false, pages: 1 })
  }
  return rows
}

// ───────────────────────────── cf history's pages ─────────────────────────────

/** Rows for every shared history and every edge: the pages Node counts. */
function pageCountRows() {
  const sources = [
    ...Object.keys(HISTORIES).map((history) => ({ history })),
    ...edges().map((conversations) => ({ conversations })),
  ]
  return sources.map((row) => ({
    ...row,
    pages: historyPages(given(source(row)), { message }),
  }))
}

/** One page, as the API asks for it, or the range error it turns into a 400. */
function pageRow(row, { page, find, tools }) {
  const asked = { page: numberOf(page), find, tools }
  try {
    const shown = historyPage(given(source(row)), { message, ...asked })
    return {
      ...row,
      page,
      find,
      tools,
      answer: { page: shown.page, pages: shown.pages, ...said(shown.text) },
    }
  } catch (cause) {
    if (!(cause instanceof RangeError)) throw cause
    return { ...row, page, find, tools, rangeError: cause.message }
  }
}

/** Every page of a history asked as `find` and `tools` say; a sample of them when there are many. */
function pagesOf(row, { find = null, tools = false, all = true }) {
  const { pages: count } = historyPage(given(source(row)), { message, page: 1, find, tools })
  // A history with nothing to show says so on page 1, whichever is asked.
  const every = Array.from({ length: count }, (_, at) => at + 1)
  const wanted = count === 0 ? [1] : all || count <= 6 ? every : [1, 2, count - 1, count]
  return [...new Set(wanted)].map((page) => pageRow(row, { page, find, tools }))
}

function pageRows() {
  const rows = []
  for (const name of Object.keys(HISTORIES)) {
    const row = { history: name }
    rows.push(...pagesOf(row, { all: true }), ...pagesOf(row, { tools: true, all: false }))
  }
  for (const conversations of edges()) rows.push(...pagesOf({ conversations }, {}))
  // Searches: found, not found, in a tool's output with and without asking for it, in any case.
  const found = [
    ['talk', 'grammar'],
    ['talk', 'GRAMMAR'],
    ['talk', 'human'],
    ['talk', 'Human: half'],
    ['talk', '\n'],
    ['talk', 'm-5'],
    ['talk', 'zeus'],
    ['talk', 'earlier m-'],
    ['talk', 'ConsensFlow m-'],
    ['talk', 'the handoff that brought'],
    ['talk', 'nowhere at all'],
    ['talk', ''],
    ['talk', 'kimi'],
    ['talk', '2026-10-01T10:00'],
    ['talk', '2026-10-02T09'],
    ['tools', 'ship it'],
    ['tools', 'SHIP IT'],
    ['tools', 'ok 12'],
    ['tools', 'tool output'],
    ['tools', 'nowhere'],
    ['deliveries', 'quoted'],
    ['deliveries', 'no longer on record'],
    ['long', 'answer 77'],
    ['long', 'x'],
    ['long', 'last thing'],
    ['long', 'RÉSUMÉ'],
    ['long', 'résumé 🙂'],
    ['lines', 'line 100'],
    ['lines', 'short'],
    ['unicode', 'ship it'],
    ['unicode', 'İ'],
    ['unicode', 'i̇stanbul'],
    ['unicode', 'istanbul'],
    ['unicode', 'οδος'],
    ['unicode', 'οδοσ'],
    ['unicode', 'ΟΔΟΣ'],
    ['unicode', 'straße'],
    ['unicode', 'STRASSE'],
    ['unicode', 'strasse'],
    ['unicode', 'ß'],
    ['unicode', 'ẞ'],
    ['unicode', 'k'],
    ['unicode', 'K'],
    ['unicode', 'K'],
    ['unicode', 'å'],
    ['unicode', 'ǆ'],
    ['unicode', 'ǅ'],
    ['unicode', 'ας.'],
    ['unicode', 'ασα'],
    ['unicode', '.*'],
    ['unicode', '[a-z]+'],
    ['unicode', '$ ^'],
    ['unicode', 'regex'],
    ['unicode', '🙂'],
    ['unicode', '漢字'],
    ['bare', 'x'],
    ['bare', ''],
    ['none', 'x'],
  ]
  for (const [history, find] of found) {
    const row = { history }
    rows.push(
      ...pagesOf(row, { find, all: false }),
      ...pagesOf(row, { find, tools: true, all: false }),
    )
  }
  // Pages that are none: below the first, between two, past the last, no integer, no number.
  const strange = [0, -1, 1.5, 2.0000001, 1e21, 2 ** 53, Number.NaN, Infinity, -Infinity, -0]
  for (const history of ['talk', 'long', 'tools', 'none', 'bare']) {
    const row = { history }
    const count = historyPage(given(HISTORIES[history]), { message, page: 1 }).pages
    for (const page of [...strange, count + 1, count + 2]) {
      rows.push(pageRow(row, { page: numeral(page), find: null, tools: false }))
    }
    rows.push(pageRow(row, { page: count + 1, find: 'x', tools: true }))
  }
  return rows
}

/** The handoff's tables. */
export function handoffTables() {
  return {
    lastWords: lastWordsRows(),
    isHandoff: isHandoffRows(),
    handoffText: handoffRows(),
    historyPages: pageCountRows(),
    historyPage: pageRows(),
  }
}
