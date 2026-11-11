import { button, element, redraw } from '../dom.js'
import { HARNESS_NAMES, harnessLabel, isMember, modelShown, nameApart } from './board.js'
import { harnessesOf } from './computer-use.js'
import { holdEnter, ProjectSections } from './project-sections.js'
import { isImageAgentRole, readRoles, roleIds, roleLabel, takesNoTier, untiered } from './roles.js'

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

/** The work tiers, most critical first. */
const TIER_LABEL = {
  critical: 'Critical work',
  complex: 'Complex work',
  standard: 'Standard work',
  light: 'Light work',
}
const TIERS = Object.keys(TIER_LABEL)
const rank = (list, value) => (list.includes(value) ? list.indexOf(value) : list.length)
/** A staff reads by role, in the order the picker offers them, then by tier, the most critical first, then by name. */
const byRoleAndTier = (a, b) =>
  rank(roleIds(), a.role) - rank(roleIds(), b.role) ||
  rank(TIERS, a.tier) - rank(TIERS, b.tier) ||
  a.name.localeCompare(b.name)
/**
 * What a staff table says under a member's name: its tier and its harness, and what it runs on
 * (its model's id and its effort) only where its name does not say it (HMD-12); a pick of New
 * project has no name made yet, and says its harness, its effort and its tier.
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

/** One row of a staff table: the member, one of its roles, a Remove for that role, and an Edit of the member where it has one. */
function staffRow(who, role, remove, edit = null) {
  const row = element('tr')
  row.dataset.role = role
  row.append(who, element('td', null, roleLabel(role)))
  const tools = element('td')
  if (edit !== null) tools.append(edit)
  tools.append(remove)
  row.append(tools)
  return row
}

/** A staff table's first cell: who, and what it runs. */
function whoCell(name, runs) {
  const cell = element('td')
  cell.append(
    element('span', 'member-name', name),
    element('br'),
    element('span', 'member-meta', runs),
  )
  return cell
}

/**
 * A staff table's first cell for a member whose harness no longer lists its model (T-1143):
 * the model marked, and what is wrong in words.
 */
function goneCell(name, model, runs, harness) {
  const cell = element('td')
  const meta = element('span', 'member-meta')
  meta.append(element('mark', 'model-gone', model), ` · ${runs}`)
  cell.append(
    element('span', 'member-name', name),
    element('br'),
    meta,
    element('br'),
    element(
      'span',
      'member-gone',
      `${harness} no longer lists ${model}: edit it to a model it lists.`,
    ),
  )
  return cell
}

/**
 * A staff table's first cell for a member the update could not run on what it ran on
 * (T-1202): no model, and what to do in words.
 */
function needsEditCell(name, runs, agent) {
  const cell = whoCell(name, runs)
  cell.append(
    element('br'),
    element(
      'span',
      'member-gone',
      `The update could not tell what ${agent ?? 'its agent'} ran on: edit it to a model its harness lists, or remove it.`,
    ),
  )
  return cell
}

/** A staff table with nobody in it says so, and what to do, across its three columns. */
function nobodyRow(text) {
  const row = element('tr', 'staff-empty')
  const cell = element('td', null, text)
  cell.colSpan = 3
  row.append(cell)
  return row
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
      // The id, exactly as the harness takes it: a contributor model's says so itself
      // (`-contributor`), and the warning stands under the select once it is picked.
      const option = new Option(id, id)
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

/** The tiers a member's tier select offers, the most critical first, as the page names them. */
const TIER_PICK = Object.entries(TIER_LABEL)

/**
 * Add member and Edit member (T-1143): a role, a harness installed here, a model that
 * harness lists of itself, an effort it lists for that model and the tier the human
 * chooses, on the [`ModelPicker`] the chief's picker shares. The tier starts empty: the
 * human picks it, and nothing is saved without one (no bundled list says one,
 * harness-models). An image designer draws on Codex. Edit member moves a member to another harness,
 * model, effort or tier from its next window; its handle and roles stay. Refresh reads the
 * picked harness's models again, and says what the staff's members run that it no longer
 * lists.
 */
class MemberPicker {
  #fieldset
  #role
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
  /** The harnesses installed here, as `harnesses.missing` said them last. */
  #missing = []
  /** Told of a refresh of `harness` (its kind): the staff's marks of gone models are drawn again. */
  #refreshed

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
    return isImageAgentRole(this.#role.value) && this.editing === null
  }

  /** The roles of the member being edited: none while the form adds one. */
  #editedRoles = null

  /**
   * Whether the member the form adds, in the role picked, or edits, in the roles it holds, takes
   * no tier: none of its roles has tiers (`takesNoTier`). It is asked none, and sends none.
   */
  get #untiered() {
    return takesNoTier(this.#editedRoles ?? [this.#role.value])
  }

  constructor(form, lists, refreshed) {
    this.#fieldset = form.querySelector('.staff-add')
    this.#legend = this.#fieldset.querySelector('legend')
    this.#role = form.elements.role
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
    if (this.#tier.options.length === 0) {
      const ask = new Option('Pick a tier', '')
      ask.disabled = true
      this.#tier.append(ask, ...TIER_PICK.map(([tier, label]) => new Option(label, tier)))
    }
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

  /** The form as it adds a member: the harnesses installed here, nothing picked. */
  reset(missing) {
    this.#fillRoles()
    this.#missing = missing
    this.editing = null
    this.#editedRoles = null
    this.#legend.textContent = 'Add a member'
    this.#role.disabled = false
    this.#submit.textContent = this.#adds
    this.#cancel.hidden = true
    this.#tier.value = ''
    this.#current = null
    this.#origin = null
    this.say('')
    this.#fillHarnesses()
  }

  /**
   * The form as it edits `member` (a lane's participant): its harness's models read, its
   * model, effort and tier picked where its harness still lists them.
   */
  async edit(member) {
    this.#fillRoles()
    this.editing = member.handle
    this.#editedRoles = member.roles
    this.#current = member.model ? { model: member.model, effort: member.effort ?? null } : null
    this.#origin = null
    this.#legend.textContent = `Edit @${member.handle}`
    this.#role.disabled = true
    this.#submit.textContent = 'Save member'
    this.#cancel.hidden = false
    this.say('')
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
    if (kind === '') return say('Pick a harness for the member.')
    if (this.#designer) {
      this.say('')
      return { harness: kind, modelName: null }
    }
    if (model === undefined) return this.#picker.open ? say("Pick the member's model.") : null
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
    // A member whose roles have no tier is asked none.
    this.#tier.closest('label').hidden = designer || this.#untiered
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
 * approval, wake-up calls and specs. A member of the staff is picked as Add member picks one
 * (`MemberPicker`): a role, a harness, a model it lists, an effort and a tier. The project
 * starts with them all, in one request.
 */
export class NewProjectDialog {
  #dialog
  #form
  #staff
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
   * The members picked for the new project, one row per role: each `{harness, model, effort,
   * tier, modelName, role}`, the last staff's to start with.
   */
  #picked = []
  /** Whether a start is on its way: a second ask waits for its answer, as the daemon's. */
  #starting = false

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
    this.#staff = dialog.querySelector('#new-project-staff')
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
    this.#member = new MemberPicker(this.#form, lists, () => {})
    // Computer use is shown for the harnesses the chief and the staff run on, as they are picked.
    this.#form.elements.chiefHarness.addEventListener('change', () => this.#drawComputerUse())
    this.#form.elements.memberAdd.addEventListener('click', () => {
      const picked = this.#member.values()
      if (picked === null) return
      const role = this.#member.role
      const same = (pick) =>
        pick.harness === picked.harness &&
        pick.model === picked.model &&
        pick.effort === picked.effort
      if (this.#picked.some((pick) => same(pick) && pick.role === role)) {
        this.#member.say(`${modelShown(picked)} is on the staff as ${roleLabel(role)} already.`)
        return
      }
      // One member holds each of its roles in one tier: the tier picked now is the member's. A
      // role with no tier is picked with none, and leaves the member's as it is.
      if (picked.tier !== undefined) {
        for (const pick of this.#picked) if (same(pick)) pick.tier = picked.tier
      }
      this.#picked.push({ ...picked, role })
      this.#member.reset(this.#missing)
      this.#draw()
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
      ...this.#picked.map((pick) => pick.harness),
    ]
    this.#sections.computerUse.draw(new Set(kinds.filter(Boolean)))
  }

  /** What is said above the buttons: the daemon's refusal of the start, until the next try or the next opening. */
  #say(words) {
    this.#refusal.textContent = words
    this.#refusal.hidden = words === ''
  }

  /**
   * Starts the project as the dialog says it, in one request: its chief, staff
   * and approval, and the calls and specs unless they are as the dialog opened.
   * The dialog stays until the daemon has opened the project. One it refuses (a
   * specs folder outside the project, an interval under a minute) is said here
   * in the daemon's own words, as the settings of a project say them, with
   * what the human chose still in the dialog, and no project is made.
   */
  async #start() {
    if (this.#starting || !this.#chief.picked()) return
    // A section holding what no project can start with says so, as its Save does in Project settings.
    const blocked = this.#sections.blocked()
    if (blocked !== null) {
      this.#say(blocked)
      return
    }
    this.#starting = true
    const form = this.#form
    const { gate, wakeUp, specs, rules, compaction, advisorTool, promptSuggestion } =
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
    if (compaction !== null) body.compaction = compaction
    if (advisorTool) body.advisorTool = true
    if (promptSuggestion) body.promptSuggestion = true
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

  /**
   * The last project's staff, picked: its members on a harness installed here, in their
   * roles; an image designer runs on its harness's own model at no effort and in no tier,
   * and is picked as it is.
   */
  #pickLast(lastStaff) {
    this.#picked = lastStaff
      .filter(
        (member) =>
          (member.model || member.designer) && !this.#missing.includes(harnessOf(member.harness)),
      )
      .flatMap((member) =>
        member.roles.map((role) =>
          member.designer
            ? // An image designer runs on its harness's own model, at no effort and in no tier.
              { harness: member.harness, role }
            : {
                harness: member.harness,
                // The model as its harness's list names it (`modelName`): what the daemon takes for
                // a pick. Devin's `model` is the launch id for the effort, which no list holds.
                model: member.modelName || member.model,
                effort: member.effort,
                // A role with no tier is picked with none: the member's tier is its work's.
                ...(untiered(role) ? {} : { tier: member.tier }),
                modelName: member.modelName,
                role,
              },
        ),
      )
    this.#draw()
  }

  #draw() {
    const rows = this.#picked
      .map((pick) => ({ ...pick, name: modelShown(pick) }))
      .sort(byRoleAndTier)
      .map((pick) => {
        const remove = button(
          'Remove',
          'quiet-button',
          () => {
            this.#picked = this.#picked.filter(
              (kept) =>
                !(
                  kept.harness === pick.harness &&
                  kept.model === pick.model &&
                  kept.effort === pick.effort &&
                  kept.role === pick.role
                ),
            )
            this.#draw()
          },
          `Remove ${roleLabel(pick.role)} ${pick.name}`,
        )
        const row = staffRow(whoCell(pick.name, runsLabel(pick, false)), pick.role, remove)
        row.dataset.model = pick.model ?? ''
        return row
      })
    if (rows.length === 0) {
      rows.push(nobodyRow('Nobody yet: pick a role, a harness, a model and a tier, then Add.'))
    }
    this.#staff.replaceChildren(...rows)
    this.#drawComputerUse()
  }

  /** The picked rows as the daemon takes a staff: each member once, with its roles. */
  #pickedStaff() {
    const staff = new Map()
    for (const { harness, model, effort, tier, role, read } of this.#picked) {
      // An image designer holds no other role: it is a member of its own beside a worker.
      const key = `${harness} ${model} ${effort ?? ''} ${isImageAgentRole(role)}`
      if (!staff.has(key)) staff.set(key, { harness, model, effort, tier, roles: [] })
      const member = staff.get(key)
      member.roles.push(role)
      // A member that holds a role with no tier first has the tier of the one it holds with one.
      if (member.tier === undefined && tier !== undefined) member.tier = tier
      // The read the pick was made from (a pick copied from the last staff was made from none:
      // it is checked against the read the dialog shows of its harness, else the daemon reads).
      const named = read ?? (model ? this.#lists.readOf(harnessOf(harness)) : null)
      if (named !== null && member.read === undefined) member.read = named
    }
    return [...staff.values()]
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
 * Claude Code's prompt suggestion (`PromptSuggestion`), and the sections New project has too
 * (`ProjectSections`): human approval, wake-up calls and specs. Each saves as it
 * always did: the staff as it is added to or removed from, the approval as it is
 * ticked, the rules, the compaction, the advisor tool, the prompt suggestion, the calls and the
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
  #members
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
  /** Computer use: a part for each of the project's harnesses that has one. */
  #computerUse
  /** The project the dialog was last opened for, by id: the one it is for until it closes. */
  #project = null
  /** The board of that project, as the dialog last drew it. */
  #board = null
  /** The member whose removal waits for the human's yes, kept across redraws. */
  #removing = null
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
    this.#members = dialog.querySelector('#staff-members')
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
    this.#member = new MemberPicker(this.#form, this.#lists, (kind) => {
      void this.#markGone([kind])
    })
    this.#form.elements.memberCancel.addEventListener('click', () => {
      this.#member.reset(this.#missing)
    })
    this.#form.addEventListener('submit', (event) => {
      event.preventDefault()
      const values = this.#member.values()
      if (values === null) return
      // The name the harness gives the model is the daemon's to read from its listing.
      const { modelName, ...picked } = values
      const project = this.#project
      const editing = this.#member.editing
      const role = this.#member.role
      void page.act(async () => {
        try {
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
            this.#member.reset(this.#missing)
            return
          }
          const held = this.#board.lanes
            .map((lane) => lane.participant)
            .filter(isMember)
            .map((member) => member.handle)
          const { member } = await page.daemon('member.add', { project, ...picked, roles: [role] })
          page.note(
            held.includes(member.handle)
              ? `@${member.handle} is ${roleLabel(role)} now too.`
              : `@${member.handle} joined the staff as ${roleLabel(role)}.`,
          )
        } catch (refused) {
          // The daemon's words go where the human picked, as well as on the status line.
          this.#member.say(refused instanceof Error ? refused.message : String(refused))
          throw refused
        }
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
    this.#missing = missing
    readEveryHarness(this.#lists, missing)
    this.#gone = new Map()
    this.#member.reset(missing)
    this.#wakeUp.show(calls)
    this.#rules.show(board.project)
    this.#compaction.show(board.project)
    this.#advisorTool.show(board.project)
    this.#promptSuggestion.show(board.project)
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
    this.#draw()
  }

  /** Draws the staff from the project's board, in place. */
  #draw() {
    const board = this.#board
    // The members only: a member's sessions are lanes too, named after it.
    const members = board.lanes.map((lane) => lane.participant).filter(isMember)
    const rows = members
      .flatMap((member) =>
        this.#memberRows(member).map((entry) => ({
          ...entry,
          tier: member.tier,
          name: member.handle,
        })),
      )
      .sort(byRoleAndTier)
      .map((entry) => entry.row)
    if (rows.length === 0) rows.push(nobodyRow('Nobody yet: add the members this project may use.'))
    redraw(this.#members, rows)
    this.#gate.checked = board.project.gate ?? false
    this.#specs.draw(board.project)
    this.#computerUse.draw(harnessesOf(board))
  }

  /** A member as the project's board has it now, which a Remove kept across redraws acts on. */
  #memberNow(member) {
    return (
      this.#board.lanes.find(
        (lane) => lane.participant.handle === member.handle && isMember(lane.participant),
      )?.participant ?? member
    )
  }

  /**
   * A member's rows, one per role, each with the role it stands for: Remove
   * drops that role, or, for its last role, asks first and takes the member
   * off the staff.
   */
  #memberRows(member) {
    const page = this.#page
    const name = `@${member.handle}`
    // A model its harness no longer lists is marked, and said (T-1143); ending its shift
    // and its replacement are the staff chains' (CHN-11).
    const gone = this.#modelGone(member)
    // What the member runs on, its own (harness-models), and its tier, in a row of `roles`:
    // none in the row of a role with no tier.
    const who = (roles = member.roles) => {
      const runs = runsLabel({ ...member, roles })
      return member.needsEdit
        ? needsEditCell(name, runs, member.agent)
        : gone
          ? goneCell(name, modelShown(member), runs, HARNESS_NAMES[member.harness])
          : whoCell(name, runs)
    }
    if (this.#removing === member.handle) {
      const row = element('tr')
      row.dataset.handle = member.handle
      row.append(who())
      // The ask takes the role's column and the Remove's.
      const cell = element('td')
      cell.colSpan = 2
      const keep = button(`Keep ${name}`, 'quiet-button', () => {
        this.#removing = null
        this.#draw()
      })
      const yes = button(`Remove ${name}`, 'danger-button', () =>
        page.act(async () => {
          const { handle } = this.#memberNow(member)
          await page.daemon('member.remove', { project: this.#project, agent: handle })
          this.#removing = null
          page.note(`${name} left the staff.`)
        }),
      )
      cell.append(
        element('span', 'member-confirm', `Remove ${name}? Its open tasks are cancelled. `),
        keep,
        yes,
      )
      row.append(cell)
      return [{ role: member.roles[0], row }]
    }
    return member.roles.map((role) => {
      const remove = button(
        'Remove',
        'quiet-button',
        () => {
          const { handle, roles } = this.#memberNow(member)
          if (roles.length === 1) {
            this.#removing = handle
            this.#draw()
            return
          }
          void page.act(async () => {
            await page.daemon('member.roles', {
              project: this.#project,
              agent: handle,
              roles: roles.filter((held) => held !== role),
            })
          })
        },
        `Remove ${roleLabel(role)} ${name}`,
      )
      // An image designer has nothing to edit: Codex's own model, no effort, no tier.
      const edit = member.designer
        ? null
        : button(
            'Edit',
            'quiet-button',
            () => void this.#member.edit(this.#memberNow(member)),
            `Edit ${name}`,
          )
      const row = staffRow(who([role]), role, remove, edit)
      row.dataset.handle = member.handle
      if (gone) row.dataset.modelGone = member.model
      return { role, row }
    })
  }
}
