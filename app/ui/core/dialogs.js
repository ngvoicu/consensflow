import { button, element, redraw } from '../dom.js'
import {
  HARNESS_NAMES,
  harnessLabel,
  isMember,
  modelShown,
  nameApart,
  outWords,
  tryAgain,
  vacationButton,
} from './board.js'
import { harnessesOf } from './computer-use.js'
import { modelOptionText } from './model-option.js'
import { holdEnter, ProjectSections } from './project-sections.js'
import {
  isImageAgentRole,
  readRoles,
  roleIds,
  roleLabel,
  takesNoTier,
  tierColumns,
  tiersOf,
} from './roles.js'
import {
  anchorOf,
  crowded,
  holdersOf,
  inTheWay,
  NO_SLOT,
  slotHead,
  slotName,
  slotRows,
  takenWords,
  together,
} from './slots.js'

/**
 * The dialogs that say who works on a project and how: New project (its
 * folder, the harness, model and effort its chief runs on, and its settings: the
 * first staff, human approval, wake-up calls and specs), Switch chief and Project
 * settings (the same four, for a project that is open). A member is a harness, a
 * model it lists, an effort and a tier (harness-models). Each keeps what it is
 * in the middle of to itself, and reads the page's board as it is when it draws
 * or acts; what it changes on the page goes back
 * through the page's callbacks. A dialog that cannot open says why by
 * throwing, as the daemon's refusals do.
 */

/** How a picker says a tier, one of the daemon's: "Critical work". */
const tierLabel = (tier) => `${tier.charAt(0).toUpperCase()}${tier.slice(1)} work`
const rank = (list, value) => (list.includes(value) ? list.indexOf(value) : list.length)
/** The tiers a member of `roles` can be in: each tier every one of its roles with tiers has a slot at. */
const tiersFor = (roles) =>
  tierColumns().filter((tier) =>
    roles.every((role) => tiersOf(role).length === 0 || tiersOf(role).includes(tier)),
  )
/**
 * What a member's part says under its name: its tier and its harness, and what it runs on (its
 * model's id and its effort) only where its name does not say it (HMD-12); a pick of New project
 * has no name made yet, and says its harness, its effort and its tier.
 */
const runsLabel = (member, named = true) => {
  // A member whose roles have no tier is said to have none (an image designer's: a design task
  // has none, designer P2); a staff row has its roles, a pick of New project its role.
  const tier = takesNoTier(member.roles ?? [member.role]) ? null : member.tier
  return (
    named
      ? [
          ...(nameApart(member) ? [modelShown(member), member.effort] : []),
          tier,
          member.harness && harnessLabel(member),
        ]
      : [member.harness && harnessLabel(member), member.effort, tier]
  )
    .filter(Boolean)
    .join(' · ')
}

/** A member's roles in the order the picker offers them. */
const inRoleOrder = (roles) => [...roles].sort((a, b) => rank(roleIds(), a) - rank(roleIds(), b))

/**
 * What a member's own part holds, in words, as its slot's card is named by them (T-2341): its
 * Edit where it has one, its Vacation where it has one to change, and its backup where it may
 * have one or has one. An image designer has no Edit, and is offered no backup.
 */
const opensSaid = ({ edit, vacation, backup }) =>
  together([edit && 'Edit', vacation && 'Vacation', backup && 'backup'].filter(Boolean))

/**
 * The first lines of a member's part, or of its backup's, under its name: what it runs on, its
 * model marked where its harness lists it no more (T-1143) with what to do in words, or what to
 * do where the update could not tell what it ran on (T-1202), and on Vacation where it rests.
 */
function aboutLines(
  runs,
  { gone = null, harness = null, needsEdit = false, agent = null, resting = false },
) {
  const about = element('div', 'slot-part-about')
  const meta = element('span', 'member-meta')
  if (gone) meta.append(element('mark', 'model-gone', gone), ` · ${runs}`)
  else meta.textContent = runs
  about.append(meta)
  if (gone) {
    about.append(
      element(
        'span',
        'member-gone',
        `${harness} no longer lists ${gone}: edit it to a model it lists.`,
      ),
    )
  }
  if (needsEdit) {
    about.append(
      element(
        'span',
        'member-gone',
        `The update could not tell what ${agent ?? 'its agent'} ran on: edit it to a model its harness lists, or remove it.`,
      ),
    )
  }
  if (resting) {
    const mark = element('span', 'slot-mark', 'on Vacation')
    mark.dataset.tone = 'rest'
    about.append(mark)
  }
  return about
}

/** A part's acts, in one row that wraps where the dialog is narrow. */
function actsOf(acts) {
  const row = element('div', 'slot-part-acts')
  row.append(...acts)
  return row
}

/**
 * A member's backup in its part, the same in both staff dialogs: the backup (`about`, its name's
 * lines, and `acts`: Change backup, its Vacation and Remove backup), or none (null, with Add
 * backup). An image designer is offered no backup, and shows the one it has (an earlier
 * version's) with its Vacation and Remove backup.
 */
function backupPart(owner, backup, about, acts) {
  const part = element('div', backup === null ? 'backup-part backup-none' : 'backup-part')
  const who = element('div', 'backup-who')
  // Said to a screen reader as "backup of @zeus …", or "backup for @zeus: none".
  who.append(
    element('span', 'team-kind', 'backup'),
    element('span', 'visually-hidden', backup === null ? ` for ${owner}: ` : ` of ${owner} `),
  )
  if (backup === null) who.append(element('span', 'member-meta', 'none'))
  else who.append(element('span', 'member-name', backup), about)
  part.append(who, actsOf(acts))
  return part
}

/** A backup's acts, the same in both staff dialogs: named for the member they are for. */
const addBackup = (owner, action) =>
  button('Add backup', 'quiet-button', action, `Add backup for ${owner}`)
const changeBackup = (owner, action) =>
  button('Change backup', 'quiet-button', action, `Change backup for ${owner}`)
const removeBackup = (owner, action) =>
  button('Remove backup', 'quiet-button', action, `Remove backup for ${owner}`)

/**
 * Why the member form is to be shut, as a staff dialog's staff is now (T-2341, round 2), or none
 * where what it was opened for is as it was: the empty slot of its Add is held, the member of its
 * backup's form left or its backup is another (or none, or one now) than the form was opened on,
 * or the member it edits left or was renamed. `look` reads the staff: `holder(role, tier)`,
 * `member(key)`, `backup(ownerKey)` (the key of its backup, or null), `backupName(key)` and
 * `renamed(key)` (the words for a member that is another by now, or none).
 */
function formGoneWords(form, look) {
  if (form.slot !== null) {
    const held = look.holder(form.slot.role, form.slot.tier)
    return held
      ? `The ${slotName(form.slot.role, form.slot.tier)} slot is ${held.name}'s now, so its Add was closed. Nothing was added.`
      : null
  }
  if (form.backup !== null) {
    const { owner, changing, of, ofName } = form.backup
    const what = changing ? 'Change backup' : 'Add backup'
    if (!look.member(owner.key)) {
      return `${owner.label} left the staff, so ${what} was closed. Nothing was saved.`
    }
    const now = look.backup(owner.key)
    if (now === of) return null
    if (now === null)
      return `${owner.label} has no backup now, so ${what} was closed. Nothing was saved.`
    return of === null
      ? `${owner.label} has a backup now, ${look.backupName(now)}, so Add backup was closed. Nothing was saved.`
      : `${owner.label}'s backup is ${look.backupName(now)} now, not ${ofName}, so Change backup was closed. Nothing was saved.`
  }
  if (form.editing !== null && !look.member(form.editing)) {
    return (
      look.renamed?.(form.editing) ??
      `${form.editingName} left the staff, so its Edit was closed. Nothing was saved.`
    )
  }
  return null
}

/**
 * Draws a staff dialog's slots, the member form in the place a slot's row has for it (T-2341):
 * under an empty slot its Add opened, or in the part of the member whose Edit or backup opened
 * it. A redraw leaves the form where it is while its place stays; a form whose place goes is
 * put back under the slots first, so a redraw never takes it out of the dialog. The keyboard
 * stays in the form wherever it goes.
 */
function drawSlots(body, rows, form) {
  const table = body.closest('table')
  const had = form.contains(document.activeElement) ? document.activeElement : null
  const wanted = rows.flatMap((row) => [...row.querySelectorAll('.slot-form-place')])[0]
  const held = form.parentElement
  if (body.contains(form) && held.dataset.keep !== wanted?.dataset.keep) table.after(form)
  redraw(body, rows)
  const place = body.querySelector('.slot-form-place')
  if (place !== null && form.parentElement !== place) place.append(form)
  if (place === null && body.contains(form)) table.after(form)
  if (had?.isConnected && document.activeElement !== had) {
    had.focus({ preventScroll: true })
  }
}

/**
 * The member's part, once a slot's card opened it: in sight with the slot it came from, the
 * keyboard on its first act.
 */
function showPart(body) {
  const row = body.querySelector('tr.slot-open-row')
  if (row === null) return
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches
  row.scrollIntoView({ block: 'nearest', behavior: still ? 'auto' : 'smooth' })
  const first = row.querySelector('.slot-part-acts button, .slot-part button:not(.slot-part-close)')
  ;(first ?? row.querySelector('.slot-part-close'))?.focus({ preventScroll: true })
}

/**
 * Where the keyboard goes once a member's part is shut: back to the card it was opened from, as
 * the slots are drawn again. Read before they are.
 */
function backToCard(body) {
  const card = body.querySelector('.slot-holder[data-lit="open"]')
  if (card === null) return () => {}
  const slot = card.closest('[data-slot]').dataset.slot
  const member = card.dataset.member
  return () =>
    body
      .querySelector(
        `[data-slot="${slot}"] .slot-holder[data-member="${CSS.escape(member)}"] .slot-open`,
      )
      ?.focus()
}

/** The name a harness's program goes by, which `models.get` and `missing` say: Claude Code's is `claude`. */
const harnessOf = (kind) => (kind === 'claude-code' ? 'claude' : kind)

const NO_HARNESS = 'No harness is installed here: install one from Agents, Harnesses.'

/** Every harness installed here, by kind: what a dialog reads as it opens, whatever its pickers offer. */
const installedKinds = (missing) =>
  Object.keys(HARNESS_NAMES).filter((kind) => !missing.includes(harnessOf(kind)))

/**
 * A dialog's reads, started as it opens (A4-28): what each harness lists now, every installed
 * one at once and side by side, whatever role or harness the form was left on. The reads of an
 * earlier opening are forgotten.
 */
function readEveryHarness(lists, missing) {
  lists.clear()
  lists.start(installedKinds(missing))
}

/** What `--model` takes for a listed model: Pi's provider and id joined, the id else. */
const modelOf = (model) => (model.provider ? `${model.provider}/${model.id}` : model.id)

/**
 * The models each harness lists of itself (`models.get`, CHF-03), read as a dialog opens
 * (A4-28): each installed harness is asked at once ([`start`]), side by side, and what a
 * harness answers is what the dialog shows of it, until the human's Refresh reads it again.
 * Nothing is kept past the dialog: each opening starts its reads afresh ([`clear`]), and no
 * list read before is shown as the harness's own now. A read that is refused is not kept,
 * so the next ask reads again. **Every picker of a harness in a dialog shows the one read of
 * it**: a Refresh in one is told to the others ([`onRefreshed`]), and what a picker submits
 * names the read it showed (`read`, [`readOf`]), which the daemon checks it against.
 */
export class ModelLists {
  #daemon
  /** Each harness's answer, as asked: a promise, kept from the moment it is asked. */
  #answers = new Map()
  /** Each harness's answer once it came, which its pickers show: it carries its read's id. */
  #came = new Map()
  /** Who is told of a Refresh, with the harness and the picker that asked it. */
  #listeners = new Set()

  constructor(daemon) {
    this.#daemon = daemon
  }

  /** Forgets every read: a dialog opening reads each harness again. */
  clear() {
    this.#answers.clear()
    this.#came.clear()
  }

  /** The id of the read the pickers of `harness` show now, once it came; none else. */
  readOf(harness) {
    return this.#came.get(harness)?.read ?? null
  }

  /** `listener(harness, source)` is told when a Refresh of `harness` that `source` asked has come. */
  onRefreshed(listener) {
    this.#listeners.add(listener)
  }

  /**
   * Asks each of `kinds` (by kind, `claude-code`) that this dialog has no read of, all at
   * once: the reads of the harnesses run side by side, each under its own bound, and a pick
   * of one of them finds its read made or on its way.
   */
  start(kinds) {
    for (const kind of kinds) this.get(harnessOf(kind))
  }

  /** `harness`'s answer (by its program's name, `claude`): the one read already, or a read. */
  get(harness) {
    return this.#answers.get(harness) ?? this.#ask(harness, false)
  }

  /** The human's Refresh: `harness`'s list read again from the harness itself, for every picker of it. */
  refresh(harness, source = null) {
    const asked = this.#ask(harness, true)
    asked.then(
      () => {
        for (const listener of this.#listeners) listener(harness, source)
      },
      () => {},
    )
    return asked
  }

  #ask(harness, refresh) {
    const asked = this.#daemon('models.get', refresh ? { harness, refresh } : { harness })
    this.#answers.set(harness, asked)
    asked.then(
      (answer) => {
        if (this.#answers.get(harness) === asked) this.#came.set(harness, answer)
      },
      () => {
        if (this.#answers.get(harness) === asked) this.#answers.delete(harness)
      },
    )
    return asked
  }
}

/** A listing's model that `member` (its `model` and `effort`) runs, and at what effort; none
 * where the listing has no such model. Devin's member holds the exact id Devin lists for an
 * effort (`launchIds`). */
function whereListed(models, member) {
  for (const model of models) {
    if (modelOf(model) === member.model) return { model, effort: member.effort ?? null }
    for (const [effort, id] of Object.entries(model.launchIds ?? {})) {
      if (id === member.model) return { model, effort }
    }
  }
  return null
}

/**
 * The id a launch passes for `model` at `effort`: Devin's listed id for that effort
 * (`launchIds`), else the model's own. It is what an edited member stores, so that a pick can
 * be compared with the member's stored choice by what it launches, not by how it is shown.
 */
function launchIdOf(model, effort) {
  return (effort ? model.launchIds?.[effort] : undefined) ?? modelOf(model)
}

/**
 * Fills an effort select with the efforts `model` lists (none picked where it lists none or
 * none is picked yet), its harness's default picked where it says one.
 */
function fillEfforts(select, model) {
  if (model === undefined || model.efforts.length === 0) {
    const none = new Option(model === undefined ? 'Pick a model first' : 'No effort to pick', '')
    none.disabled = true
    select.replaceChildren(none)
    select.value = ''
    select.disabled = true
    return
  }
  const ask = new Option('Pick an effort', '')
  ask.disabled = true
  select.replaceChildren(ask, ...model.efforts.map((effort) => new Option(effort, effort)))
  select.value = model.efforts.includes(model.defaultEffort) ? model.defaultEffort : ''
  select.disabled = false
}

/**
 * A harness select, and a model select filled with the models that harness lists of itself
 * (`models.get`): what the chief's picker (CHF-07) and Add member and Edit member (T-1143)
 * share. Nothing is picked to start with. While a harness's models are read the model
 * select says so; a harness whose models cannot be read, or that lists none, leaves it
 * unpicked, and says why. A contributor model says so beside its name, and a stale list is
 * said to be one. `say` shows the words under the selects; `changed` hears of each pick.
 */
class ModelPicker {
  #harness
  #model
  #lists
  #say
  #changed
  #takenOff
  /** The listing read for the harness picked now, and the answer's words. */
  #answer = null
  /** Whether the select holds a member's own model, for want of a list to pick from. */
  #kept = false
  /** The member's own `{model, effort}` the last `show` was given, held for a re-show. */
  #current = null
  /** Which read the model select waits for: an answer to an older one is let go. */
  #asked = 0

  /**
   * `takenOff(kind, id)` is a model listed and not to be picked (the chief's own, for
   * Switch chief); `switchHelp` whether the harness's own words for switching its model
   * in the chief's window are said.
   */
  constructor({
    harness,
    model,
    lists,
    say,
    changed = () => {},
    takenOff = () => false,
    switchHelp = false,
  }) {
    this.#harness = harness
    this.#model = model
    this.#lists = lists
    this.#say = say
    this.#changed = changed
    this.#takenOff = takenOff
    this.switchHelp = switchHelp
    /** Whether picking a harness reads its models: an image designer picks none (designer P2). */
    this.reads = true
    harness.addEventListener('change', () => {
      if (this.reads) void this.show(harness.value)
    })
    model.addEventListener('change', () => {
      this.#sayHelp()
      this.#changed()
    })
    // A Refresh another picker asked of the harness this one shows is the read this one shows now.
    lists.onRefreshed((refreshed, source) => {
      const kind = this.kind
      if (source === this || kind === '' || !this.reads || harnessOf(kind) !== refreshed) return
      void this.show(kind, { wanted: this.#model.value || null, current: this.#current })
    })
  }

  /** The harnesses installed here, of `only` where it names some, none picked; with none it refuses, and says where to get one. */
  fill(missing, only = null) {
    const installed = Object.entries(HARNESS_NAMES).filter(
      ([kind]) => !missing.includes(harnessOf(kind)) && (only === null || only.includes(kind)),
    )
    if (installed.length === 0) throw new Error(NO_HARNESS)
    const ask = new Option('Pick a harness', '')
    ask.disabled = true
    this.#harness.replaceChildren(ask, ...installed.map(([kind, name]) => new Option(name, kind)))
    this.#harness.value = ''
    this.#asked += 1
    this.#none('Pick a harness first', '')
  }

  /** The harness picked, by its kind (`claude-code`). */
  get kind() {
    return this.#harness.value
  }

  /** Picks `kind` with no model to read or to pick: an image designer's Codex (designer P2). */
  choose(kind) {
    this.#harness.value = kind
    this.#asked += 1
    this.#none('', '')
  }

  /** The model picked, as its harness lists it; none while none is. */
  model() {
    return this.#answer?.listing.models.find((model) => modelOf(model) === this.#model.value)
  }

  /** Whether the model select has anything to pick: a harness whose models could not be read says why already. */
  get open() {
    return !this.#model.disabled
  }

  /** Whether the select holds the member's own model, no list having come to pick from. */
  get keeping() {
    return this.#kept
  }

  /** The id of the read whose list the select shows; none where it shows none (no list came). */
  get read() {
    return this.#answer?.read ?? null
  }

  /**
   * `kind`'s models, as the read made as the dialog opened answers, or read again
   * (`refresh`, the human's Refresh); `wanted` is picked where it is listed (`{model,
   * effort}`'s model's name). Where no list comes (the harness could not be read, or lists
   * none) the select says why and holds no old list; a member's own model (`current`, its
   * `{model, effort}`) stays in it as the member's value, so that its form loses nothing.
   * Answers whether the models came: a later pick of another harness wins.
   */
  async show(kind, { refresh = false, wanted = null, current = null } = {}) {
    this.#harness.value = kind
    const name = HARNESS_NAMES[kind]
    this.#asked += 1
    const asked = this.#asked
    this.#current = current
    this.#none(`Reading ${name}'s models…`, '')
    let answer
    try {
      const harness = harnessOf(kind)
      answer = await (refresh ? this.#lists.refresh(harness, this) : this.#lists.get(harness))
    } catch (cause) {
      if (asked !== this.#asked) return false
      this.#unlisted(
        `${name}'s models could not be read: ${cause instanceof Error ? cause.message : String(cause)}`,
        current,
      )
      return false
    }
    if (asked !== this.#asked) return false
    const stale = staleWhy(answer.listing)
    if (answer.listing.models.length === 0) {
      this.#unlisted(`${name} lists no model${stale ? `: ${stale}` : ''}.`, current)
      return false
    }
    const ask = new Option('Pick a model', '')
    ask.disabled = true
    const options = answer.listing.models.map((model) => {
      const id = modelOf(model)
      // The text describes the model; the value stays the id the harness takes.
      const option = new Option(modelOptionText(model), id)
      // A model its harness lists and will not run is no pick, nor one taken off.
      option.disabled = model.disabled || this.#takenOff(kind, id)
      return option
    })
    this.#model.replaceChildren(ask, ...options)
    this.#model.value = ''
    this.#kept = false
    // The member's own stored model, kept in the select while its harness could not be read
    // (Devin's `swe-2-high`), is picked again as the family and effort the list gives it.
    let pick = wanted
    if (pick !== null && current !== null && pick === current.model) {
      const found = whereListed(answer.listing.models, current)
      if (found) pick = modelOf(found.model)
    }
    if (pick !== null && options.some((option) => option.value === pick && !option.disabled)) {
      this.#model.value = pick
    }
    this.#model.disabled = false
    this.#answer = answer
    this.#sayHelp()
    this.#changed()
    return true
  }

  /**
   * No list to pick from, and why: the select holds `current`, the member's own model, where
   * there is one (it stays the member's value, at the effort it had), else nothing to pick.
   */
  #unlisted(why, current) {
    if (!current) {
      this.#none('No model to pick', why)
      return
    }
    const option = new Option(current.model, current.model)
    this.#model.replaceChildren(option)
    this.#model.value = current.model
    this.#model.disabled = false
    this.#answer = {
      listing: {
        models: [
          {
            id: current.model,
            efforts: current.effort ? [current.effort] : [],
            launchIds: {},
            contributor: false,
            disabled: false,
          },
        ],
        freshness: { state: 'stale', why },
      },
    }
    this.#kept = true
    this.#say(`${why} ${current.model} stays as the model it runs.`)
    this.#changed()
  }

  /** No model to pick, and why. */
  #none(words, why) {
    this.#kept = false
    const ask = new Option(words, '')
    ask.disabled = true
    this.#model.replaceChildren(ask)
    this.#model.value = ''
    this.#model.disabled = true
    this.#answer = null
    this.#say(why)
    this.#changed()
  }

  /** A contributor model's warning, a stale list's reason, and the harness's own switch where it is said. */
  #sayHelp() {
    const answer = this.#answer
    const stale = answer ? staleWhy(answer.listing) : null
    this.#say(
      [
        this.model()?.contributor ? answer?.contributorWarning : null,
        stale ? `This list may be out of date: ${stale}.` : null,
        this.switchHelp ? answer?.switchHelp : null,
      ]
        .filter(Boolean)
        .join(' '),
    )
  }
}

/** Why a listing is stale, in its own words; none where it is the account's own. */
const staleWhy = (listing) => (listing.freshness?.state === 'stale' ? listing.freshness.why : null)

/**
 * A chief's harness, model and effort, as New project and Switch chief pick them (CHF-07,
 * HMD-04): a harness installed here, then one of the models that harness lists of itself
 * (`models.get`, the [`ModelPicker`] Add member shares), at one of the efforts it lists for
 * it, its default picked where it says one. Each is required where there is one to pick,
 * and nothing is picked to start with. A harness whose models cannot be read leaves the
 * model unpicked, and says why. Below,
 * the harness's own words for switching its model in the chief's window, which keeps
 * its conversation (`switchHelp`), and the warning beside a contributor model. Refresh
 * reads the picked harness's models again.
 */
class ChiefPicker {
  #picker
  #help
  #effort
  #lists
  /** The `{harness, model}` the chief is on: no switch. */
  #current = null

  constructor(form, lists) {
    this.#help = form.querySelector('.chief-help')
    this.#effort = form.elements.chiefEffort
    this.#lists = lists
    this.#picker = new ModelPicker({
      harness: form.elements.chiefHarness,
      model: form.elements.chiefModel,
      lists: this.#lists,
      say: (words) => this.#say(words),
      changed: () => fillEfforts(this.#effort, this.#picker?.model()),
      takenOff: (kind, id) => this.#current?.harness === kind && this.#current?.model === id,
      switchHelp: true,
    })
    form.elements.chiefRefresh?.addEventListener('click', () => {
      if (this.#picker.kind !== '') void this.#picker.show(this.#picker.kind, { refresh: true })
    })
  }

  /**
   * The harnesses installed here, none picked; with none it refuses, and says where to
   * get one. `current` is the chief's own harness and model, which no switch is to. The
   * dialog reads the harnesses' models as it opens ([`ModelLists`]).
   */
  fill(missing, current = null) {
    this.#current = current
    this.#picker.fill(missing)
  }

  /** The picked harness, model and effort (where the model lists any), as the daemon takes a chief. */
  values() {
    const model = this.#picker.model()
    const picked = { harness: this.#picker.kind, model: model ? modelOf(model) : '' }
    if (this.#effort.value !== '') picked.effort = this.#effort.value
    // The read the model was picked from: the daemon checks the pick against that list.
    if (this.#picker.read !== null) picked.read = this.#picker.read
    return picked
  }

  /**
   * Whether a harness, a model and its effort are picked; where not, it says what to pick.
   * A model select a harness left with nothing to pick is disabled, and so is no form's to
   * check.
   */
  picked() {
    const model = this.#picker.model()
    if (this.#picker.kind !== '' && model !== undefined) {
      if (model.efforts.length === 0 || this.#effort.value !== '') return true
      this.#say(`Pick the effort ${HARNESS_NAMES[this.#picker.kind]} runs ${modelOf(model)} at.`)
      return false
    }
    // A harness whose models could not be read says why already.
    if (this.#picker.kind === '') this.#say('Pick a harness for the chief.')
    else if (this.#picker.open) this.#say("Pick the chief's model.")
    return false
  }

  #say(words) {
    this.#help.textContent = words
    this.#help.hidden = words === ''
  }
}

/**
 * Add member and Edit member (T-1143), on the [`ModelPicker`] the chief's picker shares: a
 * harness installed here, a model that harness lists of itself and an effort it lists for that
 * model. The form is shut until a slot's Add, an Edit or a backup opens it, in sight, its harness
 * picked first; its dialog puts it in the slot's row it was opened from (`placed`, T-2341). A slot's Add adds a member in that slot: its role and tier are the slot's, shown
 * in the form's name and not asked (D8), and an image designer draws on Codex. Edit member moves
 * a member to another harness, model, effort or tier from its next window; its handle and roles
 * stay, and its tier picker says of each tier whose slot is in the way. Refresh reads the picked
 * harness's models again, and says what the staff's members run that it no longer lists.
 */
class MemberPicker {
  #fieldset
  #role
  #harness
  #effort
  #tier
  #refresh
  #submit
  #cancel
  #legend
  #help
  #picker
  #lists
  /** The member being edited, by handle: none while the form adds one. */
  editing = null
  /** What the member being edited is called, as the form's name says it. */
  editingName = null
  /**
   * The owner, whether this form is replacing its backup, and the backup it was opened on (`of`,
   * its handle, or none for an Add backup); null for a member.
   */
  backup = null
  /**
   * Which opening of the form this is: each opening and each reset makes a new one, so that an
   * answer to a save the person left behind acts on the form no more (T-2341, round 2).
   */
  generation = 0
  /** The slot the form adds a member in, `{ role, tier }`: none while it edits or is shut. */
  slot = null
  /** The name of the button that opened the form with the keyboard on it: it has the keyboard back at a Cancel. */
  #opener = null
  /** The harnesses installed here, as `harnesses.missing` said them last. */
  #missing = []
  /** Told of a refresh of `harness` (its kind): the staff's marks of gone models are drawn again. */
  #refreshed
  /** Told the form opened, before it comes into sight: its dialog puts it where it was opened from. */
  #placed

  /** What the add button says while the form adds a member. */
  #adds

  /** The member being edited, as its form was first drawn: its own `{model, effort}`, none else. */
  #current = null

  /**
   * What the form held once the member being edited was drawn in it: its harness, model and
   * effort. A save that leaves these as they are saves the member's own launch choice, with no
   * list asked for what did not change.
   */
  #origin = null

  /** The controls of a model, an effort and a tier, which an image designer is given none of. */
  #modelControls

  /** Whether the form adds an image designer: it draws on Codex's own model and has no tier. */
  get #designer() {
    return this.backup === null && isImageAgentRole(this.#role.value) && this.editing === null
  }

  /** The roles of the member being edited: none while the form adds one. */
  #editedRoles = null

  /**
   * Whether the member the form adds, in the role picked, or edits, in the roles it holds, takes
   * no tier: none of its roles has tiers (`takesNoTier`). It is asked none, and sends none.
   */
  get #untiered() {
    return this.backup !== null || takesNoTier(this.#editedRoles ?? [this.#role.value])
  }

  constructor(form, lists, refreshed, placed) {
    this.#fieldset = form.querySelector('.staff-add')
    this.#placed = placed
    this.#legend = this.#fieldset.querySelector('legend')
    this.#role = form.elements.role
    this.#harness = form.elements.memberHarness
    this.#effort = form.elements.memberEffort
    this.#tier = form.elements.memberTier
    this.#refresh = form.elements.memberRefresh
    // What an image designer has none of: Codex draws on its own model, at no effort, and a
    // design task has no tier (designer P2).
    this.#modelControls = [
      form.elements.memberModel.closest('label'),
      form.elements.memberEffort.closest('label'),
      form.elements.memberTier.closest('label'),
      form.elements.memberRefresh,
    ]
    this.#submit = this.#fieldset.querySelector('[name="memberAdd"]')
    this.#cancel = form.elements.memberCancel
    this.#help = this.#fieldset.querySelector('.member-help')
    this.#refreshed = refreshed
    this.#lists = lists
    this.#adds = this.#submit.textContent
    this.#picker = new ModelPicker({
      harness: form.elements.memberHarness,
      model: form.elements.memberModel,
      lists,
      say: (words) => this.#sayHelp(words),
      changed: () => fillEfforts(this.#effort, this.#picker?.model()),
    })
    this.#role.addEventListener('change', () => this.#fillHarnesses())
    this.#refresh.addEventListener('click', () => void this.refresh())
  }

  /**
   * The roles the form offers, as the daemon said them as the dialog opened: drawn once they are
   * read, and not before (the form is made as the page loads, before the daemon answered).
   */
  #fillRoles() {
    if (this.#role.options.length !== 0) return
    for (const role of roleIds()) {
      const option = element('option', null, roleLabel(role))
      option.value = role
      this.#role.append(option)
    }
  }

  /** Whether a slot's Add, an Edit or a backup has the form open. */
  get open() {
    return !this.#fieldset.hidden
  }

  /** The form itself, which its dialog places in the slot's row it was opened from. */
  get fieldset() {
    return this.#fieldset
  }

  /**
   * After a Cancel, the form shut: the keyboard back on the button that opened it, as the
   * dialog draws it now, where it had the keyboard when it did.
   */
  refocus(root) {
    const name = this.#opener
    this.#opener = null
    if (name === null) return
    const again = [...root.querySelectorAll('button')].find(
      (node) => (node.getAttribute('aria-label') ?? node.textContent) === name,
    )
    again?.focus()
  }

  /** The form's name, its legend. */
  #name(words) {
    this.#legend.textContent = words
  }

  /** The form shut, as it is until a slot's Add, an Edit or a backup opens it: nothing picked. */
  reset(missing) {
    this.generation += 1
    this.#fillRoles()
    this.#missing = missing
    this.editing = null
    this.backup = null
    this.slot = null
    this.#editedRoles = null
    this.#name('Add a member')
    this.#role.disabled = false
    this.#submit.textContent = this.#adds
    this.#fillTiers([])
    this.#current = null
    this.#origin = null
    this.say('')
    this.#fillHarnesses()
    this.#fieldset.hidden = true
  }

  /**
   * The form as a slot's Add opens it (D8): a new member in that slot, its role and tier the
   * slot's, shown in the form's name and not asked.
   */
  forSlot(role, tier) {
    this.reset(this.#missing)
    this.slot = { role, tier }
    this.#role.value = role
    this.#fillTiers(tier === null ? [] : [tier])
    this.#tier.value = tier ?? ''
    this.#name(`Add the ${slotName(role, tier)}`)
    this.#fillHarnesses()
    this.#reveal()
  }

  /**
   * The tiers the tier select offers, as the daemon's roles say them, most critical first; each
   * `blocked` names whose slot is in its way, and cannot be picked.
   */
  #fillTiers(tiers, blocked = new Map()) {
    const ask = new Option('Pick a tier', '')
    ask.disabled = true
    this.#tier.replaceChildren(
      ask,
      ...tiers.map((tier) => {
        const words = blocked.get(tier)
        const option = new Option(words ? `${tierLabel(tier)}: ${words}` : tierLabel(tier), tier)
        option.disabled = Boolean(words)
        return option
      }),
    )
    this.#tier.value = ''
  }

  /** Reuse the member's harness, model and effort picker for its backup. */
  async backupFor(owner, current = null) {
    this.reset(this.#missing)
    this.backup = {
      owner,
      changing: current !== null,
      of: current?.handle ?? null,
      ofName: current && (current.label ?? `@${current.handle}`),
    }
    if (current !== null) {
      await this.edit({ ...current, roles: ['worker'] })
      if (this.backup?.owner !== owner) return
    } else {
      this.#fillHarnesses(null)
    }
    this.#name(`${current ? 'Change' : 'Add'} backup for ${owner.label}`)
    this.#tier.closest('label').hidden = true
    this.#submit.textContent = current ? 'Save backup' : 'Add backup'
    this.#cancel.hidden = false
    this.#reveal()
  }

  /**
   * Brings the form into sight, its harness picked first, once a slot's Add or a member's part
   * fills it: placed first in the slot's row it was opened from, in a grid that can be longer
   * than the dialog.
   */
  #reveal() {
    // What opened the form, by its name: a redraw may draw it anew while the form is open.
    const by = document.activeElement
    if (by instanceof HTMLButtonElement && !this.#fieldset.contains(by)) {
      this.#opener = by.getAttribute('aria-label') ?? by.textContent
    }
    this.#fieldset.hidden = false
    this.#cancel.hidden = false
    this.#placed()
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches
    this.#fieldset.scrollIntoView({ block: 'nearest', behavior: still ? 'auto' : 'smooth' })
    this.#harness.focus({ preventScroll: true })
  }

  /**
   * The form as it edits `member` (a lane's participant, or a draft's member named by `label`):
   * its harness's models read, its model, effort and tier picked where its harness still lists
   * them. The tiers `blocked` names are offered with whose slot is in their way, and cannot be
   * picked.
   */
  async edit(member, blocked = new Map()) {
    this.generation += 1
    this.#fillRoles()
    this.editing = member.handle
    this.editingName = member.label ?? `@${member.handle}`
    this.slot = null
    this.#editedRoles = member.roles
    this.#current = member.model ? { model: member.model, effort: member.effort ?? null } : null
    this.#origin = null
    this.#fillTiers(tiersFor(member.roles), blocked)
    this.#name(`Edit ${member.label ?? `@${member.handle}`}`)
    this.#role.disabled = true
    this.#submit.textContent = 'Save member'
    this.#cancel.hidden = false
    this.say('')
    this.#reveal()
    this.#fillHarnesses(member.roles.some(isImageAgentRole) ? ['codex'] : null)
    const kind = member.harness ?? ''
    if (!(kind in HARNESS_NAMES) || this.#missing.includes(harnessOf(kind))) return
    let answer = null
    try {
      answer = await this.#lists.get(harnessOf(kind))
    } catch {
      // The picker says why when it reads them.
    }
    if (this.editing !== member.handle) return
    const found = answer && member.model ? whereListed(answer.listing.models, member) : null
    // Where its harness's list does not come, the member's own model stays in the form.
    const current = member.model ? { model: member.model, effort: member.effort ?? null } : null
    await this.#picker.show(kind, { wanted: found ? modelOf(found.model) : null, current })
    if (this.editing !== member.handle) return
    const effort = found?.effort ?? (this.#picker.keeping ? current?.effort : null)
    if ([...this.#effort.options].some((option) => option.value === effort)) {
      this.#effort.value = effort
    }
    this.#tier.value = member.tier ?? ''
    this.#origin = this.#held(kind)
  }

  /** The harness, model and effort the form holds now, as an edit compares them. */
  #held(kind) {
    const model = this.#picker.model()
    return {
      kind,
      model: model ? modelOf(model) : null,
      effort: this.#effort.value === '' ? null : this.#effort.value,
    }
  }

  /** The role picked. */
  get role() {
    return this.#role.value
  }

  /**
   * What the human picked, as `member.add` and `member.edit` take it; none where something
   * is left to pick, which it says.
   */
  values() {
    const kind = this.#picker.kind
    const model = this.#picker.model()
    const say = (words) => {
      this.say(words)
      return null
    }
    if (kind === '')
      return say(this.backup ? 'Pick a harness for the backup.' : 'Pick a harness for the member.')
    if (this.#designer) {
      this.say('')
      return { harness: kind, modelName: null }
    }
    if (model === undefined)
      return this.#picker.open
        ? say(this.backup ? "Pick the backup's model." : "Pick the member's model.")
        : null
    if (model.efforts.length > 0 && this.#effort.value === '') {
      return say(`Pick the effort ${HARNESS_NAMES[kind]} runs ${modelOf(model)} at.`)
    }
    const tiered = !this.#untiered
    if (tiered && this.#tier.value === '') {
      return say("Pick the member's tier: the work the chief gives it, from critical to light.")
    }
    this.say('')
    const values = {
      harness: kind,
      model: modelOf(model),
      effort: this.#effort.value === '' ? null : this.#effort.value,
      // A member whose roles have no tier is given none: the daemon keeps it in the lowest.
      ...(tiered ? { tier: this.#tier.value } : {}),
      modelName: modelOf(model),
    }
    // The read the model was picked from: the daemon checks the pick against that list.
    if (this.#picker.read !== null) values.read = this.#picker.read
    // An edit that leaves the member's model and effort as they were saves its own, already
    // resolved launch choice: the daemon checks nothing that did not change. They were left
    // when the form holds what it drew, or the member's own stored choice by what it launches
    // (Devin's `swe-2` at `high` is the `swe-2-high` it stores, and a Refresh that fails
    // puts that id back in the form as it is).
    const origin = this.#origin
    const stored = this.#current
    if (
      this.editing !== null &&
      origin !== null &&
      origin.kind === kind &&
      ((origin.model === values.model && origin.effort === values.effort) ||
        (stored !== null &&
          launchIdOf(model, values.effort) === stored.model &&
          values.effort === (stored.effort ?? null)))
    ) {
      values.unchanged = true
    }
    return values
  }

  /** The human's Refresh: the picked harness's models read again from the harness itself. */
  async refresh() {
    const kind = this.#picker.kind
    if (kind === '') {
      this.say('Pick a harness to read its models again.')
      return
    }
    const wanted = this.#picker.model()
    const effort = this.#effort.value
    // A member being edited keeps its own model, effort and tier through a Refresh that fails.
    const current = this.editing !== null && kind === this.#origin?.kind ? this.#current : null
    const shown = await this.#picker.show(kind, {
      refresh: true,
      wanted: wanted ? modelOf(wanted) : null,
      current,
    })
    const kept = shown ? effort : this.#picker.keeping ? current?.effort : null
    if (kept && [...this.#effort.options].some((option) => option.value === kept)) {
      this.#effort.value = kept
    }
    if (shown) this.#refreshed(kind)
  }

  /** The words above the button: what is left to pick, or why the daemon said no. */
  say(words) {
    const hint = this.#fieldset.querySelector('.staff-hint')
    hint.textContent = words
    hint.hidden = words === ''
  }

  /**
   * The harnesses a member in the role picked may run on: Codex alone draws, and an image
   * designer is given Codex with no model, effort or tier to pick (designer P2).
   */
  #fillHarnesses(only = isImageAgentRole(this.#role.value) ? ['codex'] : null) {
    const designer = this.#designer
    for (const control of this.#modelControls) control.hidden = designer
    // A member whose roles have no tier is asked none, and a slot's Add none: its tier is the slot's.
    this.#tier.closest('label').hidden = designer || this.#untiered || this.slot !== null
    this.#picker.reads = !designer
    try {
      this.#picker.fill(this.#missing, only)
      if (designer) this.#picker.choose('codex')
      this.#sayHelp(
        designer
          ? "An image designer draws on Codex's own model: it takes no model, effort or tier."
          : '',
      )
      this.#submit.disabled = false
      this.say('')
    } catch (refused) {
      this.say(
        only?.includes('codex')
          ? 'Image designers draw on Codex, which is not installed here: install it from Agents, Harnesses.'
          : refused.message,
      )
      this.#submit.disabled = true
    }
  }

  #sayHelp(words) {
    this.#help.textContent = words
    this.#help.hidden = words === ''
  }
}

/**
 * New project, on the folder the human picked: its chief's harness, model and effort, and
 * the project's settings, the sections Project settings has (`ProjectSections`): the staff
 * (the last project's members, on the harnesses installed here, to start with), human
 * approval, wake-up calls and specs. The staff is drawn as Project settings draws it (D8):
 * every slot, from the draft, again after each Add, slot given, Remove or Edit, and a member's
 * own things in its part, opened from any of its slots (T-2341). A slot's Add picks a new member as Add member picks one
 * (`MemberPicker`): a harness, a model it lists and an effort, in the slot's role and tier. The
 * project starts with them all, in one request, once no slot holds several.
 */
export class NewProjectDialog {
  #dialog
  #form
  #slots
  #slotHead
  #refusal
  #sections
  #page
  #chief
  #member
  /** The dialog's reads of what each harness lists: its pickers share them. */
  #lists
  /** The harnesses not installed here, as the dialog last read them. */
  #missing = []
  /**
   * The draft: the members the new project starts with, the last staff's to start with, in the
   * order they came. Each is `{ _id, harness, model, effort, modelName, tier, roles, read,
   * designer, backupOf, vacationSince }`; a backup names its member's `_id` in `backupOf`, and
   * holds no slot: it takes its member's roles and tier.
   */
  #draft = []
  #nextId = 1
  /** Whether a start is on its way: a second ask waits for its answer, as the daemon's. */
  #starting = false
  /** The draft's member whose last role is asked about before it goes: `{ key, role }`, or none. */
  #removing = null
  /** Whether the words beside Start are a crowded slot's: they follow the slot as it is mended. */
  #crowdSaid = false
  /** The draft's member whose part is open, by its number, and the role of the slot it was opened from. */
  #opened = null
  /** Why a form was closed under the person, under the row it was open in: `{ role, words }`. */
  #closedWhy = null

  /**
   * `page.onStart(project)` is told of the project the daemon opened. `page.daemon` asks the
   * daemon, `page.act` runs a change and redraws the page. `page.specs()` is what the app
   * says of specs as the page last read it, which the Specs section says what following it
   * means by, and `page.readSpecs()` reads it again, as the dialog opens.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    // Start is the button's: Enter in no field of the dialog starts the project.
    holdEnter(this.#form)
    this.#slots = dialog.querySelector('#new-project-slots')
    this.#slotHead = dialog.querySelector('table.slots thead')
    this.#refusal = dialog.querySelector('.dialog-refusal')
    this.#sections = new ProjectSections(
      dialog.querySelector('[data-project-sections]'),
      null,
      page.specs,
      {
        daemon: page.daemon,
        act: page.act,
        report: page.report,
        // Before there is a project, the folder picked is what computer use is read for.
        scope: () => (dialog.open ? { directory: this.#form.elements.directory.value } : null),
      },
    )
    this.#page = page
    // One set of reads for the dialog: the chief's picker and the members' share them.
    const lists = new ModelLists(page.daemon)
    this.#lists = lists
    this.#chief = new ChiefPicker(this.#form, lists)
    this.#member = new MemberPicker(
      this.#form,
      lists,
      () => {},
      () => {
        this.#closedWhy = null
        this.#draw()
      },
    )
    // Computer use is shown for the harnesses the chief and the staff run on, as they are picked.
    this.#form.elements.chiefHarness.addEventListener('change', () => this.#drawComputerUse())
    this.#form.elements.memberAdd.addEventListener('click', () => this.#picked())
    this.#form.elements.memberCancel.addEventListener('click', () => {
      this.#member.reset(this.#missing)
      this.#draw()
      this.#member.refocus(dialog)
    })
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#start()
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /** Opens on `directory`; with no harness installed here it refuses, and says where to get one. */
  async open(directory) {
    const [{ missing = [], roles }, { staff }, , screen] = await Promise.all([
      this.#page.daemon('harnesses.missing'),
      this.#page.daemon('staff.last'),
      this.#page.readSpecs(),
      this.#sections.computerUse.read({ directory }),
    ])
    this.#missing = missing
    readRoles(roles)
    this.#sections.computerUse.show(screen)
    readEveryHarness(this.#lists, missing)
    this.#chief.fill(missing)
    this.#member.reset(missing)
    this.#removing = null
    this.#opened = null
    this.#closedWhy = null
    this.#pickLast(staff)
    this.#sections.reset()
    this.#say('')
    this.#form.elements.directory.value = directory
    this.#drawComputerUse()
    this.#dialog.showModal()
  }

  /** Computer use, drawn for the harnesses of the chief picked and of the members picked. */
  #drawComputerUse() {
    const kinds = [
      this.#form.elements.chiefHarness.value,
      ...this.#draft.map((member) => member.harness),
    ]
    this.#sections.computerUse.draw(new Set(kinds.filter(Boolean)))
  }

  /**
   * What is said above the buttons: the daemon's refusal of the start, until the next try or the
   * next opening; a crowded slot's words, until the slot is mended (`#draw`).
   */
  #say(words) {
    this.#refusal.textContent = words
    this.#refusal.hidden = words === ''
    this.#crowdSaid = false
  }

  /**
   * Starts the project as the dialog says it, in one request: its chief, staff
   * and approval, and the calls and specs unless they are as the dialog opened.
   * The dialog stays until the daemon has opened the project. A slot the draft names
   * several members for holds the start back, in its words (D8). One the daemon refuses (a
   * specs folder outside the project, an interval under a minute) is said here
   * in the daemon's own words, as the settings of a project say them, with
   * what the human chose still in the dialog, and no project is made.
   */
  async #start() {
    if (this.#starting || !this.#chief.picked()) return
    const crowd = this.#crowded()
    if (crowd !== null) {
      this.#say(crowd)
      this.#crowdSaid = true
      return
    }
    // A section holding what no project can start with says so, as its Save does in Project settings.
    const blocked = this.#sections.blocked()
    if (blocked !== null) {
      this.#say(blocked)
      return
    }
    this.#starting = true
    const form = this.#form
    const { gate, wakeUp, specs, rules, compaction, advisorTool, promptSuggestion, ownHands } =
      this.#sections.values()
    const body = {
      directory: form.elements.directory.value,
      ...this.#chief.values(),
      gate,
      staff: this.#pickedStaff(),
    }
    if (wakeUp !== null) body.wakeUp = wakeUp
    if (specs !== null) body.specs = specs
    if (rules !== null) body.rules = rules
    // Always said, null for the harness's own point: a request that says nothing starts on the default.
    body.compaction = compaction
    if (advisorTool) body.advisorTool = true
    if (promptSuggestion) body.promptSuggestion = true
    if (ownHands) body.ownHands = true
    this.#say('')
    let refused = null
    try {
      await this.#page.act(async () => {
        try {
          const { project } = await this.#page.daemon('project.open', body)
          this.#page.onStart(project)
          // Closed before the page is drawn for the project: its chief's terminal takes
          // the keyboard when its card is first shown, which a dialog over the page
          // would not let it.
          this.#dialog.close()
        } catch (cause) {
          // A dialog the human left meanwhile has no one to say it to: the status line does.
          if (!this.#dialog.open) throw cause
          refused = cause instanceof Error ? cause.message : String(cause)
        }
      })
    } finally {
      this.#starting = false
    }
    if (refused !== null) this.#say(refused)
  }

  /** The words of the slots the draft names several members for, or none. */
  #crowded() {
    const words = crowded(this.#members(), true).map((slot) => slot.words)
    return words.length === 0 ? null : words.join(' ')
  }

  /**
   * The last project's staff, as the draft: its members on a harness installed here, each with
   * its roles, and its backup with its own id, its link and its Vacation; an image designer runs
   * on its harness's own model at no effort and in no tier, and is picked as it is, its backup
   * too.
   */
  #pickLast(lastStaff) {
    this.#nextId = Math.max(0, ...lastStaff.map((member) => member.id ?? 0)) + 1
    const available = lastStaff.filter(
      (member) =>
        (member.model || member.designer) && !this.#missing.includes(harnessOf(member.harness)),
    )
    const owners = new Set(
      available.filter((member) => member.backupOf == null).map((member) => member.id),
    )
    this.#draft = available
      .filter((member) => member.backupOf == null || owners.has(member.backupOf))
      .map((member) => {
        const drawn = {
          _id: member.id ?? this.#nextId++,
          harness: member.harness,
          roles: member.backupOf == null ? [...member.roles] : member.roles.slice(0, 1),
          designer: Boolean(member.designer),
          backupOf: member.backupOf,
          vacationSince: member.vacationSince,
        }
        // An image designer runs on its harness's own model, at no effort and in no tier.
        if (member.designer) return drawn
        return {
          ...drawn,
          // The model as its harness's list names it (`modelName`): what the daemon takes for
          // a pick. Devin's `model` is the launch id for the effort, which no list holds.
          model: member.modelName || member.model,
          effort: member.effort,
          // A member whose roles have no tier is picked with none.
          ...(takesNoTier(member.roles) ? {} : { tier: member.tier }),
          modelName: member.modelName,
        }
      })
    this.#draw()
  }

  /** The draft's member of number `id` that is no backup. */
  #memberOf(id) {
    return this.#draft.find((member) => member._id === id && member.backupOf == null)
  }

  /** The draft's members that hold slots, as the slots read them, each with its backup's name. */
  #members() {
    return this.#draft
      .filter((member) => member.backupOf == null)
      .map((member) => {
        const designer = member.designer || member.roles.some(isImageAgentRole)
        const backup = this.#backupOf(member._id)
        return {
          key: member._id,
          name: modelShown(member),
          runs: [member.harness && harnessLabel(member), member.effort].filter(Boolean).join(' · '),
          roles: member.roles,
          tier: member.tier ?? null,
          designer,
          marks: member.vacationSince ? [{ text: 'on Vacation', tone: 'rest' }] : [],
          backup: backup ? modelShown(backup) : null,
          // A draft's member can only be brought back from a Vacation it was copied with.
          opens: opensSaid({
            edit: !designer,
            vacation: Boolean(member.vacationSince),
            backup: !designer || Boolean(backup),
          }),
        }
      })
  }

  /** The draft's backup of its member numbered `id`, or none. */
  #backupOf(id) {
    return this.#draft.find((member) => member.backupOf === id)
  }

  /**
   * What the form picked, put in the draft: a backup for its member, a member's own edit, or a
   * new member in the slot the form is for, refused in the ledger's words where the draft holds
   * that slot by now.
   */
  #picked() {
    const picked = this.#member.values()
    if (picked === null) return
    const { backup, editing, slot } = this.#member
    if (backup !== null) {
      const owner = this.#memberOf(backup.owner.key)
      if (owner) {
        const old = this.#draft.find((member) => member.backupOf === owner._id)
        this.#draft = this.#draft.filter((member) => member.backupOf !== owner._id)
        this.#draft.push({
          ...picked,
          _id: old?._id ?? this.#nextId++,
          backupOf: owner._id,
          roles: owner.roles.slice(0, 1),
          designer: false,
        })
      }
    } else if (editing !== null) {
      const member = this.#memberOf(Number(editing))
      if (member) {
        const { tier, unchanged, ...runs } = picked
        Object.assign(member, runs)
        if (tier !== undefined) member.tier = tier
      }
    } else if (slot !== null) {
      const holder = holdersOf(this.#members(), slot.role, slot.tier)[0]
      if (holder) {
        this.#member.say(takenWords(slot.role, slot.tier, holder.name))
        return
      }
      const designer = isImageAgentRole(slot.role)
      this.#draft.push(
        designer
          ? { harness: picked.harness, _id: this.#nextId++, roles: [slot.role], designer }
          : { ...picked, _id: this.#nextId++, roles: [slot.role], designer },
      )
    }
    this.#member.reset(this.#missing)
    // A refusal of the start goes with the change; a crowded slot's words follow the slot (`#draw`).
    if (!this.#crowdSaid) this.#say('')
    this.#draw()
    this.#member.refocus(this.#dialog)
  }

  /** Takes `role` from the draft's member `key`; with none left it goes, its backup with it. */
  #removeRole(key, role) {
    const member = this.#memberOf(key)
    if (!member) return
    // Its last role takes the member and its backup out of the draft: asked first, in the slot,
    // as Project settings asks.
    if (member.roles.length === 1 && member.roles[0] === role) {
      this.#removing = { key, role }
      this.#draw()
      return
    }
    member.roles = member.roles.filter((held) => held !== role)
    this.#draw()
  }

  /** The draft's member `key` out, its backup with it, once the person said so. */
  #removeMember(key) {
    this.#removing = null
    this.#draft = this.#draft.filter((kept) => kept._id !== key && kept.backupOf !== key)
    this.#draw()
  }

  /** What the ask before a draft member's last role says: that its backup goes with it, where it has one. */
  #removeWords(holder) {
    const backup = this.#draft.some((member) => member.backupOf === holder.key)
    return backup
      ? `Remove ${holder.name} and its backup from the new project's staff?`
      : `Remove ${holder.name} from the new project's staff?`
  }

  #draw() {
    const members = this.#members()
    const closing = this.#formGone(members)
    const slot = this.#member.slot
    // A part whose member left the draft is shut; one whose slot went is drawn, and shut, at the
    // slot it is drawn under.
    const anchor = anchorOf(members, this.#opened)
    this.#opened = anchor && { key: anchor.member.key, role: anchor.role }
    this.#slotHead.replaceChildren(slotHead())
    drawSlots(
      this.#slots,
      slotRows(members, {
        draft: true,
        id: 'new-project',
        picking:
          slot === null ? null : slot.tier === null ? slot.role : `${slot.role}:${slot.tier}`,
        add: (role, tier) => {
          this.#opened = null
          this.#member.forSlot(role, tier)
          this.#draw()
        },
        give: (role, holder) => {
          const member = this.#memberOf(holder.key)
          if (member && !member.roles.includes(role)) member.roles.push(role)
          this.#draw()
        },
        remove: (role, holder) => this.#removeRole(holder.key, role),
        confirming: this.#removing,
        keep: () => {
          this.#removing = null
          this.#draw()
        },
        confirm: (holder) => this.#removeMember(holder.key),
        confirmWords: (holder) => this.#removeWords(holder),
        opened: this.#opened,
        gone: this.#closedWhy,
        formHere: this.#member.open && slot === null,
        open: (role, holder) => this.#open(role, holder.key),
        shut: () => this.#shut(),
        part: (holder) => this.#part(holder, members),
      }),
      this.#member.fieldset,
    )
    if (closing) this.#slots.querySelector('tr.slot-form-gone td')?.focus()
    // A crowded slot's words beside Start follow the slot: the slot as it is now, or none.
    if (this.#crowdSaid) {
      const crowd = this.#crowded()
      this.#say(crowd ?? '')
      this.#crowdSaid = crowd !== null
    }
    this.#drawComputerUse()
  }

  /**
   * The form shut where what it was opened for is gone or changed (T-2341, round 2): the member
   * its Edit is for left the draft, the empty slot of its Add is held, or the backup its Change
   * was opened on is not its member's now. The line under the row it was open in says why, and
   * nothing was saved. Says whether the form had the keyboard, which the line takes.
   */
  #formGone(members) {
    const form = this.#member
    if (!form.open) return false
    const why = formGoneWords(form, {
      holder: (role, tier) => holdersOf(members, role, tier)[0],
      member: (key) => members.find((member) => String(member.key) === String(key)),
      backup: (key) => {
        const backup = this.#backupOf(Number(key))
        return backup ? String(backup._id) : null
      },
      backupName: (key) => modelShown(this.#draft.find((one) => String(one._id) === key) ?? {}),
    })
    if (why === null) return false
    const had = form.fieldset.contains(document.activeElement)
    this.#closedWhy = { role: form.slot?.role ?? this.#opened?.role ?? NO_SLOT, words: why }
    form.reset(this.#missing)
    return had
  }

  /** The part of the draft's member `key`, opened from its slot of `role`; open there already, it shuts. */
  #open(role, key) {
    if (this.#opened?.key === key && this.#opened.role === role) {
      this.#shut()
      return
    }
    // The form open for another member, or for a slot's Add, is left as a Cancel leaves it.
    if (this.#member.open) this.#member.reset(this.#missing)
    this.#opened = { key, role }
    this.#draw()
    showPart(this.#slots)
  }

  /** The open part shut, the form in it with it, and the keyboard back on its card. */
  #shut() {
    const back = backToCard(this.#slots)
    if (this.#member.open && this.#member.slot === null) this.#member.reset(this.#missing)
    this.#opened = null
    this.#draw()
    back()
  }

  /**
   * A draft member's own part: what it runs on and its state, its Edit and the end of a Vacation
   * it was copied with, and its backup, with that backup's own.
   */
  #part(holder, members) {
    const member = this.#memberOf(holder.key)
    const id = member._id
    const name = holder.name
    const designer = holder.designer
    const acts = []
    if (!designer) {
      acts.push(
        button(
          'Edit',
          'quiet-button',
          () => {
            const shown = members.find((one) => one.key === id)
            void this.#member.edit(
              { ...member, handle: String(id), label: name },
              shown ? inTheWay(members, shown) : new Map(),
            )
            this.#draw()
          },
          `Edit ${name}`,
        ),
      )
    }
    if (member.vacationSince) {
      acts.push(
        vacationButton(name, true, () => {
          member.vacationSince = null
          this.#draw()
        }),
      )
    }
    const nodes = [aboutLines(runsLabel(member, false), { resting: Boolean(member.vacationSince) })]
    if (acts.length > 0) nodes.push(actsOf(acts))
    const backup = this.#backupOf(id)
    // An image designer is offered no backup: one it has from before is shown, to keep or remove.
    if (designer && !backup) return nodes
    const owner = { key: id, label: name }
    if (!backup) {
      nodes.push(
        backupPart(name, null, null, [
          addBackup(name, () => {
            void this.#member.backupFor(owner)
            this.#draw()
          }),
        ]),
      )
      return nodes
    }
    const shown = modelShown(backup)
    const backupActs = designer
      ? []
      : [
          changeBackup(name, () => {
            void this.#member.backupFor(owner, {
              ...backup,
              handle: String(backup._id),
              label: modelShown(backup),
            })
            this.#draw()
          }),
        ]
    if (backup.vacationSince) {
      backupActs.push(
        // Said as the backup's: its member may run on the same model, and its Bring back is apart.
        vacationButton(`backup ${shown}`, true, () => {
          backup.vacationSince = null
          this.#draw()
        }),
      )
    }
    backupActs.push(
      removeBackup(name, () => {
        this.#draft = this.#draft.filter((kept) => kept.backupOf !== id)
        this.#draw()
      }),
    )
    const about = aboutLines(
      runsLabel({ ...backup, roles: member.roles, tier: member.tier }, false),
      { resting: Boolean(backup.vacationSince) },
    )
    const part = backupPart(name, shown, about, backupActs)
    part.dataset.model = backup.model ?? ''
    nodes.push(part)
    return nodes
  }

  /** The draft as the daemon takes a staff: each member once, with its roles, its backups after their link. */
  #pickedStaff() {
    const owners = new Set(
      this.#draft.filter((member) => member.backupOf != null).map((member) => member.backupOf),
    )
    return this.#draft.map((member) => {
      const { harness, model, effort, read, _id, backupOf, vacationSince } = member
      // A backup takes its member's tier.
      const tier = backupOf == null ? member.tier : this.#memberOf(backupOf)?.tier
      const entry = {
        harness,
        model,
        effort,
        ...(tier !== undefined ? { tier } : {}),
        roles: [...member.roles],
        ...(owners.has(_id) || backupOf != null ? { id: _id } : {}),
        ...(backupOf != null ? { backupOf } : {}),
        ...(vacationSince ? { vacationSince } : {}),
      }
      // The read the pick was made from (a pick copied from the last staff was made from none:
      // it is checked against the read the dialog shows of its harness, else the daemon reads).
      const named = read ?? (model ? this.#lists.readOf(harnessOf(harness)) : null)
      if (named !== null) entry.read = named
      return entry
    })
  }
}

/**
 * Switch the chief: the chief goes on in a new window on another harness, model and
 * effort, and the daemon hands the work over to it (the dispatcher's switchChief).
 */
export class SwitchChiefDialog {
  #dialog
  #form
  #page
  #chief
  /** The dialog's reads of what each harness lists. */
  #lists
  /** The project whose chief the dialog switches: the one it was opened for. */
  #project = null

  /**
   * `page.selected()` is the project the human has chosen now, `page.board()`
   * its board, and `page.onSwitch()` is told once the daemon took the switch. `page.daemon`
   * asks the daemon, `page.act` runs a change and redraws the page.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#page = page
    this.#lists = new ModelLists(page.daemon)
    this.#chief = new ChiefPicker(this.#form, this.#lists)
    const form = this.#form
    form.addEventListener('submit', (event) => {
      event.preventDefault()
      if (!this.#chief.picked()) return
      const chief = this.#chief.values()
      const when = form.elements.when.value
      const askFirst = form.elements.note.checked
      const project = this.#project
      dialog.close()
      void page.act(async () => {
        await page.daemon('chief.switch', { project, ...chief, when, note: askFirst })
        page.onSwitch()
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /**
   * Opens for `chief`'s project: every harness installed here, none picked yet, and the
   * models of the one picked; the harness and model the chief is on are no switch, and
   * another model of its harness is a fresh conversation with a handoff. With no harness
   * installed it refuses, and says where to get one.
   */
  async open(chief) {
    const { missing } = await this.#page.daemon('harnesses.missing')
    // Asked for in a project the human has left since: it stays shut.
    if (this.#page.selected() !== chief.projectId) return
    const form = this.#form
    readEveryHarness(this.#lists, missing)
    this.#chief.fill(missing, { harness: chief.harness, model: chief.ranOn ?? chief.model })
    // Each switch starts from the gentle one: the chief finishes its turn, unasked.
    form.elements.when.value = 'turn'
    form.elements.note.checked = false
    this.#project = chief.projectId
    this.#dialog.showModal()
  }
}

/**
 * Project settings: the project's staff (who the chief may hand work to, drawn
 * from the project's board, and drawn again in place while open, so it stays
 * current), the rules for the staff (`StaffRules`), the size its members' windows
 * compact at, whatever their harness (`Compaction`), whether its Claude Code members run
 * with Claude Code's own advisor tool (`AdvisorTool`), whether its Claude Code windows make
 * Claude Code's prompt suggestion (`PromptSuggestion`), how the chief works (`OwnHands`), and the sections New project has too
 * (`ProjectSections`): human approval, wake-up calls and specs. Each saves as it
 * always did: the staff as it is added to or removed from, the approval as it is
 * ticked, the rules, the compaction, the advisor tool, the prompt suggestion, own hands, the calls and the
 * specs by their own Save.
 *
 * The dialog is for one project, the one it is asked for, from then until it
 * closes. Everything it shows is that project's and every change it makes goes
 * to it, by its id, whichever project the page shows meanwhile. It opens only
 * while the human is still on that project, and it takes the board of no other.
 */
export class ProjectSettingsDialog {
  #dialog
  #form
  #slots
  #slotHead
  #gate
  #page
  /** The project's wake-up calls, read when the dialog opens and saved as a whole. */
  #wakeUp
  /** What the project says of specs: its own section of the dialog. */
  #specs
  /** The rules for the staff, shown as the board has them and saved as a whole. */
  #rules
  /** The size the members' windows compact at, whatever their harness, shown as the board has it. */
  #compaction
  #advisorTool
  #promptSuggestion
  #ownHands
  /** Computer use: a part for each of the project's harnesses that has one. */
  #computerUse
  /** The project the dialog was last opened for, by id: the one it is for until it closes. */
  #project = null
  /** The board of that project, as the dialog last drew it. */
  #board = null
  /**
   * The removal that waits for the human's yes, kept across redraws: a member's last role, by
   * its handle and that role's slot (`{ key, role }`).
   */
  #removing = null
  /**
   * The member whose part is open, by its id (an Edit may rename it), and the role of the slot it
   * was opened from (T-2341).
   */
  #opened = null
  /** Why a form was closed under the person, under the row it was open in: `{ role, words }`. */
  #closedWhy = null
  /**
   * The opening of the member form whose save is on its way (its `generation`): a redraw before
   * its answer is the person's own change, which shuts no form.
   */
  #saving = null
  /**
   * What the daemon refused of a slot the person changed (a give, a Remove), said under its
   * role's row until the next change of a slot (`{ role, words }`): the status line is behind
   * the dialog.
   */
  #slotRefusal = null
  /** Refusals beside the member whose Vacation button was pressed. */
  #vacationErrors = new Map()
  /** Refusals of a Remove backup, by the member it was pressed for: said beside it, in its part. */
  #backupErrors = new Map()
  /** Each harness's models, read once while the dialog is open (T-1143). */
  #lists
  /** Add member and Edit member. */
  #member
  /** The harnesses not installed here, as the dialog last read them. */
  #missing = []
  /**
   * The models of the staff's harnesses that the harness no longer lists, by kind: what a
   * member on one of them is marked by. A harness whose list could not be read marks none.
   */
  #gone = new Map()

  /**
   * `page.board()` is the board shown, `page.selected()` the project the human
   * has chosen and `page.missing()` the harnesses not installed here, all as they are now;
   * `page.daemon` asks the daemon, `page.act` runs a change and redraws the
   * page, `page.note` tells the human what came of it and `page.report` says
   * a refusal. `page.specs()` is what the app says of specs as the page last
   * read it, which the Specs section says what following it means by, and
   * `page.readSpecs()` reads it again, as the dialog opens.
   */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#form = dialog.querySelector('form')
    this.#slots = dialog.querySelector('#staff-slots')
    this.#slotHead = dialog.querySelector('table.slots thead')
    this.#page = page
    // The sections save for the project the dialog is for, not for the one the page shows, and a
    // dialog that is shut is for none: what is answered after it is the status line's.
    const sections = new ProjectSections(
      dialog.querySelector('[data-project-sections]'),
      {
        project: () => (dialog.open ? this.#project : null),
        daemon: page.daemon,
        act: page.act,
        note: page.note,
        report: page.report,
        chiefRunning: () => this.#chiefRunning(),
      },
      page.specs,
      {
        daemon: page.daemon,
        act: page.act,
        report: page.report,
        scope: () => (dialog.open ? { project: this.#project } : null),
      },
    )
    this.#gate = sections.gate
    this.#wakeUp = sections.wakeUp
    this.#specs = sections.specs
    this.#rules = sections.rules
    this.#compaction = sections.compaction
    this.#advisorTool = sections.advisorTool
    this.#promptSuggestion = sections.promptSuggestion
    this.#ownHands = sections.ownHands
    this.#computerUse = sections.computerUse
    // What the dialog changes is its project's, whichever board the page shows now.
    this.#gate.addEventListener('change', () => {
      const project = this.#project
      return page.act(async () => {
        await page.daemon('project.gate', { project, gate: this.#gate.checked })
        page.note(
          this.#gate.checked
            ? 'Every message between agents now waits for your approval.'
            : 'Messages between agents go straight through again.',
        )
      })
    })
    this.#lists = new ModelLists(page.daemon)
    this.#member = new MemberPicker(
      this.#form,
      this.#lists,
      (kind) => {
        void this.#markGone([kind])
      },
      () => {
        this.#closedWhy = null
        this.#draw()
      },
    )
    this.#form.elements.memberCancel.addEventListener('click', () => {
      this.#member.reset(this.#missing)
      this.#draw()
      this.#member.refocus(dialog)
    })
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      // Enter in a field of another section adds nobody: the form is shut until a slot opens it.
      if (!this.#member.open) return
      const values = this.#member.values()
      if (values === null) return
      // The name the harness gives the model is the daemon's to read from its listing.
      const { modelName, ...picked } = values
      const project = this.#project
      const editing = this.#member.editing
      const backup = this.#member.backup
      const slot = this.#member.slot
      // Its answer is this opening's: one that comes once the person has left the form, or opened
      // it for another, shuts nothing and says nothing there (the status line says a refusal).
      const asked = this.#member.generation
      const mine = () => this.#member.generation === asked
      this.#saving = asked
      const after = page.act(async () => {
        try {
          if (backup !== null) {
            const { member } = await page.daemon(backup.changing ? 'backup.change' : 'backup.add', {
              project,
              member: backup.owner.key,
              ...picked,
            })
            page.note(`${backup.owner.label}'s backup is @${member.handle}.`)
            if (mine()) this.#member.reset(this.#missing)
            return
          }
          if (editing !== null) {
            const { member } = await page.daemon('member.edit', {
              project,
              member: editing,
              ...picked,
            })
            // The name holds the model (HMD-12): a member edited to another one is renamed.
            page.note(
              member.handle === editing
                ? `@${editing} runs ${modelName} from its next window.`
                : `@${editing} is @${member.handle} now, and runs ${modelName} from its next window.`,
            )
            if (mine()) this.#member.reset(this.#missing)
            return
          }
          if (slot === null) return
          // A slot's Add always makes a new member (D8): the same model at a second tier is a
          // second member, and a member is given a role by its slot's give, never by an Add.
          const { member } = await page.daemon('member.add', {
            project,
            ...picked,
            roles: [slot.role],
          })
          page.note(`@${member.handle} joined the staff as ${roleLabel(slot.role)}.`)
          if (mine()) this.#member.reset(this.#missing)
        } catch (refused) {
          // The daemon's words go where the human picked, as well as on the status line.
          if (mine()) this.#member.say(refused instanceof Error ? refused.message : String(refused))
          throw refused
        }
      })
      // Saved, the keyboard is back on what opened the form, in the member's part.
      void Promise.resolve(after).finally(() => {
        if (this.#saving === asked) this.#saving = null
        if (dialog.open && !this.#member.open) this.#member.refocus(dialog)
      })
    })
    dialog.querySelector('[value="cancel"]').addEventListener('click', () => dialog.close())
  }

  /**
   * Whether the chief of the project the dialog is for has a window open now, as the board
   * the page shows says it where that board is the project's, else as the dialog's own board
   * did when it opened. A chief with a window open is told of a change of its specs or of the
   * rules for the staff as soon as it is at rest; one without has it in the launch that opens
   * its window.
   */
  #chiefRunning() {
    const shown = this.#page.board()
    const board = shown?.project.id === this.#project ? shown : this.#board
    return (board?.lanes ?? []).some(
      (lane) => lane.participant.role === 'chief' && lane.pane !== null,
    )
  }

  /**
   * Opens for `project`, the one the human asked it for, with no removal waiting from
   * the last time: its wake-up calls as the daemon has them, and its staff, approval and
   * specs as its board says. It stays shut when, by the time the daemon has answered, the
   * human has gone on (another project is chosen, or the board shown is another's) or a
   * dialog is up already: what was asked for is not what the human is after any more.
   */
  async open(project) {
    const [calls, , computerUse] = await Promise.all([
      this.#wakeUp.read(project),
      this.#page.readSpecs(),
      this.#computerUse.read({ project }),
    ])
    const board = this.#page.board()
    if (this.#dialog.open || this.#page.selected() !== project || board?.project.id !== project) {
      return
    }
    const missing = this.#page.missing()
    this.#project = project
    this.#board = board
    this.#removing = null
    this.#opened = null
    this.#closedWhy = null
    this.#saving = null
    this.#slotRefusal = null
    this.#vacationErrors.clear()
    this.#backupErrors.clear()
    this.#missing = missing
    readEveryHarness(this.#lists, missing)
    this.#gone = new Map()
    this.#member.reset(missing)
    this.#wakeUp.show(calls)
    this.#rules.show(board.project)
    this.#compaction.show(board.project)
    this.#advisorTool.show(board.project)
    this.#promptSuggestion.show(board.project)
    this.#ownHands.show(board.project)
    this.#computerUse.show(computerUse)
    this.#draw()
    this.#specs.draw(board.project, true)
    this.#dialog.showModal()
    void this.#markGone(this.#staffHarnesses())
  }

  /** The harnesses the staff's members run on, by kind, each once. */
  #staffHarnesses() {
    const kinds = this.#board.lanes
      .map((lane) => lane.participant)
      .filter(isMember)
      .map((member) => member.harness)
      .filter((kind) => kind in HARNESS_NAMES && !this.#missing.includes(harnessOf(kind)))
    return [...new Set(kinds)]
  }

  /**
   * Reads what each of `kinds` lists (once while the dialog is open, or as Refresh read it),
   * and marks the members whose model it no longer lists. A harness whose list could not be
   * read, or lists nothing, marks nobody: what it lists is not known.
   */
  async #markGone(kinds) {
    const project = this.#project
    await Promise.all(
      kinds.map(async (kind) => {
        let answer
        try {
          answer = await this.#lists.get(harnessOf(kind))
        } catch {
          this.#gone.delete(kind)
          return
        }
        this.#gone.set(kind, answer.listing.models.length === 0 ? null : answer.listing.models)
      }),
    )
    if (this.#dialog.open && this.#project === project) this.#draw()
  }

  /**
   * Whether `member`'s harness, as the dialog read it, no longer lists the model it runs; a
   * member on its harness's default runs no model of the list's.
   */
  #modelGone(member) {
    const models = this.#gone.get(member.harness)
    if (!models || !member.model) return false
    return whereListed(models, member) === null
  }

  /**
   * Draws the staff again while the dialog is open, from the board of its project; the
   * board of another project is not its own, whenever it comes. A closed dialog is drawn
   * when it opens.
   */
  render() {
    const board = this.#page.board()
    if (!this.#dialog.open || board?.project.id !== this.#project) return
    this.#board = board
    this.#ownHands.update(board.project)
    this.#draw()
  }

  /**
   * Draws the staff from the project's board, in place: every slot, and the part of the member
   * opened from one of them (T-2341).
   */
  #draw() {
    const board = this.#board
    // The members only: a member's sessions are lanes too, named after it.
    const members = board.lanes.map((lane) => lane.participant).filter(isMember)
    const primaries = members.filter((member) => member.backupOf == null)
    const backups = new Map(
      members
        .filter((member) => member.backupOf != null)
        .map((member) => [member.backupOf, member]),
    )
    const holders = primaries.map((member) => this.#slotMember(member, backups.get(member.id)))
    const closing = this.#formGone(primaries, backups, holders)
    // A part whose member left the staff is shut; one renamed by an Edit stays open, and one whose
    // slot went is drawn, and shut, at the slot it is drawn under.
    const opened = primaries.find((member) => member.id === this.#opened?.id)
    const anchor = opened && anchorOf(holders, { key: opened.handle, role: this.#opened.role })
    this.#opened = anchor ? { id: opened.id, role: anchor.role } : null
    const slot = this.#member.slot
    this.#slotHead.replaceChildren(slotHead())
    drawSlots(
      this.#slots,
      slotRows(holders, {
        draft: false,
        id: 'staff',
        picking:
          slot === null ? null : slot.tier === null ? slot.role : `${slot.role}:${slot.tier}`,
        confirming: this.#removing,
        refusal: this.#slotRefusal,
        add: (role, tier) => {
          this.#opened = null
          this.#member.forSlot(role, tier)
          this.#draw()
        },
        give: (role, holder) => this.#give(role, holder.key),
        remove: (role, holder) => this.#removeRole(role, holder.key),
        keep: () => {
          this.#removing = null
          this.#draw()
        },
        confirm: (holder) => this.#removeMember(holder.key, this.#removing?.role),
        opened: this.#opened && { key: opened.handle, role: this.#opened.role },
        gone: this.#closedWhy,
        formHere: this.#member.open && slot === null,
        open: (role, holder) => this.#open(role, holder.key),
        shut: () => this.#shut(),
        part: (holder) => {
          const member = primaries.find((one) => one.handle === holder.key)
          return this.#part(member, backups.get(member.id))
        },
      }),
      this.#member.fieldset,
    )
    if (closing) this.#slots.querySelector('tr.slot-form-gone td')?.focus()
    this.#gate.checked = board.project.gate ?? false
    this.#specs.draw(board.project)
    this.#computerUse.draw(harnessesOf(board))
  }

  /**
   * The form shut where what it was opened for is gone or changed, by another window (T-2341,
   * round 2): the member its Edit is for left the staff or was renamed, the empty slot of its Add
   * is held, or the backup its Change was opened on is not its member's now. The line under the
   * row it was open in says why, and nothing was saved. The person's own save on its way shuts
   * nothing: what it changed is theirs. Says whether the form had the keyboard, which the line takes.
   */
  #formGone(primaries, backups, holders) {
    const form = this.#member
    if (!form.open || form.generation === this.#saving) return false
    const byHandle = (handle) => primaries.find((member) => member.handle === handle)
    const why = formGoneWords(form, {
      holder: (role, tier) => holdersOf(holders, role, tier)[0],
      member: byHandle,
      backup: (handle) => backups.get(byHandle(handle)?.id)?.handle ?? null,
      backupName: (handle) => `@${handle}`,
      renamed: (handle) => {
        const now = primaries.find((member) => member.id === this.#opened?.id)
        return now && now.handle !== handle
          ? `@${handle} is @${now.handle} now, so its Edit was closed. Nothing was saved.`
          : null
      },
    })
    if (why === null) return false
    const had = form.fieldset.contains(document.activeElement)
    this.#closedWhy = { role: form.slot?.role ?? this.#opened?.role ?? NO_SLOT, words: why }
    form.reset(this.#missing)
    return had
  }

  /** The part of the member `handle`, opened from its slot of `role`; open there already, it shuts. */
  #open(role, handle) {
    const { id } = this.#memberNow({ handle })
    if (this.#opened?.id === id && this.#opened.role === role) {
      this.#shut()
      return
    }
    // The form open for another member, or for a slot's Add, is left as a Cancel leaves it.
    if (this.#member.open) this.#member.reset(this.#missing)
    this.#opened = { id, role }
    this.#draw()
    showPart(this.#slots)
  }

  /** The open part shut, the form in it with it, and the keyboard back on its card. */
  #shut() {
    const back = backToCard(this.#slots)
    if (this.#member.open && this.#member.slot === null) this.#member.reset(this.#missing)
    this.#opened = null
    this.#draw()
    back()
  }

  /**
   * A member as its slots show it: its name, what it runs on where its name does not say, its
   * state in a few words (on Vacation, out, needs an edit), its backup's name, and what its part
   * holds.
   */
  #slotMember(member, backup) {
    const marks = []
    if (member.needsEdit) marks.push({ text: 'needs an edit', tone: 'out' })
    else if (this.#modelGone(member)) marks.push({ text: 'model no longer listed', tone: 'out' })
    if (member.vacationSince) marks.push({ text: 'on Vacation', tone: 'rest' })
    const lane = this.#board.lanes.find(
      (found) => found.participant.handle === member.handle && isMember(found.participant),
    )
    const out = lane ? outWords(lane, Date.now()) : null
    if (out !== null) marks.push({ text: out.split(/; |: /)[0], tone: 'out' })
    const designer = Boolean(member.designer)
    return {
      key: member.handle,
      name: `@${member.handle}`,
      runs: [
        ...(nameApart(member) ? [modelShown(member), member.effort] : []),
        harnessLabel(member),
      ]
        .filter(Boolean)
        .join(' · '),
      roles: member.roles,
      tier: takesNoTier(member.roles) ? null : member.tier,
      designer,
      marks,
      backup: backup ? `@${backup.handle}` : null,
      opens: opensSaid({ edit: !designer, vacation: true, backup: !designer || Boolean(backup) }),
    }
  }

  /** Gives the empty slot of `role` at its tier to the member `handle` (`member.roles`). */
  #give(role, handle) {
    const page = this.#page
    const { roles } = this.#memberNow({ handle })
    void this.#slotAct(role, async () => {
      await page.daemon('member.roles', {
        project: this.#project,
        agent: handle,
        roles: inRoleOrder([...roles, role]),
      })
      page.note(`@${handle} is ${roleLabel(role)} now too.`)
    })
  }

  /** The role's Remove in its slot: the member keeps its other roles; its last asks first. */
  #removeRole(role, handle) {
    const page = this.#page
    const { roles } = this.#memberNow({ handle })
    if (roles.length === 1) {
      this.#removing = { key: handle, role }
      this.#draw()
      return
    }
    void this.#slotAct(role, async () => {
      await page.daemon('member.roles', {
        project: this.#project,
        agent: handle,
        roles: roles.filter((held) => held !== role),
      })
    })
  }

  /** The member off the staff, once the human said yes: its open tasks are cancelled. */
  #removeMember(handle, role) {
    const page = this.#page
    return this.#slotAct(role, async () => {
      await page.daemon('member.remove', { project: this.#project, agent: handle })
      this.#removing = null
      page.note(`@${handle} left the staff.`)
    })
  }

  /**
   * A change of a slot (a give, a Remove), run as `page.act` runs it, the status line saying a
   * refusal; the dialog says it too, under the role's row, where the person acted, while it is
   * still open for the same project.
   */
  #slotAct(role, work) {
    const project = this.#project
    this.#slotRefusal = null
    return this.#page.act(async () => {
      try {
        await work()
      } catch (refused) {
        if (this.#dialog.open && this.#project === project) {
          const words = refused instanceof Error ? refused.message : String(refused)
          this.#slotRefusal = { role, words }
        }
        throw refused
      }
    })
  }

  /**
   * A member out of quota says so in its part, with the human's way to try it again now: the
   * quota may have been renewed early, by hand (T-1357). A member not out says nothing.
   */
  #sayOut(about, member) {
    const lane = this.#board.lanes.find(
      (found) => found.participant.handle === member.handle && isMember(found.participant),
    )
    const words = lane ? outWords(lane, Date.now()) : null
    if (words === null) return
    const page = this.#page
    const { participant } = lane
    const out = element('span', 'member-out-line')
    out.append(element('span', 'member-out', words))
    // A busy model is tried again after a back-off no press shortens (T-1357).
    if (!lane.busy) {
      out.append(
        tryAgain(participant, () =>
          page.act(async () => {
            await page.daemon('member.back', {
              project: this.#project,
              participant: participant.handle,
            })
            page.note(`Trying @${participant.handle} again: what waited for it goes on.`)
          }),
        ),
      )
    }
    about.append(out)
  }

  #vacation(member) {
    const project = this.#project
    const controls = document.createDocumentFragment()
    controls.append(
      vacationButton(
        `@${member.handle}`,
        Boolean(member.vacationSince),
        () =>
          void this.#page.act(async () => {
            try {
              await this.#page.daemon(
                member.vacationSince ? 'member.vacation_back' : 'member.vacation',
                {
                  project,
                  member: member.handle,
                },
              )
              this.#vacationErrors.delete(member.handle)
            } catch (refused) {
              if (this.#dialog.open && this.#project === project)
                this.#vacationErrors.set(
                  member.handle,
                  refused instanceof Error ? refused.message : String(refused),
                )
              throw refused
            }
          }),
      ),
    )
    const refused = this.#vacationErrors.get(member.handle)
    if (refused) {
      const hint = element('span', 'staff-hint', refused)
      hint.setAttribute('role', 'alert')
      controls.append(hint)
    }
    return controls
  }

  /** A member as the project's board has it now, which a Remove kept across redraws acts on. */
  #memberNow(member) {
    return (
      this.#board.lanes.find(
        (lane) => lane.participant.handle === member.handle && isMember(lane.participant),
      )?.participant ?? member
    )
  }

  /** What a member's part, or its backup's, says under its name: what it runs on and its state. */
  #about(member) {
    const gone = this.#modelGone(member)
    const about = aboutLines(runsLabel(member), {
      gone: !member.needsEdit && gone ? modelShown(member) : null,
      harness: HARNESS_NAMES[member.harness],
      needsEdit: Boolean(member.needsEdit),
      agent: member.agent,
      resting: Boolean(member.vacationSince),
    })
    this.#sayOut(about, member)
    return about
  }

  /**
   * A member's own part (T-2341), the same whichever of its slots opened it: what it runs on and
   * its state, Edit and its Vacation, and its backup with that backup's own. Each slot's Remove
   * is the slot's.
   */
  #part(member, backup) {
    const name = `@${member.handle}`
    // An image designer has nothing to edit: Codex's own model, no effort, no tier.
    const edit = member.designer
      ? []
      : [
          button(
            'Edit',
            'quiet-button',
            () => {
              const now = this.#memberNow(member)
              const members = this.#board.lanes
                .map((lane) => lane.participant)
                .filter((one) => isMember(one) && one.backupOf == null)
                .map((one) => this.#slotMember(one))
              const shown = members.find((one) => one.key === now.handle)
              void this.#member.edit(now, shown ? inTheWay(members, shown) : new Map())
              this.#draw()
            },
            `Edit ${name}`,
          ),
        ]
    const about = this.#about(member)
    if (this.#modelGone(member)) about.dataset.modelGone = member.model
    const nodes = [about, actsOf([...edit, this.#vacation(member)])]
    // An image designer is offered no backup: one it has from before is shown, to keep or remove.
    if (member.designer && !backup) return nodes
    const owner = { key: member.handle, label: name }
    if (!backup) {
      nodes.push(
        backupPart(name, null, null, [
          addBackup(name, () => {
            void this.#member.backupFor(owner)
            this.#draw()
          }),
        ]),
      )
      return nodes
    }
    // An image designer's backup (an earlier version's) has nothing to change: it is kept or removed.
    const change = member.designer
      ? []
      : [
          changeBackup(name, () => {
            void this.#member.backupFor(owner, this.#memberNow(backup))
            this.#draw()
          }),
        ]
    const project = this.#project
    const acts = [
      ...change,
      this.#vacation(backup),
      removeBackup(
        name,
        () =>
          void this.#page.act(async () => {
            try {
              await this.#page.daemon('backup.remove', { project, member: member.handle })
              this.#backupErrors.delete(member.handle)
              this.#page.note(`${name}'s backup left the staff.`)
            } catch (refused) {
              // Said beside it, in the dialog: the status line is behind it.
              if (this.#dialog.open && this.#project === project)
                this.#backupErrors.set(
                  member.handle,
                  refused instanceof Error ? refused.message : String(refused),
                )
              throw refused
            }
          }),
      ),
    ]
    const refused = this.#backupErrors.get(member.handle)
    if (refused) {
      const hint = element('span', 'staff-hint', refused)
      hint.setAttribute('role', 'alert')
      acts.push(hint)
    }
    const part = backupPart(name, `@${backup.handle}`, this.#about(backup), acts)
    part.dataset.handle = backup.handle
    nodes.push(part)
    return nodes
  }
}
