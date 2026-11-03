import { button, element, redraw } from '../dom.js'
import { HARNESS_NAMES, isMember, modelShown } from './board.js'
import { ComputerUseSection } from './computer-use.js'
import { ProjectSections } from './project-sections.js'

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

const ROLES = ['worker', 'advisor', 'reviewer', 'tester', 'designer']
const ROLE_LABEL = {
  worker: 'Worker',
  advisor: 'Advisor',
  reviewer: 'Reviewer',
  tester: 'Tester',
  designer: 'Image designer',
}
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
  rank(ROLES, a.role) - rank(ROLES, b.role) ||
  rank(TIERS, a.tier) - rank(TIERS, b.tier) ||
  a.name.localeCompare(b.name)
/**
 * "Opus · claude · high · standard": what a member runs on and the tier a task finds
 * it by, the effort where it has one.
 */
const runsLabel = (member) =>
  [modelShown(member), member.harness && harnessOf(member.harness), member.effort, member.tier]
    .filter(Boolean)
    .join(' · ')

/** One row of a staff table: the member, one of its roles, a Remove for that role, and an Edit of the member where it has one. */
function staffRow(who, role, remove, edit = null) {
  const row = element('tr')
  row.dataset.role = role
  row.append(who, element('td', null, ROLE_LABEL[role]))
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
  meta.append(element('mark', 'model-gone', model), runs.slice(model.length))
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

/** What `--model` takes for a listed model: Pi's provider and id joined, the id else. */
const modelOf = (model) => (model.provider ? `${model.provider}/${model.id}` : model.id)

/**
 * The models each harness lists of itself (`models.get`, CHF-03), read once per harness
 * while a dialog is open: the dialog that asks a harness again is answered from that read,
 * until the human's Refresh reads it again. A read that is refused is not kept, so the
 * next ask reads again. The chief's picker and Add member share one per dialog.
 */
export class ModelLists {
  #daemon
  /** Each harness's answer, as asked: a promise, kept from the moment it is asked. */
  #answers = new Map()

  constructor(daemon) {
    this.#daemon = daemon
  }

  /** Forgets every read: a dialog opening reads each harness again at its first ask. */
  clear() {
    this.#answers.clear()
  }

  /** `harness`'s answer (by its program's name, `claude`): the one read already, or a read. */
  get(harness) {
    return this.#answers.get(harness) ?? this.#ask(harness, false)
  }

  /** The human's Refresh: `harness`'s list read again from the harness itself. */
  refresh(harness) {
    return this.#ask(harness, true)
  }

  #ask(harness, refresh) {
    const asked = this.#daemon('models.get', refresh ? { harness, refresh } : { harness })
    this.#answers.set(harness, asked)
    asked.catch(() => {
      if (this.#answers.get(harness) === asked) this.#answers.delete(harness)
    })
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
    harness.addEventListener('change', () => void this.show(harness.value))
    model.addEventListener('change', () => {
      this.#sayHelp()
      this.#changed()
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

  /** The model picked, as its harness lists it; none while none is. */
  model() {
    return this.#answer?.listing.models.find((model) => modelOf(model) === this.#model.value)
  }

  /** Whether the model select has anything to pick: a harness whose models could not be read says why already. */
  get open() {
    return !this.#model.disabled
  }

  /**
   * `kind`'s models, read once while the dialog is open, or read again (`refresh`, the
   * human's Refresh); `wanted` is picked where it is listed (`{model, effort}`'s model's
   * name). Answers whether the models came: a later pick of another harness wins.
   */
  async show(kind, { refresh = false, wanted = null } = {}) {
    this.#harness.value = kind
    const name = HARNESS_NAMES[kind]
    this.#asked += 1
    const asked = this.#asked
    this.#none(`Reading ${name}'s models…`, '')
    let answer
    try {
      const harness = harnessOf(kind)
      answer = await (refresh ? this.#lists.refresh(harness) : this.#lists.get(harness))
    } catch (cause) {
      if (asked !== this.#asked) return false
      this.#none(
        'No model to pick',
        `${name}'s models could not be read: ${cause instanceof Error ? cause.message : String(cause)}`,
      )
      return false
    }
    if (asked !== this.#asked) return false
    const stale = staleWhy(answer.listing)
    if (answer.listing.models.length === 0) {
      this.#none('No model to pick', `${name} lists no model${stale ? `: ${stale}` : ''}.`)
      return false
    }
    const ask = new Option('Pick a model', '')
    ask.disabled = true
    const options = answer.listing.models.map((model) => {
      const id = modelOf(model)
      const option = new Option(
        model.contributor ? `${model.label} · Contributor` : model.label,
        id,
      )
      // A model its harness lists and will not run is no pick, nor one taken off.
      option.disabled = model.disabled || this.#takenOff(kind, id)
      return option
    })
    this.#model.replaceChildren(ask, ...options)
    this.#model.value = ''
    if (wanted !== null && options.some((option) => option.value === wanted && !option.disabled)) {
      this.#model.value = wanted
    }
    this.#model.disabled = false
    this.#answer = answer
    this.#sayHelp()
    this.#changed()
    return true
  }

  /** No model to pick, and why. */
  #none(words, why) {
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

  constructor(form, daemon) {
    this.#help = form.querySelector('.chief-help')
    this.#effort = form.elements.chiefEffort
    this.#lists = new ModelLists(daemon)
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
   * get one. `current` is the chief's own harness and model, which no switch is to. Each
   * opening reads the harnesses' models afresh at their first ask.
   */
  fill(missing, current = null) {
    this.#current = current
    this.#lists.clear()
    this.#picker.fill(missing)
  }

  /** The picked harness, model and effort (where the model lists any), as the daemon takes a chief. */
  values() {
    const model = this.#picker.model()
    const picked = { harness: this.#picker.kind, model: model ? modelOf(model) : '' }
    if (this.#effort.value !== '') picked.effort = this.#effort.value
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
      this.#say(`Pick the effort ${HARNESS_NAMES[this.#picker.kind]} runs ${model.label} at.`)
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

  constructor(form, lists, refreshed) {
    this.#fieldset = form.querySelector('.staff-add')
    this.#legend = this.#fieldset.querySelector('legend')
    this.#role = form.elements.role
    this.#effort = form.elements.memberEffort
    this.#tier = form.elements.memberTier
    this.#refresh = form.elements.memberRefresh
    this.#submit = this.#fieldset.querySelector('[name="memberAdd"]')
    this.#cancel = form.elements.memberCancel
    this.#help = this.#fieldset.querySelector('.member-help')
    this.#refreshed = refreshed
    this.#lists = lists
    this.#adds = this.#submit.textContent
    if (this.#role.options.length === 0) {
      for (const role of ROLES) {
        const option = element('option', null, ROLE_LABEL[role])
        option.value = role
        this.#role.append(option)
      }
    }
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

  /** The form as it adds a member: the harnesses installed here, nothing picked. */
  reset(missing) {
    this.#missing = missing
    this.editing = null
    this.#legend.textContent = 'Add a member'
    this.#role.disabled = false
    this.#submit.textContent = this.#adds
    this.#cancel.hidden = true
    this.#tier.value = ''
    this.say('')
    this.#fillHarnesses()
  }

  /**
   * The form as it edits `member` (a lane's participant): its harness's models read, its
   * model, effort and tier picked where its harness still lists them.
   */
  async edit(member) {
    this.editing = member.handle
    this.#legend.textContent = `Edit @${member.handle}`
    this.#role.disabled = true
    this.#submit.textContent = 'Save member'
    this.#cancel.hidden = false
    this.say('')
    this.#fillHarnesses(member.roles.includes('designer') ? ['codex'] : null)
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
    await this.#picker.show(kind, { wanted: found ? modelOf(found.model) : null })
    if (this.editing !== member.handle) return
    if ([...this.#effort.options].some((option) => option.value === found?.effort)) {
      this.#effort.value = found.effort
    }
    this.#tier.value = member.tier ?? ''
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
    if (model === undefined) return this.#picker.open ? say("Pick the member's model.") : null
    if (model.efforts.length > 0 && this.#effort.value === '') {
      return say(`Pick the effort ${HARNESS_NAMES[kind]} runs ${model.label} at.`)
    }
    if (this.#tier.value === '') {
      return say("Pick the member's tier: the work the chief gives it, from critical to light.")
    }
    this.say('')
    return {
      harness: kind,
      model: modelOf(model),
      effort: this.#effort.value === '' ? null : this.#effort.value,
      tier: this.#tier.value,
      modelName: model.label,
    }
  }

  /** The human's Refresh: the picked harness's models read again from the harness itself. */
  async refresh() {
    const kind = this.#picker.kind
    if (kind === '') {
      this.say('Pick a harness to read its models again.')
      return
    }
    const wanted = this.#picker.model()
    const shown = await this.#picker.show(kind, {
      refresh: true,
      wanted: wanted ? modelOf(wanted) : null,
    })
    if (shown) this.#refreshed(kind)
  }

  /** The words above the button: what is left to pick, or why the daemon said no. */
  say(words) {
    const hint = this.#fieldset.querySelector('.staff-hint')
    hint.textContent = words
    hint.hidden = words === ''
  }

  /** The harnesses a member in the role picked may run on: Codex alone draws. */
  #fillHarnesses(only = this.#role.value === 'designer' ? ['codex'] : null) {
    try {
      this.#picker.fill(this.#missing, only)
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
    this.#staff = dialog.querySelector('#new-project-staff')
    this.#refusal = dialog.querySelector('.dialog-refusal')
    this.#sections = new ProjectSections(
      dialog.querySelector('[data-project-sections]'),
      null,
      page.specs,
    )
    this.#page = page
    this.#chief = new ChiefPicker(this.#form, page.daemon)
    this.#member = new MemberPicker(this.#form, new ModelLists(page.daemon), () => {})
    this.#form.elements.memberAdd.addEventListener('click', () => {
      const picked = this.#member.values()
      if (picked === null) return
      const role = this.#member.role
      const same = (pick) =>
        pick.harness === picked.harness &&
        pick.model === picked.model &&
        pick.effort === picked.effort
      if (this.#picked.some((pick) => same(pick) && pick.role === role)) {
        this.#member.say(`${modelShown(picked)} is on the staff as ${ROLE_LABEL[role]} already.`)
        return
      }
      // One member holds each of its roles in one tier: the tier picked now is the member's.
      for (const pick of this.#picked) if (same(pick)) pick.tier = picked.tier
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
    const [{ missing = [] }, { staff }] = await Promise.all([
      this.#page.daemon('harnesses.missing'),
      this.#page.daemon('staff.last'),
      this.#page.readSpecs(),
    ])
    this.#missing = missing
    this.#chief.fill(missing)
    this.#member.reset(missing)
    this.#pickLast(staff)
    this.#sections.reset()
    this.#say('')
    this.#form.elements.directory.value = directory
    this.#dialog.showModal()
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
    this.#starting = true
    const form = this.#form
    const { gate, wakeUp, specs } = this.#sections.values()
    const body = {
      directory: form.elements.directory.value,
      ...this.#chief.values(),
      gate,
      staff: this.#pickedStaff(),
    }
    if (wakeUp !== null) body.wakeUp = wakeUp
    if (specs !== null) body.specs = specs
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
   * roles; one on its harness's default (an image designer from before the models were the
   * harnesses' own) is left for the human to pick a model for.
   */
  #pickLast(lastStaff) {
    this.#picked = lastStaff
      .filter((member) => member.model && !this.#missing.includes(harnessOf(member.harness)))
      .flatMap((member) =>
        member.roles.map((role) => ({
          harness: member.harness,
          model: member.model,
          effort: member.effort,
          tier: member.tier,
          modelName: member.modelName,
          role,
        })),
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
          `Remove ${ROLE_LABEL[pick.role]} ${pick.name}`,
        )
        const row = staffRow(whoCell(pick.name, runsLabel(pick)), pick.role, remove)
        row.dataset.model = pick.model ?? ''
        return row
      })
    if (rows.length === 0) {
      rows.push(nobodyRow('Nobody yet: pick a role, a harness, a model and a tier, then Add.'))
    }
    this.#staff.replaceChildren(...rows)
  }

  /** The picked rows as the daemon takes a staff: each member once, with its roles. */
  #pickedStaff() {
    const staff = new Map()
    for (const { harness, model, effort, tier, role } of this.#picked) {
      // An image designer holds no other role: it is a member of its own beside a worker.
      const key = `${harness} ${model} ${effort ?? ''} ${role === 'designer'}`
      if (!staff.has(key)) staff.set(key, { harness, model, effort, tier, roles: [] })
      staff.get(key).roles.push(role)
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
    this.#chief = new ChiefPicker(this.#form, page.daemon)
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
 * current), and the sections New project has too (`ProjectSections`): human
 * approval, wake-up calls and specs. Each saves as it always did: the staff as
 * it is added to or removed from, the approval as it is ticked, the calls and
 * the specs by their own Save.
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
      },
      page.specs,
    )
    this.#gate = sections.gate
    this.#wakeUp = sections.wakeUp
    this.#specs = sections.specs
    this.#computerUse = new ComputerUseSection(dialog.querySelector('.computer-use'), {
      daemon: page.daemon,
      act: page.act,
      report: page.report,
      project: () => (dialog.open ? this.#project : null),
    })
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
            await page.daemon('member.edit', { project, member: editing, ...picked })
            page.note(`@${editing} runs ${modelName} from its next window.`)
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
              ? `@${member.handle} is ${ROLE_LABEL[role]} now too.`
              : `@${member.handle} joined the staff as ${ROLE_LABEL[role]}.`,
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
      this.#computerUse.read(project),
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
    this.#lists.clear()
    this.#gone = new Map()
    this.#member.reset(missing)
    this.#wakeUp.show(calls)
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
    this.#computerUse.draw(board)
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
    // What the member runs on, its own (harness-models), and its tier.
    const runs = runsLabel(member)
    // A model its harness no longer lists is marked, and said (T-1143); ending its shift
    // and its replacement are the staff chains' (CHN-11).
    const gone = this.#modelGone(member)
    const who = () =>
      member.needsEdit
        ? needsEditCell(name, runs, member.agent)
        : gone
          ? goneCell(name, modelShown(member), runs, HARNESS_NAMES[member.harness])
          : whoCell(name, runs)
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
        `Remove ${ROLE_LABEL[role]} ${name}`,
      )
      const edit = button(
        'Edit',
        'quiet-button',
        () => void this.#member.edit(this.#memberNow(member)),
        `Edit ${name}`,
      )
      const row = staffRow(who(), role, remove, edit)
      row.dataset.handle = member.handle
      if (gone) row.dataset.modelGone = member.model
      return { role, row }
    })
  }
}
