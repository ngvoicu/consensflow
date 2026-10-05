/**
 * The records' pure functions, as tables of what Node answers:
 * - quota (`hosts/lib/quota.js`): resets named in words, against instants,
 *   in named zones and in the machine's (`DEFAULT_ZONE`, which the generator
 *   pins); Codex's rate limits; OpenCode's retries; the status tests;
 * - `localeCompare`, by which the Claude and OpenCode readers break ties
 *   between item ids: ICU's collation, which the port emulates for ASCII;
 * - the reasons that are ConsensFlow's own sentences, by how each begins.
 *
 * A call that throws is written `{"throws": true}`.
 */
import {
  codexQuota,
  DEVIN_REFUSAL,
  exhaustedQuota,
  opencodeRetryQuota,
  quotaStatus,
  refusedForQuota,
} from '../../../hosts/lib/quota.js'
import { fixtureJson, fixtureLines } from './fixtures.mjs'
import { OUR_REASONS } from './runner.mjs'

/** The zone a reset that names none is read in; the generator sets `TZ` to it. */
export const DEFAULT_ZONE = 'America/Los_Angeles'

const answer = (call) => {
  try {
    const value = call()
    return value === undefined ? { undefined: true } : value
  } catch {
    return { throws: true }
  }
}

const RESET_TEXTS = [
  // In a span: units by their first letter, summed.
  'Resets in 3 days.',
  'Weekly usage limit reached. Resets in 3hr 4min.',
  'resets in 35 minutes',
  'Resets in 4h 30m',
  'resets in 2 months',
  'reset in 1 week and 2 days',
  'resets in 1 week, 2 days, and 3 hours',
  'resets in 5 parsecs',
  'Resets in soon',
  'RESETS IN 90 SECONDS',
  'resets in 0 minutes',
  // At a time, in a zone or the machine's.
  'resets 7:30pm (Europe/Bucharest)',
  'resets Sep 29 at 11am (Europe/Bucharest)',
  'resets 3pm (America/New_York)',
  'resets 3pm',
  'resets at 3pm (Europe/Bucharest)',
  'resets tomorrow at 3pm (Europe/Bucharest)',
  'resets 13pm (UTC)',
  'resets 12am (Asia/Tokyo)',
  'resets 12pm (Asia/Tokyo)',
  'resets 7:75pm (UTC)',
  'resets Feb 31 at 9am (UTC)',
  'resets Feb 29 at 9am (UTC)',
  'resets Dec 31 at 11pm (Pacific/Auckland)',
  'resets Jan 1 at 12am (Pacific/Kiritimati)',
  'resets 3pm (Mars/Olympus)',
  'resets 3pm ( Europe/Bucharest )',
  'resets 3pm (europe/bucharest)',
  'resets 3:30am (Australia/Lord_Howe)',
  'resets 2:15am (Asia/Kathmandu)',
  'resets Sept 29 at 11am (UTC)',
  'resets Xyz 29 at 11am (UTC)',
  // Gaps and folds.
  'resets Mar 28 at 3:30am (Europe/Bucharest)',
  'resets Oct 25 at 3:30am (Europe/Bucharest)',
  'resets Mar 14 at 2:30am (America/New_York)',
  'resets Nov 1 at 1:30am (America/New_York)',
  'resets Oct 4 at 2:30am (Australia/Lord_Howe)',
  // Nothing of a reset.
  "You've hit your limit.",
  '',
]

const INSTANTS = [
  // Around the folds and gaps above, a day before each and on the day.
  Date.UTC(2026, 2, 27, 12),
  Date.UTC(2026, 9, 24, 12),
  Date.UTC(2026, 9, 25, 12),
  Date.UTC(2026, 10, 1, 12),
  Date.UTC(2027, 2, 13, 12),
  Date.UTC(2027, 2, 27, 12),
  // Month and year ends.
  Date.UTC(2026, 11, 31, 23, 30),
  Date.UTC(2027, 0, 31, 12),
  Date.UTC(2028, 1, 28, 12),
  // Just after a time-only reset passed today, and a date gone by just over a day.
  Date.UTC(2026, 8, 19, 17, 30),
  Date.UTC(2026, 8, 30, 9, 1),
  Date.UTC(2026, 8, 30, 8, 59),
  // Fractions of a millisecond, and a clock with none.
  Date.UTC(2026, 8, 19, 10) + 0.75,
  Number.NaN,
  Number.POSITIVE_INFINITY,
  8.64e15 + 1,
]

const LIMITS = [
  undefined,
  null,
  {},
  { primary: { used_percent: 12.5, resets_at: 1789900000 } },
  { secondary: { used_percent: 95, resets_at: 1790423393 } },
  {
    primary: { used_percent: 94.99, resets_at: 1789900000 },
    secondary: { used_percent: 94.99, resets_at: 1790423393 },
  },
  { primary: { used_percent: 97, resets_at: 1790423393.5 }, secondary: { used_percent: 12 } },
  { primary: { used_percent: '97' }, secondary: { used_percent: 3, resets_at: '1790423393' } },
  { primary: { used_percent: 100 }, rate_limit_reached_type: 'primary' },
  { rate_limit_reached_type: null },
  { rate_limit_reached_type: 0 },
  { primary: { used_percent: 2.0 }, secondary: { used_percent: -1 } },
  { primary: { used_percent: 1e21 } },
]

const RETRIES = [
  undefined,
  null,
  { type: 'idle' },
  { type: 'retry', message: 'Too Many Requests', next: 0 },
  { type: 'retry', message: 'Too many requests', next: 59_999 },
  { type: 'retry', message: 'Too many requests', next: 60_000 },
  { type: 'retry', message: 'Overloaded', next: 3_600_000 },
  { type: 'retry', message: 'Usage limit reached', next: 3_600_000 },
  { type: 'retry', message: 'x', action: { reason: 'free_tier_limit' }, next: 1_000 },
  { type: 'retry', message: 'x', action: { reason: 'QUOTA spent' } },
  { type: 'retry', message: 'rate limit hit', next: 'soon' },
  { type: 'retry', message: 'rate limit hit', next: '7200000' },
]

const REFUSALS = [
  '429: {"type":"GoUsageLimitError"}',
  'OpenAI API error (429): {"type":"GoUsageLimitError"}',
  '402 This request requires more credits.',
  '500: provider down',
  '4290: no',
  'x429: no',
  '(429)',
  'abc(402): x',
  'abc\n(429): x',
  'error 429: x',
  ' 429: x',
  '',
  null,
  undefined,
  429,
]

const STATUSES = [
  429,
  402,
  401,
  500,
  '429',
  ' 429 ',
  '402.0',
  '0x1AD',
  '4.29e2',
  null,
  undefined,
  '',
  true,
  [429],
]

const DEVIN_TEXTS = [
  'Rate limit exceeded',
  'You have hit your usage limit.',
  'Your daily usage quota has been exhausted.',
  'quota has been exhausted',
  'QUOTA EXHAUSTED',
  'quota nearly exhausted',
  'limit',
  '',
]

function quotaTable() {
  const resets = RESET_TEXTS.map((text) => ({
    text,
    at: INSTANTS.map((atMs) => answer(() => exhaustedQuota(text, atMs))),
  }))
  return {
    defaultZone: DEFAULT_ZONE,
    instants: INSTANTS.map((atMs) => (Number.isFinite(atMs) ? atMs : String(atMs))),
    resets,
    codex: LIMITS.map((limits) => ({
      limits: limits ?? { undefined: true },
      quota: answer(() => codexQuota(limits)),
    })),
    opencode: RETRIES.flatMap((status) =>
      [0, 1_000].map((nowMs) => ({
        status: status ?? { undefined: true },
        nowMs,
        quota: answer(() => opencodeRetryQuota(status, nowMs)),
      })),
    ),
    refused: REFUSALS.map((text) => ({
      text: text ?? { undefined: true },
      refused: refusedForQuota(text),
    })),
    statuses: STATUSES.map((status) => ({
      status: status ?? { undefined: true },
      quota: quotaStatus(status),
    })),
    devin: DEVIN_TEXTS.map((text) => ({ text, refusal: DEVIN_REFUSAL.test(text) })),
  }
}

/** Every printable ASCII character, and the ids the fixtures sort. */
function collationTable() {
  const characters = Array.from({ length: 95 }, (_, code) => String.fromCharCode(code + 32))
  const sign = (left, right) => Math.sign(left.localeCompare(right))
  const ids = new Set()
  for (const name of [
    'claude-code/fragments.jsonl',
    'claude-code/queue-pop-all.jsonl',
    'claude-code/v263-tool-loop.jsonl',
  ]) {
    for (const line of fixtureLines(name)) {
      const record = JSON.parse(line)
      for (const id of [record.uuid, record.message?.id]) if (typeof id === 'string') ids.add(id)
    }
  }
  for (const name of ['opencode/tool-result.json', 'opencode/v130-tool-loop.json']) {
    const fixture = fixtureJson(name)
    for (const row of [...fixture.message, ...fixture.part]) ids.add(row.id)
  }
  const words = [
    ...ids,
    'a',
    'A',
    'ab',
    'aB',
    'Ab',
    'AB',
    'a1',
    'a_',
    'a-',
    'a-b',
    'a_b',
    'ab-',
    'msg_1',
    'msg-1',
    'msg1',
    'msg_a',
    'msg_A',
    'prt_0',
    'prt_Z',
    'prt_z',
    // Pairs a locale's collation tailors where root's does not: Danish and
    // Norwegian `aa` after `z`, Lithuanian `y` before `j`, Hungarian `cs`.
    'aa',
    'z',
    'y',
    'j',
    'cs',
    'cz',
    '',
    ' ',
    'a b',
    'ab ',
  ].sort()
  return {
    characters: characters.join(''),
    // Row i, column j: the sign of characters[i].localeCompare(characters[j]).
    matrix: characters.map((left) =>
      characters.map((right) => ['<', '=', '>'][sign(left, right) + 1]).join(''),
    ),
    words,
    pairs: words.map((left) =>
      words.map((right) => ['<', '=', '>'][sign(left, right) + 1]).join(''),
    ),
  }
}

/**
 * Throws unless this process's `localeCompare` orders ASCII as ICU's root
 * does, as `localeCompare(…, 'en')` does: the goldens, and the Rust readers,
 * hold root's order. A locale may tailor letters alone or pairs of them
 * (Norwegian, Czech and Hungarian tailor pairs only), and may ignore
 * punctuation (Thai ignores `_`), so every one- and two-character string of
 * printable ASCII is sorted as root sorts it, and each neighbour compared
 * both ways: two collations that agree on every neighbour, ties too, are one
 * order there.
 */
export function refuseTailoredCollation() {
  const printable = Array.from({ length: 95 }, (_, code) => String.fromCharCode(code + 32))
  const strings = [
    ...printable,
    ...printable.flatMap((left) => printable.map((right) => left + right)),
  ].sort((left, right) => left.localeCompare(right, 'en'))
  const tailored = strings.slice(1).some((right, at) => {
    const left = strings[at]
    return Math.sign(left.localeCompare(right)) !== Math.sign(left.localeCompare(right, 'en'))
  })
  if (tailored) {
    const locale = new Intl.Collator().resolvedOptions().locale
    throw new Error(
      `this process collates as ${locale}, which orders ASCII as root does not: unset LC_ALL, LC_MESSAGES and LANG first`,
    )
  }
}

/** The tables, as one golden. */
export function tables() {
  return { quota: quotaTable(), collation: collationTable(), reasons: { ours: OUR_REASONS } }
}
