import { button, element, iconButton } from '../dom.js'
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
 * A member here is `{ key, name, runs, roles, tier, marks }`: `key` tells it from the others (a
 * handle, or a draft's own number), `name` is what it is called (`@hera`, or a draft's model),
 * `runs` what it runs on where its name does not say, and `marks` its state in a few words
 * (`{ text, tone }`).
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
const together = (names) =>
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
 * draft says.
 */
export function slotRows(members, hooks) {
  const columns = tierColumns()
  const rows = []
  for (const role of roleIds()) {
    const row = element('tr', 'slot-row')
    row.dataset.role = role
    const head = element('th', 'slot-role', roleLabel(role))
    head.scope = 'row'
    row.append(head)
    const tiers = tiersOf(role)
    if (tiers.length === 0) {
      const cell = slotCell(members, role, null, hooks)
      cell.colSpan = Math.max(columns.length, 1)
      row.append(cell)
    } else {
      for (const tier of columns) {
        if (tiers.includes(tier)) row.append(slotCell(members, role, tier, hooks))
        else row.append(element('td', 'slot-none'))
      }
    }
    rows.push(row)
    const crowds = (tiers.length === 0 ? [null] : tiers)
      .map((tier) => ({ tier, holders: holdersOf(members, role, tier) }))
      .filter(({ holders }) => holders.length > 1)
    if (crowds.length > 0) {
      const words = element('tr', 'slot-words')
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
      const refused = element('tr', 'slot-refusal')
      refused.dataset.role = role
      const cell = element('td', null, hooks.refusal.words)
      cell.colSpan = columns.length + 1
      cell.setAttribute('role', 'alert')
      refused.append(cell)
      rows.push(refused)
    }
  }
  return rows
}

/** One slot: its holders, or empty, with its Add and the members it could be given to. */
function slotCell(members, role, tier, hooks) {
  const key = tier === null ? role : `${role}:${tier}`
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

/** A holder of a slot: its name, what it runs on, its state and the role's Remove, or the ask before its last role goes. */
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
  const top = element('div', 'slot-holder-top')
  top.append(
    element('span', 'member-name', member.name),
    iconButton(
      CROSS,
      `Remove the ${roleName(role)} role`,
      () => hooks.remove(role, member),
      `Remove ${roleLabel(role)} ${member.name}`,
      'slot-remove',
    ),
  )
  part.append(top)
  if (member.runs) part.append(element('span', 'member-meta', member.runs))
  for (const mark of member.marks ?? []) {
    const said = element('span', 'slot-mark', mark.text)
    said.dataset.tone = mark.tone
    part.append(said)
  }
  return part
}
