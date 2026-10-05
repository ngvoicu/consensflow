/**
 * The instructions each role's window starts with (`roleInstructions`), the
 * staff as the chief reads it (`staffOf`, `teamTable`) and the work tiers
 * (`workTierList`).
 *
 * A call that throws a TypeError is written `{"throws": true}`, as the launch
 * goldens do: V8 words it, and it names no sentence of ours. One that throws
 * an Error of the code's own, or a file's, is written with its message.
 *
 * An eval's card (`CONSENSFLOW_EVAL_CHIEF_CARD`) is a file the row describes:
 * its `text` or its `bytes`, a `missing` file named relative to the working
 * folder, a `folder`, or the variable set to nothing (`empty`).
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { roleInstructions, staffOf } from '../../../src/core/roles.js'
import { teamTable, workTierList } from '../../../src/skill.js'

const ZEUS = { name: 'zeus', roles: ['worker', 'reviewer'], workTier: 'standard' }
const DIANA = { name: 'diana', roles: ['advisor'], workTier: 'light' }
const OSIRIS = { name: 'osiris', roles: ['designer'], workTier: 'critical' }
const SOL = { name: 'sol', roles: ['worker', 'advisor', 'reviewer'], workTier: 'complex' }
/** Names and roles the table must escape, though no agent id holds a bar or a line end. */
const ODD = { name: 'a|b', roles: ['x|y', 'z\r\n\nw'], workTier: 'light' }
const NO_ROLES = { name: 'plain', roles: [], workTier: 'critical' }
const LINE_END = { name: 'multi\nline\r\nname', roles: ['worker'], workTier: 'standard' }

const STAFFS = [[], [ZEUS], [ZEUS, DIANA, OSIRIS, SOL], [ODD, NO_ROLES, LINE_END, ZEUS]]
const CFS = [
  undefined,
  '/opt/consensflow/bin/cf',
  'C:\\Program Files\\ConsensFlow\\bin\\cf.exe',
  '',
  "/Users/me/My Apps/cf `x` $HOME 'q'",
]

const MEMBERS = ['advisor', 'worker', 'reviewer', 'designer']

/** The error a call throws, said as a row holds it. */
function thrown(cause) {
  return cause instanceof TypeError ? { throws: true } : { throws: cause.message }
}

/** `roleInstructions` as a row says it was asked, with the card's file made and removed around it. */
function instructions({ role, staff, cf, card }) {
  const options = cf === undefined ? {} : { cf }
  const saved = process.env.CONSENSFLOW_EVAL_CHIEF_CARD
  const folder = card === null ? null : mkdtempSync(join(tmpdir(), 'cf-engine-goldens-'))
  try {
    if (card?.text !== undefined || card?.bytes !== undefined) {
      const file = join(folder, 'card.md')
      writeFileSync(file, card.text ?? Buffer.from(card.bytes))
      process.env.CONSENSFLOW_EVAL_CHIEF_CARD = file
    } else if (card?.missing !== undefined) {
      process.env.CONSENSFLOW_EVAL_CHIEF_CARD = card.missing
    } else if (card?.folder) {
      process.env.CONSENSFLOW_EVAL_CHIEF_CARD = folder
    } else if (card?.empty) {
      process.env.CONSENSFLOW_EVAL_CHIEF_CARD = ''
    } else {
      delete process.env.CONSENSFLOW_EVAL_CHIEF_CARD
    }
    return { text: roleInstructions(role, staff, options) }
  } catch (cause) {
    return thrown(cause)
  } finally {
    if (saved === undefined) delete process.env.CONSENSFLOW_EVAL_CHIEF_CARD
    else process.env.CONSENSFLOW_EVAL_CHIEF_CARD = saved
    if (folder !== null) rmSync(folder, { recursive: true, force: true })
  }
}

function roleRows() {
  const rows = []
  const add = ({ role, staff = [], cf, card = null }) =>
    rows.push({
      role,
      staff,
      cf: cf === undefined ? { undefined: true } : cf,
      card,
      answer: instructions({ role, staff, cf, card }),
    })
  // The chief's text, filled with the tiers and each staff; its cf, said or not.
  for (const staff of STAFFS) for (const cf of CFS.slice(0, 2)) add({ role: 'chief', staff, cf })
  for (const cf of CFS.slice(2)) add({ role: 'chief', staff: [ZEUS], cf })
  // A member's text is the same whatever the staff is, and says its cf the same way.
  for (const role of MEMBERS) for (const cf of CFS) add({ role, staff: [ZEUS], cf })
  add({ role: 'worker', staff: [] })
  add({ role: 'reviewer', staff: STAFFS[2] })
  add({ role: 'advisor', staff: [{ ...ZEUS, workTier: null }], cf: CFS[1] })
  // A role that has no text.
  for (const role of [
    'pm',
    'king',
    'human',
    '',
    'Chief',
    'CHIEF',
    'chief ',
    ' worker',
    'workers',
  ]) {
    add({ role, staff: [ZEUS], cf: CFS[1] })
  }
  for (const role of ['constructor', '__proto__', 'toString', 'hasOwnProperty']) {
    add({ role, staff: [] })
  }
  // A tier the chief's table cannot name.
  for (const workTier of [null, 'bogus', '', 'Critical', undefined]) {
    add({ role: 'chief', staff: [ZEUS, { ...DIANA, workTier }] })
  }
  // An eval's card, which only the chief reads, and which says nothing of the board.
  const cards = [
    { text: 'You work in this project for its owner.\n' },
    { text: '' },
    { text: 'one line, no end' },
    { text: 'é ă 漢字 🙂 and CRLF\r\nlines\r\n' },
    { text: `${'long card line\n'.repeat(500)}` },
    { bytes: [0x68, 0x69, 0xff, 0xfe, 0xc3, 0x28, 0xe2, 0x82, 0x0a] },
    { bytes: [0xef, 0xbb, 0xbf, 0x78] },
    { bytes: [0xed, 0xa0, 0x80, 0x0a, 0xf0, 0x9f, 0x99] },
    { missing: 'engine-text-goldens-no-such-card.md' },
    { folder: true },
    { empty: true },
  ]
  for (const card of cards) {
    add({ role: 'chief', staff: [ZEUS], cf: CFS[1], card })
    add({ role: 'chief', staff: [{ ...ZEUS, workTier: 'bogus' }], card })
  }
  for (const role of MEMBERS) add({ role, staff: [ZEUS], cf: CFS[1], card: cards[0] })
  add({ role: 'pm', staff: [], card: cards[0] })
  return rows
}

/** A participant as the ledger's `participantView` writes one. */
const participant = (fields) => ({
  id: 1,
  projectId: 1,
  handle: 'x',
  role: 'worker',
  agent: null,
  harness: null,
  designer: false,
  createdAt: '2026-09-19T10:00:01.000Z',
  leftAt: null,
  tier: null,
  roles: [],
  outUntil: null,
  outSince: null,
  memberId: null,
  member: null,
  session: null,
  ...fields,
})

function staffOfRows() {
  const human = participant({ id: 1, handle: 'human', role: 'human' })
  const chief = participant({
    id: 2,
    handle: 'chief',
    role: 'chief',
    agent: 'astraeus',
    harness: 'codex',
  })
  const zeus = participant({
    id: 3,
    handle: 'zeus',
    role: 'worker',
    roles: ['worker', 'reviewer'],
    agent: 'zeus',
    harness: 'pi',
    tier: 'standard',
  })
  const session = participant({
    id: 4,
    handle: 'zeus-amber-pine',
    role: 'worker',
    roles: ['worker', 'reviewer'],
    agent: 'zeus',
    harness: 'pi',
    tier: 'standard',
    memberId: 3,
    member: 'zeus',
    session: 'amber-pine',
  })
  const diana = participant({
    id: 5,
    handle: 'diana',
    role: 'advisor',
    roles: ['advisor'],
    agent: 'diana',
    harness: 'claude-code',
    tier: 'light',
  })
  const osiris = participant({
    id: 6,
    handle: 'osiris',
    role: 'designer',
    roles: ['designer'],
    agent: 'osiris',
    harness: 'codex',
    designer: true,
    tier: 'critical',
  })
  const old = participant({
    id: 7,
    handle: 'old',
    role: 'worker',
    roles: ['worker'],
    tier: 'light',
  })
  const untiered = participant({
    id: 8,
    handle: 'untiered',
    role: 'worker',
    roles: ['worker'],
    agent: 'untiered',
    harness: 'devin',
  })
  const left = participant({
    id: 9,
    handle: 'left',
    role: 'reviewer',
    roles: ['reviewer'],
    agent: 'left',
    harness: 'opencode',
    tier: 'complex',
    leftAt: '2026-09-20T10:00:00.000Z',
  })
  const lists = [
    [],
    [human],
    [human, chief],
    [human, chief, zeus],
    [human, chief, zeus, session, diana, osiris],
    [human, chief, old, untiered, left],
    [session],
    [chief, zeus, session, diana, osiris, old, untiered, left, human],
  ]
  return lists.map((participants) => ({ participants, staff: staffOf({ participants }) }))
}

function teamTableRows() {
  const lists = [...STAFFS, [DIANA], [{ ...ZEUS, workTier: 'light' }, NO_ROLES]]
  const rows = lists.map((members) => ({ members, table: teamTable(members) }))
  for (const workTier of [null, 'bogus', undefined]) {
    const members = [ZEUS, { ...DIANA, workTier }]
    try {
      rows.push({ members, table: teamTable(members) })
    } catch (cause) {
      rows.push({ members, ...thrown(cause) })
    }
  }
  return rows
}

/** The roles' tables. */
export function roleTables() {
  return {
    staffOf: staffOfRows(),
    teamTable: teamTableRows(),
    workTierList: workTierList(),
    roleInstructions: roleRows(),
  }
}
