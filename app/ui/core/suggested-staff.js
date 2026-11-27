import { button, element, icon, redraw } from '../dom.js'
import { HARNESS_NAMES } from './board.js'
import { harnessOf, modelOf } from './model-option.js'
import { isImageAgentRole, roleIds, roleLabel, tierColumns, tiersOf } from './roles.js'
import { holdersOf, slotName, together } from './slots.js'
import { STAND_IN } from './suggested-staff-data.js'

/**
 * The suggested staff (T-2404): one staff ConsensFlow ships and is built with, offered in New
 * project and in Project settings alike, above the slots. Its shape is `suggested-staff-data.js`'s.
 *
 * It is offered only where it can work here: a member of it is offered where its harness is
 * installed and lists its model, at its effort, in the read the dialog made as it opened (never
 * from anything kept). A harness that is not installed, or whose models could not be read, or
 * that does not list a model, leaves that part out, and the panel says so in a plain word; where
 * no part works, nothing is offered. One choice takes it: it fills only the empty slots it names.
 * A slot that holds a member is left as it is and said; to take the suggestion's member there,
 * the person removes theirs first. A member already on the staff that runs just as one of its
 * members (harness, model, effort and tier) is given that member's empty slots, as "give it to"
 * gives one, rather than made twice. What it fills is an ordinary member afterwards.
 */

/** The suggestion as the daemon sent it last; the page's stand-in until it sends one. */
let suggestion = STAND_IN

/**
 * Keeps the suggestion a `harnesses.missing` answer sends (`suggestedStaff`): null where there is
 * none to offer. An answer that says nothing of it keeps the last.
 */
export function readSuggestion(value) {
  if (value === undefined) return
  suggestion = value !== null && Array.isArray(value.members) ? value : null
}

/**
 * Whether `entry` is a member the slots can hold: roles the daemon names, at a tier each has a
 * slot at, on a harness ConsensFlow runs; an image designer on Codex alone.
 */
function fits(entry) {
  if (entry === null || typeof entry !== 'object') return false
  const { roles } = entry
  if (!Array.isArray(roles) || roles.length === 0) return false
  if (!roles.every((role) => roleIds().includes(role))) return false
  if (!(entry.harness in HARNESS_NAMES)) return false
  if (entry.designer) return entry.harness === 'codex' && roles.every(isImageAgentRole)
  return (
    typeof entry.model === 'string' &&
    entry.model !== '' &&
    roles.every((role) => !isImageAgentRole(role) && tiersOf(role).includes(entry.tier))
  )
}

/**
 * Where a member of the suggestion comes in its list: by its tier, most critical first, then by its
 * first role's row in the slots; the image designer, which has no tier, last.
 */
function rank(entry) {
  const first = roleIds().find((role) => entry.roles.includes(role))
  const tier = entry.designer ? tierColumns().length : tierColumns().indexOf(entry.tier)
  return tier * roleIds().length + roleIds().indexOf(first)
}

/** The suggestion's members the slots can hold, in the list's order; none where there is none. */
const entriesOf = (value) =>
  (value?.members ?? []).filter(fits).sort((one, other) => rank(one) - rank(other))

/** The harnesses the suggestion's members run on, by kind, each once. */
const kindsOf = (entries) => [...new Set(entries.map((entry) => entry.harness))]

/** A suggested member's tier, as its slots read it: none for the image designer's. */
const tierOf = (entry) => (entry.designer ? null : entry.tier)

/** Whether a member on the staff runs just as `entry` would: one member, not two alike. */
const sameAs = (entry, member) =>
  entry.designer
    ? Boolean(member.designer)
    : !member.designer &&
      member.harness === entry.harness &&
      member.model === entry.model &&
      (member.effort ?? null) === (entry.effort ?? null) &&
      member.tier === entry.tier

/** What a suggested member is called and runs on, as a draft's card says it. */
const nameOf = (entry) => (entry.designer ? `${HARNESS_NAMES[entry.harness]} default` : entry.model)
const runsOf = (entry) => [HARNESS_NAMES[entry.harness], entry.effort].filter(Boolean).join(' · ')

/**
 * Whether `entry` runs here, by what its harness's read said (`read`): `{ model, read }` where it
 * does, `{ left }` where it does not and why, `{ pending }` while its harness is read.
 */
function runsHere(entry, read) {
  if (read.state === 'reading') return { pending: true }
  if (read.state === 'missing' || read.state === 'failed') return { left: read.state }
  if (entry.designer) return { read: null }
  const model = read.models.find((one) => modelOf(one) === entry.model && !one.disabled)
  const efforts = model?.efforts ?? []
  const effort = entry.effort ?? null
  if (!model || (efforts.length === 0 ? effort !== null : !efforts.includes(effort))) {
    return { left: 'unlisted' }
  }
  return { model, read: read.read }
}

/**
 * What the suggestion would do to `members` (each `{ key, name, roles, tier, designer, harness,
 * model, effort }`, its model as its harness's list names it), as each harness's read says
 * (`readOf(kind)`: `{ state: 'reading' | 'missing' | 'failed' | 'read', models, read, why }`):
 * - `rows`, a suggested member each, with its slots, each `empty`, `same` (a member that runs
 *   as it holds it) or `kept` (another holds it), and `left` where it does not run here;
 * - `makes`, what a fill does, each for its suggested member (`entry`): a new member with the
 *   empty slots' roles (`add`, with what Add member sends), or those roles given to the member on
 *   the staff that runs as it (`give`, `to` its key);
 * - `fills`, the slots that fills, `kept`, the slots it leaves to the member there, `pending`
 *   whether a read it needs is on its way, and `waiting`, the empty slots of the members whose
 *   harness is installed (what it may yet fill), `gaps`, the empty slots each part that does
 *   not run here would have filled, and `harnesses`, the kinds of those that fill.
 */
export function planOf(entries, members, readOf) {
  const rows = []
  const makes = []
  const kept = []
  const gaps = []
  let pending = false
  let waiting = 0
  for (const entry of entries) {
    const tier = tierOf(entry)
    const slots = roleIds()
      .filter((role) => entry.roles.includes(role))
      .map((role) => {
        const holders = holdersOf(members, role, tier)
        if (holders.length === 0) return { role, tier, state: 'empty', holders: [] }
        const state = holders.some((holder) => sameAs(entry, holder)) ? 'same' : 'kept'
        return { role, tier, state, holders: holders.map((holder) => holder.name) }
      })
    const read = readOf(entry.harness)
    const here = runsHere(entry, read)
    const empty = slots.filter((slot) => slot.state === 'empty').map((slot) => slot.role)
    rows.push({ entry, slots, left: here.left ?? null })
    if (read.state !== 'missing' && read.state !== 'failed') waiting += empty.length
    if (here.pending) {
      pending = true
      continue
    }
    if (here.left) {
      if (empty.length > 0)
        gaps.push({ entry, left: here.left, why: read.why, slots: empty.length })
      continue
    }
    for (const slot of slots) if (slot.state === 'kept') kept.push(slot)
    if (empty.length === 0) continue
    const like = entry.designer ? null : members.find((member) => sameAs(entry, member))
    if (like) {
      makes.push({ kind: 'give', entry, to: like.key, name: like.name, roles: empty })
      continue
    }
    makes.push({
      kind: 'add',
      entry,
      roles: empty,
      picked: entry.designer
        ? { harness: entry.harness }
        : {
            harness: entry.harness,
            model: modelOf(here.model),
            effort: entry.effort ?? null,
            tier: entry.tier,
            modelName: modelOf(here.model),
            ...(here.read != null ? { read: here.read } : {}),
          },
    })
  }
  const fills = makes.reduce((sum, make) => sum + make.roles.length, 0)
  const harnesses = kindsOf(makes.map((make) => make.entry))
  return { rows, makes, fills, kept, gaps, pending, waiting, harnesses }
}

/** "1 empty slot", "3 empty slots". */
const slots = (count, words = 'slot') => `${count} ${words}${count === 1 ? '' : 's'}`

/** What the panel says of the parts that do not run here, a sentence a harness or a model. */
function gapWords(gaps) {
  const said = []
  const byHarness = new Map()
  for (const gap of gaps) {
    if (gap.left === 'unlisted') continue
    const key = `${gap.left} ${gap.entry.harness}`
    const seen = byHarness.get(key) ?? { ...gap, slots: 0 }
    seen.slots += gap.slots
    byHarness.set(key, seen)
  }
  for (const gap of byHarness.values()) {
    const name = HARNESS_NAMES[gap.entry.harness]
    const what = `so the ${slots(gap.slots)} it puts on ${name} ${gap.slots === 1 ? 'is' : 'are'} left out`
    said.push(
      gap.left === 'missing'
        ? `${name} is not installed here, ${what}. Install ${name} from Agents, Harnesses to take ${gap.slots === 1 ? 'it' : 'them'} too.`
        : `${name}'s models could not be read${gap.why ? ` (${gap.why})` : ''}, ${what}.`,
    )
  }
  for (const gap of gaps) {
    if (gap.left !== 'unlisted') continue
    const { entry } = gap
    const at = entry.effort ? ` at ${entry.effort}` : ''
    said.push(
      `${HARNESS_NAMES[entry.harness]} here does not list ${entry.model}${at}, so the ${slots(gap.slots)} for it ${gap.slots === 1 ? 'is' : 'are'} left out.`,
    )
  }
  return said
}

/** What the panel says of the slots it leaves to the members there. */
function keptWords(kept) {
  if (kept.length === 0) return null
  const each = kept.map(
    (slot) => `the ${slotName(slot.role, slot.tier)} slot (${together(slot.holders)})`,
  )
  const one = kept.length === 1
  return `It leaves the ${one ? 'slot' : `${kept.length} slots`} that already ${one ? 'holds' : 'hold'} a member as ${one ? 'it is' : 'they are'}: ${together(each)}. To take its member there, remove yours first, then fill again.`
}

const SPARK = ['M12 3.5 13.9 10.1 20.5 12 13.9 13.9 12 20.5 10.1 13.9 3.5 12 10.1 10.1Z']
const CHEVRON = ['M7 10l5 5 5-5']
const CHECK = ['M5 12.5l4.5 4.5L19 7.5']
const PLUS = ['M12 6v12', 'M6 12h12']

/** A suggested member's row of the list: its tier, its name and what it runs on, and its slots. */
function rowOf({ entry, slots: held, left }) {
  const row = element('li', 'suggest-row')
  if (left) row.dataset.left = left
  const tier = element(
    'span',
    'suggest-tier',
    entry.designer ? '' : `${entry.tier.charAt(0).toUpperCase()}${entry.tier.slice(1)}`,
  )
  const who = element('span', 'suggest-who')
  who.append(
    element('span', 'member-name', nameOf(entry)),
    element('span', 'member-meta', runsOf(entry)),
  )
  if (left) {
    who.append(
      element(
        'span',
        'suggest-left-mark',
        left === 'missing'
          ? 'not installed here'
          : left === 'failed'
            ? 'models not read'
            : 'not listed here',
      ),
    )
  }
  const chips = element('span', 'suggest-slots')
  for (const slot of held) {
    const chip = element('span', 'suggest-slot')
    const state = left ? 'left' : slot.state
    chip.dataset.state = state
    if (state === 'empty') {
      chip.append(icon(PLUS), element('span', 'visually-hidden', 'fills '))
    } else if (state === 'same') {
      chip.append(icon(CHECK))
    }
    chip.append(roleLabel(slot.role))
    if (state === 'same') chip.append(element('span', 'suggest-slot-note', ' · in place'))
    if (state === 'kept') {
      chip.append(element('span', 'suggest-slot-note', ` · held by ${together(slot.holders)}`))
    }
    chips.append(chip)
  }
  row.append(tier, who, chips)
  return row
}

/**
 * The panel of a staff dialog that offers the suggested staff, in place above its slots
 * (`root`, `.suggest` in each dialog's Staff), drawn again with the staff (`draw`). `id` names
 * it in its dialog, `lists` is the dialog's reads of the harnesses (`ModelLists`), `changed()`
 * draws the dialog again once a read came, and `fill(plan)` takes the suggestion, as the
 * dialog's staff takes a member: then `filling()` while it is on its way, and `filled(count,
 * refused)` once it is done.
 */
export class SuggestedStaff {
  #root
  #id
  #lists
  #changed
  #fill
  /** Each harness the suggestion runs on, by kind: as the dialog's read of it says, this opening. */
  #reads = new Map()
  /** Which ask of each harness's list is the last: an answer to an earlier one acts no more. */
  #asks = new Map()
  #asked = 0
  /** Whether its list of what it puts where is open. */
  #open = false
  /** The slots this opening's fills filled, and what was refused of the last: `{ slots, words }`. */
  #taken = 0
  #refused = []
  /** Whether a fill is on its way. */
  #busy = false
  /** The plan the panel shows, which its Fill takes. */
  #plan = null

  constructor(root, { id, lists, changed, fill }) {
    this.#root = root
    this.#id = id
    this.#lists = lists
    this.#changed = changed
    this.#fill = fill
    // A Refresh of a harness is the read the suggestion goes by from then on.
    lists.onRefreshed((program) => {
      for (const kind of this.#reads.keys()) {
        if (harnessOf(kind) === program && this.#reads.get(kind).state !== 'missing') {
          this.#read(kind)
        }
      }
    })
  }

  /** The dialog opened: what was taken or opened before is forgotten, and each harness it runs on is read. */
  start(missing) {
    this.#reads.clear()
    this.#asks.clear()
    this.#open = false
    this.#taken = 0
    this.#refused = []
    this.#busy = false
    this.#plan = null
    for (const kind of kindsOf(entriesOf(suggestion))) {
      if (missing.includes(harnessOf(kind))) this.#reads.set(kind, { state: 'missing' })
      else this.#read(kind)
    }
  }

  /** Reads `kind`'s list as the dialog's reads have it now; an earlier opening's, or ask's, answer acts no more. */
  #read(kind) {
    this.#asked += 1
    const asked = this.#asked
    this.#asks.set(kind, asked)
    this.#reads.set(kind, { state: 'reading' })
    this.#lists.get(harnessOf(kind)).then(
      (answer) => {
        if (this.#asks.get(kind) !== asked) return
        this.#reads.set(kind, { state: 'read', models: answer.listing.models, read: answer.read })
        this.#changed()
      },
      (cause) => {
        if (this.#asks.get(kind) !== asked) return
        const why = cause instanceof Error ? cause.message : String(cause)
        this.#reads.set(kind, { state: 'failed', why })
        this.#changed()
      },
    )
  }

  /** A fill is on its way: Fill waits for it. */
  filling() {
    this.#busy = true
    this.#refused = []
  }

  /** A fill is done: `count` slots filled, and each make refused, `{ slots, words }` in the daemon's words. */
  filled(count, refused = []) {
    this.#busy = false
    this.#taken += count
    this.#refused = refused
  }

  /**
   * Draws the panel for the staff as it is (`members`, as `planOf` takes them): its offer, or
   * that it checks what the harnesses list, or what its fill did; nothing where it can do
   * nothing here.
   */
  draw(members) {
    const entries = entriesOf(suggestion)
    const plan = planOf(entries, members, (kind) => this.#reads.get(kind) ?? { state: 'missing' })
    this.#plan = plan
    let state = 'none'
    if (entries.length > 0) {
      if (plan.pending && plan.waiting > 0) state = 'checking'
      else if (plan.fills > 0 || this.#busy) state = 'offer'
      else if (this.#taken > 0 || this.#refused.length > 0) state = 'done'
    }
    const had = this.#root.contains(document.activeElement)
    this.#root.hidden = state === 'none'
    this.#root.dataset.state = state
    redraw(this.#root, state === 'none' ? [] : this.#nodes(state, plan))
    // The keyboard stays in the panel when what had it went (Fill, once it filled): on Fill where
    // it is to press, else on what the fill did, else on the panel's name while a fill goes on.
    const heading = this.#root.querySelector('h3')
    const lost = !this.#root.contains(document.activeElement) || document.activeElement === heading
    if (had && !this.#root.hidden && lost) {
      const next =
        this.#root.querySelector('.suggest-take:not(:disabled)') ??
        this.#root.querySelector('.suggest-done') ??
        heading
      if (next !== document.activeElement) next?.focus({ preventScroll: true })
    }
  }

  #nodes(state, plan) {
    const title = `${this.#id}-suggest-title`
    this.#root.setAttribute('role', 'region')
    this.#root.setAttribute('aria-labelledby', title)
    const head = element('div', 'suggest-head')
    const heading = element('h3', null, 'Suggested staff')
    heading.id = title
    heading.tabIndex = -1
    head.append(icon(SPARK), heading)
    const nodes = [head]
    if (state === 'done') {
      const done = element('p', 'suggest-done')
      done.setAttribute('role', 'status')
      done.tabIndex = -1
      done.append(
        icon(CHECK),
        element(
          'span',
          null,
          this.#taken > 0
            ? `Filled ${slots(this.#taken)} from the suggested staff. Each is a member like any other: open its card to change it, or remove it from its slot.`
            : 'Nothing was filled.',
        ),
      )
      nodes.push(done, ...this.#refusal())
      return nodes
    }
    nodes.push(element('p', 'suggest-about', suggestion.about ?? ''))
    if (state === 'checking') {
      nodes.push(
        element('p', 'suggest-fills suggest-checking', 'Checking what your harnesses list…'),
      )
    } else {
      const on = together(plan.harnesses.map((kind) => HARNESS_NAMES[kind]))
      nodes.push(
        element(
          'p',
          'suggest-fills',
          `It fills ${slots(plan.fills, 'empty slot')} with members on ${on}.`,
        ),
      )
      const kept = keptWords(plan.kept)
      if (kept) nodes.push(element('p', 'suggest-kept', kept))
    }
    for (const words of gapWords(plan.gaps)) nodes.push(element('p', 'suggest-gap', words))
    nodes.push(...this.#refusal())
    const acts = element('div', 'suggest-acts')
    const ready = state === 'offer' && !this.#busy
    const take = button(
      this.#busy
        ? 'Filling…'
        : ready
          ? `Fill ${slots(plan.fills, 'empty slot')}`
          : 'Fill empty slots',
      'suggest-take',
      () => {
        if (this.#busy || this.#plan === null || this.#plan.fills === 0) return
        void this.#fill(this.#plan)
      },
      ready ? `Fill ${slots(plan.fills, 'empty slot')} with the suggested staff` : undefined,
    )
    take.disabled = !ready
    acts.append(take)
    if (state === 'offer') {
      const list = `${this.#id}-suggest-list`
      const toggle = button('What it puts where', 'suggest-toggle', () => {
        this.#open = !this.#open
        this.#changed()
      })
      toggle.setAttribute('aria-expanded', String(this.#open))
      toggle.setAttribute('aria-controls', list)
      toggle.append(icon(CHEVRON))
      acts.append(toggle)
      nodes.push(acts)
      if (this.#open) {
        const rows = element('ul', 'suggest-list')
        rows.id = list
        rows.setAttribute('aria-label', 'What the suggested staff puts where')
        rows.append(...plan.rows.map(rowOf))
        nodes.push(rows)
      }
    } else {
      nodes.push(acts)
    }
    return nodes
  }

  /** What the daemon refused of the last fill, in its words. */
  #refusal() {
    if (this.#refused.length === 0) return []
    const count = this.#refused.reduce((sum, refused) => sum + refused.slots, 0)
    const words = this.#refused.map((refused) => refused.words.replace(/[.\s]+$/, ''))
    const said = element(
      'p',
      'suggest-refused',
      `${slots(count)} could not be filled: ${words.join('; ')}.`,
    )
    said.setAttribute('role', 'alert')
    return [said]
  }
}
