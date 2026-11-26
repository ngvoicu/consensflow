import { button, element, icon, iconButton } from '../dom.js'
import {
  aRole,
  isImageAgentRole,
  roleIds,
  roleLabel,
  roleName,
  tierColumns,
  tiersOf,
} from './roles.js'

/**
 * The staff as slots (`architecture/staff-slots.md`, section 6): a role at a tier is a slot, and
 * holds one member; a role with no tiers (the image designer's) is one slot. Project settings and
 * New project draw them alike, from the daemon's roles and their tiers and from the staff (or New
 * project's draft): every slot, in the roles' order, the empty ones with their Add, and a slot the
 * update left to several members with each of them and the rule. The page keeps no list of roles
 * or tiers of its own.
 *
 * A member here is `{ key, name, runs, roles, tier, marks, backup, opens }`: `key` tells it from
 * the others (a handle, or a draft's own number), `name` is what it is called (`@hera`, or a
 * draft's model), `runs` what it runs on where its name does not say, `marks` its state in a few
 * words (`{ text, tone }`), `backup` its backup's name or none, and `opens` what its own part
 * holds, in words ("Edit, Vacation and backup"; empty where it holds nothing to do).
 *
 * Every operation on a member starts from its own slot (T-2341, the owner: "can we have all the
 * operations from here?"): its holder's card opens the member's own part in a row under the
 * slot's row, its Edit, its Vacation and its backup, with the member form there when one of them
 * opens it. An empty slot's Add opens the form in the same place. One part is open at a time, and
 * a member in several slots is one member from any of them: its other slots are lit with it.
 */

/** Every slot the daemon's roles name, in their order: a role at each of its tiers, or alone. */
export const allSlots = () =>
  roleIds().flatMap((role) =>
    tiersOf(role).length === 0
      ? [{ role, tier: null }]
      : tiersOf(role).map((tier) => ({ role, tier })),
  )

/** What a slot is called: "standard worker", "image designer". */
export const slotName = (role, tier) =>
  tier === null ? roleName(role) : `${tier} ${roleName(role)}`

/** The same with its article: "a standard worker", "an image designer". */
const aSlot = (role, tier) => (tier === null ? aRole(role) : `a ${slotName(role, tier)}`)

/** The row of the members that hold no slot, by its key in place of a role's. */
export const NO_SLOT = 'none'

/**
 * The roles whose slots `member` holds, in the daemon's order: each a role the daemon names, at a
 * tier that role has, or a role with no tiers. A member with none is drawn in the row of its own
 * (T-2341, round 2): a role with no tier saved, or a tier its role has no slot at.
 */
export const seatsOf = (member) =>
  roleIds().filter(
    (role) =>
      member.roles.includes(role) &&
      (tiersOf(role).length === 0 || tiersOf(role).includes(member.tier)),
  )

/** The members that hold no slot, in the staff's order. */
const unseated = (members) => members.filter((member) => seatsOf(member).length === 0)

/** The members that hold a slot, in the staff's order. */
export const holdersOf = (members, role, tier) =>
  members.filter((member) => member.roles.includes(role) && (tier === null || member.tier === tier))

/** The tiers a member could not be edited to, each with the words of the slot in its way. */
export function inTheWay(members, member) {
  const blocked = new Map()
  for (const tier of tierColumns()) {
    if (tier === member.tier) continue
    for (const role of member.roles) {
      if (tiersOf(role).length === 0) continue
      const holder = holdersOf(members, role, tier).find((other) => other.key !== member.key)
      if (holder) {
        blocked.set(tier, `the ${slotName(role, tier)} slot is ${holder.name}'s`)
        break
      }
    }
  }
  return blocked
}

/** The ledger's words for a slot another holds (`slot-taken`). */
export const takenWords = (role, tier, name) =>
  tier === null
    ? `the ${roleName(role)} slot is ${name}'s: a project has one`
    : `the ${slotName(role, tier)} slot is ${name}'s: a role and tier holds one member. Pick another tier`

const COUNT = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine']
/** Words said together: "Edit, Vacation and backup". */
export const together = (names) =>
  names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`

/**
 * What a slot several members hold says, as `cf staff` says it: each holder, the rule and the way
 * to meet it. A draft says it can start once one is left, for nothing of it takes work yet.
 */
export function crowdedWords(role, tier, names, draft) {
  const which = `The ${slotName(role, tier)} slot holds ${COUNT[names.length] ?? names.length} members, ${together(names)}`
  const rule =
    tier === null
      ? `a project has one ${roleName(role)}. Remove all but one`
      : `a role and tier holds one. Edit all but one to another tier, or remove the ${roleName(role)} role from them`
  return draft
    ? `${which}: ${rule}. The project starts once one is left.`
    : `${which}: ${rule}; removing a member cancels the tasks it holds. All take work until then.`
}

/** Every slot several members hold, with its words, in the slots' order. */
export const crowded = (members, draft) =>
  allSlots()
    .map(({ role, tier }) => ({ role, tier, holders: holdersOf(members, role, tier) }))
    .filter(({ holders }) => holders.length > 1)
    .map(({ role, tier, holders }) => ({
      role,
      tier,
      words: crowdedWords(
        role,
        tier,
        holders.map((holder) => holder.name),
        draft,
      ),
    }))

/** The members an empty slot could be given to: those at its tier that may hold its role too. */
const takers = (members, role, tier) =>
  isImageAgentRole(role)
    ? []
    : members.filter(
        (member) =>
          !member.designer &&
          !member.roles.includes(role) &&
          (tier === null || member.tier === tier),
      )

/** A member's roles in the daemon's order, as their labels say them: "Worker, Reviewer". */
const rolesSaid = (member) =>
  roleIds()
    .filter((role) => member.roles.includes(role))
    .map(roleLabel)
    .join(', ')

/**
 * What a give offers a member as: its name, and where another member of the staff goes by the
 * same name (a draft's members are named by their model), the roles it holds, and then what it
 * runs on, so that two members the person can tell apart are never offered alike.
 */
function takerName(member, members) {
  const twins = members.filter((other) => other.key !== member.key && other.name === member.name)
  if (twins.length === 0) return member.name
  const held = `${member.name} · ${rolesSaid(member)}`
  const alike = twins.some((other) => `${other.name} · ${rolesSaid(other)}` === held)
  return alike && member.runs ? `${held} · ${member.runs}` : held
}

const CROSS = ['M6 6l12 12', 'M18 6 6 18']
const CHEVRON = ['M7 10l5 5 5-5']

/** A slot's key, as its cell names it: `worker:standard`, or `designer`. */
const slotKey = (role, tier) => (tier === null ? role : `${role}:${tier}`)

/** What the member form's place in a slot's row is marked by: a redraw leaves it as it is. */
const formPlace = (key) => {
  const place = element('div', 'slot-form-place')
  place.dataset.keep = `member-form ${key}`
  return place
}

/** The head of the slots: a column for the roles, then one a tier, most critical first. */
export function slotHead() {
  const row = element('tr')
  row.append(element('th', 'slot-role-head', 'Role'))
  for (const tier of tierColumns()) {
    const cell = element('th', null, tier.charAt(0).toUpperCase() + tier.slice(1))
    cell.scope = 'col'
    cell.dataset.tier = tier
    row.append(cell)
  }
  return row
}

/**
 * The slots' rows, a role a row: each slot its holder, every holder where several hold it, or
 * empty with its Add. `hooks` acts on them: `add(role, tier)` opens the form for an empty slot,
 * `give(role, member)` gives one to a member at its tier, `remove(role, member)` takes the role
 * from a holder; `confirming` is the `{ key, role }` whose removal asks first, with its `keep()`
 * and `confirm(member)` and the ask's words (`confirmWords(member)`, a staff's own where none is
 * given); `refusal` is the `{ role, words }` of what the daemon refused of a slot of a role, said
 * under that role's row; `picking` is the slot the form is open for; `draft` says the words a
 * draft says. A member's own part: `opened` is the `{ key, role }` of the member whose part is
 * open and of the slot it was opened from, `open(role, member)` opens it there (or shuts it, open
 * there already), `shut()` shuts it, `part(member)` is what it holds (its state, its acts and its
 * backup), `formHere` says the member form is open in it, and `id` names it in its dialog. `gone`
 * is the `{ role, words }` of a form shut under the person, said under that role's row. The
 * members no slot holds are drawn last, in a row of their own (`NO_SLOT`).
 */
export function slotRows(members, hooks) {
  const columns = tierColumns()
  const rows = []
  const anchor = anchorOf(members, hooks.opened)
  const drawn = { ...hooks, anchor, members }
  for (const role of roleIds()) {
    const row = keyed(element('tr', 'slot-row'), 'slot', role)
    row.dataset.role = role
    const head = element('th', 'slot-role', roleLabel(role))
    head.scope = 'row'
    row.append(head)
    const tiers = tiersOf(role)
    if (tiers.length === 0) {
      const cell = slotCell(members, role, null, drawn)
      cell.colSpan = Math.max(columns.length, 1)
      row.append(cell)
    } else {
      for (const tier of columns) {
        if (tiers.includes(tier)) row.append(slotCell(members, role, tier, drawn))
        else row.append(element('td', 'slot-none'))
      }
    }
    rows.push(row, ...opened(role, tiers.length === 0 ? [null] : tiers, anchor, hooks))
    const crowds = (tiers.length === 0 ? [null] : tiers)
      .map((tier) => ({ tier, holders: holdersOf(members, role, tier) }))
      .filter(({ holders }) => holders.length > 1)
    if (crowds.length > 0) {
      const words = keyed(element('tr', 'slot-words'), 'words', role)
      words.dataset.role = role
      const cell = element('td')
      cell.colSpan = columns.length + 1
      cell.setAttribute('role', 'note')
      cell.append(
        ...crowds.map(({ tier, holders }) =>
          element(
            'p',
            null,
            crowdedWords(
              role,
              tier,
              holders.map((holder) => holder.name),
              hooks.draft,
            ),
          ),
        ),
      )
      words.append(cell)
      rows.push(words)
    }
    if (hooks.refusal?.role === role) {
      // Said where the person acted, in the dialog: not behind it on the board's status line.
      const refused = keyed(element('tr', 'slot-refusal'), 'refusal', role)
      refused.dataset.role = role
      const cell = element('td', null, hooks.refusal.words)
      cell.colSpan = columns.length + 1
      cell.setAttribute('role', 'alert')
      refused.append(cell)
      rows.push(refused)
    }
  }
  const apart = unseated(members)
  if (apart.length > 0) {
    // The members no slot holds, so that none the daemon sent is missing: each opens its part,
    // whose Edit gives it a tier its role has a slot at.
    const row = keyed(element('tr', 'slot-row slot-row-apart'), 'slot', NO_SLOT)
    row.dataset.role = NO_SLOT
    const head = element('th', 'slot-role', 'No slot')
    head.scope = 'row'
    const cell = element('td', 'slot slot-apart')
    cell.dataset.slot = NO_SLOT
    cell.colSpan = Math.max(columns.length, 1)
    const cards = element('div', 'slot-apart-cards')
    cards.append(...apart.map((member) => holderPart(member, NO_SLOT, drawn)))
    cell.append(
      element(
        'p',
        'slot-apart-words',
        'A member here holds no slot: none of its roles has a slot at its tier. Edit it to give it one.',
      ),
      cards,
    )
    row.append(head, cell)
    rows.push(row, ...opened(NO_SLOT, [null], anchor, hooks))
  }
  return rows
}

/** A row marked with what it is and its role, which a redraw tells it from the others by. */
function keyed(row, what, role) {
  row.dataset.key = `${what} ${role}`
  return row
}

/**
 * What a slot of `role`'s row opened, right under it: its member's own part or the form of its
 * Add, and the line that says why a form opened there was closed under the person (`gone`).
 */
function opened(role, tiers, anchor, hooks) {
  const rows = []
  if (anchor?.role === role) {
    rows.push(openRow(role, anchor.tier, [memberPart(anchor.member, hooks)]))
  } else {
    const adding = tiers.find((tier) => hooks.picking === slotKey(role, tier))
    if (adding !== undefined) {
      rows.push(openRow(role, adding, [formPlace(`slot ${slotKey(role, adding)}`)]))
    }
  }
  if (hooks.gone?.role === role) {
    const row = keyed(element('tr', 'slot-form-gone'), 'gone', role)
    row.dataset.role = role
    const cell = element('td', null, hooks.gone.words)
    cell.colSpan = tierColumns().length + 1
    cell.setAttribute('role', 'status')
    // The keyboard comes here when it was in the form that closed.
    cell.tabIndex = -1
    row.append(cell)
    rows.push(row)
  }
  return rows
}

/**
 * Where the open member's part goes: under the row of the slot it was opened from, or, where the
 * member holds that slot no more, under the first of the slots it holds, or the row of the members
 * that hold none. None where it left. A dialog keeps the role it is drawn under as the one its
 * part was opened from, so that the card it is drawn under shuts it.
 */
export function anchorOf(members, opened) {
  if (!opened) return null
  const member = members.find((one) => one.key === opened.key)
  if (!member) return null
  const seats = seatsOf(member)
  if (seats.length === 0) return { member, role: NO_SLOT, tier: null }
  const role = seats.includes(opened.role) ? opened.role : seats[0]
  return { member, role, tier: tiersOf(role).length === 0 ? null : member.tier }
}

/**
 * A row under a role's row, for what one of its slots opened: as wide as the slots, its caret at
 * the slot's column (`--column`), or along the row where the slot spans it.
 */
function openRow(role, tier, nodes) {
  const columns = tierColumns()
  const row = keyed(element('tr', 'slot-open-row'), 'open', role)
  row.dataset.role = role
  const cell = element('td')
  cell.colSpan = columns.length + 1
  const column = tier === null ? -1 : columns.indexOf(tier)
  cell.dataset.column = column < 0 ? 'all' : String(column)
  cell.style.setProperty('--column', String(Math.max(column, 0)))
  cell.style.setProperty('--columns', String(Math.max(columns.length, 1)))
  cell.append(...nodes)
  row.append(cell)
  return row
}

/**
 * A member's own part, under the slot it was opened from: its name and its Hide, what the dialog
 * puts in it (first what it runs on and its state, then its acts and its backup) with the slots it
 * holds after its first, where it holds several (what is changed here is changed in each), and
 * the member form where one of its acts opened it.
 */
function memberPart(member, hooks) {
  const part = element('div', 'slot-part')
  part.id = `${hooks.id}-member`
  part.setAttribute('role', 'region')
  part.setAttribute('aria-label', member.name)
  const head = element('div', 'slot-part-head')
  head.append(
    element('span', 'member-name', member.name),
    iconButton(
      CROSS,
      'Hide',
      () => hooks.shut(),
      `Hide ${member.name}'s ${member.opens}`,
      'slot-part-close',
    ),
  )
  // What it runs on first, as its card says it, then the slots it holds where it holds several.
  const [about, ...rest] = hooks.part(member)
  part.append(head, about)
  const held = roleIds().filter((role) => member.roles.includes(role))
  if (held.length > 1) {
    const count = COUNT[held.length] ?? String(held.length)
    part.append(
      element(
        'p',
        'slot-part-slots',
        `One member in ${count} slots: ${together(held.map(roleLabel))}. What you change here changes it in ${held.length === 2 ? 'both' : `all ${count}`}.`,
      ),
    )
  }
  part.append(...rest)
  // The member form, where its Edit or its backup opened it.
  if (hooks.formHere) part.append(formPlace(`member ${member.key}`))
  return part
}

/** One slot: its holders, or empty, with its Add and the members it could be given to. */
function slotCell(members, role, tier, hooks) {
  const key = slotKey(role, tier)
  const cell = element('td', 'slot')
  cell.dataset.slot = key
  const holders = holdersOf(members, role, tier)
  if (hooks.picking === key) cell.dataset.picking = 'true'
  if (holders.length > 1) cell.dataset.crowded = 'true'
  if (holders.length === 0) {
    const empty = element('div', 'slot-empty')
    empty.append(
      element('span', 'slot-empty-mark', 'empty'),
      button(
        'Add',
        'quiet-button slot-add',
        () => hooks.add(role, tier),
        `Add ${aSlot(role, tier)}`,
      ),
    )
    const given = takers(members, role, tier)
    if (given.length === 1) {
      // One member could take it: a link that says whom.
      const [member] = given
      const name = takerName(member, members)
      const gives = element('div', 'slot-gives')
      gives.append(
        button(
          `give it to ${name}`,
          'slot-give',
          () => hooks.give(role, member),
          `Give the ${slotName(role, tier)} slot to ${name}`,
        ),
      )
      empty.append(gives)
    } else if (given.length > 1) {
      // Several could: one quiet choice of them, so an empty slot stays one line or two.
      const gives = element('div', 'slot-gives')
      const pick = element('select', 'slot-give-pick')
      pick.setAttribute('aria-label', `Give the ${slotName(role, tier)} slot to`)
      const ask = new Option('give it to…', '')
      ask.disabled = true
      pick.append(
        ask,
        ...given.map((member) => new Option(takerName(member, members), String(member.key))),
      )
      pick.value = ''
      pick.addEventListener('change', () => {
        const member = given.find((one) => String(one.key) === pick.value)
        if (member) hooks.give(role, member)
      })
      gives.append(pick)
      empty.append(gives)
    }
    cell.append(empty)
    return cell
  }
  for (const member of holders) cell.append(holderPart(member, role, hooks))
  return cell
}

/**
 * A holder of a slot: its name, what it runs on, its state and its backup, which open the member's
 * own part, and the role's Remove; or the ask before its last role goes.
 */
function holderPart(member, role, hooks) {
  const part = element('div', 'slot-holder')
  part.dataset.member = String(member.key)
  if (hooks.confirming?.key === member.key && hooks.confirming.role === role) {
    part.classList.add('slot-confirm')
    part.append(
      element(
        'p',
        'member-confirm',
        hooks.confirmWords?.(member) ?? `Remove ${member.name}? Its open tasks are cancelled.`,
      ),
    )
    const actions = element('div', 'slot-confirm-actions')
    actions.append(
      button('Keep', 'quiet-button', () => hooks.keep(), `Keep ${member.name}`),
      button('Remove', 'danger-button', () => hooks.confirm(member), `Remove ${member.name}`),
    )
    part.append(actions)
    return part
  }
  const open = hooks.anchor?.member.key === member.key
  if (open) part.dataset.lit = hooks.anchor.role === role ? 'open' : 'same'
  const about = [element('span', 'member-name', member.name)]
  if (member.runs) about.push(element('span', 'member-meta', member.runs))
  for (const mark of member.marks ?? []) {
    const said = element('span', 'slot-mark', mark.text)
    said.dataset.tone = mark.tone
    about.push(said)
  }
  if (member.backup) {
    const backup = element('span', 'slot-backup')
    backup.append(element('span', 'slot-backup-kind', 'backup'), ` ${member.backup}`)
    about.push(backup)
  }
  // Its controls are named apart from every other's in the dialog: by its tier where another
  // member goes by its name (a draft's, named by its model), and by the slot where it holds several.
  const twin = (hooks.members ?? []).some(
    (other) => other.key !== member.key && other.name === member.name,
  )
  const at =
    twin && member.tier ? ` at ${member.tier.charAt(0).toUpperCase()}${member.tier.slice(1)}` : ''
  const slot = role !== NO_SLOT && seatsOf(member).length > 1 ? `, ${roleLabel(role)} slot` : ''
  if (member.opens) {
    // The card opens the member's own part: its Edit, its Vacation and its backup.
    const opener = element('button', 'slot-open')
    opener.type = 'button'
    opener.setAttribute('aria-label', `${member.name}${at}${slot}: ${member.opens}`)
    opener.setAttribute('aria-expanded', open && hooks.anchor.role === role ? 'true' : 'false')
    if (open && hooks.anchor.role === role)
      opener.setAttribute('aria-controls', `${hooks.id}-member`)
    opener.addEventListener('click', () => hooks.open(role, member))
    opener.append(...about, icon(CHEVRON))
    part.append(opener)
  } else {
    const plain = element('div', 'slot-about')
    plain.append(...about)
    part.append(plain)
  }
  // A member no slot holds has no slot's role to take: its part's Edit gives it one.
  if (role === NO_SLOT) return part
  part.append(
    iconButton(
      CROSS,
      `Remove the ${roleName(role)} role`,
      () => hooks.remove(role, member),
      `Remove ${roleLabel(role)} ${member.name}${at}`,
      'slot-remove',
    ),
  )
  return part
}
