import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { chromium, expect, test } from '@playwright/test'

import { tempEnv } from '../../tests/helpers.mjs'
import { agentsServer } from './agents-server.mjs'
import { refusedByPolicy, serveUi } from './serve-ui.mjs'

/**
 * The board page (TEST-BDC-13), against a stand-in for the app:
 * `daemon_request` answers from an in-page model and records every call, the way
 * the Rust app forwards the page's requests to the core. The page is served
 * under the app's own Content-Security-Policy, and each test is held to the
 * policy refusing nothing the page does.
 */
let ui
/** What the app's policy refused in the test running. */
let refused

test.setTimeout(15_000)

test.beforeAll(async () => {
  ui = await serveUi()
})

test.afterAll(() => ui?.close())

test.beforeEach(({ page }) => {
  refused = refusedByPolicy(page)
})

test.afterEach(() => {
  expect(refused, "the app's policy refuses nothing the page does").toEqual([])
})

const at = (minutesAgo) => new Date(Date.now() - minutesAgo * 60_000).toISOString()
const MEMBER = ['worker', 'advisor', 'reviewer', 'tester']
/**
 * What the fixture's members run on, by handle (harness-models: a member holds its own model
 * and effort, and is shown by them).
 */
const RUNS = {
  zeus: { model: 'claude-sonnet-5', effort: 'high' },
  diana: { model: 'gpt-5.6-luna', effort: 'low' },
  athena: { model: 'muse-spark' },
  hera: { model: 'gpt-6-astra', effort: 'max' },
  kronos: { model: 'openrouter/anthropic/claude-opus-5.5', effort: 'xhigh' },
}
/** A member of the last staff, as `staff.last` answers it: what it runs on, its tier and roles. */
const lastMember = (name, harness, roles, tier = 'standard') => ({
  harness,
  ...RUNS[name],
  modelName: RUNS[name].model,
  designer: false,
  tier,
  role: roles[0],
  roles,
})

const participant = (id, handle, role, extra = {}) => ({
  id,
  projectId: 1,
  handle,
  role,
  roles: [role],
  agent: null,
  harness: role === 'human' ? null : 'claude-code',
  ...(MEMBER.includes(role) ? (RUNS[handle] ?? {}) : {}),
  // The fixture's members are named for nobody's model (zeus): the daemon says so, and the
  // card says what each runs on.
  ...(MEMBER.includes(role) ? { nameApart: true } : {}),
  designer: false,
  tier: MEMBER.includes(role) ? 'standard' : null,
  outUntil: null,
  memberId: null,
  member: null,
  session: null,
  ...extra,
})

/** A session of a member: its own participant, named after the member, on what the member runs on. */
const session = (id, member, name, role = 'worker', extra = {}) =>
  participant(id, `${member.handle}-${name}`, role, {
    memberId: member.id,
    member: member.handle,
    session: name,
    harness: member.harness,
    model: member.model,
    effort: member.effort,
    modelName: member.modelName,
    tier: member.tier,
    ...extra,
  })
const task = (number, title, state, requester, assignee, minutesAgo = 3, extra = {}) => ({
  id: number,
  projectId: 1,
  number,
  title,
  body: title,
  state,
  requester,
  assignee,
  pool: null,
  tier: null,
  purpose: null,
  needs: [],
  blockedBy: [],
  deletedAt: null,
  createdAt: at(minutesAgo + 1),
  updatedAt: at(minutesAgo),
  ...extra,
})
/** A message of a task's thread as the daemon reads it: delivered, sent `minutesAgo`. */
const message = (id, kind, sender, recipient, body, minutesAgo = 1, extra = {}) => ({
  id,
  kind,
  sender,
  recipient,
  state: 'delivered',
  reason: null,
  body,
  createdAt: at(minutesAgo),
  ...extra,
})

/** A fresh listing of `models`, as a harness gives one. */
const listed = (models) => ({
  models,
  readAt: 1_793_000_000_000,
  freshness: { state: 'fresh' },
  how: 'the harness itself',
})

function model() {
  return {
    // What each harness lists of its models, by its program's name, and how its own
    // window switches its model.
    models: {
      claude: {
        listing: listed([
          { id: 'claude-opus-5-5', label: 'Opus 5.5', efforts: [] },
          { id: 'claude-sonnet-5', label: 'Sonnet 5', efforts: [] },
        ]),
        switchHelp: 'Claude switches with /model and s.',
      },
      codex: {
        listing: listed([
          { id: 'gpt-6-astra', label: 'GPT-6 Astra', efforts: [] },
          { id: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', efforts: [] },
        ]),
        switchHelp: 'Codex switches with /model and s.',
      },
      pi: {
        listing: listed([
          { id: 'muse-spark', provider: 'meta', label: 'Muse Spark', efforts: [] },
          {
            id: 'muse-spark-1.3-contributor',
            provider: 'meta',
            label: 'Muse Spark 1.3',
            efforts: [],
            contributor: true,
          },
        ]),
        switchHelp: 'Pi switches with /model and Enter.',
      },
      devin: { listing: listed([{ id: 'swe-2', label: 'SWE-2', efforts: [] }]) },
      muse: {
        listing: listed([
          { id: 'muse-spark-1.3', label: 'Muse Spark 1.3', efforts: [] },
          {
            id: 'muse-spark-1.3-contributor',
            label: 'Muse Spark 1.3',
            efforts: [],
            contributor: true,
          },
        ]),
        switchHelp: 'Muse switches with /model, and Enter makes it the default.',
      },
    },
    projects: [
      { id: 1, name: 'harbour', directory: '/work/harbour', state: 'open' },
      {
        id: 2,
        name: 'foundry',
        directory: '/work/foundry',
        state: 'suspended',
      },
    ],
    // What the app says of specs for every project, as the agents file keeps it (each part
    // only where set), and the Specmint the build bundles, as the daemon reads it from the
    // bundled files: the page draws both from what `specs.get` answers.
    specsApp: { on: false },
    specmint: {
      version: '3.0.0',
      variants: [
        { name: 'core', version: '3.0.0', commit: 'c1c8949', about: 'Markdown specs' },
        { name: 'core-html', version: '3.0.0', commit: 'e1762ab', about: 'HTML spec pages' },
        { name: 'tdd', version: '3.0.0', commit: '0fede12', about: 'Test-first Markdown specs' },
        {
          name: 'tdd-html',
          version: '3.0.0',
          commit: '9ab34cd',
          about: 'Test-first specs as HTML pages',
        },
      ],
    },
    boards: {
      1: {
        project: {
          id: 1,
          name: 'harbour',
          directory: '/work/harbour',
          state: 'open',
        },
        open: [
          task(6, 'Write the docs', 'open', 'chief', null, 1, { pool: 'worker', tier: 'standard' }),
        ],
        lanes: [
          {
            participant: participant(1, 'human', 'human'),
            tasks: [],
            activity: { state: 'closed' },
            pane: null,
          },
          {
            participant: participant(2, 'chief', 'chief', { model: 'claude-opus-5-5' }),
            tasks: [task(1, 'Ship the release notes', 'working', 'human', 'chief', 12)],
            activity: { state: 'working' },
            pane: { id: 'p1-chief', generation: 5 },
          },
          {
            participant: participant(3, 'zeus', 'worker'),
            tasks: [
              task(2, 'Write the parser', 'done', 'chief', 'zeus', 2, {
                result: 'Parser done, 14 tests.',
              }),
              task(4, 'Add the tests', 'queued', 'chief', 'zeus', 1),
              task(5, 'Old spike', 'accepted', 'chief', 'zeus', 90),
            ],
            activity: { state: 'waiting', reason: 'permission to run a command' },
            pane: { id: 'p1-zeus', generation: 7 },
          },
          {
            participant: participant(4, 'diana', 'worker', {
              harness: 'codex',
              tier: 'light',
              outUntil: at(-90),
            }),
            tasks: [
              task(
                3,
                '<img src=x onerror=window.__pwned=1> hostile title',
                'failed',
                'chief',
                'diana',
                5,
              ),
            ],
            activity: { state: 'closed' },
            pane: null,
          },
        ],
      },
      2: {
        project: {
          id: 2,
          name: 'foundry',
          directory: '/work/foundry',
          state: 'suspended',
        },
        open: [],
        lanes: [
          {
            participant: participant(9, 'human', 'human'),
            tasks: [],
            activity: { state: 'closed' },
            pane: null,
          },
        ],
      },
    },
    inbox: {
      1: [
        {
          id: 9,
          kind: 'note',
          state: 'queued',
          sender: null,
          recipient: 'human',
          taskNumber: 3,
          body: "T-3 failed: @diana's window closed.",
          createdAt: at(4),
        },
      ],
    },
    tasks: {
      '1:2': {
        ...task(2, 'Write the parser', 'done', 'chief', 'zeus', 2),
        messages: [
          message(20, 'task', 'chief', 'zeus', 'Write the parser', 3),
          message(21, 'result', 'zeus', 'chief', 'Parser done, 14 tests.', 2),
        ],
      },
      '1:3': {
        ...task(3, 'hostile', 'failed', 'chief', 'diana', 5),
        messages: [],
      },
    },
  }
}

async function open(page, data = model()) {
  await page.addInitScript((data) => {
    window.__calls = []
    window.__model = data
    // Every listener of each event, as the app keeps them: one registered twice hears it
    // twice. `__listeners.get(name)` says the event to all of them; an event no listener
    // is there for is heard by none, as the app says it.
    window.__handlers = new Map()
    window.__listeners = new Map()
    // The drops the app keeps for the page to ask of, by their numbers, as file_drop.rs keeps
    // them: the last 16, the first numbered 1. `__drop` is a drop on the app's window, which
    // the app tells the page of (`file-drop`); a drop of no file is told with no number.
    window.__drops = new Map()
    window.__dropped = 0
    window.__busyLookups = data.busyLookups ?? 0
    window.__drop = (paths, position) => {
      let id = null
      if (paths.length > 0) {
        window.__dropped += 1
        id = window.__dropped
        window.__drops.set(id, paths)
        if (window.__drops.size > 16) window.__drops.delete(window.__drops.keys().next().value)
      }
      window.__listeners.get('file-drop')?.({
        event: 'file-drop',
        id: 0,
        payload: { id, paths, position },
      })
    }
    // How long an operation takes to answer, in ms, set by a test while it runs:
    // the real core answers when it can, and a page that assumes at once races.
    window.__delay = {}
    // The answer to an operation, held by a test until it lets it go (`__hold[operation]`
    // is a promise, `__release[operation]` what lets it go); it holds the next request
    // for that operation, and only that one. And every operation answered, in order.
    window.__hold = {}
    window.__release = {}
    window.__answered = []
    // The read ids `models.get` answers with, in order: what a submission names as the list its
    // picker was shown (A4-28).
    window.__reads = []
    // Why the daemon is down, while a test has it down: the app answers every
    // core request with this, as Rust does with no bridge to the daemon.
    window.__down = null
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    // A project's wake-up calls as the daemon says them: the interval in the
    // largest unit that holds it whole, the default words where none were written.
    const UNIT_MS = { days: 86_400_000, hours: 3_600_000, minutes: 60_000 }
    const DEFAULT_WORDS =
      'Do you still have work to do? If tasks are left on the board or in your plan, carry on.'
    const wakeUpOf = (project) => {
      const saved = data.wakeUps?.[project] ?? { on: false, everyMs: 3_600_000, text: null }
      const [unit, size] = Object.entries(UNIT_MS).find(([, ms]) => saved.everyMs % ms === 0)
      return {
        on: saved.on,
        everyMs: saved.everyMs,
        every: saved.everyMs / size,
        unit,
        text: saved.text ?? DEFAULT_WORDS,
        defaultText: saved.text === null,
      }
    }
    // A project as the list has it and as its board has it: the daemon says one and the same.
    const shownProject = (id) =>
      [data.projects.find((project) => project.id === id), data.boards[id]?.project].filter(Boolean)
    const specsAnswer = () =>
      answer({
        app: data.specsApp,
        applied: {
          folder: data.specsApp.folder ?? '.specs',
          variant: data.specsApp.variant ?? 'core',
        },
        specmint: data.specmint,
      })
    const operations = {
      'projects.list': () => answer({ projects: data.projects }),
      'harnesses.missing': () => answer({ missing: data.missing ?? [] }),
      // What a harness lists of its models, by its program's name (CHF-07): the fixture's,
      // or a refusal a test names for it (`modelsRefusal`).
      'models.get': ({ harness }) => {
        const refused = data.modelsRefusal?.[harness]
        if (refused !== undefined) return { ok: false, error: refused }
        const { listing, switchHelp = null } = data.models[harness]
        // A token of the daemon's life, as the daemon names a read: a text, never a number.
        const read = `0123456789abcdef.${window.__reads.length + 1}`
        window.__reads.push({ harness, read })
        return answer({
          harness,
          read,
          listing,
          default: null,
          contributorWarning:
            'Contributor model: Meta may train on what it sees, including your code.',
          switchHelp,
        })
      },
      // Add member and Edit member (T-1143): the member named by its model's id and its
      // effort (`cf_ledger::handles`, HMD-12), or a refusal a test names.
      'member.add': (body) => {
        if (data.memberRefusal !== undefined) return { ok: false, error: data.memberRefusal }
        // An image designer is Codex's own model, named by it, and takes no model, effort or
        // tier (designer P2): the daemon refuses any, and so does the mock.
        if (body.roles?.includes('designer')) {
          if (['model', 'effort', 'tier'].some((field) => body[field] != null)) {
            return {
              ok: false,
              error:
                "an image designer draws on Codex's own model: it takes no model, effort or tier",
            }
          }
          return answer({ member: { handle: 'codex-default', roles: body.roles, tier: 'light' } })
        }
        // Named as the daemon names it: Pi's id without its provider, then the effort.
        const id = body.harness === 'pi' ? body.model.replace(/^[^/]+\//, '') : body.model
        const words = [id, body.effort]
          .filter(Boolean)
          .join('-')
          .toLowerCase()
          .split(/[^a-z0-9]+/)
          .filter(Boolean)
        const handle = data.memberHandle ?? words.join('-')
        return answer({ member: { handle, roles: body.roles, tier: body.tier } })
      },
      'member.edit': (body) => {
        if (data.memberRefusal !== undefined) return { ok: false, error: data.memberRefusal }
        // The name holds the model it runs on (HMD-12): an edited member is renamed by the
        // daemon, its windows with it, and the board says the name it left. A test names
        // the new handle (`editRenames`) as the daemon would make it.
        const lane = data.boards[body.project]?.lanes.find(
          (one) => one.participant.handle === body.member,
        )
        if (data.editRenames && lane) {
          const words = [body.model.replace(/^[^/]+\//, ''), body.effort]
            .filter(Boolean)
            .join('-')
            .toLowerCase()
            .split(/[^a-z0-9]+/)
            .filter(Boolean)
          const handle = data.editRenames === true ? words.join('-') : data.editRenames
          for (const other of data.boards[body.project].lanes) {
            const mine = other.participant
            if (mine.handle === body.member || mine.member === body.member) {
              const own = mine.session ? `-${mine.session}` : ''
              mine.formerHandles = [...(mine.formerHandles ?? []), mine.handle]
              mine.handle = `${handle}${own}`
              if (mine.member) mine.member = handle
              Object.assign(mine, {
                harness: body.harness,
                model: body.model,
                effort: body.effort,
                modelName: body.model,
                tier: body.tier,
              })
            }
          }
          return answer({ member: { handle, tier: body.tier } })
        }
        return answer({ member: { handle: body.member, tier: body.tier } })
      },
      'staff.last': () => answer({ staff: data.lastStaff ?? [] }),
      'member.roles': ({ project, agent, roles }) => {
        const lane = data.boards[project].lanes.find((l) => l.participant.handle === agent)
        lane.participant.roles = roles
        return answer({ member: lane.participant })
      },
      'board.get': ({ project }) => answer({ board: data.boards[project] }),
      // A project's computer use, as the daemon reads it (`computerUse` of the model, on macOS
      // with no grant and Claude Code's off, unless a test says otherwise), and the System
      // Settings panes it opens, by name.
      'computer_use.get': ({ project, directory }) => {
        const said = data.computerUse ?? {}
        // Of a saved project's folder, or of a folder asked for before there is a project (New
        // project): as the daemon reads Claude Code's own configuration for either.
        const folder = directory ?? data.boards[project]?.project.directory
        return answer({
          system: said.system ?? 'macos',
          grantsOf: said.grantsOf ?? null,
          permissions:
            'permissions' in said
              ? said.permissions
              : { accessibility: false, screenRecording: false },
          harnesses: {
            'claude-code': {
              on: said.on ?? false,
              folder,
            },
            codex: { setUp: said.codexSetUp ?? false },
          },
        })
      },
      'computer_use.open': ({ pane }) => answer({ opened: pane }),
      'project.gate': ({ project, gate }) => {
        data.boards[project].project.gate = gate
        return answer({ project: data.boards[project].project })
      },
      // A meeting with the chief, as the daemon has it: the time it began, or none, and what it
      // held is let go at its Stop.
      'project.meeting': ({ project, on }) => {
        const board = data.boards[project]
        if (on) board.project.meeting = board.project.meeting ?? new Date().toISOString()
        else {
          delete board.project.meeting
          delete board.held
        }
        return answer({ project: board.project })
      },
      // A project renamed, and a line written under its name, as the daemon does it: checked,
      // kept on the project of the list and on the one of its board, and refused in its words.
      'project.rename': ({ project, name }) => {
        if (typeof name !== 'string' || name.trim() === '' || name.length > 100) {
          return { ok: false, error: 'name must be text, not empty, at most 100 characters' }
        }
        for (const shown of shownProject(project)) shown.name = name
        return answer({ project: shownProject(project)[0] })
      },
      'project.describe': ({ project, description }) => {
        const line = (description ?? '').trim()
        if (line.length > 200) {
          const error =
            "a project's description is one line of plain text: it is at most 200 characters"
          return { ok: false, error }
        }
        for (const shown of shownProject(project)) {
          if (line === '') delete shown.description
          else shown.description = line
        }
        return answer({ project: shownProject(project)[0] })
      },
      // The rules for a project's staff, as the daemon keeps them: trimmed, none where blank,
      // refused over 8,000 bytes or 150 lines or with a control character, in its words.
      'project.rules': ({ project, rules }) => {
        const kept = (rules ?? '').trim()
        const refused = (why) => ({
          ok: false,
          error: `the rules for the staff are plain text: ${why}`,
        })
        const bytes = new TextEncoder().encode(kept).length
        const lines = kept === '' ? 0 : kept.split('\n').length
        const control = (character) => {
          const code = character.codePointAt(0)
          return (
            character !== '\n' &&
            character !== '\t' &&
            (code < 0x20 || (code >= 0x7f && code <= 0x9f))
          )
        }
        if ([...kept].some(control)) {
          return refused('they have no control characters but line breaks and tabs')
        }
        if (bytes > 8000) return refused(`they are at most 8000 bytes, and these are ${bytes}`)
        if (lines > 150) return refused(`they are at most 150 lines, and these are ${lines}`)
        for (const shown of shownProject(project)) {
          if (kept === '') delete shown.staffRules
          else shown.staffRules = kept
        }
        return answer({ project: shownProject(project)[0] })
      },
      // The size the members' windows compact at, whatever their harness, as the daemon keeps
      // it: a whole number of tokens from 100,000 to 1,000,000, or none (null or left out) for
      // each harness's own point, refused in its words otherwise.
      'project.compaction': ({ project, compaction }) => {
        const size = compaction ?? null
        if (size !== null && !(Number.isInteger(size) && size >= 100000 && size <= 1000000)) {
          return {
            ok: false,
            error:
              "the compaction size is a whole number of tokens from 100000 to 1000000, or none for each harness's own point",
          }
        }
        for (const shown of shownProject(project)) {
          if (size === null) delete shown.compaction
          else shown.compaction = size
        }
        return answer({ project: shownProject(project)[0] })
      },
      // Whether the project's Claude Code members run with Claude Code's own advisor tool, as the
      // daemon keeps it: on (true) or off (false, the default), refused in its words otherwise. A
      // board says it only where it is on.
      'project.advisor_tool': ({ project, advisorTool }) => {
        if (typeof advisorTool !== 'boolean') {
          return { ok: false, error: 'the advisor tool is on (true) or off (false)' }
        }
        for (const shown of shownProject(project)) {
          if (advisorTool) shown.advisorTool = true
          else delete shown.advisorTool
        }
        return answer({ project: shownProject(project)[0] })
      },
      'wakeup.get': ({ project }) => answer({ wakeUp: wakeUpOf(project) }),
      'wakeup.set': ({ project, on, every, unit, text }) => {
        if (!(unit in UNIT_MS) || !Number.isInteger(every) || every < 1) {
          const error = `a wake-up comes every whole number of minutes, hours or days, not ${every} "${unit}"`
          return { ok: false, error }
        }
        if (text.trim() === '') {
          const error = 'the wake-up text must be text, not empty, at most 1000000 characters'
          return { ok: false, error }
        }
        data.wakeUps ??= {}
        data.wakeUps[project] = {
          on,
          everyMs: every * UNIT_MS[unit],
          text: text === DEFAULT_WORDS ? null : text,
        }
        return answer({ wakeUp: wakeUpOf(project) })
      },
      // What a project says of specs: kept as it was said, each part left out when
      // it says none, and the daemon's own refusal where a test has it refuse.
      'project.specs': ({ project, on, folder, variant }) => {
        if (data.specsRefusal) return { ok: false, error: data.specsRefusal }
        const said = {}
        if (on !== null) said.on = on
        if (folder !== null) said.folder = folder
        if (variant !== null) said.variant = variant
        const shown = data.boards[project].project
        if (Object.keys(said).length === 0) delete shown.specs
        else shown.specs = said
        return answer({ project: shown })
      },
      // What the app says of specs for every project, as the daemon answers it: the raw word
      // of the app, what a project that names none then uses (the documented `.specs` and
      // `core` where the app names none), and the Specmint the build bundles.
      'specs.get': () => specsAnswer(),
      // Each part given is kept (`null`, or a blank folder, names none again), unless a test has
      // the daemon refuse with its own words (`specsAppRefusal`), which keeps nothing.
      'specs.set': ({ on, folder, variant }) => {
        if (data.specsAppRefusal) return { ok: false, error: data.specsAppRefusal }
        const app = data.specsApp
        if (on !== undefined) app.on = on
        if (folder !== undefined) {
          if (folder === null || folder.trim() === '') delete app.folder
          else app.folder = folder.trim()
        }
        if (variant !== undefined) {
          if (variant === null) delete app.variant
          else app.variant = variant
        }
        return specsAnswer()
      },
      // The newest messages one frame holds (all of them, unless a test says
      // how many fit) and how many there are; asked for what For you lists,
      // only the notes not yet read.
      'inbox.get': ({ project, unread }) => {
        const all = (data.inbox[project] ?? []).filter(
          (message) => !unread || (message.state === 'queued' && message.kind === 'note'),
        )
        const messages = all.slice(0, data.inboxFit ?? all.length)
        return answer({ messages, total: all.length, shown: messages.length })
      },
      'task.get': ({ project, task }) => {
        const found = data.tasks[`${project}:${task}`]
        return found === undefined
          ? { ok: false, error: `no task T-${task} in this project` }
          : answer({ task: found })
      },
      // Finished tasks leave the board and still read, or the daemon refuses
      // them all in its own words (a test says which with `deleteRefusal`).
      'tasks.delete': ({ project, tasks }) => {
        if (data.deleteRefusal) return { ok: false, error: data.deleteRefusal }
        const board = data.boards[project]
        const stays = (task) => !tasks.includes(task.number)
        board.open = board.open.filter(stays)
        for (const lane of board.lanes) lane.tasks = lane.tasks.filter(stays)
        for (const number of tasks) {
          const kept = data.tasks[`${project}:${number}`]
          if (kept !== undefined) kept.deletedAt = new Date().toISOString()
        }
        return answer({})
      },
      // A resumed task goes on in its window, or back on the board when that
      // window has ended (a test says so with `resumesOpen`).
      'task.resume': ({ project, task }) => {
        const found = data.tasks[`${project}:${task}`]
        return answer({ task: { ...found, state: found.resumesOpen ? 'open' : 'queued' } })
      },
      // Finish answers with the jobs and agents the turn left, which it ends none of (a test
      // says how many with `finishLeaves`, `{ jobs, agents }`).
      'task.finish': ({ project, task }) =>
        answer({
          task: { ...data.tasks[`${project}:${task}`], state: 'done' },
          ...(data.finishLeaves ?? { jobs: 1, agents: 0 }),
        }),
      // The last `limit` items, as the daemon gives them, and how many came.
      'task.transcript': ({ project, task, limit = Number.POSITIVE_INFINITY }) => {
        const { items, total } = data.transcripts?.[`${project}:${task}`] ?? { items: [], total: 0 }
        const last = items.slice(Math.max(0, items.length - limit))
        return answer({ items: last, total, shown: last.length })
      },
      // A start the daemon refuses is refused whole, in its words, and no project is made (a
      // test says which words with `openRefusal`).
      'project.open': (body) => {
        if (data.openRefusal) return { ok: false, error: data.openRefusal }
        const project = { id: 3, name: 'new', directory: body.directory }
        // This runs in the page: a participant as the board has one, written out here.
        const person = (id, handle, role, harness) => ({
          id,
          projectId: 3,
          handle,
          role,
          roles: [role],
          agent: null,
          harness,
          designer: false,
          tier: null,
          outUntil: null,
          memberId: null,
          member: null,
          session: null,
        })
        // A test that goes on to the new project (`openMakesBoard`) has it made as the daemon makes
        // one: what the human said in New project is kept with it, for its Project settings.
        if (data.openMakesBoard) {
          Object.assign(project, { state: 'open', gate: body.gate === true })
          if (body.rules !== undefined) project.staffRules = body.rules
          // The daemon's board says a size only where there is one: null is the harness's own point.
          if (typeof body.compaction === 'number') project.compaction = body.compaction
          if (body.advisorTool === true) project.advisorTool = true
          data.projects.push(project)
          data.boards[3] = {
            project,
            open: [],
            lanes: [
              {
                participant: person(30, 'human', 'human', null),
                tasks: [],
                activity: { state: 'closed' },
                pane: null,
              },
              {
                participant: { ...person(31, 'chief', 'chief', body.harness), model: body.model },
                tasks: [],
                activity: { state: 'working' },
                pane: { id: 'p3-chief', generation: 1 },
              },
            ],
          }
          data.inbox[3] = []
        }
        return answer({ project })
      },
    }
    const invoke = async (command, args = {}) => {
      window.__calls.push([
        command,
        JSON.parse(JSON.stringify(args, (key, value) => (key === 'onOutput' ? 'channel' : value))),
      ])
      if (command === 'daemon_request') {
        const handle = operations[args.operation]
        const ms = window.__delay[args.operation] ?? 0
        if (ms > 0) await new Promise((wake) => setTimeout(wake, ms))
        const held = window.__hold[args.operation]
        if (held !== undefined) {
          delete window.__hold[args.operation]
          await held
        }
        if (window.__down !== null) {
          return {
            ok: false,
            error: 'not-available-yet',
            operation: args.operation,
            detail: window.__down,
          }
        }
        const result = handle ? handle(args.body) : { ok: true }
        window.__answered.push(args.operation)
        return result
      }
      if (command === 'subscribe_output') {
        window.__output = args.onOutput
        return { ok: true }
      }
      // The harnesses screen's address, as Rust hands it: the daemon's page with
      // the UI token, or why there is none.
      if (command === 'agents_screen') {
        // Held by a test (`hold(page, 'agents_screen')`), as the app holds it while the
        // daemon starts.
        const held = window.__hold.agents_screen
        if (held !== undefined) {
          delete window.__hold.agents_screen
          await held
        }
        return data.screens
          ? { ok: true, url: `${data.screens.url}/${args.page}?token=${data.screens.token}` }
          : { ok: false, error: 'the harnesses screen is not available: the daemon is not up' }
      }
      if (command === 'pane_input_enqueue' || command === 'pane_reply_enqueue')
        return { ok: true, ticket: 't' }
      if (command.startsWith('update_')) return { ok: false, error: 'not in this test' }
      // A link opened in the browser, or refused as a test says (`window.__linkRefused`).
      if (command === 'open_link') return window.__linkRefused ?? { ok: true }
      // The short names Windows keeps for the paths of the drop numbered `id`, as a test says
      // them (`shortNames`, by a path's long name: `{ short }`, or `{ error }` in the app's
      // words), or none; held as a daemon operation is. As the app does, only for the paths of
      // that drop, and only for a drop it keeps.
      if (command === 'short_paths') {
        const held = window.__hold.short_paths
        if (held !== undefined) {
          delete window.__hold.short_paths
          await held
        }
        const dropped = window.__drops.get(args.id)
        if (dropped === undefined) {
          return { ok: false, error: 'it is not one of the last 16 drops the app heard' }
        }
        // Another lookup runs (a page's before a reload, reading a share far away) for
        // as many asks as the test says (`busyLookups`).
        if (window.__busyLookups > 0) {
          window.__busyLookups -= 1
          return { ok: false, busy: true, error: 'another lookup of short names is running' }
        }
        const none = { error: 'Windows keeps no short name for it' }
        const notDropped = { error: 'it was not dropped on the app' }
        return {
          ok: true,
          paths: args.paths.map((path) =>
            dropped.includes(path) ? (data.shortNames?.[path] ?? none) : notDropped,
          ),
        }
      }
      return { ok: true }
    }
    class Channel {
      onmessage = null
    }
    const emulators = []
    window.__emulators = emulators
    window.__TAURI__ = {
      core: { invoke, Channel },
      event: {
        listen: async (name, handler) => {
          window.__calls.push(['listen', { name }])
          const all = window.__handlers.get(name) ?? []
          all.push(handler)
          window.__handlers.set(name, all)
          window.__listeners.set(name, (event) => {
            for (const each of [...all]) each(event)
          })
          return () => all.splice(all.indexOf(handler), 1)
        },
      },
      dialog: { open: async () => '/work/fresh' },
      // A test that needs the real xterm asks for it; the rest type into a stand-in.
      test: data.realTerminals
        ? {}
        : {
            createEmulator: (host) => {
              // The keyboard lives in a field inside the host, as xterm's does.
              host.append(
                Object.assign(document.createElement('textarea'), { className: 'stub-input' }),
              )
              const emulator = {
                host,
                written: [],
                // How many writes it was given, each counted as it comes, before it is done.
                writes: 0,
                // Retired by the page: its window is over for good.
                disposed: false,
                // A write is done once `__writeGate` lets it go, where a test holds it.
                write: async (bytes) => {
                  emulator.writes += 1
                  await window.__writeGate
                  emulator.written.push(...bytes)
                },
                onData: (callback) => {
                  emulator.type = callback
                  return { dispose() {} }
                },
                fit() {},
                dispose() {
                  emulator.disposed = true
                },
              }
              emulators.push(emulator)
              return emulator
            },
          },
    }
  }, data)
  await page.goto(`${ui.origin}/index.html`)
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true')
}

/** Buttons as the human reads them: an icon button by its tip, any other by its text. */
const named = (buttons) =>
  buttons.evaluateAll((all) => all.map((button) => button.dataset.tip ?? button.textContent))

/** A row's tools, so read. */
const toolsOf = (row) => named(row.locator('.row-tools button'))

/**
 * The bodies the page sent for 'operation', as it sent them, but for the read each model is picked
 * from (`read`, the id `models.get` answered with: A4-28), which 'reads' keeps. Most cases are
 * about what was asked, not which read it was asked from; the ones about the read ask for it.
 */
const calls = (page, operation, { reads = false } = {}) =>
  page.evaluate(
    ([operation, reads]) => {
      const unread = (body) => {
        if (reads || body === null || typeof body !== 'object') return body
        const { read, ...rest } = body
        if (Array.isArray(rest.staff)) {
          rest.staff = rest.staff.map(({ read: named, ...member }) => member)
        }
        return rest
      }
      return window.__calls
        .filter(([command, args]) => command === 'daemon_request' && args.operation === operation)
        .map(([, args]) => unread(args.body))
    },
    [operation, reads],
  )

/** Picks a chief's harness, then one of its models, by what each says, as the human does (CHF-07). */
async function pickChief(dialog, harness, model) {
  await dialog.locator('[name="chiefHarness"]').selectOption({ label: harness })
  const models = dialog.locator('[name="chiefModel"]')
  await expect(models).toBeEnabled()
  await models.selectOption({ label: model })
}

/** What a select offers: each option's words, and whether it is disabled. */
const offered = (select) =>
  select
    .locator('option')
    .evaluateAll((all) => all.map((option) => [option.textContent, option.disabled]))

/**
 * Add member's form (T-1143): a role, a harness by its name, one of the models it lists by
 * its label, an effort and a tier, each where given.
 */
async function pickMember(dialog, { role, harness, model, effort, tier }) {
  const add = dialog.locator('.staff-add')
  if (role !== undefined) await add.locator('[name="role"]').selectOption(role)
  await add.locator('[name="memberHarness"]').selectOption({ label: harness })
  await expect(add.locator('[name="memberModel"]')).toBeEnabled()
  await add.locator('[name="memberModel"]').selectOption({ label: model })
  if (effort !== undefined) await add.locator('[name="memberEffort"]').selectOption(effort)
  if (tier !== undefined) await add.locator('[name="memberTier"]').selectOption(tier)
}

/** An image designer, as Add member picks one: the role, and nothing else (Codex is picked for it). */
async function pickDesigner(dialog) {
  const add = dialog.locator('.staff-add')
  await add.locator('[name="role"]').selectOption('designer')
  await expect(add.locator('[name="memberHarness"]')).toHaveValue('codex')
}

test('draws the kanban: a row per participant, a column per state, and cards that stay when done', async ({
  page,
}) => {
  await open(page)
  await expect(page.getByRole('heading', { name: 'harbour' })).toBeVisible()
  const table = page.getByRole('table', { name: 'Tasks' })
  // The last heading holds its own control too (Delete finished).
  await expect(table.locator('thead th')).toHaveText([
    'Staff',
    'Backlog',
    'Queued',
    'Working',
    'Waiting',
    'Done',
    /^Finished/,
  ])
  const rows = table.locator('tbody tr')
  // Nothing assigns the human a task: what is for them is in For you, not a row.
  await expect(rows).toHaveCount(3)
  await expect(rows.locator('.row-name')).toHaveText(['Chief of Staff', '@zeus', '@diana'])
  const zeus = table.locator('tr[data-handle="zeus"]')
  await expect(zeus.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(zeus.locator('.row-status')).toHaveText('Waiting: permission to run a command')
  // The chief's row holds its tasks; what it is doing is on its card in the dock.
  const chief = table.locator('tr[data-handle="chief"]')
  await expect(chief.locator('.row-status')).toHaveCount(0)
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-status'),
  ).toHaveText('Working')
  await expect(zeus.locator('td[data-state="queued"] .card-title')).toHaveText(['Add the tests'])
  const done = zeus.locator('td[data-state="done"] button.card[data-task="2"]')
  await expect(done.locator('.card-title')).toHaveText('Write the parser')
  await expect(done.locator('.card-result')).toHaveText('Parser done, 14 tests.')
  const spike = zeus.locator('td[data-state="finished"] button.card[data-task="5"]')
  await expect(spike.locator('.card-title')).toHaveText('Old spike')
  await expect(spike.locator('.card-state')).toHaveText('Accepted')
  await expect(
    page.locator('tr[data-handle="diana"] td[data-state="finished"] .card-title'),
  ).toHaveText('<img src=x onerror=window.__pwned=1> hostile title')
  expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
})

test('shows an agent-written title as text, never as markup', async ({ page }) => {
  await open(page)
  const title = page.locator('tr[data-handle="diana"] .card-title')
  await expect(title).toHaveText('<img src=x onerror=window.__pwned=1> hostile title')
  expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  await expect(page.locator('tr[data-handle="diana"] img')).toHaveCount(0)
})

test("gives a session's lane the human's hand on its window: show (opening a closed one), hide, delete", async ({
  page,
}) => {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const zeus = zeusLane.participant
  Object.assign(zeusLane, { tasks: [], activity: { state: 'closed' }, pane: null })
  data.boards[1].lanes.push(
    {
      participant: session(20, zeus, 'amber-pine'),
      tasks: [task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(22, zeus, 'brisk-birch'),
      tasks: [task(23, 'Write the docs', 'done', 'chief', 'zeus-brisk-birch', 30)],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  await unfold(page)
  // A live window and a closed one offer the same Show: nothing closes a
  // window from the board, the work in it is paused or cancelled on its card.
  const live = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect.poll(() => toolsOf(live)).toEqual(['Show terminal', 'Delete session'])
  const closed = page.locator('tr[data-handle="zeus-brisk-birch"]')
  await expect.poll(() => toolsOf(closed)).toEqual(['Show terminal', 'Delete session'])
  await expect(closed.locator('.row-status')).toHaveCount(0)
  // Showing a closed one opens it again, and unfolds the dock it shows in.
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeHidden()
  await closed.getByRole('button', { name: "Show @zeus · brisk-birch's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-brisk-birch' }])
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeVisible()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const card = dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')
  // A live session's terminal is in the dock once the human shows it, and
  // asks the daemon nothing: its window is open already.
  await expect(card).toHaveCount(0)
  await live.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect(card).toHaveCount(1)
  expect(await calls(page, 'session.open')).toEqual([{ project: 1, handle: 'zeus-brisk-birch' }])
  await expect.poll(() => toolsOf(live)).toEqual(['Hide terminal', 'Delete session'])
  // Neither the chief's card nor a session's offers a Close.
  await expect(dock.getByRole('button', { name: /^Close/ })).toHaveCount(0)
  await closed.getByRole('button', { name: "Delete @zeus · brisk-birch's session" }).click()
  await expect
    .poll(() => calls(page, 'session.end'))
    .toEqual([{ project: 1, handle: 'zeus-brisk-birch' }])
  await expect(page.locator('#status')).toHaveText(
    "@zeus-brisk-birch is off the board, its conversation kept: a follow-up brings it back. Its tasks stay on @zeus's lane.",
  )
  await expect(page.getByRole('button', { name: "Delete @zeus's session" })).toHaveCount(0)
})

test('gives a member with two roles a row per role, each with its own sessions and cards', async ({
  page,
}) => {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const zeus = zeusLane.participant
  zeus.roles = ['worker', 'reviewer']
  Object.assign(zeusLane, {
    tasks: [
      task(31, 'Old parser', 'accepted', 'chief', 'zeus', 40, { pool: 'worker', tier: 'standard' }),
    ],
    activity: { state: 'closed' },
    pane: null,
  })
  data.boards[1].lanes.push(
    {
      participant: session(40, zeus, 'amber-pine', 'worker'),
      tasks: [task(41, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(42, zeus, 'brisk-birch', 'reviewer'),
      tasks: [
        task(43, 'Review T-2', 'working', 'chief', 'zeus-brisk-birch', 2, {
          pool: 'reviewer',
          tier: 'standard',
        }),
      ],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-brisk-birch', generation: 4 },
    },
  )
  await open(page, data)
  await unfold(page)
  const rows = await page
    .locator('tbody tr[data-handle^="zeus"]')
    .evaluateAll((rows) => rows.map((row) => [row.dataset.handle, row.dataset.role]))
  expect(rows).toEqual([
    ['zeus', 'worker'],
    ['zeus-amber-pine', 'worker'],
    ['zeus', 'reviewer'],
    ['zeus-brisk-birch', 'reviewer'],
  ])
  const worker = page.locator('tr[data-handle="zeus"][data-role="worker"]')
  const reviewer = page.locator('tr[data-handle="zeus"][data-role="reviewer"]')
  await expect(worker.locator('.row-meta')).toHaveText(/^worker · standard/)
  await expect(reviewer.locator('.row-meta')).toHaveText(/^reviewer · standard/)
  await expect(worker.locator('.row-status')).toHaveText('1 terminal open, one per task')
  await expect(reviewer.locator('.row-status')).toHaveText('1 terminal open, one per task')
  await expect(worker.locator('button.card[data-task="31"]')).toHaveCount(1)
  await expect(reviewer.locator('button.card')).toHaveCount(0)
})

test('gives a member who is a worker and a tester a row for each, every task on the row of its pool', async ({
  page,
}) => {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  zeusLane.participant.roles = ['worker', 'tester']
  zeusLane.tasks = [
    task(31, 'Old parser', 'accepted', 'chief', 'zeus', 40, { pool: 'worker', tier: 'standard' }),
    task(32, 'Test T-31', 'done', 'chief', 'zeus', 30, { pool: 'tester', tier: 'standard' }),
  ]
  await open(page, data)
  const rows = await page
    .locator('tbody tr[data-handle="zeus"]')
    .evaluateAll((rows) => rows.map((row) => [row.dataset.handle, row.dataset.role]))
  expect(rows).toEqual([
    ['zeus', 'worker'],
    ['zeus', 'tester'],
  ])
  const worker = page.locator('tr[data-handle="zeus"][data-role="worker"]')
  const tester = page.locator('tr[data-handle="zeus"][data-role="tester"]')
  await expect(tester.locator('.row-meta')).toHaveText(/^tester · standard/)
  await expect(worker.locator('button.card[data-task="31"]')).toHaveCount(1)
  await expect(worker.locator('button.card[data-task="32"]')).toHaveCount(0)
  await expect(tester.locator('button.card[data-task="32"]')).toHaveCount(1)
})

test("draws a member's sessions as lanes under it, named, and counts its open windows", async ({
  page,
}) => {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const zeus = zeusLane.participant
  // A member runs no window of its own: its work is in its sessions.
  Object.assign(zeusLane, { tasks: [], activity: { state: 'closed' }, pane: null })
  data.boards[1].lanes.push(
    {
      participant: session(20, zeus, 'amber-pine'),
      tasks: [task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(21, zeus, 'brisk-birch'),
      tasks: [
        task(22, 'Write the docs', 'done', 'chief', 'zeus-brisk-birch', 1, {
          result: 'Docs done.',
        }),
      ],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  await unfold(page)
  const rows = page.locator('table[aria-label="Tasks"] tbody tr')
  await expect(rows.evaluateAll((nodes) => nodes.map((n) => n.dataset.handle))).resolves.toEqual([
    'chief',
    'zeus',
    'zeus-amber-pine',
    'zeus-brisk-birch',
    'diana',
  ])
  const first = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect(first).toHaveAttribute('data-session', 'zeus')
  await expect(first.locator('.row-name')).toHaveText('@zeus · amber-pine')
  await expect(first.locator('.row-meta')).toContainText('worker session of @zeus')
  await expect(first.locator('td[data-state="working"] button.card')).toHaveCount(1)
  const second = page.locator('tr[data-handle="zeus-brisk-birch"]')
  await expect(second.locator('.row-status')).toHaveCount(0)
  await expect(page.locator('tr[data-handle="zeus"] .row-status')).toHaveText(
    '1 terminal open, one per task',
  )
  // Its terminal, once shown, is named in the dock as on its lane.
  await first.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(
    dock.locator('.terminal-card[data-handle="zeus-amber-pine"] .terminal-name'),
  ).toHaveText('@zeus · amber-pine')
})

test('asks the human nothing in For you: the chief asks in its terminal', async ({ page }) => {
  const data = model()
  // Even a question that reached the human's inbox (an older ledger's) is not shown.
  data.inbox[1].push({
    id: 12,
    kind: 'question',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: 1,
    body: 'Ship it to production today?',
    questions: null,
    createdAt: at(2),
  })
  await open(page, data)
  const bay = page.getByRole('region', { name: 'For you' })
  await expect(bay.locator('.foryou-status')).toHaveText('1 note')
  await expect(bay.locator('.strip-message[data-message="12"]')).toHaveCount(0)
  await expect(bay.getByRole('textbox')).toHaveCount(0)
  await expect(bay.locator('.bay-empty')).toHaveCount(0)
  await expect(bay.getByRole('list', { name: 'Waiting for you' })).toHaveCount(0)
})

test("points the human to the chief's terminal while the chief waits for them there", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((l) => l.participant.handle === 'chief')
  chief.activity = { state: 'waiting', reason: 'input needed' }
  await open(page, data)
  const foryou = page.getByRole('region', { name: 'For you' })
  await expect(foryou.locator('.foryou-status')).toHaveText('1 waiting · 1 note')
  const strip = foryou.getByTestId('chief-asks')
  await expect(strip.locator('.strip-title')).toHaveText(
    'The chief is asking you something in its terminal',
  )
  await expect(strip.locator('.strip-route')).toHaveText('Waiting: input needed')
  // Nothing is answered here: the way to its terminal, the dock unfolded and the chief's card in front.
  await expect(strip.getByRole('textbox')).toHaveCount(0)
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await strip.getByRole('button', { name: "Show Chief of Staff's terminal" }).click()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeVisible()
  await expect(page.locator('#stage .terminal-card[data-handle="chief"]')).toHaveAttribute(
    'data-focused',
    'true',
  )
  // Back at work, the chief waits for nothing of the human's.
  await page.evaluate(() => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    lane.activity = { state: 'working' }
    window.__listeners.get('state-changed')()
  })
  await expect(foryou.getByTestId('chief-asks')).toHaveCount(0)
  await expect(foryou.locator('.foryou-status')).toHaveText('1 note')
})

test("shows a task waiting for a member in its requester's backlog, with the tier it waits for", async ({
  page,
}) => {
  await open(page)
  const foryou = page.getByRole('region', { name: 'For you' })
  await expect(foryou.locator('.foryou-status')).toHaveText('1 note')
  // The human puts no task on the board: the chief does, told in its terminal.
  await expect(page.getByRole('button', { name: 'New task' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: /^Give .* a task$/ })).toHaveCount(0)
  const card = page.locator(
    'tr[data-handle="chief"] td[data-state="open"] button.card[data-task="6"]',
  )
  await expect(card.locator('.card-route')).toHaveText('for a standard worker')
  await expect(page.locator('tr[data-handle="zeus"] .row-meta')).toHaveText(
    'worker · standard · Claude Code · claude-sonnet-5 · high',
  )
})

test("draws the chief's row only while a task is on it: its own, or one it asked for that waits for a member", async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').tasks = []
  data.boards[1].open = []
  await open(page, data)
  const row = page.locator('tr[data-handle="chief"]')
  await expect(page.locator('tr[data-handle="zeus"]')).toHaveCount(1)
  await expect(row).toHaveCount(0)
  /** The chief's own tasks, as `tasks` has them. */
  const own = (tasks) =>
    changed(
      page,
      (tasks) => {
        window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief').tasks = tasks
      },
      tasks,
    )
  // A task of its own (cf task add --self): the row comes for it, headed by its name alone.
  await own([task(7, 'Tidy the notes', 'working', 'chief', 'chief', 1)])
  await expect(row.locator('td[data-state="working"] button.card[data-task="7"]')).toHaveCount(1)
  await expect(row.locator('.row-head')).toHaveText('Chief of Staff')
  await expect(row.locator('.row-head button')).toHaveCount(0)
  // Finished, the task stays on the row as any does; with none left, the row goes.
  await own([task(7, 'Tidy the notes', 'accepted', 'chief', 'chief', 1)])
  await expect(row.locator('td[data-state="finished"] button.card[data-task="7"]')).toHaveCount(1)
  await own([])
  await expect(row).toHaveCount(0)
  // A task it asked for, waiting for a member, sits in its backlog: the row comes back for it.
  await changed(
    page,
    (open) => {
      window.__model.boards[1].open = open
    },
    [task(8, 'Write the docs', 'open', 'chief', null, 1, { pool: 'worker', tier: 'standard' })],
  )
  await expect(row.locator('td[data-state="open"] button.card[data-task="8"]')).toHaveCount(1)
})

test('keeps a task paused in the backlog in the backlog, marked paused, and resumes it from there', async ({
  page,
}) => {
  const data = model()
  const paused = task(8, 'Add the lexer', 'paused', 'chief', null, 4, {
    pool: 'worker',
    tier: 'standard',
  })
  data.boards[1].open.push(paused)
  data.tasks['1:8'] = { ...paused, messages: [], resumesOpen: true }
  await open(page, data)
  const chief = page.locator('tr[data-handle="chief"]')
  // Beside T-6, which waits for a member; it is not queued, for no window has it.
  await expect(chief.locator('td[data-state="open"] button.card')).toHaveText([/^T-8/, /^T-6/])
  await expect(page.locator('td[data-state="queued"] button.card[data-task="8"]')).toHaveCount(0)
  const card = chief.locator('td[data-state="open"] button.card[data-task="8"]')
  await expect(card).toHaveAttribute('data-state', 'paused')
  await expect(card.locator('.card-state')).toHaveText('Paused')
  await card.click()
  const drawer = page.getByRole('complementary', { name: 'Task T-8' })
  await expect(drawer.getByRole('button')).toHaveText(['Close', 'Resume', 'Cancel task'])
  await drawer.getByRole('button', { name: 'Resume' }).click()
  await expect.poll(() => calls(page, 'task.resume')).toEqual([{ project: 1, task: 8 }])
  await expect(page.locator('#status')).toHaveText('T-8 is back on the board.')
})

test('lists a task called off or failed before any member had it in Finished, and deletes it as any finished task', async ({
  page,
}) => {
  const data = model()
  const never = { pool: 'worker', tier: 'standard' }
  data.boards[1].open.push(
    task(8, 'Write the changelog', 'cancelled', 'chief', null, 30, never),
    task(9, 'Add the lexer', 'failed', 'chief', null, 20, never),
  )
  data.tasks['1:8'] = { ...data.boards[1].open[1], messages: [] }
  await open(page, data)
  const finished = page.locator('tr[data-handle="chief"] td[data-state="finished"] button.card')
  await expect(finished).toHaveText([/^T-9/, /^T-8/])
  await expect(finished.first()).toHaveAttribute('data-state', 'failed')
  await expect(finished.last()).toHaveAttribute('data-state', 'cancelled')
  // From its drawer, once the human confirms.
  await finished.last().click()
  const drawer = page.getByRole('complementary', { name: 'Task T-8' })
  await expect(drawer.getByRole('button')).toHaveText(['Close', 'Delete task'])
  await drawer.getByRole('button', { name: 'Delete task' }).click()
  await page
    .getByRole('dialog', { name: 'Delete T-8?' })
    .getByRole('button', { name: 'Delete for good' })
    .click()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [8] }])
  await expect(page.locator('button.card[data-task="8"]')).toHaveCount(0)
  // And from the Finished heading, with the others that may go.
  const heading = page.getByRole('table', { name: 'Tasks' }).locator('th[data-state="finished"]')
  await heading.getByRole('button', { name: 'Delete finished' }).click()
  await page
    .getByRole('dialog', { name: 'Delete 3 finished tasks?' })
    .getByRole('button', { name: 'Delete for good' })
    .click()
  await expect
    .poll(() => calls(page, 'tasks.delete'))
    .toEqual([
      { project: 1, tasks: [8] },
      { project: 1, tasks: [3, 5, 9] },
    ])
  await expect(page.locator('button.card[data-task="9"]')).toHaveCount(0)
})

test("keeps a removed member's tasks on the board, on the row of whoever asked: finished in Finished, the rest by their state, a paused one in the backlog", async ({
  page,
}) => {
  const data = model()
  // @athena left the staff: her lane is gone, and no lane has her tasks.
  data.boards[1].open.push(
    task(8, 'Old spike', 'accepted', 'chief', 'athena', 90),
    task(9, 'Add the lexer', 'paused', 'chief', 'athena-brisk-birch', 5),
    task(10, 'Check the grammar', 'done', 'chief', 'athena-calm-brook', 3, {
      result: 'Grammar checked.',
    }),
    task(11, 'Wire the docs', 'cancelled', 'chief', 'athena', 2),
  )
  await open(page, data)
  await expect(page.locator('tr[data-handle="athena"]')).toHaveCount(0)
  const chief = page.locator('tr[data-handle="chief"]')
  await expect(chief.locator('td[data-state="finished"] button.card')).toHaveText([/^T-11/, /^T-8/])
  await expect(chief.locator('td[data-state="open"] button.card')).toHaveText([/^T-9/, /^T-6/])
  await expect(chief.locator('td[data-state="done"] button.card')).toHaveText([/^T-10/])
  await expect(chief.locator('td[data-state="queued"] button.card')).toHaveCount(0)
})

test("says the effort an agent runs at, after its model: on a member's row and a session's; the chief's card its harness and model", async ({
  page,
}) => {
  const data = model()
  const { lanes } = data.boards[1]
  Object.assign(lanes.find((lane) => lane.participant.handle === 'chief').participant, {
    harness: 'codex',
    model: 'gpt-6-astra',
  })
  const zeus = lanes.find((lane) => lane.participant.handle === 'zeus').participant
  lanes.push(
    {
      participant: session(20, zeus, 'amber-pine'),
      tasks: [],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 1 },
    },
    {
      participant: participant(7, 'athena', 'advisor', { harness: 'pi', tier: 'light' }),
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  await unfold(page)
  const meta = (handle) => page.locator(`tr[data-handle="${handle}"] .row-meta`)
  // The chief's card: its harness and model, and no effort, which a chief is not picked at.
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-meta'),
  ).toHaveText('Codex · gpt-6-astra')
  await expect(meta('zeus')).toHaveText('worker · standard · Claude Code · claude-sonnet-5 · high')
  await expect(meta('zeus-amber-pine')).toHaveText(
    'worker session of @zeus · Claude Code · claude-sonnet-5 · high',
  )
  // An agent with no effort shows none.
  await expect(meta('athena')).toHaveText('advisor · light · Pi · muse-spark')
})

test('shows what a task on the board waits for, on its card and in its drawer', async ({
  page,
}) => {
  const data = model()
  data.boards[1].open.push(
    task(7, 'Wire the parser', 'open', 'chief', null, 1, {
      pool: 'worker',
      tier: 'standard',
      needs: [
        { number: 2, state: 'done' },
        { number: 6, state: 'open' },
      ],
      blockedBy: [2, 6],
    }),
  )
  data.tasks['1:7'] = { ...data.boards[1].open[1], messages: [] }
  await open(page, data)
  const card = page.locator(
    'tr[data-handle="chief"] td[data-state="open"] button.card[data-task="7"]',
  )
  await expect(card.locator('.card-route')).toHaveText(
    'blocked by T-2, T-6 · for a standard worker',
  )
  await card.click()
  const drawer = page.getByRole('complementary', { name: 'Task T-7' })
  await expect(drawer.locator('.drawer-meta')).toContainText('needs T-2 (done), T-6 (open)')
})

test("shows a member out of quota, and a reviewer's review tasks as cards on its row like any task", async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(7, 'hera', 'reviewer', { harness: 'pi' }),
    tasks: [
      task(9, 'Review T-8', 'working', 'chief', 'hera', 1, { pool: 'reviewer', tier: 'standard' }),
      task(10, 'Review T-2', 'done', 'chief', 'hera', 2, { pool: 'reviewer', tier: 'standard' }),
    ],
    activity: { state: 'working' },
    pane: { id: 'p1-hera', generation: 2 },
  })
  await open(page, data)
  const diana = page.locator('tr[data-handle="diana"]')
  await expect(diana.getByTestId('lamp')).toHaveAttribute('data-state', 'out')
  await expect(diana.locator('.row-status')).toHaveText(/^Out of quota until (\S+ )?\d\d:\d\d$/)
  const hera = page.locator('tr[data-handle="hera"]')
  await expect(hera.locator('td[data-state="working"] button.card[data-task="9"]')).toHaveCount(1)
  await expect(hera.locator('td[data-state="done"] button.card[data-task="10"]')).toHaveCount(1)
  // A review hangs under no other card: it is a task of its own.
  await expect(page.locator('.reviews')).toHaveCount(0)
})

test("shows a tester's test tasks as cards on its row like any task, and a test nobody has says it waits for a tester", async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(7, 'tess', 'tester', { harness: 'pi', tier: 'light' }),
    tasks: [
      task(9, 'Test T-8', 'working', 'chief', 'tess', 1, { pool: 'tester', tier: 'light' }),
      task(10, 'Test T-2', 'done', 'chief', 'tess', 2, { pool: 'tester', tier: 'light' }),
    ],
    activity: { state: 'working' },
    pane: { id: 'p1-tess', generation: 2 },
  })
  // No tester has taken this one yet: it waits in its requester's backlog for one of its tier.
  data.boards[1].open.push(
    task(11, 'Test T-9', 'open', 'chief', null, 1, { pool: 'tester', tier: 'standard' }),
  )
  await open(page, data)
  const tess = page.locator('tr[data-handle="tess"]')
  await expect(tess.locator('.row-meta')).toHaveText(/^tester · light · Pi/)
  await expect(tess.locator('td[data-state="working"] button.card[data-task="9"]')).toHaveCount(1)
  await expect(tess.locator('td[data-state="done"] button.card[data-task="10"]')).toHaveCount(1)
  const waiting = page.locator(
    'tr[data-handle="chief"] td[data-state="open"] button.card[data-task="11"]',
  )
  await expect(waiting.locator('.card-route')).toHaveText('for a standard tester')
})

test('says an account that refused for its billing or login is unavailable, with no time, and tries it again on the human’s word', async ({
  page,
}) => {
  // MUS-23: nothing resets such an account, so no time is shown and nothing is said to wait for a reset.
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana').participant.outUntil =
    '9999-12-31T23:59:59.998Z'
  await open(page, data)
  const diana = page.locator('tr[data-handle="diana"]')
  await expect(diana.getByTestId('lamp')).toHaveAttribute('data-state', 'out')
  await expect(diana.locator('.row-status')).toHaveText(
    'Unavailable: its account was refused for billing or login. Press Try again now once it works',
  )
  await diana.getByRole('button', { name: 'Try @diana again now, once its account works' }).click()
  expect(await calls(page, 'member.back')).toEqual([{ project: 1, participant: 'diana' }])
})

test('says a member whose harness moved its model on its own is held, why in the words the engine gave, and what to do', async ({
  page,
}) => {
  // MUS-24: the move holds the member as a refused account does, and the row says what moved,
  // and that only the human's word for that member lets it go on.
  const data = model()
  const diana = data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana')
  diana.participant.outUntil = '9999-12-31T23:59:59.999Z'
  diana.activity = {
    state: 'out',
    reason:
      'moved: its model moved from muse-spark-1.3 to muse-spark-1.3-contributor on its own (Muse switched on its own), a model nobody chose',
  }
  await open(page, data)
  await expect(page.locator('tr[data-handle="diana"] .row-status')).toHaveText(
    'Held: its model moved from muse-spark-1.3 to muse-spark-1.3-contributor on its own (Muse switched on its own), a model nobody chose. Press Try again now once it is on the model you chose',
  )
  await expect(
    page.getByRole('button', {
      name: 'Try @diana again now, once it is on the model you chose',
    }),
  ).toBeVisible()
})

test('says a held member whose engine lost its words after a restart is held for its model, not for a billing', async ({
  page,
}) => {
  const data = model()
  const diana = data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana')
  diana.participant.outUntil = '9999-12-31T23:59:59.999Z'
  diana.activity = null
  await open(page, data)
  const status = page.locator('tr[data-handle="diana"] .row-status')
  await expect(status).toHaveText(/^Held: its model moved on its own\. Press Try again now/)
  await expect(status).not.toContainText('billing')
})

test('names the day of a reset that is not today, and tries a member or the chief out of quota again', async ({
  page,
}) => {
  // A weekly limit's reset read "11:00" two days early, and nothing in the
  // app could say the human had logged the harness into another account
  // (poker-lab, 2026-10-04).
  const data = model()
  const reset = new Date(Date.now() + 2 * 86_400_000).toISOString()
  const lanes = data.boards[1].lanes
  lanes.find((lane) => lane.participant.handle === 'diana').participant.outUntil = reset
  lanes.find((lane) => lane.participant.handle === 'chief').participant.outUntil = reset
  await open(page, data)
  const day = await page.evaluate((iso) => {
    const at = new Date(iso)
    const time = at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
    return `${at.toLocaleDateString([], { weekday: 'short' })} ${time}`
  }, reset)
  const diana = page.locator('tr[data-handle="diana"]')
  await expect(diana.locator('.row-status')).toHaveText(`Out of quota until ${day}`)
  await diana.getByRole('button', { name: 'Try @diana again now, before its quota resets' }).click()
  expect(await calls(page, 'member.back')).toEqual([{ project: 1, participant: 'diana' }])
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.locator('.terminal-status')).toHaveText(`Out of quota until ${day}`)
  await card
    .getByRole('button', { name: 'Try Chief of Staff again now, before its quota resets' })
    .click()
  expect(await calls(page, 'member.back')).toEqual([
    { project: 1, participant: 'diana' },
    { project: 1, participant: 'chief' },
  ])
  // A member not out has nothing to try again.
  await expect(page.locator('tr[data-handle="zeus"] .try-again')).toHaveCount(0)
})

test('a task held while its member is out of quota says when it goes on', async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  lane.tasks.push(task(9, 'Write the docs', 'paused', 'chief', 'zeus', 4, { heldUntil: at(-25) }))
  await open(page, data)
  await expect(page.locator('button.card[data-task="9"] .card-route')).toHaveText(
    /^out of quota until (\S+ )?\d\d:\d\d · from /,
  )
})

test('a task held for an account no time resets says it waits for the human, not for a time', async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  lane.tasks.push(
    task(9, 'Write the docs', 'paused', 'chief', 'zeus', 4, {
      heldUntil: '9999-12-31T23:59:59.998Z',
    }),
  )
  lane.tasks.push(
    task(10, 'Write the notes', 'paused', 'chief', 'zeus', 5, {
      heldUntil: '9999-12-31T23:59:59.999Z',
    }),
  )
  await open(page, data)
  await expect(page.locator('button.card[data-task="9"] .card-route')).toHaveText(
    /^unavailable until you say it works again · from /,
  )
  await expect(page.locator('button.card[data-task="10"] .card-route')).toHaveText(
    /^held until you say its model may go on · from /,
  )
})

test("opens a card's drawer on its task's story, and leaves accepting and reviews to the chief", async ({
  page,
}) => {
  await open(page)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  const steps = drawer.getByRole('list', { name: "T-2's story" }).locator('.step')
  await expect(steps.locator('.step-label')).toHaveText(['Request 1', 'Result 1'])
  await expect(steps.last().locator('.step-body')).toHaveText('Parser done, 14 tests.')
  // Accepting and asking for a review are the chief's, from its terminal.
  await expect(drawer.getByRole('button', { name: 'Accept' })).toHaveCount(0)
  await expect(drawer.getByRole('button', { name: /review/i })).toHaveCount(0)
  // Nothing here writes to the agent: that is done in its terminal.
  await expect(drawer.getByRole('textbox')).toHaveCount(0)
  // The story is one panel: no brief, result or thread apart from it.
  await expect(drawer.locator('.drawer-section-head')).toHaveText(['Story'])
})

/**
 * Harbour with T-15, which went brief, question, answer, result, follow-up
 * and result: the brief two days ago, the rest today, the last just now.
 */
function storyModel() {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(15, 'Write the lexer', 'done', 'chief', 'zeus', 0))
  data.tasks['1:15'] = {
    ...lane.tasks.at(-1),
    messages: [
      message(50, 'task', 'chief', 'zeus', 'Write the lexer', 2 * 24 * 60),
      message(
        51,
        'question',
        'zeus',
        'chief',
        '\nWhich grammar: the old one or the new one?\nThe old one is in lexer.old.',
        30,
      ),
      message(52, 'answer', 'chief', 'zeus', 'The new one.', 25),
      message(53, 'result', 'zeus', 'chief', '**Lexer done**, 22 tests.', 20),
      message(54, 'task', 'chief', 'zeus', 'Reopened: cover the errors too.', 10),
      message(55, 'result', 'zeus', 'chief', 'Errors covered, 31 tests.', 0),
    ],
  }
  return data
}

/** Which steps of a story are open, in order. */
const opened = (steps) => steps.evaluateAll((all) => all.map((step) => step.open))

test("tells a task's story in the order it happened, each request and the result it brought numbered by round", async ({
  page,
}) => {
  await open(page, storyModel())
  await page.locator('button.card[data-task="15"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-15' })
  const steps = drawer.getByRole('list', { name: "T-15's story" }).locator('.step')
  await expect(steps.locator('.step-label')).toHaveText([
    'Request 1',
    'Question',
    'Answer',
    'Result 1',
    'Request 2',
    'Result 2',
  ])
  const [asks, tells] = ['@chief → @zeus', '@zeus → @chief']
  await expect(steps.locator('.step-route')).toHaveText([asks, tells, asks, tells, asks, tells])
  // Each step's marker takes the colour of its kind.
  expect(await steps.evaluateAll((all) => all.map((step) => step.dataset.kind))).toEqual([
    'task',
    'question',
    'answer',
    'result',
    'task',
    'result',
  ])
  // When each was sent: the time today, the date with it before; all of it on hover.
  await expect(steps.nth(5).locator('time')).toHaveText(/^\d\d:\d\d$/)
  const first = steps.nth(0).locator('time')
  await expect(first).toHaveText(/^[A-Z][a-z]{2} \d{1,2} \d\d:\d\d$/)
  await expect(first).toHaveAttribute(
    'title',
    /^[A-Z][a-z]+day, [A-Z][a-z]+ \d{1,2}, \d{4} at \d\d:\d\d:\d\d$/,
  )
})

test('opens only the newest step of a story, and folds each of the others to its first line', async ({
  page,
}) => {
  await open(page, storyModel())
  await page.locator('button.card[data-task="15"]').click()
  const steps = page.getByRole('list', { name: "T-15's story" }).locator('.step')
  expect(await opened(steps)).toEqual([false, false, false, false, false, true])
  // A folded step reads the first line of its body that says something,
  // its marks gone; the open one reads whole.
  const question = steps.nth(1)
  await expect(question.locator('.step-preview')).toHaveText(
    'Which grammar: the old one or the new one?',
  )
  await expect(question.locator('.step-preview')).toBeVisible()
  await expect(question.locator('.step-body')).toBeHidden()
  await expect(steps.nth(3).locator('.step-preview')).toHaveText('Lexer done, 22 tests.')
  const newest = steps.nth(5)
  await expect(newest.locator('.step-preview')).toBeHidden()
  await expect(newest.locator('.step-body')).toHaveText('Errors covered, 31 tests.')
  // A line too long for the drawer ends in an ellipsis: one line, cut by CSS.
  expect(
    await question
      .locator('.step-preview')
      .evaluate((node) => [getComputedStyle(node).whiteSpace, getComputedStyle(node).textOverflow]),
  ).toEqual(['nowrap', 'ellipsis'])
  // Opened, a step reads whole in place of its first line; from the keyboard too.
  await question.locator('summary').click()
  await expect(question.locator('.step-body')).toHaveText(
    'Which grammar: the old one or the new one?\nThe old one is in lexer.old.',
    { useInnerText: true },
  )
  await expect(question.locator('.step-preview')).toBeHidden()
  await steps.nth(2).locator('summary').focus()
  await page.keyboard.press('Enter')
  await expect(steps.nth(2).locator('.step-body')).toHaveText('The new one.')
})

test('keeps each step of a story open or shut as the human left it when the task is drawn again', async ({
  page,
}) => {
  const data = storyModel()
  // The follow-up is still on its way; the next read finds it delivered.
  data.tasks['1:15'].messages[4].state = 'queued'
  await open(page, data)
  await page.locator('button.card[data-task="15"]').click()
  const steps = page.getByRole('list', { name: "T-15's story" }).locator('.step')
  await steps.nth(1).locator('summary').click()
  await steps.nth(5).locator('summary').click()
  expect(await opened(steps)).toEqual([false, true, false, false, false, false])
  const follow = steps.nth(4)
  await expect(follow.locator('.step-state')).toHaveText('queued')
  await follow.evaluate((step) => {
    step.kept = true
  })
  await changed(page, () => {
    window.__model.tasks['1:15'].messages[4].state = 'delivered'
  })
  await expect(follow.locator('.step-state')).toHaveCount(0)
  await expect.poll(() => opened(steps)).toEqual([false, true, false, false, false, false])
  // The step that changed was drawn again where it is.
  expect(await follow.evaluate((step) => step.kept)).toBe(true)
  // A redraw that changes nothing of the task leaves its story as it is.
  await changed(page, setLamp, ['zeus', 'working'])
  expect(await opened(steps)).toEqual([false, true, false, false, false, false])
})

test('opens the newest step when a new message arrives, and leaves the others as the human left them', async ({
  page,
}) => {
  await open(page, storyModel())
  await page.locator('button.card[data-task="15"]').click()
  const steps = page.getByRole('list', { name: "T-15's story" }).locator('.step')
  await steps.nth(1).locator('summary').click()
  await steps.nth(5).locator('summary').click()
  await changed(page, () => {
    window.__model.tasks['1:15'].messages.push({
      id: 57,
      kind: 'task',
      sender: 'chief',
      recipient: 'zeus',
      state: 'delivered',
      reason: null,
      body: 'One more: the docs.',
      createdAt: new Date().toISOString(),
    })
  })
  await expect(steps.locator('.step-label').last()).toHaveText('Request 3')
  await expect.poll(() => opened(steps)).toEqual([false, true, false, false, false, false, true])
})

test("starts the story of a task not yet given, or of the chief's own, with its body as Request 1", async ({
  page,
}) => {
  const data = model()
  const board = data.boards[1]
  board.open.push(
    task(7, 'Draw the logo', 'open', 'chief', null, 1, { pool: 'designer', tier: null }),
  )
  data.tasks['1:6'] = { ...board.open[0], messages: [] }
  data.tasks['1:7'] = { ...board.open[1], messages: [] }
  const chief = board.lanes.find((l) => l.participant.handle === 'chief')
  data.tasks['1:1'] = { ...chief.tasks[0], messages: [] }
  await open(page, data)
  const storyOf = async (number) => {
    await page.locator(`button.card[data-task="${number}"]`).click()
    return page
      .getByRole('complementary', { name: `Task T-${number}` })
      .getByRole('list', { name: `T-${number}'s story` })
      .locator('.step')
  }
  const docs = await storyOf(6)
  await expect(docs.locator('.step-label')).toHaveText(['Request 1'])
  await expect(docs.locator('.step-route')).toHaveText(['@chief → a standard worker'])
  await expect(docs.locator('.step-body')).toHaveText('Write the docs')
  expect(await opened(docs)).toEqual([true])
  // It was never sent, so it has no time to say.
  await expect(docs.locator('time')).toHaveCount(0)
  const logo = await storyOf(7)
  await expect(logo.locator('.step-route')).toHaveText(['@chief → an image designer'])
  await expect(
    page.getByRole('complementary', { name: 'Task T-7' }).locator('.drawer-meta'),
  ).toContainText('@chief asked for an image designer ·')
  const own = await storyOf(1)
  await expect(own.locator('.step-label')).toHaveText(['Request 1'])
  await expect(own.locator('.step-route')).toHaveText(['@human → @chief'])
})

test('says in its step how a message is on its way, and nothing once it reached its window', async ({
  page,
}) => {
  const data = storyModel()
  const [brief, , answer, result, follow, last] = data.tasks['1:15'].messages
  brief.state = 'delivering'
  // An answer picked from options is read at once by the window that asked.
  answer.state = 'read'
  Object.assign(result, {
    state: 'failed',
    reason: 'its window closed while it was handed over',
  })
  follow.state = 'gated'
  Object.assign(last, { state: 'queued', reason: 'the harness ran out of quota' })
  await open(page, data)
  await page.locator('button.card[data-task="15"]').click()
  const steps = page.getByRole('list', { name: "T-15's story" }).locator('.step')
  const said = await steps.evaluateAll((all) =>
    all.map((step) => step.querySelector('.step-state')?.textContent ?? null),
  )
  expect(said).toEqual([
    'delivering',
    null,
    null,
    'not delivered: its window closed while it was handed over',
    'needs your approval',
    'queued: the harness ran out of quota',
  ])
})

/** Harbour with T-15's last result written as `body`, the agent's markdown. */
function resultModel(body) {
  const data = storyModel()
  data.tasks['1:15'].messages[5].body = body
  return data
}

test('draws the markdown an agent wrote in an open step: bold, italic, code, headings, lists, tables and quotes', async ({
  page,
}) => {
  await open(
    page,
    resultModel(
      [
        '## Lexer done',
        '',
        'All **31 tests** pass; errors now say *where*.',
        'Run them with `npm test`.',
        '',
        '| File | Change |',
        '| --- | --- |',
        '| `lib/lexer.js` | tokens carry their position |',
        '| lib/errors.js | a new **LexError** |',
        '',
        '1. Read the tokens',
        '2. Keep their place',
        '   - line, from *1*',
        '   - column',
        '3. Say where it failed',
        '',
        '```js',
        'const where = a*b*c // <b>as written</b>',
        '```',
        '',
        '> Errors read as before.',
        '',
        'See [the notes](https://example.com/notes) for the grammar.',
      ].join('\n'),
    ),
  )
  await page.locator('button.card[data-task="15"]').click()
  const body = page.getByRole('list', { name: "T-15's story" }).locator('.step').last()
  const drawn = body.locator('.step-body')
  // A heading is a bold line, not a heading of the page.
  await expect(drawn.locator('.md-heading')).toHaveText('Lexer done')
  await expect(drawn.locator('h1, h2, h3, h4, h5, h6')).toHaveCount(0)
  // A paragraph keeps its line breaks.
  const paragraph = drawn.locator('> p').nth(1)
  await expect(paragraph).toHaveText(
    'All 31 tests pass; errors now say where.\nRun them with npm test.',
    {
      useInnerText: true,
    },
  )
  await expect(paragraph.locator('br')).toHaveCount(1)
  await expect(paragraph.locator('strong')).toHaveText('31 tests')
  await expect(paragraph.locator('em')).toHaveText('where')
  await expect(paragraph.locator('code')).toHaveText('npm test')
  const table = drawn.locator('table')
  await expect(table.locator('th')).toHaveText(['File', 'Change'])
  await expect(table.locator('td')).toHaveText([
    'lib/lexer.js',
    'tokens carry their position',
    'lib/errors.js',
    'a new LexError',
  ])
  await expect(table.locator('td code')).toHaveText(['lib/lexer.js'])
  await expect(table.locator('td strong')).toHaveText(['LexError'])
  // A list nests by its indent.
  const items = drawn.locator('> ol > li')
  await expect(items).toHaveCount(3)
  await expect(items.nth(1).locator('> ul > li')).toHaveText(['line, from 1', 'column'])
  await expect(items.nth(1).locator('em')).toHaveText('1')
  // Code is as written: no marks read in it, no markup either.
  await expect(drawn.locator('pre code')).toHaveText('const where = a*b*c // <b>as written</b>')
  await expect(drawn.locator('pre em, pre b')).toHaveCount(0)
  await expect(drawn.locator('blockquote')).toHaveText('Errors read as before.')
  // A link is its words, where it leads on hover: nothing to follow.
  await expect(drawn.locator('a')).toHaveCount(0)
  const link = drawn.getByTitle('https://example.com/notes')
  await expect(link).toHaveText('the notes')
  await expect(drawn.locator('> p').last()).toHaveText('See the notes for the grammar.')
})

test('shows markup in what an agent wrote as text, never as elements, its markdown drawn around it', async ({
  page,
}) => {
  const dialogs = []
  page.on('dialog', (dialog) => {
    dialogs.push(dialog.message())
    dialog.dismiss()
  })
  const script = '<script>alert(1)</script>'
  const image = '<img src=x onerror=alert(1)>'
  const data = resultModel(
    [
      script,
      image,
      '',
      `**${image}** in bold, \`${script}\` as code.`,
      '',
      '| tag | what |',
      '| --- | --- |',
      `| ${image} | an image |`,
      '',
      `[${script}](javascript:alert(1))`,
    ].join('\n'),
  )
  data.tasks['1:15'].messages[0].body = `${image} ${script}`
  await open(page, data)
  await page.locator('button.card[data-task="15"]').click()
  const steps = page.getByRole('list', { name: "T-15's story" }).locator('.step')
  const drawn = steps.last().locator('.step-body')
  await expect(drawn.locator('> p').first()).toHaveText(`${script}\n${image}`, {
    useInnerText: true,
  })
  await expect(drawn.locator('strong')).toHaveText(image)
  await expect(drawn.locator('code')).toHaveText(script)
  await expect(drawn.locator('td').first()).toHaveText(image)
  await expect(drawn.getByTitle('javascript:alert(1)')).toHaveText(script)
  // A folded step's line reads it as text too.
  await expect(steps.first().locator('.step-preview')).toHaveText(`${image} ${script}`)
  const drawer = page.getByRole('complementary', { name: 'Task T-15' })
  await expect(drawer.locator('script, img, a, iframe')).toHaveCount(0)
  await page.waitForTimeout(100)
  expect(dialogs).toEqual([])
})

test('pauses a task from its drawer, shows it paused in the queue, and resumes it', async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(11, 'Add the lexer', 'paused', 'chief', 'zeus', 4))
  data.tasks['1:4'] = { ...lane.tasks.find((t) => t.number === 4), messages: [] }
  data.tasks['1:11'] = { ...lane.tasks.find((t) => t.number === 11), messages: [] }
  await open(page, data)
  const paused = page.locator(
    'tr[data-handle="zeus"] td[data-state="queued"] button.card[data-task="11"]',
  )
  await expect(paused).toHaveAttribute('data-state', 'paused')
  await expect(paused.locator('.card-state')).toHaveText('Paused')
  await page.locator('button.card[data-task="4"]').click()
  const queued = page.getByRole('complementary', { name: 'Task T-4' })
  await queued.getByRole('button', { name: 'Pause' }).click()
  await expect.poll(() => calls(page, 'task.pause')).toEqual([{ project: 1, task: 4 }])
  await expect(page.locator('#status')).toHaveText('T-4 is paused; its work waits.')
  await paused.click()
  const drawer = page.getByRole('complementary', { name: 'Task T-11' })
  await expect(drawer.getByRole('button', { name: 'Pause' })).toHaveCount(0)
  await expect(drawer.getByRole('textbox')).toHaveCount(0)
  await drawer.getByRole('button', { name: 'Resume' }).click()
  await expect.poll(() => calls(page, 'task.resume')).toEqual([{ project: 1, task: 11 }])
  await expect(page.locator('#status')).toHaveText('T-11 resumes in @zeus.')
})

test('says a resumed task is back on the board once the window that had it has ended', async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(11, 'Add the lexer', 'paused', 'chief', 'zeus', 4))
  data.tasks['1:11'] = { ...lane.tasks.at(-1), messages: [], resumesOpen: true }
  await open(page, data)
  await page.locator('button.card[data-task="11"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-11' })
  await drawer.getByRole('button', { name: 'Resume' }).click()
  await expect(page.locator('#status')).toHaveText(
    'T-11 is back on the board: the window that had it has ended.',
  )
})

test("cancels a task from its drawer, and offers the chief's own work no Pause", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((l) => l.participant.handle === 'chief')
  data.tasks['1:1'] = { ...chief.tasks[0], messages: [] }
  data.tasks['1:4'] = { ...task(4, 'Add the tests', 'queued', 'chief', 'zeus', 1), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="1"]').click()
  const own = page.getByRole('complementary', { name: 'Task T-1' })
  await expect(own.getByRole('button')).toHaveText(['Close', 'Show terminal', 'Cancel task'])
  await page.locator('button.card[data-task="4"]').click()
  const queued = page.getByRole('complementary', { name: 'Task T-4' })
  await queued.getByRole('button', { name: 'Cancel task' }).click()
  await expect.poll(() => calls(page, 'task.cancel')).toEqual([{ project: 1, task: 4 }])
})

test("reassigns a task given by tier from its drawer, working or paused, never the chief's own", async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const tiered = { pool: 'worker', tier: 'light' }
  lane.tasks.push(
    task(12, 'Tell a joke', 'working', 'chief', 'zeus', 3, tiered),
    task(13, 'Add the lexer', 'paused', 'chief', 'zeus', 4, tiered),
  )
  data.tasks['1:12'] = { ...lane.tasks.find((t) => t.number === 12), messages: [] }
  data.tasks['1:13'] = { ...lane.tasks.find((t) => t.number === 13), messages: [] }
  data.tasks['1:4'] = { ...lane.tasks.find((t) => t.number === 4), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="12"]').click()
  const working = page.getByRole('complementary', { name: 'Task T-12' })
  await working.getByRole('button', { name: 'Reassign T-12 to another member of its tier' }).click()
  await expect.poll(() => calls(page, 'task.reassign')).toEqual([{ project: 1, task: 12 }])
  await expect(page.locator('#status')).toHaveText(
    'T-12 is back on the board for another light worker.',
  )
  await page.locator('button.card[data-task="13"]').click()
  const paused = page.getByRole('complementary', { name: 'Task T-13' })
  await expect(paused.getByRole('button', { name: /^Reassign/ })).toHaveCount(1)
  // Work given by name (the chief's own) has no tier to go back to.
  await page.locator('button.card[data-task="4"]').click()
  const named = page.getByRole('complementary', { name: 'Task T-4' })
  await expect(named.getByRole('button', { name: /^Reassign/ })).toHaveCount(0)
})

/** A board whose worker rests, its task T-12 going on in the background as its lane says. */
function heldModel(held = { task: 12, jobs: 1, agents: 0 }) {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(
    task(12, 'Serve the docs', 'working', 'chief', 'zeus', 3, { pool: 'worker', tier: 'standard' }),
  )
  // The worker's turn ended with a job running that never ends: its window is at rest.
  lane.activity = { state: 'idle' }
  lane.held = held
  data.tasks['1:12'] = { ...lane.tasks.at(-1), messages: [] }
  return data
}

test('says on the card of a task that goes on in the background what it waits on, and on its lane that the window waits and does not idle', async ({
  page,
}) => {
  await open(page, heldModel())
  const card = page.locator('button.card[data-task="12"]')
  await expect(card).toHaveAttribute('data-pending', 'background')
  await expect(card.locator('.card-held')).toHaveText('waiting on a background job')
  await expect(card).toHaveAttribute('aria-label', /Working, waiting on a background job/)
  const lane = page.locator('tr[data-handle="zeus"] .row-status')
  await expect(lane).toHaveText('Waiting on a background job')
  await expect(lane).toHaveAttribute('data-state', 'held')
  // A card of the same lane that waits on nothing says nothing of it.
  await expect(page.locator('button.card[data-task="2"] .card-held')).toHaveCount(0)
  await expect(page.locator('button.card[data-task="2"]')).not.toHaveAttribute('data-pending')
})

for (const [held, words] of [
  [{ task: 12, jobs: 3, agents: 0 }, 'waiting on 3 background jobs'],
  [{ task: 12, jobs: 0, agents: 1 }, 'waiting on a background agent'],
  [{ task: 12, jobs: 0, agents: 2 }, 'waiting on 2 background agents'],
  [{ task: 12, jobs: 1, agents: 2 }, 'waiting on background jobs and agents'],
]) {
  test(`says "${words}" on the card of a task whose lane counts ${held.jobs} jobs and ${held.agents} agents`, async ({
    page,
  }) => {
    await open(page, heldModel(held))
    await expect(page.locator('button.card[data-task="12"] .card-held')).toHaveText(words)
  })
}

test('finishes a task that goes on in the background from its drawer, for the human: Finish asks the daemon and says what follows', async ({
  page,
}) => {
  await open(page, heldModel())
  await page.locator('button.card[data-task="12"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-12' })
  await expect(drawer.locator('.drawer-meta')).toContainText('Working, waiting on a background job')
  await drawer
    .getByRole('button', {
      name: 'Finish T-12: what its worker said last is its result; nothing it left running is ended',
    })
    .click()
  await expect.poll(() => calls(page, 'task.finish')).toEqual([{ project: 1, task: 12 }])
  await expect(page.locator('#status')).toHaveText(
    "T-12 is finished: what its worker said last is its result. What it waits on is not ended: it may run on until its window closes, or past it where the host cannot tell what is the window's.",
  )
})

for (const [held, words] of [
  [{ jobs: 0, agents: 1 }, 'it may run on'],
  [{ jobs: 0, agents: 2 }, 'they may run on'],
  [{ jobs: 1, agents: 2 }, 'they may run on'],
]) {
  test(`says on Finish that the ${held.jobs} jobs and ${held.agents} agents its turn left are not ended and may run on`, async ({
    page,
  }) => {
    // Both reviews of rounds 3 and 4: what the turn left ran on after the result, in a
    // window the human had open or one kept for another task, and Finish had said it would
    // end. Both of round 6: nor does the close end all of it.
    const data = heldModel({ task: 12, ...held })
    data.finishLeaves = held
    await open(page, data)
    await page.locator('button.card[data-task="12"]').click()
    // The button promises no end of it either.
    await expect(finishOf(page, 'Task T-12')).toHaveAccessibleName(
      'Finish T-12: what its worker said last is its result; nothing it left running is ended',
    )
    await finishOf(page, 'Task T-12').click()
    await expect(page.locator('#status')).toHaveText(
      `T-12 is finished: what its worker said last is its result. What it waits on is not ended: ${words} until its window closes, or past it where the host cannot tell what is the window's.`,
    )
  })
}

/** The Finish button of the drawer of the task `name` (`Task T-12`) names, as the human reads it. */
const finishOf = (page, name) =>
  page.getByRole('complementary', { name }).getByRole('button', { name: /^Finish/ })

test('offers Finish for any working task of a member, held or not, and for none that is not working', async ({
  page,
}) => {
  // The owner's decision of 2026-10-18: the chief and the human can finish any working
  // task, whatever its window's reader says of its turn.
  const data = heldModel()
  data.finishLeaves = { jobs: 0, agents: 0 }
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(13, 'Add the lexer', 'working', 'chief', 'zeus', 4))
  data.tasks['1:13'] = { ...lane.tasks.at(-1), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="13"]').click()
  await expect(page.getByRole('complementary', { name: 'Task T-13' })).toBeVisible()
  await expect(page.locator('button.card[data-task="13"] .card-held')).toHaveCount(0)
  await finishOf(page, 'Task T-13').click()
  await expect.poll(() => calls(page, 'task.finish')).toEqual([{ project: 1, task: 13 }])
  await expect(page.locator('#status')).toHaveText(
    'T-13 is finished: what its worker said last is its result. Nothing it left running was ended.',
  )
  // A task that is done is none to finish.
  await page.locator('button.card[data-task="2"]').click()
  await expect(page.getByRole('complementary', { name: 'Task T-2' })).toBeVisible()
  await expect(finishOf(page, 'Task T-2')).toHaveCount(0)
})

test("offers Finish for a task that waits on a question, which the chief or the human may take as the result of its worker's last words", async ({
  page,
}) => {
  // The words a turn ended on may be raised as a question that nobody meant (T-1119): the
  // task waits, and Finish is the way to take them as the result.
  const data = heldModel()
  data.finishLeaves = { jobs: 0, agents: 0 }
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(14, 'Install the toolchain', 'waiting', 'chief', 'zeus', 4))
  data.tasks['1:14'] = { ...lane.tasks.at(-1), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="14"]').click()
  await expect(page.getByRole('complementary', { name: 'Task T-14' })).toBeVisible()
  await expect(finishOf(page, 'Task T-14')).toHaveCount(1)
  await finishOf(page, 'Task T-14').click()
  await expect.poll(() => calls(page, 'task.finish')).toEqual([{ project: 1, task: 14 }])
})

test('says nothing of waiting on a task that is paused, though its lane is held for it, and offers Finish, which takes what its worker said last', async ({
  page,
}) => {
  // A pause leaves the window and the job (the owner's decision): the task is paused first,
  // and resumed before it is finished.
  const data = heldModel()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.find((t) => t.number === 12).state = 'paused'
  data.tasks['1:12'].state = 'paused'
  await open(page, data)
  await expect(page.locator('button.card[data-task="12"] .card-held')).toHaveCount(0)
  await expect(page.locator('button.card[data-task="12"]')).not.toHaveAttribute('data-pending')
  // The window still holds its job, whatever the task is, and its lane says so.
  await expect(page.locator('tr[data-handle="zeus"] .row-status')).toHaveText(
    'Waiting on a background job',
  )
  await page.locator('button.card[data-task="12"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-12' })
  await expect(drawer).toBeVisible()
  await expect(drawer.locator('.drawer-meta')).toContainText('Paused')
  await expect(drawer.locator('.drawer-meta')).not.toContainText('waiting on')
  // The human's call, as the chief's `cf task finish` is: what the worker of a paused task
  // said last since its latest message is the result (a tell's turn that ended with its
  // answer, T-442), and nothing it left running is ended.
  await expect(finishOf(page, 'Task T-12')).toHaveCount(1)
  await finishOf(page, 'Task T-12').click()
  await expect.poll(() => calls(page, 'task.finish')).toEqual([{ project: 1, task: 12 }])
})

test('offers no Finish on a closed project, where nothing acts, though its task is held', async ({
  page,
}) => {
  const data = heldModel()
  data.boards[1].project.state = 'suspended'
  data.projects[0].state = 'suspended'
  await open(page, data)
  await page.locator('button.card[data-task="12"]').click()
  await expect(page.getByRole('complementary', { name: 'Task T-12' })).toBeVisible()
  await expect(finishOf(page, 'Task T-12')).toHaveCount(0)
})

test('deletes a finished task from its drawer once the human confirms, and closes the drawer', async ({
  page,
}) => {
  const data = model()
  data.tasks['1:5'] = { ...task(5, 'Old spike', 'accepted', 'chief', 'zeus', 90), messages: [] }
  data.inbox[1].push({
    id: 10,
    kind: 'note',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: 5,
    body: 'The spike is in.',
    createdAt: at(1),
  })
  await open(page, data)
  // A task the chief has still to decide is not over: it has no Delete.
  await page.locator('button.card[data-task="2"]').click()
  const done = page.getByRole('complementary', { name: 'Task T-2' })
  await expect(done.getByRole('button', { name: /^Delete/ })).toHaveCount(0)
  await done.getByRole('button', { name: 'Close the task' }).click()
  await page.locator('button.card[data-task="5"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-5' })
  await expect(drawer.getByRole('button')).toHaveText(['Close', 'Delete task'])
  await drawer.getByRole('button', { name: 'Delete task' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete T-5?' })
  await expect(dialog.locator('.dialog-help')).toHaveText(
    'A deleted task leaves the board and cf task list for good. The ledger keeps it: the chief can still read it, its brief, result and thread, with cf task get. This cannot be undone.',
  )
  await expect(dialog.getByRole('list', { name: 'Kept on the board' })).toBeHidden()
  await dialog.getByRole('button', { name: 'Keep' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'tasks.delete')).toEqual([])
  await expect(drawer).toBeVisible()
  await drawer.getByRole('button', { name: 'Delete task' }).click()
  await dialog.getByRole('button', { name: 'Delete for good' }).click()
  await expect(dialog).toBeHidden()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [5] }])
  await expect(drawer).toBeHidden()
  await expect(page.locator('#status')).toHaveText('T-5 is deleted.')
  await expect(page.locator('button.card[data-task="5"]')).toHaveCount(0)
  // Its record still reads, from a note about it, with nothing left to do on it.
  await page
    .locator('.strip-message[data-message="10"]')
    .getByRole('button', { name: 'Open task' })
    .click()
  await expect(drawer.getByRole('button')).toHaveText(['Close'])
})

test('deletes every finished task that may go from the Finished heading, after one confirmation that says how many and which stay', async ({
  page,
}) => {
  const data = model()
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  zeus.tasks.push(task(8, 'Old docs', 'cancelled', 'chief', 'zeus', 60))
  // Waiting on the board for diana's failed T-3: T-3 stays.
  data.boards[1].open.push(
    task(9, 'Retry the parser', 'open', 'chief', null, 1, {
      pool: 'worker',
      tier: 'standard',
      needs: [{ number: 3, state: 'failed' }],
      blockedBy: [3],
    }),
  )
  data.tasks['1:5'] = { ...zeus.tasks.find((t) => t.number === 5), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="5"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-5' })
  await expect(drawer).toBeVisible()
  // The drawer lies over the board's last columns: the heading is reached with the keyboard.
  const heading = page.getByRole('table', { name: 'Tasks' }).locator('th[data-state="finished"]')
  await heading.getByRole('button', { name: 'Delete finished' }).press('Enter')
  const dialog = page.getByRole('dialog', { name: 'Delete 2 finished tasks?' })
  await expect(
    dialog.getByRole('list', { name: 'Kept on the board' }).getByRole('listitem'),
  ).toHaveText(['T-3 stays: T-9 still needs it, and it failed: the chief can still reopen it.'])
  await expect(dialog.locator('#delete-tasks-going')).toBeVisible()
  await expect(dialog.locator('#delete-tasks-going')).toHaveText('T-5 and T-8 go.')
  await dialog.getByRole('button', { name: 'Keep' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'tasks.delete')).toEqual([])
  await heading.getByRole('button', { name: 'Delete finished' }).press('Enter')
  await dialog.getByRole('button', { name: 'Delete for good' }).click()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [5, 8] }])
  await expect(page.locator('#status')).toHaveText('2 finished tasks are deleted.')
  await expect(drawer).toBeHidden()
  await expect(page.locator('td[data-state="finished"] button.card')).toHaveText([/^T-3/])
  // What is left is kept for T-9: the heading still offers the control, and it says why
  // nothing more may go.
  const control = heading.getByRole('button', { name: 'Delete finished' })
  await expect(control).toBeVisible()
  await control.click()
  await expect(
    page.getByRole('dialog', { name: 'No finished task can be deleted now' }),
  ).toBeVisible()
})

test('offers Delete finished when every finished task is needed, and deletes the accepted ones, which the tasks that need them still read as accepted', async ({
  page,
}) => {
  const data = model()
  const board = data.boards[1]
  const zeus = board.lanes.find((lane) => lane.participant.handle === 'zeus')
  board.lanes.find((lane) => lane.participant.handle === 'diana').tasks = []
  zeus.tasks.push(task(8, 'Stage one', 'accepted', 'chief', 'zeus', 70))
  // Two tasks the chief holds wait on both: each finished task is needed, so none could go.
  const waits = (number, title, state) =>
    task(number, title, state, 'chief', null, 1, {
      pool: 'worker',
      tier: 'standard',
      needs: [
        { number: 5, state: 'accepted' },
        { number: 8, state: 'accepted' },
      ],
      blockedBy: [],
    })
  board.open.push(waits(7, 'Stage two', 'paused'), waits(9, 'Stage three', 'open'))
  data.tasks['1:7'] = { ...board.open.at(-2), messages: [] }
  await open(page, data)
  const finished = page.locator('td[data-state="finished"] button.card')
  await expect(finished).toHaveText([/^T-8/, /^T-5/])
  const heading = page.getByRole('table', { name: 'Tasks' }).locator('th[data-state="finished"]')
  const control = heading.getByRole('button', { name: 'Delete finished' })
  await expect(control).toBeVisible()
  await control.click()
  const dialog = page.getByRole('dialog', { name: 'Delete 2 finished tasks?' })
  await expect(dialog.locator('#delete-tasks-going')).toBeVisible()
  await expect(dialog.locator('#delete-tasks-going')).toHaveText('T-5 and T-8 go.')
  // Nothing stays: an accepted task is accepted for good, deleted or not.
  await expect(dialog.getByRole('list', { name: 'Kept on the board' })).toBeHidden()
  await dialog.getByRole('button', { name: 'Delete for good' }).click()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [5, 8] }])
  await expect(finished).toHaveCount(0)
  // The column is empty now: nothing to delete.
  await expect(heading.getByRole('button')).toHaveCount(0)
  // What waited for them still reads them as accepted.
  await page.locator('button.card[data-task="7"]').click()
  await expect(
    page.getByRole('complementary', { name: 'Task T-7' }).locator('.drawer-meta'),
  ).toContainText('needs T-5 (accepted), T-8 (accepted)')
})

test('says plainly that nothing can be deleted when every finished task is a failed one that others need, and deletes nothing', async ({
  page,
}) => {
  const data = model()
  const board = data.boards[1]
  board.lanes.find((lane) => lane.participant.handle === 'zeus').tasks.pop()
  // T-3 failed, and the chief may reopen it: two tasks wait for it.
  const waits = (number, state) =>
    task(number, `Waits ${number}`, state, 'chief', null, 1, {
      pool: 'worker',
      tier: 'standard',
      needs: [{ number: 3, state: 'failed' }],
      blockedBy: [3],
    })
  board.open.push(waits(9, 'open'), waits(10, 'paused'))
  await open(page, data)
  await expect(page.locator('td[data-state="finished"] button.card')).toHaveText([/^T-3/])
  const heading = page.getByRole('table', { name: 'Tasks' }).locator('th[data-state="finished"]')
  const control = heading.getByRole('button', { name: 'Delete finished' })
  await expect(control).toBeVisible()
  await control.click()
  const dialog = page.getByRole('dialog', { name: 'No finished task can be deleted now' })
  await expect(
    dialog.getByRole('list', { name: 'Kept on the board' }).getByRole('listitem'),
  ).toHaveText([
    'T-3 stays: T-9 and T-10 still need it, and it failed: the chief can still reopen it.',
  ])
  await expect(dialog.locator('#delete-tasks-going')).toBeHidden()
  // Nothing to confirm: the one button closes the dialog.
  await expect(dialog.getByRole('button')).toHaveText(['Close'])
  await dialog.getByRole('button', { name: 'Close' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'tasks.delete')).toEqual([])
  await expect(page.locator('td[data-state="finished"] button.card')).toHaveText([/^T-3/])
})

test('says on a deleted task that it left the board, and offers nothing to do on it', async ({
  page,
}) => {
  const data = model()
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  data.tasks['1:5'] = {
    ...zeus.tasks.find((t) => t.number === 5),
    messages: [],
    deletedAt: '2026-10-02T20:00:00.000Z',
  }
  await open(page, data)
  await page.locator('button.card[data-task="5"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-5' })
  await expect(drawer.locator('.drawer-meta')).toContainText('· deleted from the board ·')
  await expect(drawer.getByRole('button', { name: 'Delete task' })).toHaveCount(0)
})

test('says why the daemon keeps a finished task on the board, from the drawer or the heading', async ({
  page,
}) => {
  const data = model()
  // Another window put T-7 on the board, needing T-3, since this board was read.
  data.deleteRefusal =
    'T-7 still needs T-3, which failed and may be reopened: it stays on the board until T-7 is finished'
  await open(page, data)
  await page.locator('button.card[data-task="3"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-3' })
  await drawer.getByRole('button', { name: 'Delete task' }).click()
  await page
    .getByRole('dialog', { name: 'Delete T-3?' })
    .getByRole('button', { name: 'Delete for good' })
    .click()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [3] }])
  const status = page.locator('#status')
  await expect(status).toHaveText(
    'T-7 still needs T-3, which failed and may be reopened: it stays on the board until T-7 is finished',
  )
  await expect(status).toHaveAttribute('data-tone', 'error')
  await expect(drawer).toBeVisible()
  await drawer.getByRole('button', { name: 'Close the task' }).click()
  await page.evaluate(() => {
    window.__model.deleteRefusal =
      'T-8 still needs T-5, which failed and may be reopened: it stays on the board until T-8 is finished'
  })
  await page.getByRole('button', { name: 'Delete finished' }).click()
  await page
    .getByRole('dialog', { name: 'Delete 2 finished tasks?' })
    .getByRole('button', { name: 'Delete for good' })
    .click()
  await expect
    .poll(() => calls(page, 'tasks.delete'))
    .toEqual([
      { project: 1, tasks: [3] },
      { project: 1, tasks: [3, 5] },
    ])
  await expect(status).toHaveText(
    'T-8 still needs T-5, which failed and may be reopened: it stays on the board until T-8 is finished',
  )
  await expect(page.locator('td[data-state="finished"] button.card')).toHaveCount(2)
})

test('offers no delete on a closed project until it is resumed: its finished tasks only read', async ({
  page,
}) => {
  const data = closedFoundry()
  const lane = data.boards[2].lanes.find((l) => l.participant.handle === 'zeus-amber-pine')
  lane.tasks.push(
    task(4, 'Old spike', 'accepted', 'chief', 'zeus-amber-pine', 90, { projectId: 2 }),
  )
  data.tasks['2:4'] = { ...lane.tasks.at(-1), messages: [] }
  await open(page, data)
  await chooseProject(page, 'foundry')
  await expect(page.locator('td[data-state="finished"] button.card[data-task="4"]')).toHaveCount(1)
  const heading = page.getByRole('table', { name: 'Tasks' }).locator('th[data-state="finished"]')
  await expect(heading.getByRole('button')).toHaveCount(0)
  await page.locator('button.card[data-task="4"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-4' })
  await expect(drawer.getByRole('button')).toHaveText(['Close'])
  await drawer.getByRole('button', { name: 'Close the task' }).click()
  // Resumed, the same board offers both again.
  await page.evaluate(() => {
    window.__model.projects[1].state = 'open'
    window.__model.boards[2].project.state = 'open'
    window.__listeners.get('state-changed')()
  })
  await expect(heading.getByRole('button')).toHaveAccessibleName('Delete finished')
  await page.locator('button.card[data-task="4"]').click()
  await expect(drawer.getByRole('button')).toHaveText(['Close', 'Show terminal', 'Delete task'])
})

test('says so on the board when the project has no members yet', async ({ page }) => {
  const data = model()
  const board = data.boards[1]
  board.lanes = board.lanes.filter((lane) => ['human', 'chief'].includes(lane.participant.role))
  board.open = []
  await open(page, data)
  const table = page.getByRole('table', { name: 'Tasks' })
  await expect(table.locator('tr.board-empty')).toHaveText(
    'No members yet: add the members this project may use under Project settings.',
  )
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(
    page.getByRole('dialog', { name: 'Project settings' }).locator('tbody tr.staff-empty'),
  ).toHaveText('Nobody yet: add the members this project may use.')
  await open(page)
  await expect(table.locator('tr.board-empty')).toHaveCount(0)
})

test('takes a chief on another harness for no member: the board says it has none, and a member joins the staff on a model', async ({
  page,
}) => {
  const data = model()
  const board = data.boards[1]
  board.lanes = board.lanes.filter((lane) => ['human', 'chief'].includes(lane.participant.role))
  board.open = []
  // The chief runs on Codex, a member's harness too: it is still no member.
  Object.assign(board.lanes[1].participant, { harness: 'codex', model: 'gpt-6-astra' })
  await open(page, data)
  await expect(page.locator('tr.board-empty')).toHaveText(
    'No members yet: add the members this project may use under Project settings.',
  )
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('table', { name: 'On the staff' }).locator('tbody tr')).toHaveText([
    'Nobody yet: add the members this project may use.',
  ])
  await pickMember(dialog, { harness: 'Codex', model: 'gpt-6-astra', tier: 'critical' })
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: null,
        tier: 'critical',
        roles: ['worker'],
      },
    ])
  expect(await calls(page, 'member.roles')).toEqual([])
})

test("leaves notes and withdrawn messages out of a task's story, and tells a brief sent again as a request of its own", async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const brief = 'Tell a joke'
  const body = `${brief}\n\nReassigned from @gefjon-jolly-tundra (ran out of quota after starting); check the working tree for partial changes.`
  lane.tasks.push(task(14, 'Tell a joke', 'done', 'chief', 'zeus', 0, { body }))
  data.tasks['1:14'] = {
    ...lane.tasks.find((t) => t.number === 14),
    messages: [
      message(40, 'task', 'chief', 'gefjon-jolly-tundra', brief, 20),
      message(
        41,
        'note',
        null,
        'chief',
        'T-14 was taken back from @gefjon-jolly-tundra (ran out of quota after starting) and waits for another light worker.',
        15,
      ),
      message(42, 'task', 'chief', 'zeus', body, 14),
      message(43, 'question', 'zeus', 'chief', 'About cats or code?', 12),
      message(44, 'answer', 'chief', 'zeus', 'Code.', 11),
      message(
        45,
        'result',
        'zeus',
        'chief',
        'Why do programmers mix up Halloween and Christmas?',
        8,
      ),
      message(46, 'task', 'chief', 'zeus', 'Reopened: shorter, please.', 6),
      message(47, 'task', 'chief', 'zeus', 'Also no puns.', 5, { state: 'cancelled' }),
      message(48, 'result', 'zeus', 'chief', 'Oct 31 == Dec 25.', 1),
    ],
  }
  await open(page, data)
  await page.locator('button.card[data-task="14"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-14' })
  const steps = drawer.getByRole('list', { name: "T-14's story" }).locator('.step')
  // The brief went to @gefjon-jolly-tundra, then again to @zeus once it ran out of quota.
  await expect(steps.locator('.step-label')).toHaveText([
    'Request 1',
    'Request 2',
    'Question',
    'Answer',
    'Result 2',
    'Request 3',
    'Result 3',
  ])
  await expect(steps.locator('.step-route').first()).toHaveText('@chief → @gefjon-jolly-tundra')
  await expect(steps.locator('.step-preview')).toHaveText([
    'Tell a joke',
    'Tell a joke',
    'About cats or code?',
    'Code.',
    'Why do programmers mix up Halloween and Christmas?',
    'Reopened: shorter, please.',
    'Oct 31 == Dec 25.',
  ])
  await expect(steps.last().locator('.step-body')).toHaveText('Oct 31 == Dec 25.')
  await expect(drawer.locator('.drawer-meta')).toContainText('updated just now')
  await expect(drawer.locator('.drawer-meta')).not.toContainText('just now ago')
})

test('says under a body cut to fit the page where it reads whole, and right after Request 1 how many earlier messages it left out', async ({
  page,
}) => {
  const data = model()
  const cut = (start, length) => `${start}\n… (${length} characters; cut here)`
  // As the daemon reads a task too long for one frame: every request stays,
  // cut to its line at least, the earliest of the rest are left out, and
  // what it cut is marked.
  data.tasks['1:2'] = {
    ...data.tasks['1:2'],
    body: cut('Write the parser', 900_000),
    bodyCut: true,
    messagesLeftOut: 3,
    messages: [
      message(20, 'task', 'chief', 'zeus', '… (900000 characters; cut here)', 9, { bodyCut: true }),
      message(25, 'question', 'zeus', 'chief', cut('Which grammar?', 700_000), 5, {
        bodyCut: true,
      }),
      message(26, 'answer', 'chief', 'zeus', 'The recursive one.', 4),
      message(27, 'result', 'zeus', 'chief', cut('Parser done', 800_000), 3, { bodyCut: true }),
    ],
  }
  await open(page, data)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  const story = drawer.getByRole('list', { name: "T-2's story" })
  // Every request is kept, so each one and its result still has its number.
  await expect(story.locator('.step-label')).toHaveText([
    'Request 1',
    'Question',
    'Answer',
    'Result 1',
  ])
  await expect(story.locator('> li').nth(1)).toHaveText(
    '3 earlier messages not shown: cf task get T-2 shows the whole thread.',
  )
  // An open step says under its body where it reads whole, when it was cut.
  const steps = story.locator('.step')
  const whole = 'Cut to fit here: cf task get T-2 shows it whole.'
  await expect(steps.last().locator('.drawer-cut')).toHaveText(whole)
  await expect(steps.last().locator('.drawer-cut')).toBeVisible()
  for (const step of [0, 1, 2]) await steps.nth(step).locator('summary').click()
  for (const step of [0, 1]) {
    await expect(steps.nth(step).locator('.drawer-cut')).toHaveText(whole)
  }
  await expect(steps.nth(1).locator('.step-body')).toHaveText(cut('Which grammar?', 700_000), {
    useInnerText: true,
  })
  await expect(steps.nth(2).locator('.step-body')).toHaveText('The recursive one.')
  await expect(steps.nth(2).locator('.drawer-cut')).toHaveCount(0)
})

test("shows what a task's window wrote, from ConsensFlow's own copy, under the thread", async ({
  page,
}) => {
  const data = model()
  data.transcripts = {
    '1:2': {
      total: 5,
      items: [
        {
          id: 'u1',
          role: 'user',
          text: '[ConsensFlow m-20 · T-2 · task from @chief]\nWrite the parser',
          complete: true,
          at: null,
        },
        { id: 't1', role: 'tool', text: 'ok\n14 passed', complete: true, at: null },
        { id: 'a1', role: 'assistant', text: 'Parser done, 14 tests.', complete: false, at: null },
      ],
    },
  }
  await open(page, data)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  // Folded until asked: the task's story comes first.
  const fold = drawer.locator('details[data-section="transcript"]')
  await expect(fold).not.toHaveAttribute('open', '')
  await fold.locator('summary').click()
  await expect(drawer.locator('.transcript-more')).toHaveText('The last 3 of 5 items.')
  const items = drawer
    .getByRole('list', { name: "What T-2's window wrote" })
    .locator('.transcript-item')
  await expect(items.locator('.transcript-head')).toHaveText([
    'Sent to the window',
    'Tool output',
    'The agent · still writing',
  ])
  await expect(items.nth(2).locator('.transcript-body')).toHaveText('Parser done, 14 tests.')
  await expect(items.nth(2)).toHaveAttribute('data-role', 'assistant')
})

test("names a compaction's summary in what a window wrote as one, never as what was sent to it", async ({
  page,
}) => {
  const data = model()
  data.transcripts = {
    '1:2': {
      total: 2,
      items: [
        { id: 'u1', role: 'user', text: 'Write the parser', complete: true, at: null },
        {
          id: 's1',
          role: 'user',
          text: 'This session is being continued.',
          complete: true,
          at: null,
          compaction: true,
        },
      ],
    },
  }
  await open(page, data)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await drawer.locator('details[data-section="transcript"] summary').click()
  const items = drawer
    .getByRole('list', { name: "What T-2's window wrote" })
    .locator('.transcript-item')
  await expect(items.locator('.transcript-head')).toHaveText([
    'Sent to the window',
    'Compaction summary',
  ])
  await expect(items.nth(1)).toHaveAttribute('data-compaction', 'true')
  // Drawn as one: its border and its label in the theme's amber, the other's not.
  const amber = 'rgb(231, 185, 104)'
  await expect(items.nth(1)).toHaveCSS('border-left-color', amber)
  await expect(items.nth(1).locator('.transcript-head')).toHaveCSS('color', amber)
  await expect(items.nth(0)).not.toHaveCSS('border-left-color', amber)
})

/** T-2 with a long answer in what its window wrote. */
function longTranscript() {
  const data = model()
  data.transcripts = {
    '1:2': {
      total: 2,
      items: [
        { id: 'u1', role: 'user', text: 'Write the parser', complete: true, at: null },
        { id: 'a1', role: 'assistant', text: 'line\n'.repeat(200), complete: true, at: null },
      ],
    },
  }
  return data
}

test('reads what a window wrote when its fold opens, and keeps the fold and its scroll across redraws', async ({
  page,
}) => {
  await open(page, longTranscript())
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  const fold = drawer.locator('details[data-section="transcript"]')
  await expect(fold.locator('summary')).toHaveText('What the agent did2 items')
  // Opening the drawer reads how much the window wrote, not what.
  expect(await calls(page, 'task.transcript')).toEqual([{ project: 1, task: 2, limit: 0 }])
  await fold.locator('summary').click()
  const answer = drawer.locator('.transcript-body').nth(1)
  await expect(answer).toHaveText(/^line/)
  expect(await calls(page, 'task.transcript')).toEqual([
    { project: 1, task: 2, limit: 0 },
    { project: 1, task: 2 },
  ])
  await answer.evaluate((node) => {
    node.scrollTop = 600
  })
  // Redraws that change nothing of the task leave the drawer as it is, and
  // read the task again but nothing more of its window.
  const reads = (await calls(page, 'task.get')).length
  for (const lamp of ['working', 'idle']) await changed(page, setLamp, ['zeus', lamp])
  await expect.poll(async () => (await calls(page, 'task.get')).length).toBe(reads + 2)
  await expect(fold).toHaveAttribute('open', '')
  expect(await answer.evaluate((node) => node.scrollTop)).toBe(600)
  expect(await calls(page, 'task.transcript')).toHaveLength(2)
})

test('reads an open fold again when its window wrote more, and nothing else', async ({ page }) => {
  await open(page, longTranscript())
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  const fold = drawer.locator('details[data-section="transcript"]')
  await fold.locator('summary').click()
  await expect(drawer.locator('.transcript-item')).toHaveCount(2)
  const boards = (await calls(page, 'board.get')).length
  const wrote = () =>
    page.evaluate(() =>
      window.__listeners.get('state-changed')({ payload: { reason: 'transcript' } }),
    )
  await page.evaluate(() => {
    const written = window.__model.transcripts['1:2']
    written.items.push({ id: 'a2', role: 'assistant', text: 'And the tests', complete: false })
    written.total = 3
  })
  await wrote()
  await expect(drawer.locator('.transcript-item')).toHaveCount(3)
  await expect(fold.locator('summary')).toHaveText('What the agent did3 items')
  await expect(drawer.locator('.transcript-head').last()).toContainText('still writing')
  expect((await calls(page, 'board.get')).length, 'the board is not read for it').toBe(boards)
  // A shut fold is not read.
  await fold.locator('summary').click()
  const reads = (await calls(page, 'task.transcript')).length
  await wrote()
  await page.waitForTimeout(100)
  expect((await calls(page, 'task.transcript')).length).toBe(reads)
})

test('redraws a drawer whose task changed, its fold still open and read again', async ({
  page,
}) => {
  await open(page, longTranscript())
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  const fold = drawer.locator('details[data-section="transcript"]')
  await fold.locator('summary').click()
  await expect(drawer.locator('.transcript-item')).toHaveCount(2)
  // The chief sends T-2 back with a word, and the window writes on.
  await changed(page, () => {
    const parser = window.__model.tasks['1:2']
    parser.messages.push({
      id: 22,
      kind: 'task',
      sender: 'chief',
      recipient: 'zeus',
      state: 'delivered',
      reason: null,
      body: 'Reopened: cover the errors too.',
      createdAt: new Date().toISOString(),
    })
    parser.state = 'working'
    const transcript = window.__model.transcripts['1:2']
    transcript.items.push({
      id: 'a2',
      role: 'assistant',
      text: 'Covering the errors.',
      complete: false,
      at: null,
    })
    transcript.total = 3
  })
  const steps = drawer.getByRole('list', { name: "T-2's story" }).locator('.step')
  await expect(steps.locator('.step-label')).toHaveText(['Request 1', 'Result 1', 'Request 2'])
  await expect(steps.last().locator('.step-body')).toHaveText('Reopened: cover the errors too.')
  await expect(steps.last().locator('.step-body')).toBeVisible()
  await expect(fold).toHaveAttribute('open', '')
  await expect(fold.locator('summary')).toHaveText('What the agent did3 items')
  await expect(drawer.locator('.transcript-item')).toHaveCount(3)
  await expect(drawer.locator('.transcript-body').nth(2)).toHaveText('Covering the errors.')
})

test('a member the update could not run on what it ran on says it needs an edit, on its row and in the staff, with Edit at hand', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(9, 'my-coder', 'worker', {
      agent: 'my-coder',
      harness: 'codex',
      model: null,
      effort: null,
      tier: 'standard',
      needsEdit: true,
    }),
    tasks: [],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  const row = page.locator('tr[data-handle="my-coder"]')
  await expect(row.locator('.row-status')).toHaveText(
    'Needs an edit: the update could not tell what it ran on. Edit it under Project settings, or remove it',
  )
  await expect(row.locator('.row-status')).toHaveAttribute('data-state', 'missing')
  // It runs on no default, and says so.
  await expect(row.locator('.row-meta')).toContainText('no model: needs an edit')
  await expect(row.locator('.row-meta')).not.toContainText('default')
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  const member = settings.locator('tr[data-handle="my-coder"]')
  await expect(member.locator('.member-gone')).toHaveText(
    'The update could not tell what my-coder ran on: edit it to a model its harness lists, or remove it.',
  )
  await expect(member.getByRole('button', { name: 'Edit @my-coder' })).toBeVisible()
})

test('a chief the update could not tell the model of says so on its card, and Switch chief gives it one', async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { harness: 'claude-code', model: null, needsEdit: true })
  chief.activity = { state: 'closed' }
  chief.pane = null
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.locator('.terminal-status')).toHaveText(
    "Needs a model: ConsensFlow couldn't tell which model this chief used before the update. Pick one with Switch chief; what was written to the chief waits until then",
  )
  await expect(card.locator('.terminal-status')).toHaveAttribute('data-state', 'missing')
  await expect(card.locator('.terminal-meta')).toHaveText('Claude Code · no model: needs an edit')
  // The way out is on the card: Switch chief, to a model the harness lists.
  await card.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await pickChief(dialog, 'Codex', 'gpt-6-astra')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 1, harness: 'codex', model: 'gpt-6-astra', when: 'turn', note: false }])
  // The daemon takes the mark off with the switch: the card says what the chief runs on.
  await changed(page, () => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    Object.assign(lane.participant, { harness: 'codex', model: 'gpt-6-astra', needsEdit: false })
  })
  await expect(card.locator('.terminal-meta')).toHaveText('Codex · gpt-6-astra')
  await expect(card.locator('.terminal-status')).not.toContainText('Needs a model')
})

test("says on the chief's card in the dock what the chief is doing and runs on, and switches it from there", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { harness: 'codex', model: 'gpt-6-astra' })
  chief.activity = { state: 'waiting', reason: 'permission to run a command' }
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(card.locator('.terminal-status')).toHaveText('Waiting: permission to run a command')
  // Its harness and the model it runs on: a chief runs on no saved agent.
  await expect(card.locator('.terminal-meta')).toHaveText('Codex · gpt-6-astra')
  // Out of quota, it says until when, whatever its window does.
  await changed(page, () => {
    const chief = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    chief.participant.outUntil = new Date(Date.now() + 90 * 60_000).toISOString()
  })
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'out')
  await expect(card.locator('.terminal-status')).toHaveText(/^Out of quota until (\S+ )?\d\d:\d\d$/)
  await expect(card.locator('.terminal-status')).toHaveAttribute('data-state', 'out')
  // Switch chief is on the card: none on the board, though the chief's row is there for its tasks.
  await expect(page.locator('tr[data-handle="chief"]')).toHaveCount(1)
  const board = page.getByRole('region', { name: 'Board' })
  const switchChief = { name: 'Switch the chief to another harness or model' }
  await expect(board.getByRole('button', switchChief)).toHaveCount(0)
  await card.getByRole('button', switchChief).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await expect(dialog).toBeVisible()
  // The harness and model the chief runs on now is no switch.
  await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Codex' })
  await expect(dialog.locator('[name="chiefModel"]')).toBeEnabled()
  expect(await offered(dialog.locator('[name="chiefModel"]'))).toEqual([
    ['Pick a model', true],
    ['gpt-6-astra', true],
    ['gpt-5.6-luna', false],
  ])
})

test("says on the chief's card the model its window ran on, where the human switched it there", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { ranOn: 'claude-sonnet-5' })
  await open(page, data)
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-meta'),
  ).toHaveText('Claude Code · claude-sonnet-5')
  // And Switch chief takes that one for the chief's: the pick it was is a switch now.
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Claude Code' })
  await expect(dialog.locator('[name="chiefModel"]')).toBeEnabled()
  expect(await offered(dialog.locator('[name="chiefModel"]'))).toEqual([
    ['Pick a model', true],
    ['claude-opus-5-5', false],
    ['claude-sonnet-5', true],
  ])
})

test("says on a Devin chief's card the id its harness lists for the model, not the id its launch passes", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  // As migration 0031 and 0033 leave a Devin chief: `model` is the joined id Devin's launch
  // passes (`swe-2-high`), `modelName` the id Devin's list gives the model (`swe-2`), the card of
  // every other participant says the second.
  Object.assign(chief.participant, {
    harness: 'devin',
    model: 'swe-2-high',
    effort: 'high',
    modelName: 'swe-2',
  })
  await open(page, data)
  const meta = page.locator('#stage .terminal-card[data-handle="chief"] .terminal-meta')
  await expect(meta).toHaveText('Devin · swe-2')
  // Where its window ran on another model, the card says that one, as it did (O7).
  await changed(page, () => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    lane.participant.ranOn = 'swe-2-medium'
  })
  await expect(meta).toHaveText('Devin · swe-2-medium')
  // And a chief left to its harness's model says no model, as it did.
  await changed(page, () => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    Object.assign(lane.participant, { ranOn: undefined, model: null, modelName: null })
  })
  await expect(meta).toHaveText('Devin')
})

test("says on a Devin chief's card the id Devin lists after a switch to Devin, and the model it moved to after a switch away", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { harness: 'claude-code', model: 'claude-opus-5-5' })
  await open(page, data)
  const meta = page.locator('#stage .terminal-card[data-handle="chief"] .terminal-meta')
  await expect(meta).toHaveText('Claude Code · claude-opus-5-5')
  // The daemon holds the id Devin lists beside the id its launch is given (T-1345): picked in
  // New project or switched to, the chief's row says `swe-2` beside `swe-2-max`.
  const as = (fields) =>
    changed(
      page,
      (fields) => {
        const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
        Object.assign(lane.participant, { effort: null, modelName: null, ...fields })
      },
      fields,
    )
  await as({ harness: 'devin', model: 'swe-2-max', effort: 'max', modelName: 'swe-2' })
  await expect(meta).toHaveText('Devin · swe-2')
  // A switch away takes the name with it: the card says the model it moved to, whether the
  // daemon named it or not.
  await as({ harness: 'codex', model: 'gpt-6-astra', modelName: 'gpt-6-astra' })
  await expect(meta).toHaveText('Codex · gpt-6-astra')
  await as({ harness: 'claude-code', model: 'claude-sonnet-5-5' })
  await expect(meta).toHaveText('Claude Code · claude-sonnet-5-5')
})

test("keeps the chief's card in the dock while its window is down, and Switch chief is the way on", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief, { activity: { state: 'closed' }, pane: null })
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.locator('.terminal-host')).toHaveCount(0)
  await card.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await expect(dialog).toBeVisible()
})

test('a terminal and a board too narrow for its columns keep a visible scrollbar', async ({
  page,
}) => {
  await open(page)
  const rules = await page.evaluate(() =>
    [...document.styleSheets]
      .flatMap((sheet) => [...sheet.cssRules])
      .map((rule) => rule.selectorText ?? '')
      .filter((selector) => selector.includes('::-webkit-scrollbar')),
  )
  expect(rules).toEqual([
    '.xterm-viewport::-webkit-scrollbar, .board::-webkit-scrollbar',
    '.xterm-viewport::-webkit-scrollbar-track, .board::-webkit-scrollbar-track',
    '.xterm-viewport::-webkit-scrollbar-thumb, .board::-webkit-scrollbar-thumb',
    '.xterm-viewport::-webkit-scrollbar-thumb:hover, .board::-webkit-scrollbar-thumb:hover',
    '.board::-webkit-scrollbar-corner',
  ])
})

test('a redraw leaves the keyboard where the human put it in the dock', async ({ page }) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: session(20, participant(3, 'zeus', 'worker'), 'amber-pine'),
    tasks: [],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 1 },
  })
  await open(page, data)
  await unfold(page)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const keyboard = dock.locator('.terminal-card[data-handle="zeus-amber-pine"] .stub-input')
  await keyboard.focus()
  await expect(keyboard).toBeFocused()
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await page.waitForTimeout(150)
  await expect(keyboard).toBeFocused()
})

/** `change(argument)` made in the model, the state change it brings, and the redraw done. */
async function changed(page, change = () => {}, argument = null) {
  const reads = () =>
    page.evaluate(() => window.__calls.filter(([, args]) => args?.operation === 'board.get').length)
  const before = await reads()
  await page.evaluate(change, argument)
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(reads).toBeGreaterThan(before)
  await page.waitForTimeout(50)
}

/** A participant's lamp in harbour's model: `[handle, state]`. */
const setLamp = ([handle, state]) => {
  window.__model.boards[1].lanes.find((l) => l.participant.handle === handle).activity = {
    state,
  }
}

const focusedLabel = (page) =>
  page.evaluate(
    () => document.activeElement.getAttribute('aria-label') ?? document.activeElement.textContent,
  )

test('a redraw leaves the keyboard where it was, on the board, the projects and the staff', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: session(20, participant(3, 'zeus', 'worker'), 'amber-pine'),
    tasks: [],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 1 },
  })
  await open(page, data)
  await unfold(page)
  // A lamp changed, and drawn: the redraw has happened.
  const lampDrawn = async (handle, state) => {
    await changed(page, setLamp, [handle, state])
    await expect(page.locator(`tr[data-handle="${handle}"]`).getByTestId('lamp')).toHaveAttribute(
      'data-state',
      state,
    )
  }
  // A card, while another row's lamp changes.
  await page.locator('button.card[data-task="4"]').focus()
  await lampDrawn('chief', 'idle')
  expect(await focusedLabel(page)).toBe('T-4, Add the tests, Queued, from @chief')
  // The card itself moving to another column keeps the keyboard with it.
  await changed(page, () => {
    const zeus = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
    zeus.tasks.find((t) => t.number === 4).state = 'working'
  })
  await expect(page.locator('td[data-state="working"] button.card[data-task="4"]')).toHaveCount(1)
  expect(await focusedLabel(page)).toBe('T-4, Add the tests, Working, from @chief')
  // A session's Hide on its row, and on its card in the dock, while its lamp changes.
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).focus()
  await lampDrawn('zeus-amber-pine', 'idle')
  await expect(
    row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }),
  ).toBeFocused()
  const card = page.locator('.terminal-card[data-handle="zeus-amber-pine"]')
  await card.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).focus()
  await lampDrawn('zeus-amber-pine', 'working')
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'working')
  await expect(
    card.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }),
  ).toBeFocused()
  // A project's Close.
  await page.getByRole('button', { name: 'Close harbour' }).focus()
  await lampDrawn('chief', 'working')
  expect(await focusedLabel(page)).toBe('Close harbour')
  // A member's Remove in Project settings.
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await dialog.getByRole('button', { name: 'Remove Worker @diana' }).focus()
  await lampDrawn('chief', 'idle')
  expect(await focusedLabel(page)).toBe('Remove Worker @diana')
})

test('a click whose press and release straddle a redraw still lands', async ({ page }) => {
  await open(page)
  const task = page.locator('button.card[data-task="2"]')
  // Beside the windows the board may scroll sideways: the card comes into view first.
  await task.scrollIntoViewIfNeeded()
  const card = await task.boundingBox()
  await page.mouse.move(card.x + 20, card.y + 10)
  await page.mouse.down()
  // zeus's own row changes under the press: its lamp and its status.
  await changed(page, setLamp, ['zeus', 'working'])
  await expect(page.locator('tr[data-handle="zeus"]').getByTestId('lamp')).toHaveAttribute(
    'data-state',
    'working',
  )
  await page.mouse.up()
  await expect(page.getByRole('complementary', { name: 'Task T-2' })).toBeVisible()
})

test('a button kept across redraws acts on what the board shows now', async ({ page }) => {
  const data = model()
  data.projects[1] = { ...data.projects[1], state: 'open' }
  data.boards[2].project.state = 'open'
  await open(page, data)
  // A role added to zeus leaves its Worker row drawn the same: removing the
  // worker role now takes that role only, and asks nothing.
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await changed(page, () => {
    const zeus = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
    zeus.participant.roles = ['worker', 'reviewer']
  })
  await expect(dialog.locator('tr[data-handle="zeus"]')).toHaveCount(2)
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'zeus', roles: ['reviewer'] }])
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toHaveCount(0)
  await dialog.getByRole('button', { name: 'Close' }).click()
  // A project deleted and another of the same name in its place: the
  // project list's item for it chooses the new one.
  await changed(page, () => {
    const { projects, boards } = window.__model
    projects[1] = { ...projects[1], id: 4 }
    boards[4] = { ...boards[2], project: { ...boards[2].project, id: 4 } }
  })
  await chooseProject(page, 'foundry')
  await expect.poll(() => calls(page, 'board.get')).toContainEqual({ project: 4 })
})

test('the dock stays where the human scrolled it across redraws', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 900 })
  const data = model()
  data.boards[1].lanes.push({
    participant: session(20, participant(3, 'zeus', 'worker'), 'amber-pine'),
    tasks: [],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 1 },
  })
  await open(page, data)
  await unfold(page)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const stage = page.getByRole('region', { name: 'Terminals' })
  await expect(stage.locator('.terminal-card')).toHaveCount(3)
  // The terminal shown is at the right end, in view; the human scrolls back
  // to the chief, away from it.
  const focused = stage.locator('.terminal-card[data-focused="true"]')
  await expect(focused).toHaveAttribute('data-handle', 'zeus-amber-pine')
  await expect(focused).toBeInViewport()
  await stage.evaluate((node) => {
    node.scrollLeft = 0
  })
  await expect(focused).not.toBeInViewport()
  const scrolled = await stage.evaluate((node) => node.scrollLeft)
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await page.waitForTimeout(100)
  expect(await stage.evaluate((node) => node.scrollLeft)).toBe(scrolled)
  await expect(focused).not.toBeInViewport()
})

test('a note from an agent reads in its own list, marked read when seen', async ({ page }) => {
  const data = model()
  data.inbox[1].push({
    id: 16,
    kind: 'note',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: 1,
    body: 'The parser is in; I am moving to the lexer.',
    questions: null,
    createdAt: at(1),
  })
  await open(page, data)
  const bay = page.getByRole('region', { name: 'For you' })
  await expect(bay.locator('.foryou-status')).toHaveText('2 notes')
  await expect(
    bay.getByRole('list', { name: 'Waiting for you' }).locator('li[data-message="16"]'),
  ).toHaveCount(0)
  const notes = bay.getByRole('list', { name: 'Notes for you' })
  await expect(bay.locator('.foryou-sub')).toHaveText('Notes from your agents')
  const note = notes.locator('li[data-message="16"]')
  await expect(note).toContainText('The parser is in; I am moving to the lexer.')
  await expect(note).toContainText('Note from @chief')
  await note.getByRole('button', { name: 'Mark m-16 read' }).click()
  await expect.poll(() => calls(page, 'message.read')).toEqual([{ message: 16 }])
})

test('says how many earlier notes one frame did not hold, and counts them all', async ({
  page,
}) => {
  const data = model()
  const note = (id, body, minutesAgo) => ({
    id,
    kind: 'note',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: null,
    body,
    questions: null,
    createdAt: at(minutesAgo),
  })
  // Newest first, as the daemon reads them: only the two newest fit in its answer.
  data.inbox[1].unshift(note(18, 'The lexer is in.', 1), note(17, 'The parser is in.', 2))
  data.inboxFit = 2
  await open(page, data)
  const bay = page.getByRole('region', { name: 'For you' })
  await expect(bay.locator('.foryou-status')).toHaveText('3 notes')
  await expect(page.getByRole('button', { name: 'Inbox (3)' })).toBeVisible()
  const notes = bay.getByRole('list', { name: 'Notes for you' })
  await expect(notes.locator('.strip-message')).toHaveCount(2)
  await expect(notes.locator('.strips-more')).toHaveText(
    '1 earlier note not shown: mark these read to see it.',
  )
  expect(await calls(page, 'inbox.get')).toContainEqual({
    project: 1,
    participant: 'human',
    unread: true,
  })
})

// Every key goes to the pane as typed, in order, and none of it holds a paste:
// text the human leaves unsent no longer keeps a delivery out (2026-10-01).
test('every key the human types reaches the pane in order, flagged as nothing', async ({
  page,
}) => {
  await open(page)
  await expect.poll(() => page.evaluate(() => window.__emulators.length)).toBeGreaterThan(0)
  await page.evaluate(() => {
    const emulator = window.__emulators[0]
    for (const data of ['h', '\r', '\x1b[A', '\x1b[<64;10;20M', '\x1b', '\x1b[200~pasted\x1b[201~'])
      emulator.type(data)
  })
  await expect
    .poll(() =>
      page.evaluate(() =>
        window.__calls
          .filter(([command]) => command === 'pane_input_enqueue')
          .map(([, args]) => [String.fromCharCode(...args.bytes).slice(0, 3), Object.keys(args)]),
      ),
    )
    .toEqual(
      ['h', '\r', '\x1b[A', '\x1b[<', '\x1b', '\x1b[2'].map((start) => [
        start,
        ['id', 'generation', 'sequence', 'bytes'],
      ]),
    )
})

/**
 * The chief's real terminal with `line` printed in it and its first word
 * selected by a double click: what the page copies from then on, and the
 * keys that reach the chief's window.
 */
async function selecting(page, line) {
  await open(page, { ...model(), realTerminals: true })
  await page.evaluate(() => {
    window.__copied = []
    document.addEventListener('copy', (event) =>
      window.__copied.push(event.clipboardData.getData('text/plain')),
    )
  })
  await page.evaluate(
    (bytes) => window.__output.onmessage({ id: 'p1-chief', generation: 5, seq: 1, bytes }),
    [...new TextEncoder().encode(line)],
  )
  const rows = page.locator('#stage .terminal-card[data-handle="chief"] .xterm-rows')
  await expect(rows).toContainText(line)
  const box = await rows.boundingBox()
  await page.mouse.dblclick(box.x + 6, box.y + 6)
  return {
    copied: () => page.evaluate(() => window.__copied),
    sent: () =>
      page.evaluate(() =>
        window.__calls
          .filter(([command, args]) => command === 'pane_input_enqueue' && args.id === 'p1-chief')
          .map(([, args]) => args.bytes),
      ),
  }
}

// On a Mac, Cmd+C copies and Ctrl+C stays the program's interrupt, selection
// or not; Ctrl+Shift+C copies on every system. The page tells a Mac by the
// browser's user agent, which this sets, as the cases on Windows set theirs: the
// suite runs on Linux (T-1161).
test.describe('on a Mac', () => {
  test.use({
    userAgent:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
  })
  test('interrupts with Ctrl+C on a Mac even with text selected, and copies with Ctrl+Shift+C', async ({
    page,
  }) => {
    const { copied, sent } = await selecting(page, 'copy this line')
    await page.keyboard.press('Control+Shift+C')
    expect(await copied()).toEqual(['copy'])
    expect(await sent()).toEqual([])
    await page.mouse.dblclick(
      ...(await page
        .locator('#stage .terminal-card[data-handle="chief"] .xterm-rows')
        .boundingBox()
        .then((box) => [box.x + 6, box.y + 6])),
    )
    await page.keyboard.press('Control+C')
    await expect.poll(sent).toEqual([[3]])
    expect(await copied()).toEqual(['copy'])
  })
})

test.describe('on Windows', () => {
  test.use({
    userAgent:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36 Edg/140.0',
  })
  /** What reaches the chief's window when the human types `text` there, its harness `harness`. */
  const sent = async (page, harness, text) => {
    const data = model()
    data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.harness =
      harness
    await open(page, data)
    await expect.poll(() => page.evaluate(() => window.__emulators.length)).toBeGreaterThan(0)
    await page.evaluate((typed) => window.__emulators[0].type(typed), text)
    let bytes = null
    await expect
      .poll(async () => {
        bytes = await page.evaluate(
          () =>
            window.__calls.filter(([command]) => command === 'pane_input_enqueue').at(-1)?.[1]
              .bytes ?? null,
        )
        return bytes !== null
      })
      .toBe(true)
    return new TextDecoder().decode(new Uint8Array(bytes))
  }

  // A window that reads key presses on Windows (Devin, Codex) gets what the human types as
  // typed, marks included: the console host the app ships gives it them whole, where the page
  // spelled them in ASCII until T-548 (and, for a round, wrote them as win32-input-mode keys,
  // after which that host held every lone Escape). Claude reads its terminal as text.
  test("the human's marks reach a Devin window as typed", async ({ page }) => {
    expect(await sent(page, 'devin', 'a — “b” → €5, ăîș')).toBe('a — “b” → €5, ăîș')
  })

  test("the human's marks and Escape reach a Codex window as typed", async ({ page }) => {
    expect(await sent(page, 'codex', '€ 😀\u001b\r')).toBe('€ 😀\u001b\r')
  })

  test("the human's marks reach a Claude window as typed", async ({ page }) => {
    expect(await sent(page, 'claude-code', 'a — “b” → €5')).toBe('a — “b” → €5')
  })

  // Ctrl+C is the program's interrupt: with text selected in a terminal it
  // copies the text instead, as Windows Terminal does, and the selection goes,
  // so the next Ctrl+C interrupts.
  test('copies the text selected in a terminal with Ctrl+C, and interrupts with the next', async ({
    page,
  }) => {
    const { copied, sent } = await selecting(page, 'copy this line')
    await page.keyboard.press('Control+C')
    expect(await copied()).toEqual(['copy'])
    expect(await sent(), 'nothing reached the program').toEqual([])
    await page.keyboard.press('Control+C')
    await expect.poll(sent).toEqual([[3]])
    expect(await copied()).toEqual(['copy'])
  })
})

// The user agents the page tells its system by (a Mac's Cmd and Ctrl, Windows' Ctrl).
const MAC_AGENT =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15'
const WINDOWS_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36 Edg/140.0'

// Shift+Enter in a window (T-1330, T-1336, T-1348). xterm.js 6 has no kitty keyboard protocol and no
// modifyOtherKeys, and sends the plain Enter's byte for Shift+Enter, so a message was sent where the
// human asked for a new line, in every harness. `fixtures/pane-keys/newline-cases.json` holds what the
// page writes for it (`sends`, by harness and platform), what that does in each harness's input box
// (`shiftEnter`), and what each harness's input box does with each byte string a pane might write, at
// rest and while a turn runs (`make-newline-cases.mjs` says how each was found: Pi's editor and
// Codex's key readers run, the others read from their source and documentation). These presses are
// made in a real xterm and held to what the page sent. **A pane's key never starts with Escape**: a
// program takes a lone Escape for "stop", and a read that ends between the two bytes of a write
// reads it alone (T-1336). It is a line feed, one byte, in every harness on both platforms.
const NEWLINE_CASES = JSON.parse(
  readFileSync(new URL('./fixtures/pane-keys/newline-cases.json', import.meta.url), 'utf8'),
)

/** What `harness`'s input box does with the `bytes` a window was sent, on `platform`; `busy` while a turn runs. */
function inputBoxDoes(harness, platform, bytes, busy = false) {
  const table = NEWLINE_CASES.harnesses[harness].rows[platform]
  const known = table.find((each) => each.bytes.join() === bytes.join())
  if (!known) return `unknown bytes ${JSON.stringify(bytes)}`
  return busy ? known.whileBusy : known.does
}

/**
 * The bytes a real xterm in the chief's window sent when `press` was done there, the chief
 * running `harness`; nothing else is typed first.
 */
async function sentByPress(page, harness, press) {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.harness =
    harness
  await open(page, { ...data, realTerminals: true })
  const input = page.locator('#stage .terminal-card[data-handle="chief"] .xterm-helper-textarea')
  await input.focus()
  await press(input)
  let bytes = []
  await expect
    .poll(async () => {
      bytes = await page.evaluate(() =>
        window.__calls
          .filter(([command]) => command === 'pane_input_enqueue')
          .flatMap(([, args]) => args.bytes),
      )
      return bytes.length
    })
    .toBeGreaterThan(0)
  // Anything else the press sent comes in the same turn of the page: give it one more.
  await page.waitForTimeout(100)
  return page.evaluate(() =>
    window.__calls
      .filter(([command]) => command === 'pane_input_enqueue')
      .flatMap(([, args]) => args.bytes),
  )
}

for (const [platform, agent] of [
  ['mac', MAC_AGENT],
  ['windows', WINDOWS_AGENT],
]) {
  test.describe(`Shift+Enter in a window, ${platform === 'mac' ? 'on a Mac' : 'on Windows'}`, () => {
    test.use({ userAgent: agent })

    for (const harness of Object.keys(NEWLINE_CASES.harnesses)) {
      // A new line where the harness has a key for the bytes the page writes; nothing, safely, where it
      // has none (Codex, Devin and Muse on Windows: a line feed is the Enter key with Control, which
      // none binds): never the message sent, a follow-up queued or a turn interrupted.
      const does = NEWLINE_CASES.harnesses[harness].shiftEnter[platform]
      test(`is ${does === 'newline' ? 'a new line' : 'no key at all'} in a ${harness} window, and Enter is still the message sent`, async ({
        page,
      }) => {
        expect(['newline', 'nothing'], `${harness} on ${platform}`).toContain(does)
        const shifted = await sentByPress(page, harness, (input) => input.press('Shift+Enter'))
        expect(
          inputBoxDoes(harness, platform, shifted),
          `${harness} was sent ${JSON.stringify(shifted)} for Shift+Enter`,
        ).toBe(does)
        expect(
          inputBoxDoes(harness, platform, shifted, true),
          `${harness} was sent ${JSON.stringify(shifted)} for Shift+Enter while a turn runs`,
        ).toBe(does)
        expect(shifted, `what the fixture says ${harness} is sent`).toEqual(
          NEWLINE_CASES.harnesses[harness].sends[platform],
        )
      })

      // A read that ends between the two bytes of a key's write (a pty's input queue nearly full, the
      // console host's 4,096-byte read) reads an Escape alone, which stops a running turn, and then the
      // rest, which may send the message: only a single byte cannot be read in two (T-1336;
      // `crates/cf-panes/src/pty/keys.rs` holds the reads). So the page writes one byte, in every
      // harness and on both platforms (T-1348).
      test(`is one byte, never an Escape, for a ${harness} window`, async ({ page }) => {
        const shifted = await sentByPress(page, harness, (input) => input.press('Shift+Enter'))
        expect(shifted, `${harness} was sent ${JSON.stringify(shifted)}`).toEqual([10])
      })
    }

    test('Enter alone is still the message sent, in every harness', async ({ page }) => {
      for (const harness of Object.keys(NEWLINE_CASES.harnesses)) {
        const plain = await sentByPress(page, harness, (input) => input.press('Enter'))
        expect(inputBoxDoes(harness, platform, plain), `${harness} Enter`).toBe('submit')
      }
    })
  })
}

test.describe('The keys around Shift+Enter in a window', () => {
  /**
   * The bytes of the chief's window's input after `act` (the page's), sorted by what carried
   * them, the chief running `harness`.
   */
  const heard = async (page, act, harness = 'claude-code') => {
    const data = model()
    data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.harness =
      harness
    await open(page, { ...data, realTerminals: true })
    const input = page.locator('#stage .terminal-card[data-handle="chief"] .xterm-helper-textarea')
    await input.focus()
    await act(input)
    await page.waitForTimeout(150)
    return page.evaluate(() => {
      const sent = (command) =>
        window.__calls
          .filter(([name]) => name === command)
          .map(([, args]) => String.fromCharCode(...args.bytes))
      return { typed: sent('pane_input_enqueue'), replies: sent('pane_reply_enqueue') }
    })
  }

  test("Alt+Enter and Ctrl+J are xterm's: Pi's Alt+Enter queues a follow-up, so it stays ESC CR", async ({
    page,
  }) => {
    const { typed } = await heard(page, async (input) => {
      await input.press('Alt+Enter')
      await input.press('Control+j')
    })
    expect(typed).toEqual(['\x1b\r', '\n'])
  })

  // Shift+Enter is the key alone: with another modifier held it is a key of the program's own,
  // which xterm sends as it does (Pi is asked for, where Shift+Enter is a line feed).
  for (const [keys, sent] of [
    ['Shift+Alt+Enter', '\x1b\r'],
    ['Shift+Control+Enter', '\r'],
    ['Shift+Meta+Enter', '\r'],
  ]) {
    test(`${keys} is not Shift+Enter`, async ({ page }) => {
      const { typed } = await heard(page, (input) => input.press(keys), 'pi')
      expect(typed).toEqual([sent])
    })
  }

  // The premise of the page's choice, measured in a real xterm 6.0.0: it answers a program's
  // device-attributes query and nothing of the keyboard protocols (the kitty query `CSI ? u`, and
  // the request for modifyOtherKeys `CSI > 4 ; 2 m` that Pi makes when it gets no kitty answer),
  // so Pi runs without extended keys and reads a line feed, and no sequence of a protocol, as
  // Shift+Enter. If a newer xterm answers the kitty query, a program that asks for the protocol is
  // owed all its keys in it, and this fails.
  test('xterm answers no keyboard-protocol query: a Pi that asked is sent Ctrl+J all the same', async ({
    page,
  }) => {
    const { typed, replies } = await heard(
      page,
      async (input) => {
        await page.evaluate(
          (bytes) => window.__output.onmessage({ id: 'p1-chief', generation: 5, seq: 1, bytes }),
          [...new TextEncoder().encode('\x1b[>7u\x1b[?u\x1b[>4;2m\x1b[c')],
        )
        await expect
          .poll(() =>
            page.evaluate(() => window.__calls.filter(([n]) => n === 'pane_reply_enqueue').length),
          )
          .toBeGreaterThan(0)
        await input.focus()
        await input.press('Shift+Enter')
      },
      'pi',
    )
    expect(replies).toEqual(['\x1b[?1;2c'])
    expect(typed).toEqual(['\n'])
  })

  test('an input method choosing a candidate with Shift+Enter is not a new line', async ({
    page,
  }) => {
    const { typed } = await heard(
      page,
      (input) =>
        input.evaluate((field) =>
          field.dispatchEvent(
            new KeyboardEvent('keydown', {
              key: 'Enter',
              shiftKey: true,
              isComposing: true,
              bubbles: true,
              cancelable: true,
            }),
          ),
        ),
      'pi',
    )
    expect(typed).not.toContain('\n')
  })

  test('Shift+Enter is typed once, as the human typed it, and no Enter comes after it', async ({
    page,
  }) => {
    let prevented = null
    const { typed, replies } = await heard(page, async (input) => {
      // The key is the page's: nothing is left for the textarea to do with it (an Enter there
      // would be a line break typed into xterm's own field). This listens after xterm's own.
      await input.evaluate((field) => {
        field.addEventListener('keydown', (event) => {
          window.__keydownPrevented = event.defaultPrevented
        })
      })
      await input.press('Shift+Enter')
      prevented = await page.evaluate(() => window.__keydownPrevented)
    })
    expect(typed).toEqual(['\n'])
    expect(replies).toEqual([])
    expect(prevented).toBe(true)
  })
})

// A window's keys are its harness's as launched (T-1336). Editing a member changes the harness its
// next window runs (the ledger says so at once), and the window already open runs on, so the pane
// carries the harness it was launched with (`pane.harness`, from the daemon), and a window's keys
// follow that, across a page reload too.
test("New project and Switch chief say the same of the chief's model: the harness's own switch in the chief's window keeps its conversation and a reopen keeps the model", async ({
  page,
}) => {
  await open(page)
  for (const [button, name, words] of [
    [
      'New project',
      'New project',
      "The chief starts on the harness and model you pick. To change only its model later, switch it in the chief's own window: ConsensFlow follows it, and a reopen keeps it. Switch the chief, on its card, is for another harness or a fresh conversation.",
    ],
    [
      'Switch the chief to another harness or model',
      'Switch the chief',
      "The chief goes on in a new window on the harness and model you pick: a fresh conversation, whose first message is the handoff, what you and the chiefs before it said and what waits on the board. The staff keeps working. To change only the model, switch it in the chief's own window instead: it keeps its conversation, ConsensFlow follows it, and a reopen keeps it.",
    ],
  ]) {
    await page.getByRole('button', { name: button }).click()
    const dialog = page.getByRole('dialog', { name })
    await expect(dialog.locator('[data-help="chief"]')).toHaveText(words)
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toBeHidden()
  }
})

// Links in a pane (T-1291): only an OSC 8 hyperlink (words with the address hidden behind them) is
// a link; a web address a program prints as text is none, in any form (T-1323). A link is
// underlined and titled with what opens it as the pointer comes onto it, and it opens in the
// human's browser, through the app's `open_link`, only for a click with the system's key held
// (Cmd on a Mac, Ctrl on Windows, as VS Code's terminal does), so a click into a pane to type
// never opens one. What the page asks of the app is the address exactly as the program printed it;
// what may open is the app's to say
// (`link.rs`), so an address a program hides behind words that is no web page reaches nothing.
const CHIEF_TERMINAL = '#stage .terminal-card[data-handle="chief"] .xterm'

/** An OSC 8 hyperlink as a program prints it: `words`, with `uri` behind them. */
const hyperlink = (uri, words) => `\u001b]8;;${uri}\u001b\\${words}\u001b]8;;\u001b\\`

/** The chief's real terminal with `text` printed in it as a program prints it; `seen` is on screen. */
async function printing(page, text, seen) {
  await open(page, { ...model(), realTerminals: true })
  await page.evaluate(
    (bytes) => window.__output.onmessage({ id: 'p1-chief', generation: 5, seq: 1, bytes }),
    [...new TextEncoder().encode(text)],
  )
  await expect(page.locator(`${CHIEF_TERMINAL} .xterm-rows`)).toContainText(seen)
}

/** The middle of `words` where the terminal shows them, or of the row `row` (from 0) when none. */
const placeOf = (page, words, row = 0) =>
  page.evaluate(
    ([words, row]) => {
      const rows = document.querySelector('#stage .terminal-card[data-handle="chief"] .xterm-rows')
      const walker = document.createTreeWalker(
        words === null ? rows.children[row] : rows,
        NodeFilter.SHOW_TEXT,
      )
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const at = words === null ? 0 : node.data.indexOf(words)
        if (at < 0) continue
        const middle = at + Math.floor((words === null ? 4 : words.length) / 2)
        const range = document.createRange()
        range.setStart(node, middle)
        range.setEnd(node, middle + 1)
        const box = range.getBoundingClientRect()
        return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
      }
      return null
    },
    [words, row],
  )

/**
 * Where `words` are in the row `row` (from 0): their middle, or with `after` the cell just past
 * them. Found by the text the row holds, which its spans split as links and colours do.
 */
const placeInRow = (page, row, words, after = false) =>
  page.evaluate(
    ([row, words, after]) => {
      const rows = document.querySelector('#stage .terminal-card[data-handle="chief"] .xterm-rows')
      const walker = document.createTreeWalker(rows.children[row], NodeFilter.SHOW_TEXT)
      const nodes = []
      let text = ''
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        nodes.push([node, text.length])
        text += node.data
      }
      const at = text.indexOf(words)
      if (at < 0) return null
      const index = after ? at + words.length : at + Math.floor(words.length / 2)
      const [node, start] = nodes.findLast(([, from]) => from <= index)
      const range = document.createRange()
      range.setStart(node, index - start)
      range.setEnd(node, index - start + 1)
      const box = range.getBoundingClientRect()
      return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
    },
    [row, words, after],
  )

/**
 * How the terminal draws the text under `place`: its text decoration, line and style. A link's text
 * under the pointer is "underline solid"; an OSC 8 link's text is "underline dashed" otherwise,
 * whatever it points to (xterm marks all of it), and other text "none solid". The rows take no
 * pointer events, so the text is found by where it is.
 */
const decorationAt = (page, place) =>
  page.evaluate(({ x, y }) => {
    const spans = document.querySelectorAll(
      '#stage .terminal-card[data-handle="chief"] .xterm-rows span',
    )
    for (const span of spans) {
      const box = span.getBoundingClientRect()
      if (box.left <= x && x < box.right && box.top <= y && y < box.bottom) {
        const { textDecorationLine, textDecorationStyle } = getComputedStyle(span)
        return `${textDecorationLine} ${textDecorationStyle}`
      }
    }
    return 'no text there'
  }, place)

/** The addresses the page asked the app to open, in order. */
const linksOpened = (page) =>
  page.evaluate(() =>
    window.__calls.filter(([command]) => command === 'open_link').map(([, args]) => args.url),
  )

/** Two frames on: what the terminal drew for the pointer's last move is on screen. */
const afterFrames = (page) =>
  page.evaluate(
    () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))),
  )

/** A click at `place` with `keys` held, as a human's hand makes it: the pointer comes, then the click. */
async function clickWith(page, place, keys = [], button = 'left') {
  await page.mouse.move(place.x, place.y)
  for (const key of keys) await page.keyboard.down(key)
  await page.mouse.click(place.x, place.y, { button })
  for (const key of keys) await page.keyboard.up(key)
}

for (const [system, agent, key, other] of [
  ['a Mac', MAC_AGENT, 'Meta', 'Control'],
  ['Windows', WINDOWS_AGENT, 'Control', 'Meta'],
]) {
  const named = key === 'Meta' ? 'Cmd' : 'Ctrl'
  test.describe(`pane links on ${system}`, () => {
    test.use({ userAgent: agent })

    // Only an OSC 8 hyperlink is a link (the third review, T-1323): a plain address cannot be told
    // whole from the screen, as a program that lays out its text in rows of its own writes it cut.
    test(`a plain URL is no link: no underline, no title, and ${named}+click opens nothing`, async ({
      page,
    }) => {
      const plain = 'https://example.com/docs/a?x=1&y=%20z#frag'
      const web = 'https://example.org/linked'
      await printing(page, `Sources: ${plain}\r\n${hyperlink(web, 'Linked')}\r\n`, 'Linked')
      const place = await placeOf(page, plain)
      await page.mouse.move(place.x, place.y)
      await afterFrames(page)
      expect(await decorationAt(page, place), 'underlined under the pointer').toBe('none solid')
      await expect(page.locator(CHIEF_TERMINAL)).not.toHaveAttribute('title', /.*/)
      await expect(page.locator(`${CHIEF_TERMINAL} .xterm-screen`)).not.toHaveClass(
        /xterm-cursor-pointer/,
      )
      await clickWith(page, place, [key])
      // The click above was heard before this one is: the hyperlink is the only that opens.
      await clickWith(page, await placeOf(page, 'Linked'), [key])
      await expect.poll(() => linksOpened(page)).toEqual([web])
    })

    test(`a click without ${named} opens nothing, nor does the other system's key, nor another button`, async ({
      page,
    }) => {
      const url = 'https://example.com/docs'
      await printing(page, `${hyperlink(url, 'Docs')}\r\n`, 'Docs')
      const place = await placeOf(page, 'Docs')
      await clickWith(page, place)
      await clickWith(page, place, [other])
      await clickWith(page, place, ['Shift'])
      await clickWith(page, place, [key], 'right')
      await clickWith(page, place, [key], 'middle')
      // Each click above was heard before this one is: it is the only that opens.
      await clickWith(page, place, [key])
      await expect.poll(() => linksOpened(page)).toEqual([url])
    })

    test(`an OSC 8 hyperlink shows its address in the title where its words hide it, and ${named}+click opens exactly that address`, async ({
      page,
    }) => {
      const url = 'https://example.org/report?q=a%26b&n=2#top'
      await printing(page, `${hyperlink(url, 'Sources')} and more\r\n`, 'Sources and more')
      const place = await placeOf(page, 'Sources')
      expect(await decorationAt(page, place)).toBe('underline dashed')
      await page.mouse.move(place.x, place.y)
      await expect.poll(() => decorationAt(page, place)).toBe('underline solid')
      await expect(page.locator(CHIEF_TERMINAL)).toHaveAttribute(
        'title',
        `${named}+click to open ${url}`,
      )
      await expect(page.locator(`${CHIEF_TERMINAL} .xterm-screen`)).toHaveClass(
        /xterm-cursor-pointer/,
      )
      // The title and the underline go when the pointer does.
      const beside = await placeOf(page, 'more')
      await page.mouse.move(beside.x, beside.y)
      await expect(page.locator(CHIEF_TERMINAL)).not.toHaveAttribute('title', /.*/)
      await afterFrames(page)
      expect(await decorationAt(page, place), 'underlined after the pointer left').toBe(
        'underline dashed',
      )
      await clickWith(page, place)
      await clickWith(page, place, [other])
      await clickWith(page, place, [key])
      await expect.poll(() => linksOpened(page)).toEqual([url])
    })

    test(`${named}+click opens an OSC 8 hyperlink a program ended with BEL, and one over a bare URL, once each`, async ({
      page,
    }) => {
      // Claude Code and Codex end a hyperlink with BEL, Pi with ST; Codex marks the bare
      // addresses of its text too, so that one range is a link twice over.
      const url = 'https://example.org/bel?x=1&y=2'
      const bare = 'https://example.com/bare?z=3'
      const text = `\u001b]8;;${url}\u0007Docs\u001b]8;;\u0007 ${hyperlink(bare, bare)}\r\n`
      await printing(page, text, `Docs ${bare}`)
      await clickWith(page, await placeOf(page, 'Docs'), [key])
      await clickWith(page, await placeOf(page, bare), [key])
      await expect.poll(() => linksOpened(page)).toEqual([url, bare])
    })

    test('an OSC 8 hyperlink to a file, a script, data or an app scheme is no link and reaches nothing', async ({
      page,
    }) => {
      const refused = [
        ['Files', 'file:///etc/passwd'],
        ['Script', 'javascript:alert(document.cookie)'],
        ['Data', 'data:text/html,<script>alert(1)</script>'],
        ['App', 'consensflow://open?x=1'],
        ['Mail', 'mailto:someone@example.com'],
      ]
      const web = 'https://example.com/ok'
      const text =
        refused.map(([words, uri]) => hyperlink(uri, words)).join(' ') +
        ` ${hyperlink(web, 'Web')}\r\n`
      await printing(page, text, 'Files Script Data App Mail Web')
      for (const [words] of refused) {
        const place = await placeOf(page, words)
        await page.mouse.move(place.x, place.y)
        // Not a link: no underline of a link under the pointer, no title to say a click opens it.
        await afterFrames(page)
        expect(await decorationAt(page, place), words).toBe('underline dashed')
        await expect(page.locator(CHIEF_TERMINAL)).not.toHaveAttribute('title', /.*/)
        await clickWith(page, place, [key])
      }
      // The same hand on the one that is a web page opens it, so the clicks above were heard.
      const place = await placeOf(page, 'Web')
      await page.mouse.move(place.x, place.y)
      await expect(page.locator(CHIEF_TERMINAL)).toHaveAttribute(
        'title',
        `${named}+click to open ${web}`,
      )
      await clickWith(page, place, [key])
      await expect.poll(() => linksOpened(page)).toEqual([web])
    })

    test(`${named}+click opens an OSC 8 hyperlink whose words xterm wrapped over rows from any of them, whole`, async ({
      page,
    }) => {
      const url = `https://example.com/${'long-segment/'.repeat(30)}end?x=1&y=2`
      await printing(page, `${hyperlink(url, url)}\r\n`, 'long-segment')
      const first = await placeOf(page, null, 0)
      const second = await placeOf(page, null, 1)
      expect(second.y).toBeGreaterThan(first.y)
      await page.mouse.move(second.x, second.y)
      await expect.poll(() => decorationAt(page, second)).toBe('underline solid')
      await clickWith(page, first, [key])
      await clickWith(page, second, [key])
      await expect.poll(() => linksOpened(page)).toEqual([url, url])
    })

    test(`${named}+click opens a link in a window whose program has taken the mouse`, async ({
      page,
    }) => {
      const url = 'https://example.com/taken'
      // The program asks to be told of the mouse (any button, SGR), as a full-screen program does.
      await printing(page, `\u001b[?1000h\u001b[?1006h${hyperlink(url, 'Taken')}\r\n`, 'Taken')
      const place = await placeOf(page, 'Taken')
      await page.mouse.move(place.x, place.y)
      await expect.poll(() => decorationAt(page, place)).toBe('underline solid')
      await clickWith(page, place)
      await clickWith(page, place, [key])
      await expect.poll(() => linksOpened(page)).toEqual([url])
    })

    // Every way a program prints an address as text: none of it is a link, the address whole or not.
    test('a plain address in any form is no link, with the system key held or not', async ({
      page,
    }) => {
      const forms = [
        'https://example.com/a',
        'HTTPS://example.com/a',
        'https://en.wikipedia.org/wiki/Function_(mathematics)',
        "https://en.wikipedia.org/wiki/Bayes'_theorem",
        'https://example.com/a,b^c',
        'See https://example.com/a.',
        '(https://example.com/a)',
        'Source (https://en.wikipedia.org/wiki/Yahoo!)',
        '[docs](https://example.com/a?b=1&c=2)',
        '"https://example.com/a"',
        'https://[2001:db8::1]/a',
      ]
      const web = 'https://example.org/linked'
      await printing(page, `${forms.join('\r\n')}\r\n${hyperlink(web, 'Linked')}\r\n`, 'Linked')
      for (const [row, printed] of forms.entries()) {
        // The cell the address begins in, found by the twelve characters from its scheme.
        const start = printed.search(/https?:\/\//i)
        const place = await placeInRow(page, row, printed.slice(start, start + 12))
        await page.mouse.move(place.x, place.y)
        await afterFrames(page)
        await expect(page.locator(CHIEF_TERMINAL), printed).not.toHaveAttribute('title', /.*/)
        expect(await decorationAt(page, place), printed).toBe('none solid')
        await clickWith(page, place, [key])
      }
      await clickWith(page, await placeOf(page, 'Linked'), [key])
      await expect.poll(() => linksOpened(page)).toEqual([web])
    })

    test('an address with a host in letters of its own language or without its slashes is a link as an OSC 8 hyperlink and text is none', async ({
      page,
    }) => {
      // As text neither is a link, as no plain address is; as an OSC 8 hyperlink each opens whole.
      const idn = 'https://bücher.example/a'
      const bare = 'http:example.com/a'
      await printing(
        page,
        `${idn}\r\n${bare}\r\n${hyperlink(idn, 'Büchersuche')}\r\n${hyperlink(bare, 'Bare')}\r\n`,
        'Bare',
      )
      for (const [row, words] of [
        [0, idn],
        [1, bare],
      ]) {
        const place = await placeInRow(page, row, words)
        await clickWith(page, place, [key])
        await expect(page.locator(CHIEF_TERMINAL), words).not.toHaveAttribute('title', /.*/)
      }
      await clickWith(page, await placeInRow(page, 2, 'Büchersuche'), [key])
      await clickWith(page, await placeInRow(page, 3, 'Bare'), [key])
      await expect.poll(() => linksOpened(page)).toEqual([idn, bare])
    })

    test('a refusal of the app shows in the status line, and the link goes on working', async ({
      page,
    }) => {
      await printing(page, `${hyperlink('https://example.com/a', 'Docs')}\r\n`, 'Docs')
      await page.evaluate(() => {
        window.__linkRefused = {
          ok: false,
          error: 'The address is longer than 2048 characters',
        }
      })
      const place = await placeOf(page, 'Docs')
      await clickWith(page, place, [key])
      await expect(page.locator('#status')).toContainText(
        'The link did not open: The address is longer than 2048 characters',
      )
      await clickWith(page, place, [key])
      await expect.poll(async () => (await linksOpened(page)).length).toBe(2)
    })
  })
}

// What the harnesses print for a link, replayed (T-1314's review, T-1315). The cases are the output
// of Pi's, Claude Code's and Codex's own formatting code, with hyperlinks off (the plain `words
// (address)` each falls back to) and on (what ConsensFlow's launch asks of Pi and Claude Code), at
// the widths a pane has; `app/tests/fixtures/pane-links/make-harness-cases.mjs` says how each was
// made. Each case is held to one of two things: a click on the address opens exactly the address the
// program meant (an OSC 8 hyperlink), or it is no link and opens nothing (a plain address, however
// the program laid it out). Never a shortened one. A program that lays its text out in rows of its
// own (Pi writes its rows with CR/LF, so xterm does not mark them wrapped) cuts a plain address
// with nothing on the screen to say so, which is why none is a link; an OSC 8 hyperlink carries its
// address whole on every row it is drawn on.
const HARNESS_CASES = JSON.parse(
  readFileSync(new URL('./fixtures/pane-links/harness-cases.json', import.meta.url), 'utf8'),
)

const ESC = String.fromCharCode(27)
const BEL = String.fromCharCode(7)
/** An OSC 8 sequence's end: BEL, or ESC and a backslash. */
const OSC_END = `(?:${BEL}|${ESC}\\\\)`
const SGR = new RegExp(`${ESC}\\[[0-9;]*m`, 'g')
const OSC_8 = new RegExp(`${ESC}\\]8;;[^${BEL}${ESC}]*${OSC_END}`, 'g')
const HYPERLINK = new RegExp(
  `${ESC}\\]8;;([^${BEL}${ESC}]*)${OSC_END}(.*?)${ESC}\\]8;;${OSC_END}`,
  'g',
)

/** The printable columns of `raw`, a row as a program wrote it: escape sequences taken out. */
const printable = (raw) => raw.replace(SGR, '').replace(OSC_8, '')

/**
 * The columns a click should land on for `raw`'s row: the middle of each OSC 8 hyperlink to
 * `destination` it draws, the start of a plain `https://`, the first word of the row and its
 * longest (a row the address went on in, which is no link where the address was cut).
 */
function pointsOf(raw, destination) {
  const points = new Map()
  for (const span of raw.matchAll(HYPERLINK)) {
    const before = printable(raw.slice(0, span.index))
    const words = printable(span[2])
    if (span[1] === destination && words.length > 0) {
      points.set(before.length + Math.floor(words.length / 2), 'hyperlink')
    }
  }
  const text = printable(raw)
  const plain = text.search(/https?:\/\//i)
  if (plain >= 0) points.set(plain + 4, 'address')
  const first = text.search(/\S/)
  if (first >= 0 && !points.has(first + 1)) points.set(first + 1, 'word')
  // The longest run of text on the row too: where the rest of a cut address lands.
  let longest = null
  for (const run of text.matchAll(/\S+/g)) {
    if (longest === null || run[0].length > longest[0].length) longest = run
  }
  if (longest !== null) {
    const middle = longest.index + Math.floor(longest[0].length / 2)
    if (!points.has(middle)) points.set(middle, 'word')
  }
  return points
}

/**
 * Prints `printed` in a terminal of `width` columns of the page's own emulator, and answers the
 * place of each column asked for on each row: a program's rows, laid out as it laid them.
 */
const replaying = (page, printed, width) =>
  page.evaluate(
    async ([printed, width]) => {
      const { XtermEmulator } = await import('/term.js')
      window.replay?.dispose()
      const host = document.getElementById('replay-host')
      host.innerHTML = ''
      window.replayOpened = []
      window.replay = new XtermEmulator(host, { onLink: (url) => window.replayOpened.push(url) })
      window.replay.resizeObserver.disconnect()
      clearTimeout(window.replay.settling)
      window.replay.terminal.resize(width, 24)
      await new Promise((done) => window.replay.terminal.write(printed, done))
      await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))
      const box = host.querySelector('.xterm-screen').getBoundingClientRect()
      return { left: box.left, top: box.top, cell: box.width / width, row: box.height / 24 }
    },
    [printed, width],
  )

/** A blank page of the app's own origin for the replays below: the page's emulator on a host of its own. */
async function replayPage(page) {
  await page.route(`${ui.origin}/replay`, (route) =>
    route.fulfill({
      contentType: 'text/html',
      body: '<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/vendor/xterm.css"><body style="margin:0"><div id="replay-host"></div></body>',
    }),
  )
  await page.goto(`${ui.origin}/replay`)
}

/**
 * Clicks, with Cmd held, the cell at `column` of `row` (both from 0) of the replay `place` came
 * from, and answers what the page asked the app to open and what the pointer's title said.
 */
async function clickingCell(page, place, row, column) {
  const at = {
    x: place.left + (column + 0.5) * place.cell,
    y: place.top + (row + 0.5) * place.row,
  }
  await page.mouse.move(1390, 790)
  await page.mouse.move(at.x, at.y)
  await page.waitForTimeout(40)
  const title = await page.evaluate(() => window.replay.terminal.element.title)
  await clickWith(page, at, ['Meta'])
  const opened = await page.evaluate(() => window.replayOpened.splice(0))
  return { opened, title }
}

for (const [group, harness, mode] of [
  ['Pi prints as text', 'pi', 'plain'],
  ['Pi prints as OSC 8 hyperlinks (PI_HYPERLINKS=1)', 'pi', 'osc8'],
  ['Claude Code prints as text', 'claude-code', 'plain'],
  ['Claude Code is told to print OSC 8 hyperlinks (not by a launch)', 'claude-code', 'osc8'],
  ['Codex prints as OSC 8 hyperlinks', 'codex', 'osc8'],
]) {
  test.describe(`pane links: what ${group}`, () => {
    test.use({ userAgent: MAC_AGENT, viewport: { width: 1400, height: 800 } })
    test.describe.configure({ timeout: 120_000 })

    test('opens the exact address on a click, or is no link: never a shortened one', async ({
      page,
    }) => {
      await replayPage(page)
      const cases = HARNESS_CASES.filter((c) => c.harness === harness && c.mode === mode)
      expect(cases.length, `${group} has cases`).toBeGreaterThan(0)
      const wrong = []
      for (const c of cases) {
        const place = await replaying(page, c.printed, c.width)
        const rows = c.printed.split('\r\n')
        for (const [index, raw] of rows.entries()) {
          for (const [column, kind] of pointsOf(raw, c.destination)) {
            const { opened, title } = await clickingCell(page, place, index, column)
            const saw = `${c.id} row ${index} ${kind}`
            const wanted = c.expect === 'exact' && kind !== 'word' ? [c.destination] : null
            if (opened.some((url) => url !== c.destination)) {
              wrong.push(`${saw}: opened ${JSON.stringify(opened)}, not ${c.destination}`)
            } else if (title !== '' && title !== `Cmd+click to open ${c.destination}`) {
              wrong.push(`${saw}: title ${title}`)
            } else if (c.expect === 'none' && (opened.length > 0 || title !== '')) {
              wrong.push(`${saw}: a link where ${c.destination} was cut`)
            } else if (wanted && JSON.stringify(opened) !== JSON.stringify(wanted)) {
              wrong.push(`${saw}: opened ${JSON.stringify(opened)}, not ${c.destination}`)
            }
          }
        }
      }
      expect(wrong, `${cases.length} cases of ${group}`).toEqual([])
    })
  })
}

test('the chief is picked as a harness, then a model it lists, then an effort it lists for it, nothing picked, in New project and Switch chief', async ({
  page,
}) => {
  await open(page)
  for (const [button, name] of [
    ['New project', 'New project'],
    ['Switch the chief to another harness or model', 'Switch the chief'],
  ]) {
    await page.getByRole('button', { name: button }).click()
    const dialog = page.getByRole('dialog', { name })
    const harness = dialog.locator('[name="chiefHarness"]')
    const models = dialog.locator('[name="chiefModel"]')
    // Every harness installed here, in their order, none picked; the model waits for one.
    expect(await offered(harness)).toEqual([
      ['Pick a harness', true],
      ['Claude Code', false],
      ['Codex', false],
      ['Pi', false],
      ['Devin', false],
      ['Muse Code', false],
    ])
    await expect(harness).toHaveValue('')
    await expect(models).toBeDisabled()
    // The effort waits for a model, and a model that lists none has none to pick.
    const effort = dialog.locator('[name="chiefEffort"]')
    await expect(effort).toBeDisabled()
    // Its models, as the harness lists them; none picked; the harness's own switch said.
    await harness.selectOption({ label: 'Pi' })
    await expect(models).toBeEnabled()
    await expect(models).toHaveValue('')
    expect(await offered(models)).toEqual([
      ['Pick a model', true],
      ['meta/muse-spark', false],
      ['meta/muse-spark-1.3-contributor', false],
    ])
    const help = dialog.locator('.chief-help')
    await expect(help).toHaveText('Pi switches with /model and Enter.')
    // A contributor model says what it is beside it.
    await models.selectOption({ label: 'meta/muse-spark-1.3-contributor' })
    await expect(help).toHaveText(
      'Contributor model: Meta may train on what it sees, including your code. Pi switches with /model and Enter.',
    )
    expect(await offered(effort)).toEqual([['No effort to pick', true]])
    // Another harness: its own models, nothing picked again.
    await harness.selectOption({ label: 'Devin' })
    await expect(models).toBeEnabled()
    await expect(models).toHaveValue('')
    expect(await offered(models)).toEqual([
      ['Pick a model', true],
      ['swe-2', false],
    ])
    // Muse Code: no switch for one conversation, which its help says.
    await harness.selectOption({ label: 'Muse Code' })
    await expect(models).toBeEnabled()
    expect(await offered(models)).toEqual([
      ['Pick a model', true],
      ['muse-spark-1.3', false],
      ['muse-spark-1.3-contributor', false],
    ])
    await expect(dialog.locator('.chief-help')).toHaveText(
      'Muse switches with /model, and Enter makes it the default.',
    )
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toBeHidden()
  }
})

/**
 * What a harness lists of its models, as `models.get` answers it from the harness's recorded
 * answer (CHF-03's fixtures, read by its adapter: `crates/cf-harness/src/page_listings.rs`
 * writes these files and fails while they differ).
 */
const recordedListing = (harness) =>
  JSON.parse(
    readFileSync(join(import.meta.dirname, 'fixtures', 'models', `${harness}.json`), 'utf8'),
  )

/** A page whose harnesses list what they were recorded listing. */
function recordedModels(data = model()) {
  for (const harness of ['claude', 'codex', 'pi', 'devin', 'muse']) {
    data.models[harness] = { ...data.models[harness], listing: recordedListing(harness) }
  }
  return data
}

test("adds a member on a model its harness lists, the harness's own listing, never a saved agent", async ({
  page,
}) => {
  await open(page, recordedModels())
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const add = dialog.getByRole('group', { name: 'Add a member' })
  const harness = add.locator('[name="memberHarness"]')
  const models = add.locator('[name="memberModel"]')
  // Every harness installed here, none picked; the model waits for one.
  expect(await offered(harness)).toEqual([
    ['Pick a harness', true],
    ['Claude Code', false],
    ['Codex', false],
    ['Pi', false],
    ['Devin', false],
    ['Muse Code', false],
  ])
  await expect(models).toBeDisabled()
  await harness.selectOption({ label: 'Claude Code' })
  await expect(models).toBeEnabled()
  // Claude Code's models as its `initialize` answered, by its own labels and ids.
  expect(await offered(models)).toEqual([
    ['Pick a model', true],
    ['claude-opus-5-5', false],
    ['claude-fable-5-1', false],
    ['claude-sonnet-5-5', false],
    ['claude-haiku-5-5', false],
  ])
  expect(
    await models.locator('option:not([disabled])').evaluateAll((all) => all.map((o) => o.value)),
  ).toEqual(['claude-opus-5-5', 'claude-fable-5-1', 'claude-sonnet-5-5', 'claude-haiku-5-5'])
  // No saved agent is offered: the catalog's (zeus, hera…) are not a member's models.
  await expect(add.getByLabel('Agent')).toHaveCount(0)
  for (const agent of ['zeus', 'hera', 'diana', 'athena', 'pygmalion']) {
    await expect(add.locator('option', { hasText: agent })).toHaveCount(0)
  }
})

test("asks the chief's effort where its model lists efforts, and sends it with the harness and the model, in New project and Switch chief", async ({
  page,
}) => {
  await open(page, recordedModels())
  // New project: Opus lists five efforts; nothing starts until one is picked.
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-opus-5-5')
  const effort = made.dialog.locator('[name="chiefEffort"]')
  await expect(effort).toBeEnabled()
  expect(await offered(effort)).toEqual([
    ['Pick an effort', true],
    ['low', false],
    ['medium', false],
    ['high', false],
    ['xhigh', false],
    ['max', false],
  ])
  await made.start.click()
  await expect(made.dialog.locator('.chief-help')).toHaveText(
    'Pick the effort Claude Code runs claude-opus-5-5 at.',
  )
  expect(await calls(page, 'project.open')).toEqual([])
  await effort.selectOption('high')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-opus-5-5',
        effort: 'high',
        gate: false,
        compaction: 300000,
        staff: [],
      },
    ])
  // Switch chief: the effort goes with the harness and the model.
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await pickChief(dialog, 'Claude Code', 'claude-sonnet-5-5')
  await dialog.locator('[name="chiefEffort"]').selectOption('max')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([
      {
        project: 1,
        harness: 'claude-code',
        model: 'claude-sonnet-5-5',
        effort: 'max',
        when: 'turn',
        note: false,
      },
    ])
})

test("Refresh beside the chief's model reads the picked harness's models again, in New project and Switch chief", async ({
  page,
}) => {
  await open(page)
  for (const [button, name] of [
    ['New project', 'New project'],
    ['Switch the chief to another harness or model', 'Switch the chief'],
  ]) {
    await page.evaluate(() => {
      window.__calls.length = 0
    })
    await page.getByRole('button', { name: button }).click()
    const dialog = page.getByRole('dialog', { name })
    const models = dialog.locator('[name="chiefModel"]')
    await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Devin' })
    await expect(models).toBeEnabled()
    await page.evaluate(() => {
      window.__model.models.devin.listing.models.push({ id: 'swe-3', label: 'SWE-3', efforts: [] })
    })
    await dialog
      .getByRole('group', { name: 'The chief' })
      .getByRole('button', { name: 'Refresh models' })
      .click()
    await expect(models.locator('option')).toHaveText(['Pick a model', 'swe-2', 'swe-3'])
    // Every installed harness was read as the dialog opened, and Devin once more at Refresh.
    expect(await calls(page, 'models.get')).toEqual([
      ...['claude', 'codex', 'pi', 'devin', 'muse'].map((harness) => ({ harness })),
      { harness: 'devin', refresh: true },
    ])
    await page.evaluate(() => {
      window.__model.models.devin.listing.models.pop()
    })
    await dialog.getByRole('button', { name: 'Cancel' }).click()
  }
})

test("a harness whose models cannot be read leaves the chief's model unpicked, and says why", async ({
  page,
}) => {
  const data = model()
  data.modelsRefusal = { codex: 'codex answered nothing' }
  data.models.devin.listing = {
    models: [],
    readAt: null,
    freshness: { state: 'stale', why: 'devin is not logged in' },
    how: '',
  }
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const harness = dialog.locator('[name="chiefHarness"]')
  const models = dialog.locator('[name="chiefModel"]')
  await harness.selectOption({ label: 'Codex' })
  await expect(dialog.locator('.chief-help')).toHaveText(
    "Codex's models could not be read: codex answered nothing",
  )
  await expect(models).toBeDisabled()
  await harness.selectOption({ label: 'Devin' })
  await expect(dialog.locator('.chief-help')).toHaveText(
    'Devin lists no model: devin is not logged in.',
  )
  await expect(models).toBeDisabled()
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect(dialog).toBeVisible()
  expect(await calls(page, 'project.open')).toEqual([])
})

test('shows the staff as one row per member and role, and adds a member in a role, on a model its harness lists', async ({
  page,
}) => {
  const data = model()
  // A critical reviewer ahead of the workers among the lanes: the table reads by role, then by tier.
  data.boards[1].lanes.splice(2, 0, {
    participant: participant(7, 'hera', 'reviewer', { harness: 'codex', tier: 'critical' }),
    tasks: [],
    activity: { state: 'idle' },
    pane: { id: 'p1-hera', generation: 1 },
  })
  // A session of zeus has a lane of its own; the staff lists members, not their sessions.
  data.boards[1].lanes.push({
    participant: session(8, participant(3, 'zeus', 'worker'), 'pale-comet'),
    tasks: [],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-pale-comet', generation: 1 },
  })
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const table = dialog.getByRole('table', { name: 'On the staff' })
  await expect(table.locator('thead th')).toHaveText(['Member', 'Role', ''])
  await expect(table.locator('tbody tr')).toHaveText([
    /@zeus.*Worker/,
    /@diana.*Worker/,
    /@hera.*gpt-6-astra · max · critical · Codex.*Reviewer/,
  ])
  // Each member says its tier and its harness, and what it runs only where its name does not
  // (zeus is named for no model): its model's id and its effort.
  await expect(table.locator('tbody tr').first().locator('.member-meta')).toHaveText(
    'claude-sonnet-5 · high · standard · Claude Code',
  )
  await expect(table.locator('tbody tr').first().locator('td').nth(1)).toHaveText('Worker')
  await expect(dialog.getByLabel('Role').locator('option')).toHaveText([
    'Worker',
    'Advisor',
    'Reviewer',
    'Tester',
    'Image designer',
  ])
  // Add member names no saved agent: a harness, then a model it lists (T-1143).
  await expect(dialog.getByLabel('Agent')).toHaveCount(0)
  await pickMember(dialog, {
    role: 'advisor',
    harness: 'Pi',
    model: 'meta/muse-spark',
    tier: 'light',
  })
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'pi',
        model: 'meta/muse-spark',
        effort: null,
        tier: 'light',
        roles: ['advisor'],
      },
    ])
  await expect(page.locator('#status')).toHaveText('@muse-spark joined the staff as Advisor.')
  // An image designer draws on Codex: the harness select offers it alone.
  await dialog.getByLabel('Role').selectOption('designer')
  expect(await offered(dialog.locator('[name="memberHarness"]'))).toEqual([
    ['Pick a harness', true],
    ['Codex', false],
  ])
})

test('adds a member as a tester, or as one more role of a member, in the place a tester has among the roles', async ({
  page,
}) => {
  const data = model()
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(
    dialog.getByRole('group', { name: 'Staff', exact: true }).locator('.dialog-help').first(),
  ).toContainText('a tester checks it by running it when the chief asks')
  // The tester has its place among the roles, after the reviewer.
  await expect(dialog.getByLabel('Role').locator('option')).toHaveText([
    'Worker',
    'Advisor',
    'Reviewer',
    'Tester',
    'Image designer',
  ])
  // Any harness's model may test.
  await pickMember(dialog, { role: 'tester', harness: 'Devin', model: 'swe-2', tier: 'light' })
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'devin',
        model: 'swe-2',
        effort: null,
        tier: 'light',
        roles: ['tester'],
      },
    ])
  await expect(page.locator('#status')).toHaveText('@swe-2 joined the staff as Tester.')
  // A model a member runs already: the daemon gives that member the role, and the page says so.
  await page.evaluate(() => {
    window.__model.memberHandle = 'zeus'
  })
  await pickMember(dialog, { harness: 'Claude Code', model: 'claude-sonnet-5', tier: 'standard' })
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect(page.locator('#status')).toHaveText('@zeus is Tester now too.')
})

test('an image designer is picked on Codex alone, and Add member says so when Codex is not here', async ({
  page,
}) => {
  const data = model()
  // Codex is not installed here.
  data.missing = ['codex']
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const hint = dialog.locator('#staff-hint')
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(hint).toHaveText(
    'Image designers draw on Codex, which is not installed here: install it from Agents, Harnesses.',
  )
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeDisabled()
  // Another role: the harnesses installed here, Codex not among them.
  await dialog.getByLabel('Role').selectOption('worker')
  await expect(hint).toBeHidden()
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeEnabled()
  expect(await offered(dialog.locator('[name="memberHarness"]'))).toEqual([
    ['Pick a harness', true],
    ['Claude Code', false],
    ['Pi', false],
    ['Devin', false],
    ['Muse Code', false],
  ])
})

test('an image designer is added with no model, effort or tier: Codex is picked for it and the controls are gone', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const add = dialog.locator('.staff-add')
  const controls = ['memberModel', 'memberEffort', 'memberTier', 'memberRefresh']
  for (const name of controls) await expect(add.locator(`[name="${name}"]`)).toBeVisible()
  const read = await calls(page, 'models.get')
  await add.locator('[name="role"]').selectOption('designer')
  for (const name of controls) await expect(add.locator(`[name="${name}"]`)).toBeHidden()
  await expect(add.locator('[name="memberHarness"]')).toHaveValue('codex')
  await expect(add.locator('.member-help')).toHaveText(
    "An image designer draws on Codex's own model: it takes no model, effort or tier.",
  )
  // No model of Codex's is read for it, nor when the harness is picked again.
  await add.locator('[name="memberHarness"]').selectOption('codex')
  expect(await calls(page, 'models.get')).toEqual(read)
  await expect(add.locator('[name="memberModel"]')).toBeHidden()
  await expect(add.locator('.member-help')).toHaveText(
    "An image designer draws on Codex's own model: it takes no model, effort or tier.",
  )
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ project: 1, harness: 'codex', roles: ['designer'] }])
  await expect(page.locator('#status')).toHaveText(
    '@codex-default joined the staff as Image designer.',
  )
  // Another role brings them back, and picks nothing.
  await add.locator('[name="role"]').selectOption('worker')
  for (const name of controls) await expect(add.locator(`[name="${name}"]`)).toBeVisible()
  await expect(add.locator('[name="memberHarness"]')).toHaveValue('')
  await expect(add.locator('.member-help')).toBeHidden()
})

test("drops one role from a member's row, and asks before its last", async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'diana')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const table = dialog.getByRole('table', { name: 'On the staff' })
  await expect(table.locator('tbody tr[data-handle="diana"]')).toHaveCount(2)
  await dialog.getByRole('button', { name: 'Remove Worker @diana' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'diana', roles: ['reviewer'] }])
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  expect(await calls(page, 'member.remove')).toEqual([])
})

test('takes a member off the staff once the human confirms', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Keep @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toHaveCount(0)
  expect(await calls(page, 'member.remove')).toEqual([])

  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await dialog.getByRole('button', { name: 'Remove @zeus', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ project: 1, agent: 'zeus' }])
  await expect(page.locator('#status')).toHaveText('@zeus left the staff.')
})

const HARNESSES = ['claude', 'codex', 'devin', 'muse', 'pi']
const harnessesAsked = async (page) =>
  (await calls(page, 'models.get')).map((body) => body.harness).sort()

test("New project reads every installed harness's models as it opens, all at once, and each select says it is reading until its answer comes (A4-28)", async ({
  page,
}) => {
  await open(page)
  // Every read takes a while, so that a read asked after another's answer shows.
  await page.evaluate(() => {
    window.__delay['models.get'] = 1500
  })
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  // All five are asked before any answers: no harness waits for another, and none waits for
  // the human to pick it.
  await expect.poll(() => harnessesAsked(page)).toEqual(HARNESSES)
  expect(await answers(page, 'models.get')).toBe(0)
  const models = dialog.locator('[name="chiefModel"]')
  await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Pi' })
  await expect(models.locator('option')).toHaveText(["Reading Pi's models…"])
  await expect(models).toBeDisabled()
  await expect(models).toBeEnabled()
  expect(await offered(models)).toEqual([
    ['Pick a model', true],
    ['meta/muse-spark', false],
    ['meta/muse-spark-1.3-contributor', false],
  ])
  // The pick used the read made as the dialog opened: nothing was asked twice.
  expect(await harnessesAsked(page)).toEqual(HARNESSES)
})

test("Project settings reads every installed harness's models as it opens, all at once, whether the staff runs on it or not (A4-28)", async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() => {
    window.__delay['models.get'] = 1500
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect.poll(() => harnessesAsked(page)).toEqual(HARNESSES)
  expect(await answers(page, 'models.get')).toBe(0)
  const models = dialog.locator('[name="memberModel"]')
  await dialog.locator('[name="memberHarness"]').selectOption({ label: 'Devin' })
  await expect(models.locator('option')).toHaveText(["Reading Devin's models…"])
  await expect(models).toBeEnabled()
  expect(await offered(models)).toEqual([
    ['Pick a model', true],
    ['swe-2', false],
  ])
  expect(await harnessesAsked(page)).toEqual(HARNESSES)
})

test('Settings reads every installed harness as it opens again, whatever role was left picked (A4-28, review 4)', async ({
  page,
}) => {
  const data = model()
  // A project with no ordinary staff: Add member's role stays where it was left.
  data.boards[1].lanes = data.boards[1].lanes.filter((lane) =>
    ['chief', 'human'].includes(lane.participant.role),
  )
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect.poll(() => harnessesAsked(page)).toEqual(HARNESSES)
  // Designer is picked, and the dialog closed: the role is kept for the next opening.
  await dialog.locator('[name="role"]').selectOption('designer')
  await dialog.locator('[value="cancel"]').click()
  await page.evaluate(() => {
    window.__calls.length = 0
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog).toBeVisible()
  await expect.poll(() => harnessesAsked(page)).toEqual(HARNESSES)
})

test("a failed Refresh keeps the edited member's own model and effort (A4-28, review 6)", async ({
  page,
}) => {
  const data = model()
  data.models.claude.listing.models.find((m) => m.id === 'claude-sonnet-5').efforts = [
    'low',
    'high',
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await dialog.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  const models = dialog.locator('[name="memberModel"]')
  const efforts = dialog.locator('[name="memberEffort"]')
  await expect(models).toHaveValue('claude-sonnet-5')
  await expect(efforts).toHaveValue('high')
  await page.evaluate(() => {
    window.__model.modelsRefusal = { claude: 'probe read failed' }
  })
  await dialog.locator('[name="memberRefresh"]').click()
  await expect(dialog.locator('.member-help')).toContainText('probe read failed')
  // The form loses nothing: the member's model, effort and tier stay, and it can be saved.
  await expect(models).toHaveValue('claude-sonnet-5')
  await expect(models).toBeEnabled()
  await expect(efforts).toHaveValue('high')
  await expect(dialog.locator('[name="memberTier"]')).toHaveValue('standard')
  await dialog.getByRole('button', { name: 'Save member' }).click()
  await expect
    .poll(() => calls(page, 'member.edit'))
    .toEqual([
      {
        project: 1,
        member: 'zeus',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        effort: 'high',
        tier: 'standard',
        unchanged: true,
      },
    ])
})

test('a Refresh in one picker updates every picker that shows the harness, and a submission names the read each was shown (A4-28, review 5)', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const chiefModels = dialog.locator('[name="chiefModel"]')
  const memberModels = dialog.locator('[name="memberModel"]')
  await dialog.locator('[name="chiefHarness"]').selectOption('claude-code')
  await dialog.locator('[name="memberHarness"]').selectOption('claude-code')
  await expect(memberModels).toBeEnabled()
  const reads = () => page.evaluate(() => window.__reads.filter((r) => r.harness === 'claude'))
  // One read per harness in the dialog: both pickers show it.
  expect(await reads()).toHaveLength(1)
  await page.evaluate(() => {
    window.__model.models.claude.listing.models = [
      { id: 'new-current', label: 'New', efforts: [], contributor: false, disabled: false },
    ]
  })
  await dialog.locator('[name="chiefRefresh"]').click()
  await expect(chiefModels.locator('option')).toHaveText(['Pick a model', 'new-current'])
  await expect(memberModels.locator('option')).toHaveText(['Pick a model', 'new-current'])
  const [shown, refreshed] = await reads()
  expect(refreshed.read).not.toBe(shown.read)
  // A pick made from the refreshed list names the refreshed read.
  await chiefModels.selectOption('new-current')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(async () => (await calls(page, 'project.open', { reads: true })).map((body) => body.read))
    .toEqual([refreshed.read])
})

test('Add member, New project and Edit member name the read their list was shown from, and an edit that changes nothing says so (A4-28, review 5)', async ({
  page,
}) => {
  const data = model()
  data.models.claude.listing.models.find((m) => m.id === 'claude-sonnet-5').efforts = [
    'low',
    'high',
  ]
  await open(page, data)
  const readOf = (harness) =>
    page.evaluate(
      (harness) => window.__reads.filter((r) => r.harness === harness).at(-1).read,
      harness,
    )
  // Add member: the member is picked from Devin's list, and names that read.
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  await pickMember(settings, { harness: 'Devin', model: 'swe-2', tier: 'light' })
  await settings.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(async () => (await calls(page, 'member.add', { reads: true })).map((body) => body.read))
    .toEqual([await readOf('devin')])
  // Edit member that changes nothing says so, with the read its list came from; one that
  // changes the effort does not.
  await settings.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  await expect(settings.locator('[name="memberModel"]')).toHaveValue('claude-sonnet-5')
  await settings.getByRole('button', { name: 'Save member' }).click()
  await expect.poll(async () => (await calls(page, 'member.edit', { reads: true })).length).toBe(1)
  const [unchanged] = await calls(page, 'member.edit', { reads: true })
  expect(unchanged).toMatchObject({ member: 'zeus', unchanged: true, read: await readOf('claude') })
  await settings.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  await settings.locator('[name="memberEffort"]').selectOption('low')
  await settings.getByRole('button', { name: 'Save member' }).click()
  await expect.poll(async () => (await calls(page, 'member.edit', { reads: true })).length).toBe(2)
  const edits = await calls(page, 'member.edit', { reads: true })
  expect(edits[1].unchanged).toBeUndefined()
  expect(edits[1]).toMatchObject({ effort: 'low', read: await readOf('claude') })
  await settings.getByRole('button', { name: 'Close' }).click()

  // New project: the chief and a staff pick each name the read their list was shown from.
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await pickChief(dialog, 'Pi', 'meta/muse-spark')
  await pickMember(dialog, { harness: 'Devin', model: 'swe-2', tier: 'light' })
  await dialog.locator('[name="memberAdd"]').click()
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect.poll(async () => (await calls(page, 'project.open', { reads: true })).length).toBe(1)
  const [opened] = await calls(page, 'project.open', { reads: true })
  expect(opened.read).toBe(await readOf('pi'))
  expect(opened.staff.map((member) => member.read)).toEqual([await readOf('devin')])
})

test("Edit member keeps the member's own model, effort and tier where its harness's models cannot be read, so the form loses nothing (A4-28)", async ({
  page,
}) => {
  for (const [why, arrange] of [
    [
      'the daemon refuses the read',
      (data) => {
        data.modelsRefusal = { claude: 'claude answered nothing' }
      },
    ],
    [
      'the harness lists no model',
      (data) => {
        data.models.claude.listing = {
          models: [],
          readAt: null,
          freshness: { state: 'stale', why: 'claude is not logged in' },
          how: '',
        }
      },
    ],
  ]) {
    const data = model()
    arrange(data)
    await open(page, data)
    await page.getByRole('button', { name: 'Project settings' }).click()
    const dialog = page.getByRole('dialog', { name: 'Project settings' })
    const add = dialog.locator('.staff-add')
    await dialog.getByRole('button', { name: 'Edit @zeus' }).click()
    await expect(add.locator('legend'), why).toHaveText('Edit @zeus')
    const models = add.locator('[name="memberModel"]')
    // Its harness's list is not there, and the member's model stands where the list would be.
    await expect(models, why).toHaveValue('claude-sonnet-5')
    await expect(models.locator('option'), why).toHaveText(['claude-sonnet-5'])
    await expect(add.locator('[name="memberEffort"]'), why).toHaveValue('high')
    await expect(add.locator('[name="memberTier"]'), why).toHaveValue('standard')
    await expect(add.locator('.member-help'), why).toContainText(/could not be read|lists no model/)
    // Saved as it is: the daemon takes a harness it cannot read at its word.
    await dialog.getByRole('button', { name: 'Save member' }).click()
    await expect
      .poll(() => calls(page, 'member.edit'), why)
      .toEqual([
        {
          project: 1,
          member: 'zeus',
          harness: 'claude-code',
          model: 'claude-sonnet-5',
          effort: 'high',
          tier: 'standard',
          unchanged: true,
        },
      ])
    await page.evaluate(() => {
      window.__calls.length = 0
    })
  }
})

/**
 * A Devin member stores the exact id Devin lists for its effort (`swe-2-high` at `high`); its
 * picker shows the family (`swe-2`) and the effort apart (T-1329, B).
 */
const devinMember = (data) => {
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus').participant
  Object.assign(zeus, { harness: 'devin', model: 'swe-2-high', effort: 'high', modelName: 'swe-2' })
  data.models.devin.listing.models = [
    {
      id: 'swe-2',
      label: 'SWE-2',
      efforts: ['low', 'high'],
      launchIds: { high: 'swe-2-high', low: 'swe-2-low' },
    },
  ]
}

test("a Devin member's edit that changes nothing says so after a Refresh that failed, with the launch id it stores (T-1329, B)", async ({
  page,
}) => {
  const data = model()
  devinMember(data)
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await dialog.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  const models = dialog.locator('[name="memberModel"]')
  await expect(models).toHaveValue('swe-2')
  await expect(dialog.locator('[name="memberEffort"]')).toHaveValue('high')
  await page.evaluate(() => {
    window.__model.modelsRefusal = { devin: 'probe read failed' }
  })
  await dialog.locator('[name="memberRefresh"]').click()
  await expect(dialog.locator('.member-help')).toContainText('probe read failed')
  // The form restores the member's own launch id and effort, and saves them as unchanged: the
  // daemon asks no list for what did not change.
  await expect(models).toHaveValue('swe-2-high')
  await dialog.getByRole('button', { name: 'Save member' }).click()
  await expect
    .poll(() => calls(page, 'member.edit'))
    .toEqual([
      {
        project: 1,
        member: 'zeus',
        harness: 'devin',
        model: 'swe-2-high',
        effort: 'high',
        tier: 'standard',
        unchanged: true,
      },
    ])
})

test("a Devin member's edit after a failed Refresh and a Refresh that came keeps its model picked, says nothing changed, and sends what the human changes (T-1329, B)", async ({
  page,
}) => {
  const data = model()
  devinMember(data)
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await dialog.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  const models = dialog.locator('[name="memberModel"]')
  const efforts = dialog.locator('[name="memberEffort"]')
  await expect(models).toHaveValue('swe-2')
  await page.evaluate(() => {
    window.__model.modelsRefusal = { devin: 'probe read failed' }
  })
  await dialog.locator('[name="memberRefresh"]').click()
  await expect(models).toHaveValue('swe-2-high')
  // Devin lists again: its family is picked at the member's effort, not left to be picked again.
  await page.evaluate(() => {
    window.__model.modelsRefusal = {}
  })
  await dialog.locator('[name="memberRefresh"]').click()
  await expect(models).toHaveValue('swe-2')
  await expect(efforts).toHaveValue('high')
  await dialog.getByRole('button', { name: 'Save member' }).click()
  await expect.poll(async () => (await calls(page, 'member.edit', { reads: true })).length).toBe(1)
  const [unchanged] = await calls(page, 'member.edit', { reads: true })
  expect(unchanged).toMatchObject({ member: 'zeus', unchanged: true })
  // A change of effort is a pick, which the daemon checks against the read it came from.
  await dialog.getByRole('button', { name: 'Edit @zeus', exact: true }).click()
  await expect(models).toHaveValue('swe-2')
  await efforts.selectOption('low')
  await dialog.getByRole('button', { name: 'Save member' }).click()
  await expect.poll(async () => (await calls(page, 'member.edit', { reads: true })).length).toBe(2)
  const edits = await calls(page, 'member.edit', { reads: true })
  expect(edits[1].unchanged).toBeUndefined()
  expect(edits[1]).toMatchObject({ model: 'swe-2', effort: 'low' })
  expect(edits[1].read).toBe(await page.evaluate(() => window.__reads.at(-1).read))
})

test('Add member shows what a harness listed when the dialog opened, says it reads while that read runs, and reads again at Refresh or at the next opening', async ({
  page,
}) => {
  await open(page)
  // A read takes a moment, so that one is on its way when its harness is picked.
  await page.evaluate(() => {
    window.__delay['models.get'] = 800
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const harness = dialog.locator('[name="memberHarness"]')
  const models = dialog.locator('[name="memberModel"]')
  const reads = async () =>
    (await calls(page, 'models.get')).filter((body) => body.harness === 'devin')
  // Every installed harness is read as the dialog opens (A4-28), the staff's and the others'.
  await expect.poll(() => harnessesAsked(page)).toEqual(HARNESSES)
  // Devin's read is on its way: the model select says it is reading, and fills when it comes.
  await harness.selectOption({ label: 'Devin' })
  await expect(models.locator('option')).toHaveText(["Reading Devin's models…"])
  await expect(models).toBeDisabled()
  await expect(models).toBeEnabled()
  await page.evaluate(() => {
    window.__delay['models.get'] = 0
  })
  // Another harness and back: Devin's read is the one made as the dialog opened.
  await harness.selectOption({ label: 'Pi' })
  await expect(models).toBeEnabled()
  await harness.selectOption({ label: 'Devin' })
  await expect(models).toBeEnabled()
  expect(await reads()).toEqual([{ harness: 'devin' }])
  // Refresh reads it again from the harness, and keeps the model picked where it is still listed.
  await models.selectOption({ label: 'swe-2' })
  await dialog.getByRole('button', { name: 'Refresh models' }).click()
  await expect.poll(reads).toEqual([{ harness: 'devin' }, { harness: 'devin', refresh: true }])
  await expect(models).toHaveValue('swe-2')
  // The next opening reads it afresh, as it opens.
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect.poll(async () => (await reads()).length).toBe(3)
  await harness.selectOption({ label: 'Devin' })
  await expect(models).toBeEnabled()
  expect(await reads()).toHaveLength(3)
})

test('keeps a pending removal and what Add member has picked when the daemon redraws the staff', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await pickMember(dialog, {
    role: 'reviewer',
    harness: 'Codex',
    model: 'gpt-6-astra',
    tier: 'complex',
  })
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  const boards = () =>
    page.evaluate(() => window.__calls.filter(([, args]) => args.operation === 'board.get').length)
  for (let redraw = 0; redraw < 2; redraw += 1) {
    const before = await boards()
    await page.evaluate(() => window.__listeners.get('state-changed')())
    await expect.poll(boards).toBeGreaterThan(before)
  }
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await expect(dialog.getByLabel('Role')).toHaveValue('reviewer')
  await expect(dialog.locator('[name="memberHarness"]')).toHaveValue('codex')
  await expect(dialog.locator('[name="memberModel"]')).toHaveValue('gpt-6-astra')
  await expect(dialog.locator('[name="memberTier"]')).toHaveValue('complex')
})

test('Add member saves nothing until a tier is picked: the tier starts empty, whatever the model and its effort', async ({
  page,
}) => {
  await open(page, recordedModels())
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const hint = dialog.locator('#staff-hint')
  const effort = dialog.locator('[name="memberEffort"]')
  const tier = dialog.locator('[name="memberTier"]')
  const add = dialog.getByRole('button', { name: 'Add to staff' })
  // Nothing picked: it says what to pick first.
  await add.click()
  await expect(hint).toHaveText('Pick a harness for the member.')
  // The model's efforts, as Muse lists them for it, the one it runs by default picked.
  await pickMember(dialog, { harness: 'Muse Code', model: 'muse-spark-1.2' })
  expect(await offered(effort)).toEqual([
    ['Pick an effort', true],
    ['minimal', false],
    ['low', false],
    ['medium', false],
    ['high', false],
    ['xhigh', false],
  ])
  await expect(effort).toHaveValue('high')
  expect(await offered(tier)).toEqual([
    ['Pick a tier', true],
    ['Critical work', false],
    ['Complex work', false],
    ['Standard work', false],
    ['Light work', false],
  ])
  // No tier is picked for the human, and nothing is saved without one.
  await expect(tier).toHaveValue('')
  await add.click()
  await expect(hint).toHaveText(
    "Pick the member's tier: the work the chief gives it, from critical to light.",
  )
  expect(await calls(page, 'member.add')).toEqual([])
  await tier.selectOption('standard')
  await add.click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'muse',
        model: 'muse-spark-1.2',
        effort: 'high',
        tier: 'standard',
        roles: ['worker'],
      },
    ])
  await expect(hint).toBeHidden()
})

test("Add member marks a contributor model and a stale list, as the chief's picker does", async ({
  page,
}) => {
  const data = model()
  data.models.devin.listing = {
    ...data.models.devin.listing,
    freshness: { state: 'stale', why: 'devin models list failed: offline' },
  }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const help = dialog.locator('.member-help')
  await pickMember(dialog, { harness: 'Pi', model: 'meta/muse-spark-1.3-contributor' })
  await expect(help).toHaveText(
    'Contributor model: Meta may train on what it sees, including your code.',
  )
  await pickMember(dialog, { harness: 'Devin', model: 'swe-2' })
  await expect(help).toHaveText('This list may be out of date: devin models list failed: offline.')
})

test("Add member offers Devin's efforts for a model as Devin lists them, and sends the model and the effort, never one joined id", async ({
  page,
}) => {
  await open(page, recordedModels())
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await pickMember(dialog, { harness: 'Devin', model: 'claude-opus-5-5-fast' })
  expect(await offered(dialog.locator('[name="memberEffort"]'))).toEqual([
    ['Pick an effort', true],
    ['low', false],
    ['medium', false],
    ['high', false],
    ['xhigh', false],
    ['max', false],
  ])
  await dialog.locator('[name="memberEffort"]').selectOption('medium')
  await dialog.locator('[name="memberTier"]').selectOption('standard')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'devin',
        model: 'claude-opus-5-5-fast',
        effort: 'medium',
        tier: 'standard',
        roles: ['worker'],
      },
    ])
})

test('Edit member moves a member to another model, effort and tier its harness lists, and Cancel leaves it as it was', async ({
  page,
}) => {
  const data = model()
  data.models.claude.listing = listed([
    { id: 'claude-sonnet-5', label: 'Sonnet 5', efforts: ['low', 'medium', 'high'] },
    { id: 'claude-opus-5-5', label: 'Opus 5.5', efforts: ['high', 'max'] },
  ])
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const add = dialog.locator('.staff-add')
  await dialog.getByRole('button', { name: 'Edit @zeus' }).click()
  // The form takes the member as it is: its harness, its model, its effort and its tier.
  await expect(add.locator('legend')).toHaveText('Edit @zeus')
  await expect(dialog.getByLabel('Role')).toBeDisabled()
  await expect(add.locator('[name="memberHarness"]')).toHaveValue('claude-code')
  await expect(add.locator('[name="memberModel"]')).toHaveValue('claude-sonnet-5')
  await expect(add.locator('[name="memberEffort"]')).toHaveValue('high')
  await expect(add.locator('[name="memberTier"]')).toHaveValue('standard')
  // Cancel: nothing is sent, and the form adds again.
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click()
  await expect(add.locator('legend')).toHaveText('Add a member')
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeVisible()
  expect(await calls(page, 'member.edit')).toEqual([])
  // Another model and tier, saved.
  await dialog.getByRole('button', { name: 'Edit @zeus' }).click()
  await expect(add.locator('[name="memberModel"]')).toHaveValue('claude-sonnet-5')
  await add.locator('[name="memberModel"]').selectOption({ label: 'claude-opus-5-5' })
  await add.locator('[name="memberEffort"]').selectOption('max')
  await add.locator('[name="memberTier"]').selectOption('critical')
  await dialog.getByRole('button', { name: 'Save member' }).click()
  await expect
    .poll(() => calls(page, 'member.edit'))
    .toEqual([
      {
        project: 1,
        member: 'zeus',
        harness: 'claude-code',
        model: 'claude-opus-5-5',
        effort: 'max',
        tier: 'critical',
      },
    ])
  await expect(page.locator('#status')).toHaveText(
    '@zeus runs claude-opus-5-5 from its next window.',
  )
  await expect(add.locator('legend')).toHaveText('Add a member')
})

test('marks a member whose harness no longer lists its model, and Refresh takes the mark off once it lists it again', async ({
  page,
}) => {
  const data = model()
  // Claude Code lists Opus 5.5 only: zeus runs Sonnet 5.
  data.models.claude.listing = listed([{ id: 'claude-opus-5-5', label: 'Opus 5.5', efforts: [] }])
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const zeus = dialog.locator('tr[data-handle="zeus"]')
  await expect(zeus).toHaveAttribute('data-model-gone', 'claude-sonnet-5')
  await expect(zeus.locator('mark.model-gone')).toHaveText('claude-sonnet-5')
  await expect(zeus.locator('.member-gone')).toHaveText(
    'Claude Code no longer lists claude-sonnet-5: edit it to a model it lists.',
  )
  // diana's Codex lists her model: no mark.
  await expect(dialog.locator('tr[data-handle="diana"]')).not.toHaveAttribute('data-model-gone')
  // Claude Code lists it again, and the human's Refresh reads it.
  await page.evaluate(() => {
    window.__model.models.claude.listing.models.push({
      id: 'claude-sonnet-5',
      label: 'Sonnet 5',
      efforts: [],
    })
  })
  await dialog.locator('[name="memberHarness"]').selectOption({ label: 'Claude Code' })
  await dialog.getByRole('button', { name: 'Refresh models' }).click()
  await expect
    .poll(() => calls(page, 'models.get'))
    .toContainEqual({ harness: 'claude', refresh: true })
  await expect(zeus).not.toHaveAttribute('data-model-gone')
  await expect(zeus.locator('.member-gone')).toHaveCount(0)
})

test('starts a project with human approval required, and posts the checkbox with the staff', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const gate = dialog.getByLabel('Human approval required', { exact: false })
  await expect(gate).not.toBeChecked()
  await gate.check()
  await pickChief(dialog, 'Claude Code', 'claude-sonnet-5')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: true,
        compaction: 300000,
        staff: [],
      },
    ])
})

test('starts no project until its chief is picked, a harness and one of its models: never a default', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const harness = dialog.locator('[name="chiefHarness"]')
  await expect(harness).toHaveValue('')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect(dialog).toBeVisible()
  // A harness alone is no chief: its model is required too.
  await harness.selectOption({ label: 'Pi' })
  await expect(dialog.locator('[name="chiefModel"]')).toBeEnabled()
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect(dialog).toBeVisible()
  expect(await calls(page, 'project.open')).toEqual([])
  await pickChief(dialog, 'Pi', 'meta/muse-spark')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'pi',
        model: 'meta/muse-spark',
        gate: false,
        compaction: 300000,
        staff: [],
      },
    ])
  await expect(dialog).toBeHidden()
})

/** New project open on /work/fresh, and its sections by name. */
async function newProject(page) {
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const section = (name) => dialog.getByRole('group', { name, exact: true })
  return {
    dialog,
    chief: {
      harness: dialog.locator('[name="chiefHarness"]'),
      model: dialog.locator('[name="chiefModel"]'),
    },
    start: dialog.getByRole('button', { name: 'Start project' }),
    refusal: dialog.locator('.dialog-refusal'),
    staff: section('Staff'),
    approval: section('Human approval').getByRole('checkbox'),
    calls: {
      on: section('Wake-up calls').getByRole('checkbox'),
      every: section('Wake-up calls').getByLabel('Every'),
      unit: section('Wake-up calls').getByLabel('Unit'),
      words: section('Wake-up calls').getByLabel('What it says'),
      save: section('Wake-up calls').getByRole('button', { name: 'Save wake-up calls' }),
    },
    specs: {
      group: section('Specs'),
      mode: section('Specs').locator('[name="specs"]'),
      folder: section('Specs').locator('[name="specsFolder"]'),
      variant: section('Specs').locator('[name="specsVariant"]'),
      version: section('Specs').locator('.specs-version'),
      save: section('Specs').getByRole('button', { name: 'Save specs' }),
    },
    rules: {
      group: section('Rules for the staff'),
      text: section('Rules for the staff').getByLabel('The rules'),
      count: section('Rules for the staff').locator('.staff-rules-count'),
      save: section('Rules for the staff').getByRole('button', { name: 'Save rules' }),
    },
    compaction: {
      group: section('Compaction'),
      own: section('Compaction').getByRole('radio', {
        name: "The harness's own point",
      }),
      size: section('Compaction').getByRole('radio', { name: 'A size' }),
      tokens: section('Compaction').getByLabel('Size in tokens, 100,000 to 1,000,000'),
      hint: section('Compaction').locator('.members-compaction-hint'),
      save: section('Compaction').getByRole('button', {
        name: 'Save compaction',
      }),
    },
    advisorTool: {
      group: section('Advisor tool'),
      tool: section('Advisor tool').getByRole('checkbox'),
      hint: section('Advisor tool').locator('.members-advisor-tool-hint'),
      save: section('Advisor tool').getByRole('button', { name: 'Save advisor tool' }),
    },
    computerUse: section('Computer use'),
  }
}

test('New project has Staff, the rules for the staff, compaction, the advisor tool, Human approval, Wake-up calls, Specs and Computer use after the folder and the chief, with the defaults a project has', async ({
  page,
}) => {
  const data = model()
  data.lastStaff = [lastMember('zeus', 'claude-code', ['worker', 'reviewer'])]
  await open(page, data)
  const made = await newProject(page)
  const { dialog } = made
  // The folder, the chief, and then the sections of Project settings, in its order.
  expect(
    await dialog
      .locator('form > label, form > fieldset')
      .evaluateAll((nodes) =>
        nodes.map((node) =>
          node.matches('fieldset')
            ? node.querySelector('legend').textContent
            : node.querySelector('[name]').name,
        ),
      ),
  ).toEqual([
    'directory',
    'The chief',
    'Staff',
    'Rules for the staff',
    'Compaction',
    'Advisor tool',
    'Human approval',
    'Wake-up calls',
    'Specs',
    'Computer use',
  ])
  // The defaults: the last project's staff, approval off, wake-ups off and specs following the app's setting.
  await expect(made.staff.locator('tbody tr')).toHaveText([
    /claude-sonnet-5.*Worker/,
    /claude-sonnet-5.*Reviewer/,
  ])
  await expect(made.approval).not.toBeChecked()
  await expect(made.calls.on).not.toBeChecked()
  await expect(made.calls.every).toHaveValue('1')
  await expect(made.calls.unit).toHaveValue('hours')
  await expect(made.calls.words).toHaveValue(DEFAULT_WAKE_UP_WORDS)
  await expect(made.specs.mode).toHaveValue('follow')
  // Following says what the app says now, and the variant's first choice names none.
  await expect(made.specs.mode.locator('option:checked')).toHaveText(
    "Follow the app's setting (off)",
  )
  await expect(made.specs.folder).toHaveValue('')
  await expect(made.specs.folder).toHaveAttribute('placeholder', '.specs')
  await expect(made.specs.variant).toHaveValue('')
  await expect(made.specs.variant.locator('option:checked')).toHaveText('As the app says (core)')
  await expect(made.specs.version).toHaveText('Specmint 3.0.0')
  // The rules as a project that never said any has them, and the compaction as a new project
  // has it: a size of 300,000 tokens (T-1400), the harness's own point a choice.
  await expect(made.rules.text).toHaveValue('')
  await expect(made.rules.count).toHaveText('0 of 8,000 bytes · 0 of 150 lines')
  await expect(made.compaction.size).toBeChecked()
  await expect(made.compaction.own).not.toBeChecked()
  await expect(made.compaction.tokens).toBeEnabled()
  await expect(made.compaction.tokens).toHaveValue('300000')
  await expect(made.compaction.hint).toBeHidden()
  // Nothing is saved before there is a project: no Save, and nothing of when a change reaches the chief.
  await expect(made.rules.save).toBeHidden()
  await expect(made.compaction.save).toBeHidden()
  await expect(made.rules.group.locator('.dialog-help [data-live]')).toBeHidden()
  await expect(made.calls.save).toBeHidden()
  await expect(made.specs.save).toBeHidden()
  await expect(made.specs.group.locator('.specs-help [data-live]')).toBeHidden()
  await expect(made.specs.group).toContainText("The app's setting is under Settings, in Specs")
  // The same markup as Project settings has for them, control for control and word for word:
  // but for what is said of the app, which a dialog says once it has read it.
  const markup = (id) =>
    page.evaluate(
      (id) =>
        // Every section but Staff, which each dialog draws for itself: one markup for the rest.
        [...document.querySelectorAll(`#${id} .project-section`)].slice(1).map((section) => {
          const copy = section.cloneNode(true)
          // What each dialog gives its own: the rules' counter's id, and the field's reference to it.
          copy.querySelector('.staff-rules-count')?.removeAttribute('id')
          copy.querySelector('[name="staff-rules"]')?.removeAttribute('aria-describedby')
          // What the section draws from what it reads, which is the dialog's own.
          for (const drawn of copy.querySelectorAll(
            '.computer-use-harnesses, .computer-use-apps, .computer-use-grants, .computer-use-grants-of',
          )) {
            drawn.replaceChildren()
          }
          const variant = copy.querySelector('[name="specsVariant"]')
          if (variant !== null) {
            const [first] = variant.options
            first.textContent = ''
            variant.replaceChildren(first)
            copy.querySelector('[name="specs"] [value="follow"]').textContent = ''
            copy.querySelector('[name="specsFolder"]').removeAttribute('placeholder')
            copy.querySelector('.specs-version').textContent = ''
          }
          return copy.outerHTML.replaceAll(' hidden=""', '')
        }),
      id,
    )
  const [here, there] = [
    await markup('new-project-dialog'),
    await markup('project-settings-dialog'),
  ]
  expect(here).toHaveLength(7)
  expect(here).toEqual(there)
  // Left as it opened, the start says nothing of what a project has anyway: the request is the one it was.
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        compaction: 300000,
        staff: [
          {
            harness: 'claude-code',
            model: 'claude-sonnet-5',
            effort: 'high',
            tier: 'standard',
            roles: ['worker', 'reviewer'],
          },
        ],
      },
    ])
})

test('New project: starts a project with its approval, wake-up calls and specs set, all in the one request that opens it', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.approval.check()
  await made.calls.on.check()
  await made.calls.every.fill('2')
  await made.calls.unit.selectOption('days')
  await made.calls.words.fill('Anything left on the board?')
  await made.specs.mode.selectOption('on')
  await made.specs.folder.fill('  docs/specs ')
  await made.specs.variant.selectOption('tdd')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: true,
        compaction: 300000,
        staff: [],
        wakeUp: { on: true, every: 2, unit: 'days', text: 'Anything left on the board?' },
        specs: { on: true, folder: 'docs/specs', variant: 'tdd' },
      },
    ])
  await expect(made.dialog).toBeHidden()
  // The chief starts with them, so nothing is said of them afterwards, when it has launched already.
  for (const operation of ['project.gate', 'wakeup.set', 'project.specs']) {
    expect(await calls(page, operation), operation).toEqual([])
  }
})

test('New project: says all of its specs and all of its wake-up calls once either is set at all, as Project settings saves them', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  // A variant alone is a word of the project's: it follows the app's setting, with that variant.
  await made.specs.variant.selectOption('core-html')
  // The calls left off, an hour apart no more, are the human's word too.
  await made.calls.every.fill('45')
  await made.calls.unit.selectOption('minutes')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        compaction: 300000,
        staff: [],
        wakeUp: { on: false, every: 45, unit: 'minutes', text: DEFAULT_WAKE_UP_WORDS },
        specs: { on: null, folder: null, variant: 'core-html' },
      },
    ])
  await expect(made.dialog).toBeHidden()
  // Off needs no folder or variant, as in Project settings.
  await page.getByRole('button', { name: 'New project' }).click()
  await pickChief(made.dialog, 'Codex', 'gpt-6-astra')
  await made.specs.mode.selectOption('off')
  await expect(made.specs.folder).toBeDisabled()
  await expect(made.specs.variant).toBeDisabled()
  await made.start.click()
  await expect.poll(async () => (await calls(page, 'project.open')).length).toBe(2)
  expect((await calls(page, 'project.open'))[1].specs).toEqual({
    on: false,
    folder: null,
    variant: null,
  })
})

test('New project: a start the daemon refuses is said in the dialog in the daemon’s words, with what the human chose kept, and starts nothing', async ({
  page,
}) => {
  const data = model()
  data.lastStaff = [lastMember('zeus', 'claude-code', ['worker'])]
  data.openRefusal =
    'the specs folder is a folder inside the project: it stays inside it, with no `..`'
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Codex', 'gpt-6-astra')
  await made.specs.mode.selectOption('on')
  await made.specs.folder.fill('../elsewhere')
  await made.start.click()
  await expect(made.refusal).toHaveText(data.openRefusal)
  // The dialog is as the human left it, to mend and try again.
  await expect(made.dialog).toBeVisible()
  await expect(made.chief.harness).toHaveValue('codex')
  await expect(made.chief.model).toHaveValue('gpt-6-astra')
  await expect(made.specs.mode).toHaveValue('on')
  await expect(made.specs.folder).toHaveValue('../elsewhere')
  await expect(made.staff.locator('tbody tr')).toHaveText([/claude-sonnet-5.*Worker/])
  // Nothing was started: the project shown is the one that was, and the status line says nothing.
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect(page.locator('#status')).toHaveText('')
  // Mended, the next try goes through, and the refusal goes with the dialog.
  await page.evaluate(() => {
    window.__model.openRefusal = null
  })
  await made.specs.folder.fill('docs')
  await made.start.click()
  await expect(made.dialog).toBeHidden()
  const asked = await calls(page, 'project.open')
  expect(asked.map((body) => body.specs)).toEqual([
    { on: true, folder: '../elsewhere', variant: null },
    { on: true, folder: 'docs', variant: null },
  ])
  // Opened again, it is as a new project's dialog opens, with no refusal in it.
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(made.refusal).toBeHidden()
})

test('New project: a number that is no whole number of at least one holds the start back, as it holds the save in Project settings', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  // The browser holds a number under one, or none, or no whole number: nothing is asked of the daemon.
  for (const bad of ['0', '-3', '1.5', '']) {
    await made.calls.every.fill(bad)
    await made.start.click()
    await expect(made.calls.every).toHaveJSProperty('validity.valid', false)
  }
  await expect(made.dialog).toBeVisible()
  expect(await calls(page, 'project.open')).toEqual([])
  await made.calls.every.fill('2')
  await made.start.click()
  await expect.poll(() => calls(page, 'project.open')).toHaveLength(1)
  expect((await calls(page, 'project.open'))[0].wakeUp).toEqual({
    on: false,
    every: 2,
    unit: 'hours',
    text: DEFAULT_WAKE_UP_WORDS,
  })
})

test("New project: the new chief's terminal has the keyboard once its project is open, which the dialog over the page would not let it", async ({
  page,
}) => {
  const data = model()
  data.realTerminals = true
  data.projects.push({ id: 3, name: 'new', directory: '/work/fresh', state: 'open' })
  data.boards[3] = {
    project: { id: 3, name: 'new', directory: '/work/fresh', state: 'open' },
    open: [],
    lanes: [
      {
        participant: { ...participant(30, 'chief', 'chief'), projectId: 3 },
        tasks: [],
        activity: { state: 'starting' },
        pane: { id: 'p3-chief', generation: 1 },
      },
    ],
  }
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.start.click()
  await expect(page.locator('#project-title')).toHaveText('new')
  await expect(made.dialog).toBeHidden()
  // The page shows a chief's terminal the first time once, and gives it the keyboard then.
  await expect.poll(() => typingIn(page)).toBe('chief')
})

test('New project: a start asked twice while the daemon answers is asked of it once, and one refused after the human left the dialog is said on the status line', async ({
  page,
}) => {
  const data = model()
  data.openRefusal = 'the wake-up text must be text, not empty, at most 1000000 characters'
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.calls.on.check()
  await made.calls.words.fill('   ')
  await hold(page, 'project.open')
  await made.start.click()
  await made.start.click()
  await expect.poll(() => calls(page, 'project.open')).toHaveLength(1)
  // The human leaves before the daemon answers: its refusal has no dialog to be said in.
  await page.keyboard.press('Escape')
  await expect(made.dialog).toBeHidden()
  await letGo(page, 'project.open')
  await expect(page.locator('#status')).toHaveText(data.openRefusal)
  expect(await calls(page, 'project.open')).toHaveLength(1)
})

test('Enter in the number of the wake-up calls or in the specs folder of New project starts no project, and Start does', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.calls.every.press('Enter')
  await made.specs.folder.fill('docs')
  await made.specs.folder.press('Enter')
  await expect(made.dialog).toBeVisible()
  expect(await calls(page, 'project.open')).toEqual([])
  expect(await calls(page, 'wakeup.set')).toEqual([])
  expect(await calls(page, 'project.specs')).toEqual([])
  await made.start.click()
  await expect.poll(() => calls(page, 'project.open')).toHaveLength(1)
})

/** Every field a person can press Enter in, in `dialog`: what is on show and not disabled. */
const fieldsIn = (dialog) =>
  dialog.locator(
    'input:not([type=hidden]):not(:disabled):visible, select:not(:disabled):visible, textarea:not(:disabled):visible',
  )

/** What a field is, to say which one failed: its tag, type and name. */
const fieldName = (field) =>
  field.evaluate((node) =>
    `${node.tagName.toLowerCase()}${node.type ? `[${node.type}]` : ''} ${node.name || node.getAttribute('aria-label') || ''}`.trim(),
  )

test('Enter in any field of New project starts no project, and is a new line in a text area; Start alone does', async ({
  page,
}) => {
  const data = model()
  data.computerUse = {
    on: true,
    codexSetUp: true,
    permissions: { accessibility: true, screenRecording: false },
  }
  data.lastStaff = [lastMember('zeus', 'claude-code', ['worker'])]
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  // A size chosen, so the field that takes one can be pressed in too.
  await made.compaction.size.check()
  const fields = fieldsIn(made.dialog)
  const count = await fields.count()
  // The folder, the chief's three, the staff's, and every field of every section.
  expect(count).toBeGreaterThanOrEqual(16)
  for (let at = 0; at < count; at += 1) {
    const field = fields.nth(at)
    const name = await fieldName(field)
    const isText = await field.evaluate((node) => node.tagName === 'TEXTAREA')
    const before = isText ? await field.inputValue() : null
    await field.press('Enter')
    expect(await calls(page, 'project.open'), `Enter in ${name} started a project`).toEqual([])
    await expect(made.dialog, `Enter in ${name} closed the dialog`).toBeVisible()
    // A text area keeps what Enter is for: a new line.
    if (isText) expect(await field.inputValue(), `Enter in ${name}`).toBe(`${before}\n`)
  }
  // None of it saved anything either: there is no project to save for.
  for (const operation of [
    'wakeup.set',
    'project.specs',
    'project.rules',
    'project.compaction',
    'project.advisor_tool',
  ]) {
    expect(await calls(page, operation), operation).toEqual([])
  }
  // Enter keeps what it is for on a button: its press.
  await made.computerUse
    .getByRole('button', { name: 'Open System Settings for Accessibility' })
    .press('Enter')
  await expect.poll(() => calls(page, 'computer_use.open')).toEqual([{ pane: 'accessibility' }])
  expect(await calls(page, 'project.open')).toEqual([])
  await made.start.click()
  await expect.poll(() => calls(page, 'project.open')).toHaveLength(1)
})

test('Enter in a field of a section of Project settings adds nobody to the staff, saves no other section and leaves the dialog open', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = { on: true, permissions: { accessibility: true, screenRecording: false } }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  // Add a member is filled in, so a form that was submitted would add it.
  await pickMember(dialog, {
    role: 'worker',
    harness: 'Codex',
    model: 'gpt-6-astra',
    tier: 'standard',
  })
  await dialog
    .getByRole('group', { name: 'Compaction' })
    .getByRole('radio', { name: 'A size' })
    .check()
  const sections = dialog.locator('fieldset.project-section:not(:has(> .staff))')
  const fields = fieldsIn(sections)
  const count = await fields.count()
  expect(count).toBeGreaterThanOrEqual(8)
  // What a field saves of its own section on Enter, as it always did: the number of the calls and the specs' folder.
  const OWN = { 'wakeup-every': 'wakeup.set', specsFolder: 'project.specs' }
  const OPERATIONS = [
    'member.add',
    'member.edit',
    'member.roles',
    'project.gate',
    'wakeup.set',
    'project.specs',
    'project.rules',
    'project.compaction',
    'project.advisor_tool',
  ]
  const tally = async () =>
    Object.fromEntries(
      await Promise.all(
        OPERATIONS.map(async (operation) => [operation, (await calls(page, operation)).length]),
      ),
    )
  for (let at = 0; at < count; at += 1) {
    const field = fields.nth(at)
    const name = await fieldName(field)
    const isText = await field.evaluate((node) => node.tagName === 'TEXTAREA')
    const before = await tally()
    await field.press('Enter')
    // Whatever was asked of the daemon has come back before it is counted.
    await page.evaluate(() => new Promise((done) => setTimeout(done, 40)))
    const after = await tally()
    const own = OWN[await field.getAttribute('name')]
    const expected = { ...before }
    if (own !== undefined) expected[own] += 1
    expect(after, `Enter in ${name}`).toEqual(expected)
    await expect(dialog, `Enter in ${name} closed the dialog`).toBeVisible()
    if (isText) expect(await field.inputValue()).toMatch(/\n$/)
  }
  expect(await calls(page, 'member.add')).toEqual([])
})

test('no id is held by two elements of the page, and what a field is described or labelled by is in its own dialog', async ({
  page,
}) => {
  await open(page)
  // The sections are one markup cloned into two dialogs: what one says by an id, the other must not share.
  const found = await page.evaluate(() => {
    const ids = [...document.querySelectorAll('[id]')].map((node) => node.id)
    const duplicates = ids.filter((id, at) => ids.indexOf(id) !== at)
    // A description or a label is read from inside the dialog it is in (a menu's aria-controls
    // may name another dialog, which is what it opens).
    const strays = []
    for (const node of document.querySelectorAll('[aria-describedby], [aria-labelledby]')) {
      const home = node.closest('dialog') ?? document
      for (const attribute of ['aria-describedby', 'aria-labelledby']) {
        for (const id of (node.getAttribute(attribute) ?? '').split(/\s+/).filter(Boolean)) {
          const target = document.getElementById(id)
          if (target === null || !home.contains(target)) strays.push(`${attribute}=${id}`)
        }
      }
    }
    return { duplicates, strays }
  })
  expect(found).toEqual({ duplicates: [], strays: [] })
})

test("each dialog's rules for the staff are described by its own counter, whatever the other says", async ({
  page,
}) => {
  await open(page)
  const rulesOf = (dialog) =>
    dialog.getByRole('group', { name: 'Rules for the staff', exact: true }).getByLabel('The rules')
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  await rulesOf(settings).fill('abc')
  await expect(rulesOf(settings)).toHaveAccessibleDescription('3 of 8,000 bytes · 1 of 150 lines')
  await settings.getByRole('button', { name: 'Close' }).click()
  const made = await newProject(page)
  await made.rules.text.fill('abcdef\nghi')
  await expect(made.rules.text).toHaveAccessibleDescription('10 of 8,000 bytes · 2 of 150 lines')
  // Two counters, one for each dialog's field: neither field names the other's.
  const references = await page.evaluate(() =>
    [...document.querySelectorAll('textarea[name="staff-rules"]')].map((field) => [
      field.closest('dialog').id,
      field.getAttribute('aria-describedby'),
    ]),
  )
  expect(references.map(([dialog]) => dialog).sort()).toEqual([
    'new-project-dialog',
    'project-settings-dialog',
  ])
  expect(new Set(references.map(([, id]) => id)).size).toBe(2)
})

test('New project opens on the defaults every time: what was set in it before is not kept', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await made.approval.check()
  await made.calls.on.check()
  await made.calls.every.fill('9')
  await made.calls.words.fill('Hello?')
  await made.specs.mode.selectOption('on')
  await made.specs.folder.fill('docs')
  await made.specs.variant.selectOption('tdd-html')
  await made.rules.text.fill('Never push to upstream.')
  await made.compaction.size.check()
  await made.compaction.tokens.fill('250000')
  await made.dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(made.dialog).toBeHidden()
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(made.rules.text).toHaveValue('')
  await expect(made.rules.count).toHaveText('0 of 8,000 bytes · 0 of 150 lines')
  await expect(made.compaction.size).toBeChecked()
  await expect(made.compaction.tokens).toHaveValue('300000')
  await expect(made.compaction.tokens).toBeEnabled()
  await expect(made.approval).not.toBeChecked()
  await expect(made.calls.on).not.toBeChecked()
  await expect(made.calls.every).toHaveValue('1')
  await expect(made.calls.words).toHaveValue(DEFAULT_WAKE_UP_WORDS)
  await expect(made.specs.mode).toHaveValue('follow')
  await expect(made.specs.folder).toHaveValue('')
  await expect(made.specs.variant).toHaveValue('')
  await expect(made.specs.folder).toBeEnabled()
})

test("New project started without touching the compaction asks for a size of 300,000, and the harness's own point is asked for as none", async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        staff: [],
        compaction: 300000,
      },
    ])
  // The harness's own point is still a choice: said as null, which the daemon reads as none
  // (a request that said nothing starts on the size).
  await page.getByRole('button', { name: 'New project' }).click()
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.compaction.own.check()
  await expect(made.compaction.tokens).toBeDisabled()
  await made.start.click()
  await expect.poll(() => calls(page, 'project.open')).toHaveLength(2)
  expect((await calls(page, 'project.open'))[1]).toEqual({
    directory: '/work/fresh',
    harness: 'claude-code',
    model: 'claude-sonnet-5',
    gate: false,
    staff: [],
    compaction: null,
  })
})

test('New project made without touching the compaction opens in Project settings on its size of 300,000', async ({
  page,
}) => {
  const data = model()
  data.openMakesBoard = true
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.start.click()
  await expect(made.dialog).toBeHidden()
  await expect(page.locator('#project-title')).toHaveText('new')
  const section = await compactionGroup(page)
  await expect(section.size).toBeChecked()
  await expect(section.own).not.toBeChecked()
  await expect(section.amount).toHaveValue('300000')
})

/** The legends of a dialog's sections, in the order they are drawn, hidden ones too. */
const legendsOf = (page, id) =>
  page.evaluate(
    (id) =>
      [...document.querySelectorAll(`#${id} fieldset.project-section > legend`)].map(
        (legend) => legend.textContent,
      ),
    id,
  )

/**
 * The sections of Project settings that New project does not hold, by their legends, and why
 * each stays out (T-1268). A section is in both dialogs, drawn from the one markup the page
 * holds (`#project-sections`), or it is here with its reason: a section added to Project
 * settings alone fails this test until it is one or the other. Staff is in both, and drawn by
 * each for itself.
 */
const NEW_PROJECT_LACKS = {}

test('New project holds every section Project settings holds, in its order, but those named with a reason', async ({
  page,
}) => {
  const data = model()
  data.lastStaff = [lastMember('zeus', 'claude-code', ['worker'])]
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = await legendsOf(page, 'project-settings-dialog')
  await page
    .getByRole('dialog', { name: 'Project settings' })
    .getByRole('button', { name: 'Close' })
    .click()
  await newProject(page)
  const made = await legendsOf(page, 'new-project-dialog')
  // Each exception is a section Project settings has and New project lacks, and a reason.
  for (const [legend, reason] of Object.entries(NEW_PROJECT_LACKS)) {
    expect(settings, legend).toContain(legend)
    expect(made, legend).not.toContain(legend)
    expect(reason.length, `${legend} has no reason`).toBeGreaterThan(20)
  }
  expect(made).toEqual(settings.filter((legend) => !(legend in NEW_PROJECT_LACKS)))
  expect(made).toHaveLength(8)
})

test('New project: the rules for the staff and the compaction are written as they are in Project settings, counted and checked as typed, and start with the project', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  // Counted against the bounds as the human types, as in Project settings.
  await made.rules.text.fill(STAFF_RULES)
  await expect(made.rules.count).toHaveText('75 of 8,000 bytes · 2 of 150 lines')
  await expect(made.rules.group).toContainText('No secrets: no passwords, tokens or keys')
  // A size is checked as it is typed, and New project opens on the default's.
  await expect(made.compaction.size).toBeChecked()
  await expect(made.compaction.tokens).toBeEnabled()
  await expect(made.compaction.tokens).toHaveValue('300000')
  await made.compaction.tokens.fill('250000')
  await expect(made.compaction.hint).toBeHidden()
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        staff: [],
        rules: STAFF_RULES,
        compaction: 250000,
      },
    ])
  await expect(made.dialog).toBeHidden()
  // They start with the project, in the one write that makes it: nothing is saved afterwards.
  for (const operation of ['project.rules', 'project.compaction', 'project.advisor_tool']) {
    expect(await calls(page, operation), operation).toEqual([])
  }
})

test('New project: rules over a bound, or a size that is no whole number in the range, hold the start back as they hold the save in Project settings, and say why', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  // 8,001 bytes of rules.
  await made.rules.text.fill('x'.repeat(8_001))
  await expect(made.rules.count).toHaveClass(/over/)
  await expect(made.rules.text).toHaveAttribute('aria-invalid', 'true')
  await made.start.click()
  await expect(made.refusal).toContainText('8,000 bytes')
  // A size outside the range.
  await made.rules.text.fill('Short.')
  await made.compaction.size.check()
  await made.compaction.tokens.fill('99999')
  await expect(made.compaction.hint).toHaveText(
    'A size is a whole number from 100,000 to 1,000,000.',
  )
  await expect(made.compaction.tokens).toHaveAttribute('aria-invalid', 'true')
  await made.start.click()
  await expect(made.refusal).toContainText('A size is a whole number from 100,000 to 1,000,000.')
  expect(await calls(page, 'project.open')).toEqual([])
  // Put right, it starts, with both.
  await made.compaction.tokens.fill('100000')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        staff: [],
        rules: 'Short.',
        compaction: 100000,
      },
    ])
})

test('New project: a project made with rules, a compaction size and computer use said opens in Project settings with them', async ({
  page,
}) => {
  const data = model()
  data.openMakesBoard = true
  data.computerUse = { on: true, permissions: { accessibility: true, screenRecording: false } }
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.rules.text.fill(STAFF_RULES)
  await made.compaction.size.check()
  await made.compaction.tokens.fill('300000')
  // Computer use is Claude Code's own switch for the folder, read and not written: what the dialog
  // says of it for the folder is what the project's settings say of it once the project is there.
  await expect(made.computerUse.getByRole('heading', { name: 'Claude Code' })).toBeVisible()
  const said = await computerUse(made.dialog)
  await made.start.click()
  await expect(made.dialog).toBeHidden()
  await expect(page.locator('#project-title')).toHaveText('new')
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const section = (name) => dialog.getByRole('group', { name, exact: true })
  await expect(section('Rules for the staff').getByLabel('The rules')).toHaveValue(STAFF_RULES)
  await expect(section('Compaction').getByRole('radio', { name: 'A size' })).toBeChecked()
  await expect(
    section('Compaction').getByLabel('Size in tokens, 100,000 to 1,000,000'),
  ).toHaveValue('300000')
  expect(await computerUse(dialog)).toEqual(said)
  expect(said.map(({ block }) => block)).toEqual(['claude-code', 'permissions'])
})

test("New project: computer use is read for the folder picked, shown while the chief or a member runs on a harness that has one, and opens the panes as Project settings' does", async ({
  page,
}) => {
  const data = model()
  data.computerUse = {
    on: true,
    codexSetUp: true,
    permissions: { accessibility: true, screenRecording: false },
  }
  await open(page, data)
  const made = await newProject(page)
  // No project yet, so the folder is what is asked about, and nothing is asked of a project.
  await expect.poll(() => calls(page, 'computer_use.get')).toEqual([{ directory: '/work/fresh' }])
  // No harness picked: no computer use to say.
  await expect(made.computerUse).toBeHidden()
  // A chief on Pi has none; on Claude Code it has, for the folder.
  await pickChief(made.dialog, 'Pi', 'meta/muse-spark')
  await expect(made.computerUse).toBeHidden()
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await expect(made.computerUse).toBeVisible()
  expect(await computerUse(made.dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'On for this project',
      lines: [CLAUDE_ON],
    },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [MEND, KEPT],
      columns: ['Permission', 'ConsensFlow'],
      rows: [
        [ACCESSIBILITY, 'allowed', OPEN],
        [SCREEN, 'not allowed', OPEN],
      ],
    },
  ])
  // A member on Codex adds Codex's block and its column, and the panes stay one button each.
  await pickMember(made.dialog, {
    role: 'worker',
    harness: 'Codex',
    model: 'gpt-6-astra',
    tier: 'standard',
  })
  await made.dialog.getByRole('button', { name: 'Add', exact: true }).click()
  expect((await computerUse(made.dialog)).map(({ block }) => block)).toEqual([
    'claude-code',
    'codex',
    'permissions',
  ])
  await expect(made.computerUse.getByRole('button')).toHaveText([OPEN, OPEN])
  await made.computerUse
    .getByRole('button', { name: 'Open System Settings for Screen & System Audio Recording' })
    .click()
  await expect.poll(() => calls(page, 'computer_use.open')).toEqual([{ pane: 'screen-recording' }])
  // The member leaves: Codex's block goes with it.
  await made.dialog.getByRole('button', { name: /^Remove Worker/ }).click()
  expect((await computerUse(made.dialog)).map(({ block }) => block)).toEqual([
    'claude-code',
    'permissions',
  ])
  // A start sends nothing of computer use: there is nothing of it to keep.
  await made.start.click()
  await expect.poll(async () => (await calls(page, 'project.open')).length).toBe(1)
  expect(Object.keys((await calls(page, 'project.open'))[0]).sort()).toEqual([
    'compaction',
    'directory',
    'gate',
    'harness',
    'model',
    'staff',
  ])
})

test('New project: back from System Settings, computer use is read again for the folder picked, and only while the dialog is open', async ({
  page,
}) => {
  const data = model()
  data.computerUse = { on: true, permissions: { accessibility: false, screenRecording: false } }
  await open(page, data)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  const accessibility = made.computerUse.locator(
    'tr[data-grant="accessibility"] .computer-use-status',
  )
  await expect(accessibility).toHaveText('not allowed')
  // The human allows it in System Settings and comes back to the app.
  await page.evaluate(() => {
    window.__model.computerUse = {
      on: true,
      permissions: { accessibility: true, screenRecording: false },
    }
    window.dispatchEvent(new Event('focus'))
  })
  await expect(accessibility).toHaveText('allowed')
  expect(await calls(page, 'computer_use.get')).toEqual([
    { directory: '/work/fresh' },
    { directory: '/work/fresh' },
  ])
  // A dialog that is shut asks nothing.
  await made.dialog.getByRole('button', { name: 'Cancel' }).click()
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await page.evaluate(() => new Promise((done) => setTimeout(done, 100)))
  expect(await calls(page, 'computer_use.get')).toHaveLength(2)
})

test("offers a new project's chief only the harnesses installed here, and its staff only members on them", async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'devin']
  data.lastStaff = [
    {
      ...lastMember('zeus', 'devin', ['worker'], 'light'),
      model: 'swe-1-6-slow',
      modelName: 'ares',
    },
    lastMember('diana', 'codex', ['worker'], 'light'),
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  // Claude Code and Devin are not installed here.
  expect(await offered(dialog.locator('[name="chiefHarness"]'))).toEqual([
    ['Pick a harness', true],
    ['Codex', false],
    ['Pi', false],
    ['Muse Code', false],
  ])
  await expect(dialog.locator('tr[data-model="swe-1-6-slow"]')).toHaveCount(0)
  await expect(dialog.locator('tr[data-model="gpt-5.6-luna"]')).toHaveCount(1)
  expect(await offered(dialog.locator('[name="memberHarness"]'))).toEqual([
    ['Pick a harness', true],
    ['Codex', false],
    ['Pi', false],
    ['Muse Code', false],
  ])
})

test("starts a new project's staff on the model as its harness's list names it, which is not Devin's launch id for the effort", async ({
  page,
}) => {
  const data = model()
  // Devin's member holds the id its launch passes for the effort (`swe-2-max`); the list holds
  // `swe-2`, which is what the daemon takes for a pick, with the effort beside it.
  data.lastStaff = [
    {
      ...lastMember('zeus', 'devin', ['worker'], 'light'),
      model: 'swe-2-max',
      modelName: 'swe-2',
      effort: 'max',
    },
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const row = dialog.locator('tr[data-model="swe-2"]')
  await expect(row).toHaveCount(1)
  await expect(row.locator('.member-name')).toHaveText('swe-2')
  await expect(row.locator('.member-meta')).toHaveText('Devin · max · light')
  await expect(dialog.locator('tr[data-model="swe-2-max"]')).toHaveCount(0)
})

test('refuses a new project when no harness is installed here, and says where to get one', async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'codex', 'pi', 'devin', 'muse']
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(page.locator('#status')).toHaveText(
    'No harness is installed here: install one from Agents, Harnesses.',
  )
  await expect(page.getByRole('dialog', { name: 'New project' })).toBeHidden()
})

test('starts no project without a folder: none chosen, or no picker in this window', async ({
  page,
}) => {
  await open(page)
  // The picker closed with no folder chosen: nothing is asked of the core.
  await page.evaluate(() => {
    window.__picked = 0
    window.__TAURI__.dialog.open = async () => {
      window.__picked += 1
      return null
    }
  })
  await page.getByRole('button', { name: 'New project' }).click()
  await expect.poll(() => page.evaluate(() => window.__picked)).toBe(1)
  expect(await calls(page, 'staff.last')).toEqual([])
  await expect(page.getByRole('dialog', { name: 'New project' })).toBeHidden()
  // A window with no folder picker says so.
  await page.evaluate(() => {
    delete window.__TAURI__.dialog
  })
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(page.locator('#status')).toHaveText(
    'The folder picker is not available in this window.',
  )
  await expect(page.getByRole('dialog', { name: 'New project' })).toBeHidden()
})

test('says in New project who may be picked: nobody yet, and how a member is added', async ({
  page,
}) => {
  await open(page)
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const rows = dialog.getByRole('table', { name: 'Members for the staff' }).locator('tbody tr')
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(rows).toHaveText([
    'Nobody yet: pick a role, a harness, a model and a tier, then Add.',
  ])
  await expect(dialog.locator('#new-project-hint')).toBeHidden()
  // Add with nothing picked adds nobody.
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await expect(rows).toHaveText([
    'Nobody yet: pick a role, a harness, a model and a tier, then Add.',
  ])
})

test('shows and sets human approval from Project settings, which has no review policy', async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.gate = true
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByLabel('Second review of')).toHaveCount(0)
  const gate = dialog.getByLabel('Human approval required', { exact: false })
  await expect(gate).toBeChecked()
  await gate.uncheck()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 1, gate: false }])
  await expect(page.locator('#status')).toHaveText(
    'Messages between agents go straight through again.',
  )
  await gate.check()
  await expect
    .poll(() => calls(page, 'project.gate'))
    .toEqual([
      { project: 1, gate: false },
      { project: 1, gate: true },
    ])
  await expect(page.locator('#status')).toHaveText(
    'Every message between agents now waits for your approval.',
  )
  await expect(gate).toBeChecked()
})

test("names the board's button and its dialog Project settings, with Staff, Rules for the staff, Compaction, Advisor tool, Human approval, Wake-up calls and Specs as its sections, in that order", async ({
  page,
}) => {
  await open(page)
  const button = page.getByRole('button', { name: 'Project settings' })
  await expect(button).toHaveText('Project settings')
  // It is the button that was Staff, and nothing of that name is left to find.
  await expect(page.getByRole('button', { name: 'Staff', exact: true })).toHaveCount(0)
  await button.click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('heading', { name: 'Project settings' })).toBeVisible()
  // Computer use is last, and there since the project has Claude Code members.
  await expect(dialog.locator('fieldset.project-section > legend')).toHaveText([
    'Staff',
    'Rules for the staff',
    'Compaction',
    'Advisor tool',
    'Human approval',
    'Wake-up calls',
    'Specs',
    'Computer use',
  ])
  for (const name of [
    'Staff',
    'Rules for the staff',
    'Compaction',
    'Advisor tool',
    'Human approval',
    'Wake-up calls',
    'Specs',
  ]) {
    await expect(dialog.getByRole('group', { name, exact: true })).toBeVisible()
  }
  // The old title is nowhere in the page, hidden or not.
  await expect(page.locator('body')).not.toContainText('Project staff')
})

test('Project settings: each section saves in its own way as before, and none saves another', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const section = (name) => dialog.getByRole('group', { name, exact: true })
  const [approval, calling, specs] = ['Human approval', 'Wake-up calls', 'Specs'].map(section)
  const asked = () =>
    Promise.all(
      ['project.gate', 'wakeup.set', 'project.specs', 'member.add'].map((operation) =>
        calls(page, operation),
      ),
    )
  // The calls and the specs wait for their own Save: nothing is sent as they are set.
  await calling.getByRole('checkbox').check()
  await calling.getByLabel('Every').fill('3')
  await specs.locator('[name="specs"]').selectOption('on')
  await specs.locator('[name="specsFolder"]').fill('docs')
  expect(await asked()).toEqual([[], [], [], []])
  // The approval saves as it is ticked, and nothing but it.
  await approval.getByRole('checkbox').check()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 1, gate: true }])
  expect(await asked()).toEqual([[{ project: 1, gate: true }], [], [], []])
  // The calls by their own Save, the whole of them.
  await calling.getByRole('button', { name: 'Save wake-up calls' }).click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([{ project: 1, on: true, every: 3, unit: 'hours', text: DEFAULT_WAKE_UP_WORDS }])
  expect(await calls(page, 'project.specs')).toEqual([])
  // The specs by theirs.
  await specs.getByRole('button', { name: 'Save specs' }).click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: true, folder: 'docs', variant: null }])
  expect(await calls(page, 'wakeup.set')).toHaveLength(1)
  // The staff as a member is added, at once.
  await pickMember(dialog, { harness: 'Codex', model: 'gpt-6-astra', tier: 'critical' })
  await section('Staff').getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: null,
        tier: 'critical',
        roles: ['worker'],
      },
    ])
  expect(await calls(page, 'project.gate')).toHaveLength(1)
})

/** The project's Specs section of Project settings, opened. */
async function openSpecs(page) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const group = dialog.getByRole('group', { name: 'Specs' })
  return {
    dialog,
    group,
    mode: group.locator('[name="specs"]'),
    folder: group.locator('[name="specsFolder"]'),
    variant: group.locator('[name="specsVariant"]'),
    save: group.getByRole('button', { name: 'Save specs' }),
    note: group.locator('.specs-note'),
  }
}

test('saves the specs on Enter in the folder and adds nobody to the staff', async ({ page }) => {
  await open(page)
  const specs = await openSpecs(page)
  await specs.mode.selectOption('on')
  await specs.folder.fill('docs/specs')
  await specs.folder.press('Enter')
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: true, folder: 'docs/specs', variant: null }])
  expect(await calls(page, 'member.add')).toEqual([])
  expect(await calls(page, 'member.roles')).toEqual([])
})

/** The model's first project with its chief's window closed. */
function chiefClosed() {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.role === 'chief').pane = null
  return data
}

test('a project whose chief has no window open says its specs reach the chief at its next launch, and the chief is told now where it has one', async ({
  page,
}) => {
  await open(page, chiefClosed())
  const specs = await openSpecs(page)
  await specs.mode.selectOption('on')
  await specs.save.click()
  await expect.poll(() => calls(page, 'project.specs')).toHaveLength(1)
  await expect(specs.note).toHaveText('Saved: the chief is told at its next launch.')
  await specs.mode.selectOption('off')
  await specs.save.click()
  await expect.poll(() => calls(page, 'project.specs')).toHaveLength(2)
  await expect(specs.note).toHaveText(
    'Specs are off in this project: the chief is told at its next launch.',
  )
  // Its window opens (Resume): the same save now says it is told now.
  await page.evaluate(() => {
    const chief = window.__model.boards[1].lanes.find((lane) => lane.participant.role === 'chief')
    chief.pane = { id: 'p1-chief', generation: 6 }
  })
  await specs.dialog.getByRole('button', { name: 'Close' }).click()
  const again = await openSpecs(page)
  await again.mode.selectOption('on')
  await again.save.click()
  await expect.poll(() => calls(page, 'project.specs')).toHaveLength(3)
  await expect(again.note).toHaveText('Saved: the chief is told now.')
})

test('the rules for the staff say the chief is told now where its window is open, and every window gets them when it next starts', async ({
  page,
}) => {
  await open(page, chiefClosed())
  const section = await rulesGroup(page)
  await section.rules.fill(STAFF_RULES)
  await section.save.click()
  await expect.poll(() => calls(page, 'project.rules')).toHaveLength(1)
  await expect(page.locator('#status')).toHaveText(
    'The rules for the staff are saved: each window of the staff is given them when it next starts.',
  )
  await section.rules.fill('')
  await section.save.click()
  await expect.poll(() => calls(page, 'project.rules')).toHaveLength(2)
  await expect(page.locator('#status')).toHaveText(
    'The project has no rules for the staff now: windows that start from now on are given none.',
  )
})

test('a project follows the app’s setting for specs until it says on or off, and the dialog says when a change takes hold', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecs(page)
  await expect(specs.group).toContainText(
    'A change reaches the chief as soon as it is at rest if its window is open, else when it next starts',
  )
  // It says nothing: it follows what the human set for every project (Settings, Specs).
  await expect(specs.mode).toHaveValue('follow')
  await expect(specs.mode.locator('option:checked')).toHaveText(/^Follow the app's setting/)
  await expect(specs.folder).toHaveValue('')
  await expect(specs.folder).toHaveAttribute('placeholder', '.specs')
  await expect(specs.variant).toHaveValue('')
  // The variants are the ones Specmint has, as the daemon said them, after the choice of none.
  await expect(specs.variant.locator('option')).toHaveText([
    'As the app says (core)',
    'core',
    'core-html',
    'tdd',
    'tdd-html',
  ])
  await expect(specs.note).toBeHidden()

  // On, with its own folder and the variant it plans with.
  await specs.mode.selectOption('on')
  await specs.folder.fill('  docs/specs ')
  await specs.variant.selectOption('tdd')
  await specs.save.click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: true, folder: 'docs/specs', variant: 'tdd' }])
  // The chief of this board has a window open: it is told now, at its rest.
  await expect(specs.note).toHaveText('Saved: the chief is told now.')
  await expect(page.locator('#status')).toHaveText('Saved: the chief is told now.')
  // The board is drawn again and the dialog shows what the daemon keeps.
  await expect(specs.mode).toHaveValue('on')
  await expect(specs.folder).toHaveValue('docs/specs')
  await expect(specs.variant).toHaveValue('tdd')

  // Off needs no folder and no variant.
  await specs.mode.selectOption('off')
  await expect(specs.folder).toBeDisabled()
  await expect(specs.variant).toBeDisabled()
  await specs.save.click()
  await expect.poll(async () => (await calls(page, 'project.specs')).length).toBe(2)
  expect((await calls(page, 'project.specs'))[1]).toMatchObject({ project: 1, on: false })
  await expect(specs.note).toHaveText('Specs are off in this project: the chief is told now.')
  await expect(specs.mode).toHaveValue('off')

  // Back to following the app's setting: it says none of the three.
  await specs.mode.selectOption('follow')
  await expect(specs.folder).toBeEnabled()
  await specs.folder.fill('')
  await specs.save.click()
  await expect.poll(async () => (await calls(page, 'project.specs')).length).toBe(3)
  expect((await calls(page, 'project.specs'))[2]).toEqual({
    project: 1,
    on: null,
    folder: null,
    variant: 'tdd',
  })
})

test('a folder the daemon refuses is said in the dialog, where it was typed, and what was typed stays', async ({
  page,
}) => {
  const data = model()
  data.specsRefusal =
    'the specs folder is a folder inside the project: it stays inside it, with no `..`'
  await open(page, data)
  const specs = await openSpecs(page)
  await specs.mode.selectOption('on')
  await specs.folder.fill('../elsewhere')
  await specs.save.click()
  await expect(specs.note).toHaveText(data.specsRefusal)
  await expect(specs.folder).toHaveValue('../elsewhere')
  await expect(specs.mode).toHaveValue('on')
  // The refusal is no note on the status line, and the next save clears it.
  await expect(page.locator('#status')).not.toHaveText(/Saved/)
})

/** The app says specs are on in `docs/specs` with core-html, and tdd is a version ahead of the rest. */
function appThatSays(data = model()) {
  data.specsApp = { on: true, folder: 'docs/specs', variant: 'core-html' }
  data.specmint.variants.find((variant) => variant.name === 'tdd').version = '3.1.0'
  return data
}

test('a project’s Specs section says what following the app means: whether it is on, the folder and the variant, with the Specmint version beside', async ({
  page,
}) => {
  await open(page, appThatSays())
  const specs = await openSpecs(page)
  const version = specs.group.locator('.specs-version')
  // Following names what the app says now, each part where it is said.
  await expect(specs.mode.locator('option[value="follow"]')).toHaveText(
    "Follow the app's setting (on)",
  )
  await expect(specs.folder).toHaveAttribute('placeholder', 'docs/specs')
  await expect(specs.variant.locator('option').first()).toHaveText('As the app says (core-html)')
  await expect(specs.variant).toHaveValue('')
  await expect(version).toHaveText('Specmint 3.0.0')
  // A variant of its own has the version of that variant beside it, and the first choice goes back.
  await specs.variant.selectOption('tdd')
  await expect(version).toHaveText('Specmint 3.1.0')
  await specs.variant.selectOption('')
  await expect(version).toHaveText('Specmint 3.0.0')
  // Following says none of the three, and says it with the variant left to the app.
  await specs.save.click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: null, folder: null, variant: null }])
  // The app turned off, and a project that follows is told so.
  await specs.dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.specsApp = { on: false }
  })
  await openSpecs(page)
  await expect(specs.mode.locator('option[value="follow"]')).toHaveText(
    "Follow the app's setting (off)",
  )
  await expect(specs.folder).toHaveAttribute('placeholder', '.specs')
  await expect(specs.variant.locator('option').first()).toHaveText('As the app says (core)')
})

test('a project that said a variant keeps it whatever the app says, and one Specmint no longer has is still the project’s', async ({
  page,
}) => {
  const data = appThatSays()
  data.boards[1].project.specs = { on: true, variant: 'tdd' }
  await open(page, data)
  const specs = await openSpecs(page)
  await expect(specs.variant).toHaveValue('tdd')
  await expect(specs.group.locator('.specs-version')).toHaveText('Specmint 3.1.0')
  await specs.save.click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: true, folder: null, variant: 'tdd' }])
  // A variant the bundled Specmint does not have (an older build's) is not lost on the next save.
  await specs.dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.boards[1].project.specs = { on: true, variant: 'tdd-lite' }
  })
  await page.evaluate(() => window.__listeners.get('state-changed')())
  const again = await openSpecs(page)
  await expect(again.variant).toHaveValue('tdd-lite')
  await again.save.click()
  await expect
    .poll(async () => (await calls(page, 'project.specs')).at(-1))
    .toEqual({ project: 1, on: true, folder: null, variant: 'tdd-lite' })
})

test('a project’s variant that changes while its settings are open has the version of the one it is now beside it', async ({
  page,
}) => {
  await open(page, appThatSays())
  const specs = await openSpecs(page)
  const version = specs.group.locator('.specs-version')
  await expect(specs.variant).toHaveValue('')
  await expect(version).toHaveText('Specmint 3.0.0')
  // The project says another variant while the dialog is open: its board is read again.
  const reads = async () => (await calls(page, 'board.get')).length
  const before = await reads()
  await page.evaluate(() => {
    window.__model.boards[1].project.specs = { variant: 'tdd' }
  })
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(reads).toBeGreaterThan(before)
  await expect(specs.variant).toHaveValue('tdd')
  await expect(version).toHaveText('Specmint 3.1.0')
})

test('New project says what following the app means as Project settings does, and starts nothing of it unless the human sets it', async ({
  page,
}) => {
  await open(page, appThatSays())
  const made = await newProject(page)
  await expect(made.specs.mode.locator('option[value="follow"]')).toHaveText(
    "Follow the app's setting (on)",
  )
  await expect(made.specs.folder).toHaveAttribute('placeholder', 'docs/specs')
  await expect(made.specs.variant.locator('option').first()).toHaveText(
    'As the app says (core-html)',
  )
  await expect(made.specs.version).toHaveText('Specmint 3.0.0')
  await made.specs.variant.selectOption('tdd')
  await expect(made.specs.version).toHaveText('Specmint 3.1.0')
  // Chosen and taken back, the section is as it opened and the project starts with no word of specs.
  await made.specs.variant.selectOption('')
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        compaction: 300000,
        staff: [],
      },
    ])
})

test('New project opened again shows the version of the variant it opens with, not of the one chosen before it was cancelled', async ({
  page,
}) => {
  // The app plans with core-html, at 3.0.0; tdd is at 3.1.0.
  await open(page, appThatSays())
  const made = await newProject(page)
  await made.specs.variant.selectOption('tdd')
  await expect(made.specs.version).toHaveText('Specmint 3.1.0')
  await made.dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(made.dialog).toBeHidden()
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(made.dialog).toBeVisible()
  await expect(made.specs.variant).toHaveValue('')
  await expect(made.specs.version).toHaveText('Specmint 3.0.0')
})

test('a board drawn again while the dialog is open leaves what the human is typing, and a project that says otherwise sets it', async ({
  page,
}) => {
  const data = model()
  // A closed project has no staff to settle; this one is open, and says its own word.
  data.projects[1].state = 'open'
  data.boards[2].project.state = 'open'
  data.boards[2].project.specs = { on: false, folder: 'plans', variant: 'core-html' }
  await open(page, data)
  const specs = await openSpecs(page)
  await specs.folder.fill('half-typed')
  // The page reads the board again as it is told something changed.
  const reads = async () => (await calls(page, 'board.get')).length
  const before = await reads()
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(reads).toBeGreaterThan(before)
  await expect(specs.folder).toHaveValue('half-typed')
  // Another project is another's: each shows its own.
  await specs.dialog.getByRole('button', { name: 'Close' }).click()
  await chooseProject(page, 'foundry')
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(specs.mode).toHaveValue('off')
  await expect(specs.folder).toHaveValue('plans')
  await expect(specs.folder).toBeDisabled()
  await expect(specs.variant).toHaveValue('core-html')
})

/** Project settings open on project 1, and its section of the rules for the staff. */
async function rulesGroup(page) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const group = dialog.getByRole('group', { name: 'Rules for the staff' })
  return {
    dialog,
    group,
    rules: group.getByLabel('The rules'),
    count: group.locator('.staff-rules-count'),
    hint: group.locator('.staff-rules-hint'),
    save: group.getByRole('button', { name: 'Save rules' }),
  }
}

const STAFF_RULES = '- Push to origin only, never to upstream.\n- Tests use the .smoke bundle id.'

test('Project settings shows the rules for the staff the board has, counted against the bounds, and says they hold no secrets', async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.staffRules = STAFF_RULES
  await open(page, data)
  const section = await rulesGroup(page)
  await expect(section.rules).toHaveValue(STAFF_RULES)
  await expect(section.count).toHaveText('75 of 8,000 bytes · 2 of 150 lines')
  await expect(section.count).not.toHaveClass(/over/)
  await expect(section.save).toBeEnabled()
  await expect(section.group).toContainText('No secrets: no passwords, tokens or keys')
  await expect(section.group).toContainText(
    'Every member of the staff is given these rules after its role',
  )
  await expect(section.hint).toBeHidden()
  // Opening sends nothing.
  expect(await calls(page, 'project.rules')).toEqual([])
})

test('a project with no rules for its staff shows an empty field and a count of nothing', async ({
  page,
}) => {
  await open(page)
  const section = await rulesGroup(page)
  await expect(section.rules).toHaveValue('')
  await expect(section.count).toHaveText('0 of 8,000 bytes · 0 of 150 lines')
  await expect(section.save).toBeEnabled()
})

test('saves the rules for the staff as written, by their own Save, shows what the daemon kept, and shows them when the dialog opens again', async ({
  page,
}) => {
  await open(page)
  const section = await rulesGroup(page)
  await section.rules.fill(`  ${STAFF_RULES}\n\n`)
  // Counted as the ledger keeps them: trimmed.
  await expect(section.count).toHaveText('75 of 8,000 bytes · 2 of 150 lines')
  expect(await calls(page, 'project.rules')).toEqual([])
  await section.save.click()
  await expect
    .poll(() => calls(page, 'project.rules'))
    .toEqual([{ project: 1, rules: `  ${STAFF_RULES}\n\n` }])
  await expect(section.rules).toHaveValue(STAFF_RULES)
  await expect(page.locator('#status')).toHaveText(
    'The rules for the staff are saved: the chief is told now, and each window of the staff is given them when it next starts.',
  )
  // No other section saved with them.
  for (const operation of ['wakeup.set', 'project.specs', 'project.gate']) {
    expect(await calls(page, operation)).toEqual([])
  }
  await section.dialog.getByRole('button', { name: 'Close' }).click()
  const again = await rulesGroup(page)
  await expect(again.rules).toHaveValue(STAFF_RULES)
  // Taken away: an empty field saved.
  await again.rules.fill('')
  await again.save.click()
  await expect.poll(() => calls(page, 'project.rules')).toHaveLength(2)
  expect((await calls(page, 'project.rules'))[1]).toEqual({ project: 1, rules: '' })
  await expect(page.locator('#status')).toHaveText(
    'The project has no rules for the staff now: the chief is told now, and windows that start from now on are given none.',
  )
})

test('counts the rules for the staff in bytes and lines as the ledger does, and holds Save back while they are over a bound', async ({
  page,
}) => {
  await open(page)
  const section = await rulesGroup(page)
  for (const [text, count, over] of [
    ['a'.repeat(8000), '8,000 of 8,000 bytes · 1 of 150 lines', false],
    ['a'.repeat(8001), '8,001 of 8,000 bytes · 1 of 150 lines', true],
    // Three bytes each: fewer characters than the bound, more bytes.
    ['日'.repeat(2667), '8,001 of 8,000 bytes · 1 of 150 lines', true],
    [Array(150).fill('a rule').join('\n'), '1,049 of 8,000 bytes · 150 of 150 lines', false],
    [Array(151).fill('a rule').join('\n'), '1,056 of 8,000 bytes · 151 of 150 lines', true],
    // Spaces and line breaks at either end are not kept, and are not counted.
    [`\n\n   ${'a'.repeat(8000)}   \n`, '8,000 of 8,000 bytes · 1 of 150 lines', false],
  ]) {
    await section.rules.fill(text)
    await expect(section.count).toHaveText(count)
    if (over) {
      await expect(section.count).toHaveClass(/over/)
      await expect(section.rules).toHaveAttribute('aria-invalid', 'true')
      await expect(section.save).toBeDisabled()
    } else {
      await expect(section.count).not.toHaveClass(/over/)
      await expect(section.save).toBeEnabled()
    }
  }
  expect(await calls(page, 'project.rules')).toEqual([])
})

test("says a refusal of the rules for the staff in their section, in the daemon's words, and keeps what was written", async ({
  page,
}) => {
  await open(page)
  const section = await rulesGroup(page)
  await section.rules.fill('a\u0007b')
  await section.save.click()
  await expect(section.hint).toHaveText(
    'the rules for the staff are plain text: they have no control characters but line breaks and tabs',
  )
  await expect(section.hint).toBeVisible()
  await expect(section.rules).toHaveValue('a\u0007b')
})

/** Project settings open on project 1, and its section of the members' compaction. */
async function compactionGroup(page) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const group = dialog.getByRole('group', { name: 'Compaction', exact: true })
  return {
    dialog,
    group,
    own: group.getByRole('radio', { name: "The harness's own point" }),
    size: group.getByRole('radio', { name: 'A size' }),
    amount: group.getByLabel('Size in tokens, 100,000 to 1,000,000'),
    hint: group.locator('.members-compaction-hint'),
    save: group.getByRole('button', { name: 'Save compaction' }),
  }
}

test("Project settings offers each harness's own point for the members' compaction until the human chooses a size, and says in one line what it does", async ({
  page,
}) => {
  await open(page)
  const section = await compactionGroup(page)
  await expect(section.own).toBeChecked()
  await expect(section.size).not.toBeChecked()
  await expect(section.amount).toBeDisabled()
  await expect(section.amount).toHaveValue('')
  await expect(section.save).toBeEnabled()
  await expect(section.hint).toBeHidden()
  // One line, in the section, true of every harness: what it does, what a size above a
  // model's own window does, and who it leaves alone.
  await expect(section.group.locator('.dialog-help')).toHaveText([
    "From its next start, each member's harness compacts its conversation at about this size, by its own rule; a size above a model's own window leaves the harness's own point. The chief is not affected.",
  ])
  // Nothing of the old Claude-only section is left.
  await expect(section.group).not.toContainText('Claude')
  // Opening sends nothing.
  expect(await calls(page, 'project.compaction')).toEqual([])
})

test("Project settings shows the size the board has for the members' compaction", async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.compaction = 400000
  await open(page, data)
  const section = await compactionGroup(page)
  await expect(section.size).toBeChecked()
  await expect(section.own).not.toBeChecked()
  await expect(section.amount).toBeEnabled()
  await expect(section.amount).toHaveValue('400000')
  await expect(section.save).toBeEnabled()
  expect(await calls(page, 'project.compaction')).toEqual([])
})

test("saves the choice of a size by its own Save, shows what the daemon kept, and saves the harness's own point again as none", async ({
  page,
}) => {
  await open(page)
  const section = await compactionGroup(page)
  // A first size starts from the one a new project starts on: 300,000.
  await section.size.check()
  await expect(section.amount).toBeEnabled()
  await expect(section.amount).toHaveValue('300000')
  await section.amount.fill('250000')
  expect(await calls(page, 'project.compaction')).toEqual([])
  await section.save.click()
  await expect
    .poll(() => calls(page, 'project.compaction'))
    .toEqual([{ project: 1, compaction: 250000 }])
  await expect(section.amount).toHaveValue('250000')
  await expect(page.locator('#status')).toHaveText(
    "The project's members compact at about 250,000 tokens: each window is given it when it next starts.",
  )
  // No other section saved with it.
  for (const operation of ['project.rules', 'wakeup.set', 'project.specs', 'project.gate']) {
    expect(await calls(page, operation)).toEqual([])
  }
  await section.dialog.getByRole('button', { name: 'Close' }).click()
  const again = await compactionGroup(page)
  await expect(again.size).toBeChecked()
  await expect(again.amount).toHaveValue('250000')
  // The harness's own point again: none is saved, and the size is not kept in the field.
  await again.own.check()
  await expect(again.amount).toBeDisabled()
  await again.save.click()
  await expect.poll(() => calls(page, 'project.compaction')).toHaveLength(2)
  expect((await calls(page, 'project.compaction'))[1]).toEqual({ project: 1, compaction: null })
  await expect(again.own).toBeChecked()
  await expect(again.amount).toHaveValue('')
  await expect(page.locator('#status')).toHaveText(
    "The project's members use their harness's own point again: windows that start from now on are given no size.",
  )
})

test('holds the size to the range the ledger takes, and Save back while it is not a whole number in it', async ({
  page,
}) => {
  await open(page)
  const section = await compactionGroup(page)
  await section.size.check()
  for (const [text, fine] of [
    ['100000', true],
    ['1000000', true],
    ['400000', true],
    ['  400000 ', true],
    ['99999', false],
    ['1000001', false],
    ['0', false],
    ['', false],
    ['400k', false],
    ['4e5', false],
    ['-400000', false],
    ['400000.5', false],
  ]) {
    await section.amount.fill(text)
    if (fine) {
      await expect(section.save, text).toBeEnabled()
      await expect(section.hint, text).toBeHidden()
      await expect(section.amount, text).toHaveAttribute('aria-invalid', 'false')
    } else {
      await expect(section.save, text).toBeDisabled()
      await expect(section.hint, text).toHaveText(
        'A size is a whole number from 100,000 to 1,000,000.',
      )
      await expect(section.amount, text).toHaveAttribute('aria-invalid', 'true')
    }
  }
  // The harness's own point is saved whatever the field holds.
  await section.amount.fill('99999')
  await section.own.check()
  await expect(section.save).toBeEnabled()
  await expect(section.hint).toBeHidden()
  expect(await calls(page, 'project.compaction')).toEqual([])
})

test("says a refusal of the members' compaction in its section and keeps what was chosen", async ({
  page,
}) => {
  await open(page)
  const section = await compactionGroup(page)
  await section.size.check()
  await section.amount.fill('300000')
  await page.evaluate(() => {
    window.__down = 'the daemon stopped'
  })
  await section.save.click()
  await expect(section.hint).toBeVisible()
  await expect(section.hint).toContainText('the daemon stopped')
  await expect(section.amount).toHaveValue('300000')
  await expect(section.size).toBeChecked()
})

/** Project settings open on project 1, and its Advisor tool section. */
async function advisorToolGroup(page) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const group = dialog.getByRole('group', { name: 'Advisor tool', exact: true })
  return {
    dialog,
    group,
    tool: group.getByRole('checkbox'),
    hint: group.locator('.members-advisor-tool-hint'),
    save: group.getByRole('button', { name: 'Save advisor tool' }),
  }
}

test("Project settings has the advisor tool off for the project's members until the human ticks it, and says in one line whom it is for", async ({
  page,
}) => {
  await open(page)
  const section = await advisorToolGroup(page)
  await expect(section.tool).not.toBeChecked()
  await expect(section.save).toBeEnabled()
  await expect(section.hint).toBeHidden()
  // One line, in the section, true as written: Claude Code's own tool, its members, the chief
  // left alone, and the other harnesses having none.
  await expect(section.group.locator('.dialog-help')).toHaveText([
    "Claude Code's own advisor tool lets a window consult a second model. Off, each Claude Code member runs without it from its next start; on, it has the tool as your own Claude Code setting gives it. The chief is not affected and keeps its harness's own setting. Codex, Pi, Devin and Muse Code have no advisor tool, so this changes nothing for their members. A window already open keeps what it began with.",
  ])
  // Opening sends nothing.
  expect(await calls(page, 'project.advisor_tool')).toEqual([])
})

test('Project settings shows the advisor tool as the board has it for the project', async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.advisorTool = true
  await open(page, data)
  const section = await advisorToolGroup(page)
  await expect(section.tool).toBeChecked()
  expect(await calls(page, 'project.advisor_tool')).toEqual([])
})

test('saves the advisor tool by its own Save, shows what the daemon kept, and saves it off again', async ({
  page,
}) => {
  await open(page)
  const section = await advisorToolGroup(page)
  await section.tool.check()
  // Ticking sends nothing: the section has its own Save.
  expect(await calls(page, 'project.advisor_tool')).toEqual([])
  await section.save.click()
  await expect
    .poll(() => calls(page, 'project.advisor_tool'))
    .toEqual([{ project: 1, advisorTool: true }])
  await expect(section.tool).toBeChecked()
  await expect(page.locator('#status')).toHaveText(
    "The project's Claude Code members have the advisor tool as your own Claude Code setting gives it: each window is given that when it next starts.",
  )
  // No other section saved with it.
  for (const operation of [
    'project.rules',
    'project.compaction',
    'wakeup.set',
    'project.specs',
    'project.gate',
  ]) {
    expect(await calls(page, operation)).toEqual([])
  }
  await section.dialog.getByRole('button', { name: 'Close' }).click()
  const again = await advisorToolGroup(page)
  await expect(again.tool).toBeChecked()
  await again.tool.uncheck()
  await again.save.click()
  await expect.poll(() => calls(page, 'project.advisor_tool')).toHaveLength(2)
  expect((await calls(page, 'project.advisor_tool'))[1]).toEqual({
    project: 1,
    advisorTool: false,
  })
  await expect(again.tool).not.toBeChecked()
  await expect(page.locator('#status')).toHaveText(
    "The project's Claude Code members run without the advisor tool: windows that start from now on are launched without it.",
  )
})

test('says a refusal of the advisor tool in its section and keeps what was chosen', async ({
  page,
}) => {
  await open(page)
  const section = await advisorToolGroup(page)
  await section.tool.check()
  await page.evaluate(() => {
    window.__down = 'the daemon stopped'
  })
  await section.save.click()
  await expect(section.hint).toBeVisible()
  await expect(section.hint).toContainText('the daemon stopped')
  await expect(section.tool).toBeChecked()
})

test('New project has the advisor tool off, and sends it only when the human ticks it, to start with the project', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await expect(made.advisorTool.tool).not.toBeChecked()
  // Its own Save is for a saved project: New project does not show it.
  await expect(made.advisorTool.save).toBeHidden()
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        staff: [],
      },
    ])
  await expect(made.dialog).toBeHidden()
  expect(await calls(page, 'project.advisor_tool')).toEqual([])
})

test('New project with the advisor tool ticked starts the project with it, in the one write that makes it', async ({
  page,
}) => {
  await open(page)
  const made = await newProject(page)
  await pickChief(made.dialog, 'Claude Code', 'claude-sonnet-5')
  await made.advisorTool.tool.check()
  await made.start.click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        gate: false,
        staff: [],
        advisorTool: true,
      },
    ])
  await expect(made.dialog).toBeHidden()
  expect(await calls(page, 'project.advisor_tool')).toEqual([])
})

const DEFAULT_WAKE_UP_WORDS =
  'Do you still have work to do? If tasks are left on the board or in your plan, carry on.'

/** Project settings open on project 1, and its section of wake-up calls. */
async function wakeUpGroup(page) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const group = dialog.getByRole('group', { name: 'Wake-up calls' })
  return {
    dialog,
    group,
    on: group.getByRole('checkbox'),
    // A label that wraps its control is named with the control's value as well.
    every: group.getByLabel('Every'),
    unit: group.getByLabel('Unit'),
    words: group.getByLabel('What it says'),
    save: group.getByRole('button', { name: 'Save wake-up calls' }),
  }
}

test('shows the wake-up calls of the project in Project settings: off, an hour apart, and the default words', async ({
  page,
}) => {
  await open(page)
  const calling = await wakeUpGroup(page)
  await expect(calling.on).not.toBeChecked()
  await expect(calling.every).toHaveValue('1')
  await expect(calling.unit).toHaveValue('hours')
  await expect(calling.words).toHaveValue(DEFAULT_WAKE_UP_WORDS)
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 1 }])
  expect(await calls(page, 'wakeup.set')).toEqual([])
})

test('saves the wake-up calls as a number, a unit and the words, shows them as the daemon says them, and shows what was saved when the dialog opens again', async ({
  page,
}) => {
  await open(page)
  const calling = await wakeUpGroup(page)
  await calling.on.check()
  await calling.every.fill('120')
  await calling.unit.selectOption('minutes')
  await calling.words.fill('Anything left? Carry on, or say what is.')
  await calling.save.click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([
      {
        project: 1,
        on: true,
        every: 120,
        unit: 'minutes',
        text: 'Anything left? Carry on, or say what is.',
      },
    ])
  // The daemon says the interval in the largest unit that holds it: 120 minutes are 2 hours.
  await expect(calling.every).toHaveValue('2')
  await expect(calling.unit).toHaveValue('hours')
  await expect(page.locator('#status')).toHaveText(
    'Wake-up calls are on: the chief is asked every 2 hours.',
  )
  // Closed and opened again, it shows what was saved.
  await calling.dialog.getByRole('button', { name: 'Close' }).click()
  await expect(calling.dialog).toBeHidden()
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(calling.on).toBeChecked()
  await expect(calling.every).toHaveValue('2')
  await expect(calling.unit).toHaveValue('hours')
  await expect(calling.words).toHaveValue('Anything left? Carry on, or say what is.')
  // And what the daemon has when it opens, not what the page last drew: the
  // setting was changed elsewhere meanwhile.
  await calling.dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.wakeUps[1] = { on: false, everyMs: 5_400_000, text: null }
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(calling.on).not.toBeChecked()
  await expect(calling.every).toHaveValue('90')
  await expect(calling.unit).toHaveValue('minutes')
  await expect(calling.words).toHaveValue(DEFAULT_WAKE_UP_WORDS)
  expect(await calls(page, 'wakeup.get')).toHaveLength(3)
})

test('converts the wake-up interval the human picks in minutes, hours and days as the daemon counts it, and says it in the largest unit that holds it whole', async ({
  page,
}) => {
  await open(page)
  const calling = await wakeUpGroup(page)
  await calling.on.check()
  for (const [every, unit, shown, saidUnit, said] of [
    ['90', 'minutes', '90', 'minutes', 'every 90 minutes'],
    ['60', 'minutes', '1', 'hours', 'every 1 hour'],
    ['24', 'hours', '1', 'days', 'every 1 day'],
    ['36', 'hours', '36', 'hours', 'every 36 hours'],
    ['3', 'days', '3', 'days', 'every 3 days'],
    ['1', 'minutes', '1', 'minutes', 'every 1 minute'],
  ]) {
    await calling.every.fill(every)
    await calling.unit.selectOption(unit)
    await calling.save.click()
    await expect(calling.every).toHaveValue(shown)
    await expect(calling.unit).toHaveValue(saidUnit)
    await expect(page.locator('#status')).toHaveText(
      `Wake-up calls are on: the chief is asked ${said}.`,
    )
  }
  expect((await calls(page, 'wakeup.set')).map(({ every, unit }) => [every, unit])).toEqual([
    [90, 'minutes'],
    [60, 'minutes'],
    [24, 'hours'],
    [36, 'hours'],
    [3, 'days'],
    [1, 'minutes'],
  ])
})

test('turns the wake-up calls off with the same Save, and sends no save for a number that is no whole number of at least one', async ({
  page,
}) => {
  const data = model()
  data.wakeUps = { 1: { on: true, everyMs: 7_200_000, text: 'Still there?' } }
  await open(page, data)
  const calling = await wakeUpGroup(page)
  await expect(calling.on).toBeChecked()
  await expect(calling.words).toHaveValue('Still there?')
  // The browser holds back a number under one or none: nothing is sent.
  for (const bad of ['0', '-3', '1.5', '']) {
    await calling.every.fill(bad)
    await calling.save.click()
    await expect(calling.every).toHaveJSProperty('validity.valid', false)
  }
  expect(await calls(page, 'wakeup.set')).toEqual([])
  await calling.every.fill('2')
  await calling.on.uncheck()
  await calling.save.click()
  await expect(page.locator('#status')).toHaveText('Wake-up calls are off.')
  expect(await calls(page, 'wakeup.set')).toEqual([
    { project: 1, on: false, every: 2, unit: 'hours', text: 'Still there?' },
  ])
})

test('says the daemon refusal of a wake-up in the dialog and keeps what the human typed', async ({
  page,
}) => {
  await open(page)
  const calling = await wakeUpGroup(page)
  await calling.on.check()
  await calling.words.fill('   ')
  await calling.save.click()
  const hint = calling.group.locator('.wakeup-hint')
  await expect(hint).toHaveText(
    'the wake-up text must be text, not empty, at most 1000000 characters',
  )
  await expect(calling.words).toHaveValue('   ')
  await expect(calling.on).toBeChecked()
  // Written again, it saves and the refusal goes.
  await calling.words.fill('Go on?')
  await calling.save.click()
  await expect(hint).toBeHidden()
  await expect(calling.words).toHaveValue('Go on?')
})

test('saves the wake-up calls on Enter in the number and adds nobody to the staff', async ({
  page,
}) => {
  await open(page)
  const calling = await wakeUpGroup(page)
  await calling.on.check()
  await calling.every.fill('5')
  await calling.unit.selectOption('minutes')
  await calling.every.press('Enter')
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([{ project: 1, on: true, every: 5, unit: 'minutes', text: DEFAULT_WAKE_UP_WORDS }])
  expect(await calls(page, 'member.add')).toEqual([])
  expect(await calls(page, 'member.roles')).toEqual([])
})

test('lists what waits for approval in For you, and approves or declines it, writing to no agent', async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.gate = true
  const gated = (id, kind, sender, recipient, taskNumber, body, extra = {}) => ({
    id,
    kind,
    state: 'gated',
    sender,
    recipient,
    taskNumber,
    body,
    questions: null,
    choices: null,
    createdAt: at(3),
    ...extra,
  })
  data.boards[1].gated = [
    gated(30, 'task', 'chief', 'zeus-amber-pine', 4, 'Add the tests\nCover the parser.'),
    gated(31, 'result', 'zeus-amber-pine', 'chief', 2, 'Parser done, 14 tests.'),
    gated(32, 'question', 'zeus-amber-pine', 'chief', 4, 'Which parser?'),
    gated(33, 'answer', 'chief', 'zeus-amber-pine', 4, 'The recursive one.', { replyTo: 32 }),
  ]
  await open(page, data)
  const bay = page.getByRole('region', { name: 'For you' })
  await expect(bay.locator('.foryou-status')).toHaveText('4 waiting · 1 note')
  const brief = bay.locator('.strip-message[data-message="30"]')
  await expect(brief).toHaveAttribute('data-gated', 'true')
  await expect(brief.locator('.strip-route')).toHaveText(
    'Task from @chief to @zeus-amber-pine · T-4 · needs your approval',
  )
  await expect(brief.locator('.strip-title')).toHaveText('Add the tests\nCover the parser.')
  await brief.getByRole('button', { name: 'Approve m-30 for @zeus-amber-pine' }).click()
  await expect.poll(() => calls(page, 'message.approve')).toEqual([{ message: 30 }])
  await expect(page.locator('#status')).toHaveText('m-30 goes on to @zeus-amber-pine.')

  const answer = bay.locator('.strip-message[data-message="33"]')
  await expect(answer.locator('.strip-route')).toHaveText(
    'Answer from @chief to @zeus-amber-pine · T-4 · needs your approval',
  )
  await answer.getByRole('button', { name: 'Decline m-33' }).click()
  await expect.poll(() => calls(page, 'message.decline')).toEqual([{ message: 33 }])
  await expect(page.locator('#status')).toHaveText('m-33 declined; @chief is told.')

  const result = bay.locator('.strip-message[data-message="31"]')
  await expect(result.locator('.strip-route')).toHaveText(
    'Result from @zeus-amber-pine to @chief · T-2 · needs your approval',
  )
  // A result and a question go on to the chief, who decides and answers in its terminal.
  await expect(result.getByRole('textbox')).toHaveCount(0)
  await expect(result.getByRole('button', { name: /^Decline/ })).toHaveCount(0)
  const question = bay.locator('.strip-message[data-message="32"]')
  await expect(question.getByRole('textbox')).toHaveCount(0)
  await question.getByRole('button', { name: 'Approve m-32 for @chief' }).click()
  await expect
    .poll(() => calls(page, 'message.approve'))
    .toEqual([{ message: 30 }, { message: 32 }])
})

/** The harbour board with an advisor on the staff, its window open. */
function withAdvisor() {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(5, 'athena', 'advisor', { harness: 'pi' }),
    tasks: [task(7, 'Research the market', 'working', 'chief', 'athena', 4)],
    activity: { state: 'working' },
    pane: { id: 'p1-athena', generation: 4 },
  })
  return data
}

test("lists every member under the chief, with no staff groups, on the board and in the dock's strip of windows", async ({
  page,
}) => {
  await open(page, withAdvisor())
  await expect(page.locator('.board-group')).toHaveCount(0)
  expect(
    await page
      .locator('tbody tr[data-handle]')
      .evaluateAll((rows) => rows.map((row) => row.dataset.handle)),
  ).toEqual(['chief', 'zeus', 'diana', 'athena'])
  await expect(page.locator('tr[data-handle="athena"] .row-name')).toHaveText('@athena')
  await expect(page.locator('tr[data-handle="athena"] .row-meta')).toContainText('advisor')

  // The dock on the right is a strip of terminals, the chief first, then the
  // members' own (a session's waits to be shown); a member's heading row
  // offers no Show terminal.
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const cards = () =>
    dock.locator('.terminal-card').evaluateAll((cards) => cards.map((c) => c.dataset.handle))
  await expect.poll(cards).toEqual(['chief', 'zeus', 'athena'])
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'chief',
  )
  await expect(page.getByRole('button', { name: "Show @athena's terminal" })).toHaveCount(0)
  expect(await page.evaluate(() => window.__emulators.length)).toBe(3)
  expect(await page.locator('tbody tr[data-handle]').count()).toBe(
    4,
    'the board stays beside the dock',
  )
})

/**
 * The daemon's screen behind Settings (Harnesses), served as the daemon serves it, for a page
 * that is handed its address; `env` is the daemon's.
 */
async function daemonScreens() {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  return {
    env: t.env,
    runs: server.runs,
    screens: { url: server.url, token: server.token },
    close: async () => {
      await server.close()
      t.cleanup()
    },
  }
}

/** Settings, then one of its screens: the dialog it opens in. */
async function openScreen(page, name) {
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page
    .getByRole('dialog', { name: 'Settings', exact: true })
    .getByRole('button', { name })
    .click()
  return page.getByRole('dialog', { name })
}

test('opens Harnesses from Settings in a dialog where a check runs against the daemon; Escape goes back to Settings', async ({
  page,
}) => {
  // The harnesses are stand-ins at 1.2.3 that count how often they are asked their version,
  // which a check does once; what the feeds say of a newer release is theirs to say.
  const daemon = await daemonScreens()
  try {
    await open(page, { ...model(), screens: daemon.screens })
    const harnesses = await openScreen(page, 'Harnesses')
    await expect(harnesses).toBeVisible()
    const screen = harnesses.frameLocator('iframe')
    await expect(screen.locator('.host')).toHaveCount(5)
    await expect(
      screen.locator('.host').filter({ has: screen.locator('strong', { hasText: /^pi$/ }) }),
    ).toContainText('Version 1.2.3')
    const before = daemon.runs('pi')
    await screen.getByRole('button', { name: 'Check all harnesses' }).click()
    await expect.poll(() => daemon.runs('pi')).toBeGreaterThan(before)
    await expect(screen.locator('#check-note')).toHaveText('')
    await harnesses.getByRole('button', { name: 'Close' }).focus()
    await page.keyboard.press('Escape')
    await expect(harnesses).toBeHidden()
    await expect(page.getByRole('button', { name: 'Settings', exact: true })).toBeFocused()
  } finally {
    await daemon.close()
  }
})

test('Escape pressed inside a screen closes its dialog, as it closes every other dialog', async ({
  page,
}) => {
  const daemon = await daemonScreens()
  try {
    await open(page, { ...model(), screens: daemon.screens })
    const harnesses = await openScreen(page, 'Harnesses')
    await harnesses
      .frameLocator('iframe')
      .getByRole('button', { name: 'Check all harnesses' })
      .focus()
    await page.keyboard.press('Escape')
    await expect(harnesses).toBeHidden()
    await expect(page.getByRole('button', { name: 'Settings', exact: true })).toBeFocused()
  } finally {
    await daemon.close()
  }
})

/** Settings, then Specs: the dialog it opens, and its controls. */
async function openSpecsSettings(page) {
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  const settings = page.getByRole('dialog', { name: 'Settings', exact: true })
  await settings.getByRole('button', { name: 'Specs' }).click()
  const dialog = page.getByRole('dialog', { name: 'Specs', exact: true })
  return {
    settings,
    dialog,
    on: dialog.getByRole('checkbox', { name: /Plan big work as specs/ }),
    folder: dialog.locator('[name="folder"]'),
    variant: dialog.locator('[name="variant"]'),
    refusal: dialog.locator('.dialog-refusal'),
    version: dialog.locator('.specmint-version'),
    rows: dialog.locator('.specmint tbody tr'),
    close: dialog.getByRole('button', { name: 'Close' }),
  }
}

test('Settings has Specs as a second entry, under Harnesses, and it opens a dialog of the page, Settings closed behind it', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  const settings = page.getByRole('dialog', { name: 'Settings', exact: true })
  await expect(settings.locator('.settings-menu li button')).toHaveText(['Harnesses', 'Specs'])
  await settings.getByRole('button', { name: 'Specs' }).click()
  const specs = page.getByRole('dialog', { name: 'Specs', exact: true })
  await expect(specs).toBeVisible()
  await expect(settings).toBeHidden()
  // Nothing of the daemon's screens: no frame, and no screen asked for.
  await expect(specs.locator('iframe')).toHaveCount(0)
  expect(await page.evaluate(() => window.__calls.filter(([c]) => c === 'agents_screen'))).toEqual(
    [],
  )
  // Close goes back to the Settings button, as the screens' dialogs do.
  await specs.getByRole('button', { name: 'Close' }).click()
  await expect(specs).toBeHidden()
  await expect(page.getByRole('button', { name: 'Settings', exact: true })).toBeFocused()
})

test('Settings, Specs: the box, the folder and the variant every project uses unless it names its own, each kept as it changes and shown again when it opens', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  // Off until the human turns it on; no folder named, so the documented one is the hint; core.
  await expect(specs.on).not.toBeChecked()
  await expect(specs.folder).toHaveValue('')
  await expect(specs.folder).toHaveAttribute('placeholder', '.specs')
  await expect(specs.variant.locator('option')).toHaveText(['core', 'core-html', 'tdd', 'tdd-html'])
  await expect(specs.variant).toHaveValue('core')
  await expect(specs.refusal).toBeHidden()
  expect(await calls(page, 'specs.set')).toEqual([])

  await specs.on.check()
  await expect.poll(() => calls(page, 'specs.set')).toEqual([{ on: true }])
  // The folder is kept as the human leaves it, or presses Enter, and once for either.
  await specs.folder.fill('  docs/specs ')
  await specs.folder.press('Enter')
  await expect
    .poll(() => calls(page, 'specs.set'))
    .toEqual([{ on: true }, { folder: '  docs/specs ' }])
  await expect(specs.folder).toHaveValue('docs/specs')
  await specs.folder.blur()
  await specs.variant.selectOption('tdd')
  await expect
    .poll(() => calls(page, 'specs.set'))
    .toEqual([{ on: true }, { folder: '  docs/specs ' }, { variant: 'tdd' }])
  await specs.folder.fill('plans')
  await specs.folder.blur()
  await expect.poll(async () => (await calls(page, 'specs.set')).length).toBe(4)
  expect((await calls(page, 'specs.set'))[3]).toEqual({ folder: 'plans' })
  await expect(specs.refusal).toBeHidden()

  // Closed and opened again, it is what the daemon keeps, read again as it opens.
  const reads = async () => (await calls(page, 'specs.get')).length
  const before = await reads()
  await specs.close.click()
  const again = await openSpecsSettings(page)
  await expect.poll(reads).toBe(before + 1)
  await expect(again.on).toBeChecked()
  await expect(again.folder).toHaveValue('plans')
  await expect(again.variant).toHaveValue('tdd')
  // Taken back: a blank folder and off are the app saying none of it again.
  await again.folder.fill('')
  await again.folder.press('Enter')
  await again.on.uncheck()
  await expect
    .poll(async () => (await calls(page, 'specs.set')).slice(4))
    .toEqual([{ folder: '' }, { on: false }])
  await expect(again.folder).toHaveAttribute('placeholder', '.specs')
})

test('Settings, Specs: a change is what every project’s Specs section says following the app means, with no reload', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  await specs.on.check()
  await specs.folder.fill('docs/specs')
  await specs.folder.press('Enter')
  await specs.variant.selectOption('tdd-html')
  await expect.poll(async () => (await calls(page, 'specs.set')).length).toBe(3)
  await specs.close.click()
  // Project settings: following says on, the folder and the variant.
  const project = await openSpecs(page)
  await expect(project.mode.locator('option[value="follow"]')).toHaveText(
    "Follow the app's setting (on)",
  )
  await expect(project.folder).toHaveAttribute('placeholder', 'docs/specs')
  await expect(project.variant.locator('option').first()).toHaveText('As the app says (tdd-html)')
  await project.dialog.getByRole('button', { name: 'Close' }).click()
  // So does New project.
  const made = await newProject(page)
  await expect(made.specs.folder).toHaveAttribute('placeholder', 'docs/specs')
  await expect(made.specs.variant.locator('option').first()).toHaveText(
    'As the app says (tdd-html)',
  )
})

test('Settings, Specs: the Specmint it brings is shown as the daemon read it, its version and each variant’s name, version, commit and what it is', async ({
  page,
}) => {
  const data = model()
  data.specmint = {
    version: '3.4.0, 3.5.0',
    variants: [
      { name: 'core', version: '3.4.0', commit: 'aaaaaaa', about: 'Markdown specs' },
      { name: 'core-html', version: '3.4.0', commit: 'bbbbbbb', about: 'HTML spec pages' },
      { name: 'tdd', version: '3.4.0', commit: 'ccccccc', about: 'Test-first Markdown specs' },
      { name: 'tdd-html', version: '3.5.0', commit: 'ddddddd', about: 'Both of them' },
    ],
  }
  await open(page, data)
  const specs = await openSpecsSettings(page)
  await expect(specs.version).toContainText('Specmint 3.4.0, 3.5.0')
  await expect(specs.rows).toHaveCount(4)
  await expect(specs.rows).toHaveText([
    /^core\s*3\.4\.0\s*aaaaaaa\s*Markdown specs$/,
    /^core-html\s*3\.4\.0\s*bbbbbbb\s*HTML spec pages$/,
    /^tdd\s*3\.4\.0\s*ccccccc\s*Test-first Markdown specs$/,
    /^tdd-html\s*3\.5\.0\s*ddddddd\s*Both of them$/,
  ])
  // The variants the choice offers are the ones listed, in the same order.
  await expect(specs.variant.locator('option')).toHaveText(['core', 'core-html', 'tdd', 'tdd-html'])
  // A Specmint vendored again shows itself: the page keeps nothing of the last one.
  await specs.close.click()
  await page.evaluate(() => {
    window.__model.specmint.version = '3.5.1'
    window.__model.specmint.variants[2].commit = '1234567'
  })
  const again = await openSpecsSettings(page)
  await expect(again.version).toContainText('Specmint 3.5.1')
  await expect(again.rows.nth(2)).toContainText('1234567')
})

test('a Specmint variant whose bundled files do not say its version or commit shows them unknown, in Settings, Project settings and New project', async ({
  page,
}) => {
  const data = model()
  const tdd = data.specmint.variants.find((variant) => variant.name === 'tdd')
  tdd.version = null
  tdd.commit = null
  await open(page, data)
  const specs = await openSpecsSettings(page)
  await expect(specs.version).toHaveText('Specmint 3.0.0, brought with this app.')
  await expect(specs.rows.nth(2)).toHaveText(
    /^tdd\s*unknown\s*unknown\s*Test-first Markdown specs$/,
  )
  await specs.close.click()
  // The project's dialogs open as ever, and say it beside the variant.
  const project = await openSpecs(page)
  await project.variant.selectOption('tdd')
  await expect(project.group.locator('.specs-version')).toHaveText('Specmint version unknown')
  await project.variant.selectOption('core')
  await expect(project.group.locator('.specs-version')).toHaveText('Specmint 3.0.0')
  await project.dialog.getByRole('button', { name: 'Close' }).click()
  const made = await newProject(page)
  await made.specs.variant.selectOption('tdd')
  await expect(made.specs.version).toHaveText('Specmint version unknown')
  await made.dialog.getByRole('button', { name: 'Cancel' }).click()
  // No variant says one: the headline says so too.
  await page.evaluate(() => {
    window.__model.specmint.version = null
    for (const variant of window.__model.specmint.variants) variant.version = null
  })
  const again = await openSpecsSettings(page)
  await expect(again.version).toHaveText('Specmint, its version unknown, brought with this app.')
})

test('Settings, Specs: a change the daemon refuses is said under the controls in its words, what was typed stays, and the next change clears it', async ({
  page,
}) => {
  const data = model()
  await open(page, data)
  const specs = await openSpecsSettings(page)
  const words = 'the specs folder is a folder inside the project: it stays inside it, with no `..`'
  await page.evaluate((words) => {
    window.__model.specsAppRefusal = words
  }, words)
  await specs.folder.fill('../elsewhere')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toHaveText(words)
  await expect(specs.folder).toHaveValue('../elsewhere')
  // A box the daemon refuses goes back to what is kept.
  await specs.on.click()
  await expect(specs.refusal).toHaveText(words)
  await expect(specs.on).not.toBeChecked()
  await expect(page.locator('#status')).toHaveText('')
  // Mended, the next change goes through and the refusal goes.
  await page.evaluate(() => {
    window.__model.specsAppRefusal = null
  })
  await specs.folder.fill('docs')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toBeHidden()
  await expect(specs.folder).toHaveValue('docs')
  // And a refusal does not wait in the dialog for the next time it opens.
  await page.evaluate((words) => {
    window.__model.specsAppRefusal = words
  }, words)
  await specs.on.click()
  await expect(specs.refusal).toHaveText(words)
  await specs.close.click()
  await page.evaluate(() => {
    window.__model.specsAppRefusal = null
  })
  const again = await openSpecsSettings(page)
  await expect(again.refusal).toBeHidden()
})

test('Settings, Specs: with the daemon down it opens nothing and the status line says why', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() => {
    window.__down = 'the daemon stopped'
  })
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page
    .getByRole('dialog', { name: 'Settings', exact: true })
    .getByRole('button', { name: 'Specs' })
    .click()
  await expect(page.locator('#status')).toHaveText('the daemon stopped')
  await expect(page.getByRole('dialog', { name: 'Specs', exact: true })).toBeHidden()
})

test('Settings, Specs: Enter in the folder asks the daemon once, however long its answer takes', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  // The real daemon answers over the bridge a while later: Enter is a change and a submit before it does.
  await page.evaluate(() => {
    window.__delay['specs.set'] = 300
  })
  await specs.folder.fill('docs/specs')
  await specs.folder.press('Enter')
  await expect.poll(() => answers(page, 'specs.set')).toBe(1)
  expect(await calls(page, 'specs.set')).toEqual([{ folder: 'docs/specs' }])
  await expect(specs.folder).toHaveValue('docs/specs')
  // Left for another control, it asks nothing more.
  await specs.variant.focus()
  await specs.folder.press('Enter')
  expect(await calls(page, 'specs.set')).toHaveLength(1)
})

test('Settings, Specs: what the human changes while a save is on its way is kept when that save’s answer comes', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  // The switch's save waits on the daemon, and the human types a folder meanwhile.
  await hold(page, 'specs.set')
  await specs.on.check()
  await expect.poll(() => calls(page, 'specs.set')).toEqual([{ on: true }])
  await specs.folder.fill('docs/specs')
  await letGo(page, 'specs.set')
  await expect.poll(() => answers(page, 'specs.set')).toBe(1)
  // The answer, which knows no folder, leaves the one typed, and draws the switch as kept.
  await expect(specs.folder).toHaveValue('docs/specs')
  await expect(specs.on).toBeChecked()
  // It goes in with a save of its own, whose answer draws it as kept.
  await specs.folder.press('Enter')
  await expect
    .poll(() => calls(page, 'specs.set'))
    .toEqual([{ on: true }, { folder: 'docs/specs' }])
  await expect.poll(() => answers(page, 'specs.set')).toBe(2)
  await expect(specs.folder).toHaveValue('docs/specs')
  // A folder typed over a kept one while the switch's save waits stays too.
  await hold(page, 'specs.set')
  await specs.on.uncheck()
  await expect.poll(() => calls(page, 'specs.set')).toHaveLength(3)
  await specs.folder.fill('plans')
  await letGo(page, 'specs.set')
  await expect.poll(() => answers(page, 'specs.set')).toBe(3)
  await expect(specs.folder).toHaveValue('plans')
  await expect(specs.on).not.toBeChecked()
})

/** The stand-in daemon refuses every save of the app's specs in `words`, or none for `null`. */
const refuseSpecs = (page, words) =>
  page.evaluate((words) => {
    window.__model.specsAppRefusal = words
  }, words)

test('Settings, Specs: one save at a time, each with the control as it is when it is asked, so what is kept and what is shown agree however the daemon answers', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  // The variant's save waits on the daemon; the human chooses twice more and turns specs on.
  await hold(page, 'specs.set')
  await specs.variant.selectOption('tdd')
  await expect.poll(() => calls(page, 'specs.set')).toEqual([{ variant: 'tdd' }])
  await specs.variant.selectOption('core-html')
  await specs.variant.selectOption('tdd-html')
  await specs.on.check()
  // Nothing more is asked while one is on its way: an answer to a later one could come first,
  // and the earlier one be kept after it.
  expect(await calls(page, 'specs.set')).toEqual([{ variant: 'tdd' }])
  await letGo(page, 'specs.set')
  await expect.poll(() => answers(page, 'specs.set')).toBe(3)
  // Then each control once, as it is by then.
  expect(await calls(page, 'specs.set')).toEqual([
    { variant: 'tdd' },
    { variant: 'tdd-html' },
    { on: true },
  ])
  // What is kept is the human's last word, and the dialog shows it.
  expect(await page.evaluate(() => window.__model.specsApp)).toEqual({
    on: true,
    variant: 'tdd-html',
  })
  await expect(specs.variant).toHaveValue('tdd-html')
  await expect(specs.on).toBeChecked()
  // The page's copy is the newest answer: a refusal now draws the controls from it as kept.
  await refuseSpecs(page, 'the specs folder is a folder inside the project: it is text')
  await specs.folder.fill('../x')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toBeVisible()
  await expect(specs.on).toBeChecked()
  await expect(specs.variant).toHaveValue('tdd-html')
  await refuseSpecs(page, null)
  // Opened again while a save is on its way, it reads what is kept once that one is answered.
  await hold(page, 'specs.set')
  await specs.on.uncheck()
  await expect.poll(() => calls(page, 'specs.set')).toHaveLength(5)
  await specs.close.click()
  const reads = await calls(page, 'specs.get')
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page
    .getByRole('dialog', { name: 'Settings', exact: true })
    .getByRole('button', { name: 'Specs' })
    .click()
  expect(await calls(page, 'specs.get')).toEqual(reads)
  await letGo(page, 'specs.set')
  await expect(specs.dialog).toBeVisible()
  expect((await calls(page, 'specs.get')).length).toBe(reads.length + 1)
  await expect(specs.on).not.toBeChecked()
})

test('Settings, Specs: a folder the daemon refused stays as typed, with why, while the other controls are saved, until a save of it is kept or the dialog opens again', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  const words = 'the specs folder is a folder inside the project: it stays inside it, with no `..`'
  await refuseSpecs(page, words)
  await specs.folder.fill('../elsewhere')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toHaveText(words)
  await refuseSpecs(page, null)
  // The box and the variant are saved, and their answers leave the folder and its refusal.
  await specs.on.check()
  await expect.poll(() => answers(page, 'specs.set')).toBe(2)
  await expect(specs.folder).toHaveValue('../elsewhere')
  await expect(specs.refusal).toHaveText(words)
  await specs.variant.selectOption('tdd')
  await expect.poll(() => answers(page, 'specs.set')).toBe(3)
  await expect(specs.folder).toHaveValue('../elsewhere')
  await expect(specs.refusal).toHaveText(words)
  // Mended, a save of it is kept, and the refusal goes.
  await specs.folder.fill('docs')
  await specs.folder.press('Enter')
  await expect.poll(() => answers(page, 'specs.set')).toBe(4)
  await expect(specs.refusal).toBeHidden()
  await expect(specs.folder).toHaveValue('docs')
  // One refused and left in the dialog is gone when it opens again: it shows what is kept.
  await refuseSpecs(page, words)
  await specs.folder.fill('../again')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toHaveText(words)
  await specs.close.click()
  await refuseSpecs(page, null)
  const again = await openSpecsSettings(page)
  await expect(again.folder).toHaveValue('docs')
  await expect(again.refusal).toBeHidden()
})

test('Settings, Specs: a save that failed is sent again when the human asks again, and one pending or kept is not', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  // The daemon cannot write the agents file: the folder's save fails, and what was typed stays.
  const words = 'could not write the agents file: EACCES: permission denied'
  await refuseSpecs(page, words)
  await specs.folder.fill('docs/specs')
  await specs.folder.press('Enter')
  await expect(specs.refusal).toHaveText(words)
  await expect(specs.folder).toHaveValue('docs/specs')
  // It can again: Enter with nothing changed asks once more, and not again while that is on its way.
  await refuseSpecs(page, null)
  await hold(page, 'specs.set')
  await specs.folder.press('Enter')
  await expect.poll(() => calls(page, 'specs.set')).toHaveLength(2)
  await specs.folder.press('Enter')
  await letGo(page, 'specs.set')
  await expect.poll(() => answers(page, 'specs.set')).toBe(2)
  await expect(specs.refusal).toBeHidden()
  expect(await page.evaluate(() => window.__model.specsApp.folder)).toBe('docs/specs')
  // Kept, Enter asks nothing; the next change is the box's own.
  await specs.folder.press('Enter')
  await specs.on.check()
  await expect.poll(() => answers(page, 'specs.set')).toBe(3)
  expect(await calls(page, 'specs.set')).toEqual([
    { folder: 'docs/specs' },
    { folder: 'docs/specs' },
    { on: true },
  ])
})

test('Settings, Specs: a change the daemon refuses once the dialog is shut is said on the status line', async ({
  page,
}) => {
  await open(page)
  const specs = await openSpecsSettings(page)
  const words = 'the specs folder is a folder inside the project: it stays inside it, with no `..`'
  await page.evaluate((words) => {
    window.__model.specsAppRefusal = words
  }, words)
  await hold(page, 'specs.set')
  await specs.folder.fill('../elsewhere')
  await specs.folder.press('Enter')
  await expect.poll(() => calls(page, 'specs.set')).toHaveLength(1)
  await specs.close.click()
  await expect(specs.dialog).toBeHidden()
  await letGo(page, 'specs.set')
  await expect(page.locator('#status')).toHaveText(words)
  // Opened again, it is what is kept, and the refusal is not waiting in it.
  await page.evaluate(() => {
    window.__model.specsAppRefusal = null
  })
  const again = await openSpecsSettings(page)
  await expect(again.refusal).toBeHidden()
  await expect(again.folder).toHaveValue('')
})

test('a screen that cannot open says why, and no dialog opens', async ({ page }) => {
  await open(page)
  await openScreen(page, 'Harnesses')
  await expect(page.locator('#status')).toHaveText(
    'the harnesses screen is not available: the daemon is not up',
  )
  expect(await page.getByRole('dialog').count()).toBe(0)
})

test('shows a member between tasks with no status line, its window gone until the next task', async ({
  page,
}) => {
  const data = model()
  const diana = data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana')
  diana.participant.outUntil = null
  await open(page, data)
  const row = page.locator('tr[data-handle="diana"]')
  await expect(row.locator('.row-meta')).toBeVisible()
  await expect(row.locator('.row-status')).toHaveCount(0)
  await expect(row.getByTestId('lamp')).toHaveAttribute('data-state', 'closed')
})

test('shows an image designer between tasks as any member: no status line until a terminal of it opens, then the count', async ({
  page,
}) => {
  const data = model()
  // An image designer has no tier; its tasks run in sessions like any member's.
  const iris = participant(8, 'iris', 'designer', {
    agent: 'iris',
    harness: 'codex',
    designer: true,
    tier: null,
  })
  data.boards[1].lanes.push({
    participant: iris,
    tasks: [],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  const row = page.locator('tr[data-handle="iris"]')
  await expect(row.locator('.row-status')).toHaveCount(0)
  await changed(
    page,
    (iris) => {
      window.__model.boards[1].lanes.push({
        participant: { ...iris, id: 9, handle: 'iris-amber-pine', memberId: 8, member: 'iris' },
        tasks: [],
        activity: { state: 'working' },
        pane: { id: 'p1-iris-amber-pine', generation: 1 },
      })
    },
    iris,
  )
  await expect(row.locator('.row-status')).toHaveText('1 terminal open, one per task')
})

test('closes an open project from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Close harbour' }).click()
  await expect.poll(() => calls(page, 'project.close')).toEqual([{ project: 1 }])
  await expect(page.getByRole('button', { name: 'Close foundry' })).toHaveCount(0)
})

/**
 * foundry, closed, with something everywhere a control would be: its chief,
 * a member and a session of it with a finished task, a message that waited
 * for approval and a note.
 */
function closedFoundry() {
  const data = model()
  const board = data.boards[2]
  board.project.gate = true
  const member = { ...participant(12, 'zeus', 'worker'), projectId: 2 }
  const lexer = task(3, 'Write the lexer', 'done', 'chief', 'zeus-amber-pine', 30, {
    projectId: 2,
    result: 'Lexer done.',
  })
  board.lanes.push(
    {
      participant: { ...participant(10, 'chief', 'chief'), projectId: 2 },
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
    { participant: member, tasks: [], activity: { state: 'closed' }, pane: null },
    {
      participant: { ...session(13, member, 'amber-pine'), projectId: 2 },
      tasks: [lexer],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  // What its Resume will start: the lexer, not accepted yet, and two messages that wait.
  board.resume = { tasks: 1, messages: 2 }
  board.gated = [
    {
      id: 30,
      kind: 'task',
      state: 'gated',
      sender: 'chief',
      recipient: 'zeus-amber-pine',
      taskNumber: 3,
      body: 'Add the tests',
      questions: null,
      choices: null,
      createdAt: at(2),
    },
  ]
  data.inbox[2] = [
    {
      id: 31,
      kind: 'note',
      state: 'queued',
      sender: null,
      recipient: 'human',
      taskNumber: 3,
      body: 'T-3 is done.',
      createdAt: at(1),
    },
  ]
  data.tasks['2:3'] = {
    ...lexer,
    messages: [message(32, 'result', 'zeus-amber-pine', 'chief', 'Lexer done.', 1)],
  }
  return data
}

/**
 * The key that takes the keyboard to the next control, a button too. Chromium's Tab does. WebKit's
 * passes a button or a link by, and stops at fields and what has a `tabindex`, unless the system's
 * Full Keyboard Access is on (off in a Mac's defaults): then Option+Tab is the one that passes them
 * by. So it is found as the engine has it, by a Tab from a button to the one beside it.
 */
async function nextControlKey(page) {
  await page.evaluate(() => {
    const pair = document.createElement('div')
    pair.id = 'tab-probe'
    pair.innerHTML =
      '<button id="tab-first" type="button">1</button><button type="button">2</button>'
    document.body.append(pair)
    document.querySelector('#tab-first').focus()
  })
  await page.keyboard.press('Tab')
  const reached = await page.evaluate(
    () => document.activeElement?.previousElementSibling?.id === 'tab-first',
  )
  await page.evaluate(() => document.querySelector('#tab-probe').remove())
  return reached ? 'Tab' : 'Alt+Tab'
}

test('shows a closed project read-only: it reads, nothing on it acts, and a banner resumes it', async ({
  page,
}) => {
  await open(page, closedFoundry())
  await unfold(page)
  await chooseProject(page, 'foundry')
  await unfold(page)
  const main = page.locator('main.main')
  await expect(main).toHaveAttribute('data-suspended', 'true')
  const banner = page.getByRole('status').filter({ hasText: 'Waiting for Resume.' })
  await expect(banner).toContainText('1 open task, 2 messages waiting.')
  // Only its settings change: they are what its chief starts with, so they open before the Resume.
  await expect(page.getByRole('button', { name: 'Project settings' })).toBeEnabled()
  await expect(page.locator('.terminal-card')).toHaveCount(0)
  // Nothing on the board acts: no button for it to be reached by, with the keyboard either.
  const board = page.getByRole('region', { name: 'Board' })
  for (const name of [
    /^Approve/,
    /^Decline/,
    /^Mark .* read$/,
    /^Open .*'s terminal$/,
    /^Close .*'s terminal$/,
    /^Delete .*'s session$/,
  ]) {
    await expect(board.getByRole('button', { name })).toHaveCount(0)
  }
  // Nor is the chief switched from anywhere: its card in the dock went with its window.
  const switchChief = page.getByRole('button', {
    name: 'Switch the chief to another harness or model',
  })
  await expect(switchChief).toHaveCount(0)
  const next = await nextControlKey(page)
  await banner.getByRole('button', { name: 'Resume project' }).focus()
  const reached = []
  for (let press = 0; press < 4; press += 1) {
    await page.keyboard.press(next)
    reached.push(
      await page.evaluate(
        () =>
          document.activeElement.getAttribute('aria-label') ?? document.activeElement.textContent,
      ),
    )
  }
  // Folding a member's sessions only reads: it stays, as the cards do.
  expect(reached).toEqual([
    'Open task',
    'Open task',
    "Hide @zeus's 1 session",
    'T-3, Write the lexer, Done, from @chief',
  ])
  // What it says is all there to read: a card opens its task, with nothing to do on it.
  await page.locator('button.card[data-task="3"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-3' })
  await expect(drawer.locator('.step[data-kind="result"] .step-body')).toHaveText('Lexer done.')
  await expect(drawer.getByRole('button')).toHaveText(['Close'])
  await drawer.getByRole('button', { name: 'Close the task' }).click()
  await banner.getByRole('button', { name: 'Resume project' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
  await chooseProject(page, 'harbour')
  await unfold(page)
  await expect(main).toHaveAttribute('data-suspended', 'false')
  await expect(page.getByRole('button', { name: 'Project settings' })).toBeEnabled()
})

test('says in one line that a suspended project waits for Resume and what Resume will start', async ({
  page,
}) => {
  const data = closedFoundry()
  data.boards[2].resume = { tasks: 3, messages: 1 }
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, data)
  await chooseProject(page, 'foundry')
  const banner = page.getByRole('status').filter({ hasText: 'Waiting for Resume.' })
  await expect(banner.locator('strong')).toHaveText('Waiting for Resume.')
  await expect(banner.locator('span')).toHaveText('3 open tasks, 1 message waiting.')
  // One line: the banner is no taller than its button and the padding round it.
  const [height, button] = await Promise.all([
    banner.evaluate((node) => node.getBoundingClientRect().height),
    banner
      .getByRole('button', { name: 'Resume project' })
      .evaluate((node) => node.getBoundingClientRect().height),
  ])
  expect(height).toBeLessThan(button + 40)
  // Nothing open and nothing waiting is said as it is.
  await page.evaluate(() => {
    window.__model.boards[2].resume = { tasks: 0, messages: 0 }
    window.__listeners.get('state-changed')()
  })
  await expect(banner.locator('span')).toHaveText('0 open tasks, 0 messages waiting.')
})

test('deletes a closed project for good once the human confirms, never an open one', async ({
  page,
}) => {
  await open(page)
  await expect(page.getByRole('button', { name: 'Delete harbour' })).toHaveCount(0)
  await page.getByRole('button', { name: 'Delete foundry' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete foundry?' })
  await expect(dialog).toContainText('kept for its windows go for good')
  await dialog.getByRole('button', { name: 'Keep' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'project.delete')).toEqual([])
  await page.getByRole('button', { name: 'Delete foundry' }).click()
  await dialog.getByRole('button', { name: 'Delete for good' }).click()
  await expect(dialog).toBeHidden()
  await expect.poll(() => calls(page, 'project.delete')).toEqual([{ project: 2 }])
  await expect(page.locator('#status')).toHaveText('foundry is deleted.')
  // The note goes by itself; it does not sit over the windows.
  await expect(page.locator('#status')).toHaveText('', { timeout: 8_000 })
})

test('resumes a suspended project from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Resume foundry' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
})

// --- The project menu: a right-click, or the keyboard, on a project in the list. ---

/** A project's card in the list, by id, the button that chooses it, and its menu. */
const projectCard = (page, id) => page.locator(`li.project[data-project="${id}"]`)
const projectButton = (page, id) => projectCard(page, id).locator('.project-select')
const projectMenu = (page) => page.getByRole('menu')
const itemNamed = (page, name) => projectMenu(page).getByRole('menuitem', { name })

/** The menu, opened on project `id` by a right-click on its button. */
async function menuOn(page, id, position) {
  await projectButton(page, id).click({ button: 'right', position })
  await expect(projectMenu(page)).toBeVisible()
}

/**
 * The page's timers set so far have run: one set now, with the same delay, runs after them. The
 * menu gives the keyboard back to a project once a click has had its way with the focus, in a
 * timer a few milliseconds after it, so what a test says of the focus after a click waits for it.
 */
const settled = (page) => page.evaluate(() => new Promise((done) => setTimeout(done)))

/** The dialog an item of the menu opens, for project `id`. */
async function dialogFrom(page, id, item, title) {
  await menuOn(page, id)
  await itemNamed(page, item).click()
  const dialog = page.getByRole('dialog', { name: title })
  await expect(dialog).toBeVisible()
  return dialog
}

test('project menu: a right-click on a project opens its menu beside the pointer, Rename, Description and Project settings, and selects nothing', async ({
  page,
}) => {
  await open(page)
  // foundry is not the project shown: the menu is for the one right-clicked.
  const button = await projectButton(page, 2).boundingBox()
  await menuOn(page, 2, { x: 40, y: 10 })
  await expect(projectMenu(page).getByRole('menuitem')).toHaveText([
    'Rename…',
    'Description…',
    'Project settings…',
    'Specs…',
  ])
  await expect(projectMenu(page)).toHaveAccessibleName('foundry')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  const opened = await projectMenu(page).boundingBox()
  expect(Math.abs(opened.x - (button.x + 40))).toBeLessThan(2)
  expect(Math.abs(opened.y - (button.y + 10))).toBeLessThan(2)
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect(projectButton(page, 1)).toHaveAttribute('aria-current', 'true')
  await expect(projectButton(page, 2)).toHaveAttribute('aria-current', 'false')
  // The tools of a card are part of the project: a right-click there is the same.
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Close harbour' }).click({ button: 'right' })
  await expect(projectMenu(page)).toHaveAccessibleName('harbour')
  // The menu is the page's: the browser's own is taken away on a project, and left where the
  // list has none of one.
  await page.keyboard.press('Escape')
  const taken = (locator) =>
    locator.evaluate((node) => {
      const asked = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
      node.dispatchEvent(asked)
      return asked.defaultPrevented
    })
  expect(await taken(projectButton(page, 1))).toBe(true)
  await page.keyboard.press('Escape')
  expect(await taken(page.locator('#project-title'))).toBe(false)
  await expect(projectMenu(page)).toBeHidden()
})

test('project menu: a menu that would run off the window is kept inside it', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 250 })
  await open(page)
  await menuOn(page, 2, { x: 190, y: 20 })
  const box = await projectMenu(page).boundingBox()
  expect(box.y + box.height).toBeLessThanOrEqual(250)
  expect(box.x + box.width).toBeLessThanOrEqual(900)
  expect(box.y).toBeGreaterThanOrEqual(0)
})

test('project menu: Shift+F10 and the context-menu key open it on the project the keyboard is on, and the arrow keys choose', async ({
  page,
}) => {
  await open(page)
  await projectButton(page, 2).focus()
  await page.keyboard.press('Shift+F10')
  await expect(projectMenu(page)).toBeVisible()
  await expect(projectMenu(page)).toHaveAccessibleName('foundry')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  // Where there is no pointer it opens at the project's button: its left edge, just under it.
  // (The browser has its own place for the context-menu key, at the control the keyboard is on.)
  const anchored = async (id) => {
    const [menu, button] = await Promise.all([
      projectMenu(page).boundingBox(),
      projectButton(page, id).boundingBox(),
    ])
    expect(Math.abs(menu.x - button.x)).toBeLessThan(2)
    expect(Math.abs(menu.y - (button.y + button.height))).toBeLessThan(2)
  }
  await anchored(2)
  await page.keyboard.press('Escape')
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectButton(page, 2)).toBeFocused()

  // The context-menu key, from any control of a card: it is the card's project.
  await page.getByRole('button', { name: 'Close harbour' }).focus()
  await page.keyboard.press('ContextMenu')
  await expect(projectMenu(page)).toBeVisible()
  await expect(projectMenu(page)).toHaveAccessibleName('harbour')
  await anchored(1)
  // The arrow keys go round the items, Home and End to either end.
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Description…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Project settings…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Specs…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('ArrowUp')
  await expect(itemNamed(page, 'Specs…')).toBeFocused()
  await page.keyboard.press('Home')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('End')
  await expect(itemNamed(page, 'Specs…')).toBeFocused()
  await page.keyboard.press('ArrowUp')
  await expect(itemNamed(page, 'Project settings…')).toBeFocused()
  await page.keyboard.press('ArrowUp')
  await expect(itemNamed(page, 'Description…')).toBeFocused()
  // Enter chooses the item the keyboard is on.
  await page.keyboard.press('Enter')
  await expect(projectMenu(page)).toBeHidden()
  await expect(page.getByRole('dialog', { name: 'Project description' })).toBeVisible()
  await page.keyboard.press('Escape')
  // The dialog gives the keyboard back to the project as it closes: after that, not before.
  await expect(page.getByRole('dialog', { name: 'Project description' })).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()

  // Anywhere but on a project the keys open nothing.
  await page.locator('#inbox-button').focus()
  await page.keyboard.press('Shift+F10')
  await page.keyboard.press('ContextMenu')
  await expect(projectMenu(page)).toBeHidden()
})

test('project menu: Escape closes it, and so does a click elsewhere, the keyboard going back to the project', async ({
  page,
}) => {
  await open(page)
  await menuOn(page, 1)
  await page.keyboard.press('Escape')
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
  // A click on nothing in particular.
  await menuOn(page, 2)
  await page.locator('#project-title').click()
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectButton(page, 2)).toBeFocused()
  // A click on a control is that control's: the menu is shut, and the control keeps the keyboard.
  await menuOn(page, 2)
  await page.locator('#inbox-button').click()
  await settled(page)
  await expect(projectMenu(page)).toBeHidden()
  await expect(page.locator('#inbox-button')).toBeFocused()
  // Tab leaves the menu, back to the project.
  await menuOn(page, 1)
  await page.keyboard.press('Tab')
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
  // A right-click on another project moves the menu there, and the keyboard is the new menu's.
  await menuOn(page, 1, { x: 190, y: 5 })
  await menuOn(page, 2, { x: 30, y: 10 })
  await settled(page)
  await expect(projectMenu(page)).toHaveAccessibleName('foundry')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  // None of it asked the daemon for a change.
  expect(await calls(page, 'project.rename')).toEqual([])
  expect(await calls(page, 'project.describe')).toEqual([])
})

test('a click gives the control it lands on the keyboard, in every engine: a button, a checkbox, a radio button', async ({
  page,
}) => {
  await open(page)
  // Controls of its own, so that what the page draws does not decide what is measured.
  await page.evaluate(() => {
    const scratch = document.createElement('div')
    scratch.style.cssText = 'position: fixed; top: 0; left: 0; z-index: 9999; background: white;'
    scratch.innerHTML = `<button id="s-button" type="button">press</button>
      <input id="s-check" type="checkbox" aria-label="check">
      <input id="s-radio" type="radio" name="s" aria-label="radio">
      <span id="s-text">text</span>`
    document.body.append(scratch)
  })
  // WebKit leaves each of them unfocused when a click lands on it; Chromium focuses it.
  for (const id of ['#s-button', '#s-check', '#s-radio', '#s-button']) {
    await page.locator(id).click()
    await expect(page.locator(id)).toBeFocused()
  }
  // A click a script makes moves nothing: only a person's click gives the keyboard.
  await page.locator('#s-button').focus()
  await page.evaluate(() => document.querySelector('#s-check').click())
  await expect(page.locator('#s-button')).toBeFocused()
  // A click on words is no control's: it gives the keyboard to nothing.
  await page.locator('#s-text').click()
  await expect(page.locator('#s-button')).not.toBeFocused()
})

test('project menu: a board drawn again leaves the menu open, and Escape then goes back to the project as it is drawn now', async ({
  page,
}) => {
  await open(page)
  await menuOn(page, 1)
  await changed(page, () => {
    for (const shown of [window.__model.projects[0], window.__model.boards[1].project]) {
      shown.name = 'harbour two'
    }
  })
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('harbour two')
  await expect(projectMenu(page)).toBeVisible()
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
})

/** `project` gone from what the daemon lists, as when it is deleted from elsewhere. */
const goneFrom = (project) => {
  window.__model.projects = window.__model.projects.filter((listed) => listed.id !== project)
}

test('project menu: an open menu follows a rename made elsewhere, and is for the project as the page has it now', async ({
  page,
}) => {
  await open(page)
  await menuOn(page, 1)
  await expect(projectMenu(page)).toHaveAccessibleName('harbour')
  await changed(page, () => {
    for (const shown of [window.__model.projects[0], window.__model.boards[1].project]) {
      shown.name = 'harbour two'
    }
  })
  await expect(projectMenu(page)).toHaveAccessibleName('harbour two')
  await expect(projectMenu(page)).toBeVisible()
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  // What it opens is on the name the project has now.
  await itemNamed(page, 'Rename…').click()
  await expect(page.getByRole('dialog', { name: 'Rename project' }).getByLabel('Name')).toHaveValue(
    'harbour two',
  )
})

test('project menu: an open menu shuts when its project is gone, and the keyboard goes to the project that took its place, or the one before it', async ({
  page,
}) => {
  const data = model()
  data.projects.push({ id: 3, name: 'quarry', directory: '/work/quarry', state: 'suspended' })
  await open(page, data)
  // The one in the middle goes: the one after it is where it was.
  await menuOn(page, 2)
  await changed(page, goneFrom, 2)
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectCard(page, 2)).toHaveCount(0)
  await expect(projectButton(page, 3)).toBeFocused()
  // The last one goes: the one before it.
  await menuOn(page, 3)
  await changed(page, goneFrom, 3)
  await expect(projectMenu(page)).toBeHidden()
  await expect(projectCard(page, 3)).toHaveCount(0)
  await expect(projectButton(page, 1)).toBeFocused()
})

test('project menu: with no project left an open menu shuts and the keyboard goes to New project', async ({
  page,
}) => {
  await open(page)
  await menuOn(page, 1)
  await page.evaluate(() => {
    window.__model.projects = []
    window.__listeners.get('state-changed')()
  })
  await expect(projectMenu(page)).toBeHidden()
  await expect(page.locator('li.project')).toHaveCount(0)
  await expect(page.locator('#new-project')).toBeFocused()
})

test('project menu: an open menu whose project is gone leaves the keyboard where the human had put it', async ({
  page,
}) => {
  await open(page)
  await menuOn(page, 2)
  // The keyboard is elsewhere (the menu is still open: nothing was pressed outside it).
  await page.locator('#inbox-button').focus()
  await expect(projectMenu(page)).toBeVisible()
  await changed(page, goneFrom, 2)
  await expect(projectMenu(page)).toBeHidden()
  await expect(page.locator('#inbox-button')).toBeFocused()
})

test('project menu: Rename… saves with Enter, and the new name is in the list, over the board and in the buttons named after it', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Rename…', 'Rename project')
  const name = dialog.getByLabel('Name')
  await expect(name).toHaveValue('harbour')
  await expect(name).toBeFocused()
  // What is there is selected, to be typed over.
  expect(
    await name.evaluate((input) => input.value.slice(input.selectionStart, input.selectionEnd)),
  ).toBe('harbour')
  await name.fill('  Quay  ')
  await name.press('Enter')
  await expect.poll(() => calls(page, 'project.rename')).toEqual([{ project: 1, name: 'Quay' }])
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('Quay')
  await expect(page.locator('#project-title')).toHaveText('Quay')
  await expect(page.getByRole('button', { name: 'Close Quay' })).toBeVisible()
  await expect(page.locator('#status')).toHaveText('harbour is now Quay.')
  // The keyboard is on the project, drawn again under its new name.
  await expect(projectButton(page, 1)).toBeFocused()
  // The folder is the one the project was opened in, and another project is as it was.
  await expect(page.locator('#project-directory')).toHaveText('/work/harbour')
  await expect(projectButton(page, 2).locator('.project-name')).toHaveText('foundry')
  // Opened again, it shows what was saved.
  await dialogFrom(page, 1, 'Rename…', 'Rename project')
  await expect(name).toHaveValue('Quay')
})

test('project menu: Enter pressed twice while the daemon answers asks it once', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() => {
    window.__delay['project.rename'] = 300
  })
  const dialog = await dialogFrom(page, 1, 'Rename…', 'Rename project')
  const name = dialog.getByLabel('Name')
  await name.fill('Quay')
  await name.press('Enter')
  await name.press('Enter')
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'project.rename')).toEqual([{ project: 1, name: 'Quay' }])
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('Quay')
})

test('project menu: Escape in the rename dialog, and Cancel, leave the name and ask the daemon nothing', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Rename…', 'Rename project')
  const name = dialog.getByLabel('Name')
  await name.fill('Quay')
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
  await dialogFrom(page, 1, 'Rename…', 'Rename project')
  // What was typed is not kept: it opens on the name as it is.
  await expect(name).toHaveValue('harbour')
  await name.fill('Quay')
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
  expect(await calls(page, 'project.rename')).toEqual([])
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('harbour')
  await expect(page.locator('#project-title')).toHaveText('harbour')
})

test('project menu: what the daemon refuses is said in the dialog, which stays open on what was typed', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Rename…', 'Rename project')
  const name = dialog.getByLabel('Name')
  const refusal = dialog.getByRole('alert')
  await expect(refusal).toBeHidden()
  // A field left empty is the browser's to stop: nothing is asked of the daemon.
  await name.fill('')
  await name.press('Enter')
  await expect(dialog).toBeVisible()
  expect(await calls(page, 'project.rename')).toEqual([])
  // Nothing but spaces gets past the field, and is the daemon's to refuse.
  await name.fill('   ')
  await name.press('Enter')
  await expect(refusal).toHaveText('name must be text, not empty, at most 100 characters')
  await expect(dialog).toBeVisible()
  await expect(name).toHaveValue('   ')
  expect(await calls(page, 'project.rename')).toEqual([{ project: 1, name: '' }])
  // The field stops at the most a name may have; one that gets past it (a field that does not
  // stop it) is refused all the same.
  await name.fill('x'.repeat(150))
  expect((await name.inputValue()).length).toBe(100)
  await name.evaluate((input) => input.removeAttribute('maxlength'))
  await name.fill('x'.repeat(101))
  await name.press('Enter')
  await expect.poll(async () => (await calls(page, 'project.rename')).length).toBe(2)
  await expect(refusal).toHaveText('name must be text, not empty, at most 100 characters')
  await expect(name).toHaveValue('x'.repeat(101))
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('harbour')
  // Mended and saved, the refusal goes with the dialog, and is not there when it opens again.
  await name.fill('Quay')
  await name.press('Enter')
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('Quay')
  await dialogFrom(page, 1, 'Rename…', 'Rename project')
  await expect(refusal).toBeHidden()
})

/** How many rows the text of `node` is on: the tops its pieces are drawn at. */
const rowsOf = (locator) =>
  locator.evaluate((node) => {
    const range = document.createRange()
    range.selectNodeContents(node)
    return new Set([...range.getClientRects()].map((rect) => Math.round(rect.top))).size
  })

test('project menu: Description… writes a line under the name, smaller and quieter, whole on as many rows as it takes, with no ellipsis and no title', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Description…', 'Project description')
  const line = dialog.getByLabel('Description')
  await expect(line).toHaveValue('')
  await expect(line).toHaveAttribute('maxlength', '200')
  const whole =
    'Ships the cargo to the quay and back again, every tide, with not a day off in the whole year'
  await line.fill(`  ${whole}  `)
  await line.press('Enter')
  await expect
    .poll(() => calls(page, 'project.describe'))
    .toEqual([{ project: 1, description: whole }])
  await expect(dialog).toBeHidden()
  await expect(page.locator('#status')).toHaveText('The description of harbour is saved.')
  await expect(projectButton(page, 1)).toBeFocused()
  const name = projectButton(page, 1).locator('.project-name')
  const shown = projectButton(page, 1).locator('.project-description')
  await expect(shown).toHaveText(whole)
  // Under the name, smaller and quieter than it.
  const [nameBox, shownBox] = await Promise.all([name.boundingBox(), shown.boundingBox()])
  expect(shownBox.y).toBeGreaterThanOrEqual(nameBox.y + nameBox.height - 1)
  const looks = (locator) =>
    locator.evaluate((node) => {
      const style = getComputedStyle(node)
      return { size: Number.parseFloat(style.fontSize), color: style.color }
    })
  const [nameLooks, shownLooks] = await Promise.all([looks(name), looks(shown)])
  expect(shownLooks.size).toBeLessThan(nameLooks.size)
  expect(shownLooks.color).not.toBe(nameLooks.color)
  // Whole where it is too long for one row of the list: on as many rows as it takes, with no
  // ellipsis, nothing cut, and no title to read the rest by.
  expect(await rowsOf(shown)).toBeGreaterThan(1)
  expect(
    await shown.evaluate((node) => ({
      ellipsis: getComputedStyle(node).textOverflow === 'ellipsis',
      cutAcross: node.scrollWidth > node.clientWidth,
      cutDown: node.scrollHeight > node.clientHeight,
      title: node.getAttribute('title'),
    })),
  ).toEqual({ ellipsis: false, cutAcross: false, cutDown: false, title: null })
  expect(await projectButton(page, 1).getAttribute('title')).toBeNull()
  // The line is the human's alone: the board and the other project show nothing of it.
  await expect(page.locator('.board')).not.toContainText('Ships the cargo')
  await expect(projectButton(page, 2).locator('.project-description')).toHaveCount(0)
  // Written again, it is a line of the daemon's too: a short one is on a row of its own.
  await dialogFrom(page, 1, 'Description…', 'Project description')
  await expect(line).toHaveValue(whole)
  await line.fill('Moves the cargo')
  await line.press('Enter')
  await expect(shown).toHaveText('Moves the cargo')
  expect(await rowsOf(shown)).toBe(1)
  expect(await shown.getAttribute('title')).toBeNull()
})

test('project menu: a description of two hundred characters is shown whole in the list, and a word too long for it is broken, not made to widen it', async ({
  page,
}) => {
  await open(page)
  const sidebar = page.getByRole('navigation', { name: 'Projects' })
  const sizes = () =>
    sidebar.evaluate((node) => ({
      width: node.getBoundingClientRect().width,
      beyond: node.scrollWidth - node.clientWidth,
    }))
  // The list as it is with no line under any name.
  const bare = await sizes()
  // The most a description may be: two hundred characters, of words and of one.
  const words = `${'tides '.repeat(33)}ab`
  expect(words.length).toBe(200)
  const unbroken = 'x'.repeat(200)
  await changed(
    page,
    ([first, second]) => {
      for (const [at, text] of [first, second].entries()) {
        for (const shown of [window.__model.projects[at], window.__model.boards[at + 1].project]) {
          shown.description = text
        }
      }
    },
    [words, unbroken],
  )
  // Whole on several rows, each inside its card.
  const spread = projectButton(page, 1).locator('.project-description')
  await expect(spread).toHaveText(words)
  expect(await rowsOf(spread)).toBeGreaterThan(2)
  const inside = async (project, line) => {
    const [card, row] = await Promise.all([
      projectButton(page, project).boundingBox(),
      line.boundingBox(),
    ])
    expect(row.x + row.width).toBeLessThanOrEqual(card.x + card.width)
  }
  await inside(1, spread)
  // A word that no row holds is broken where it must be, and still all there.
  const broken = projectButton(page, 2).locator('.project-description')
  await expect(broken).toHaveText(unbroken)
  expect(await rowsOf(broken)).toBeGreaterThan(1)
  await inside(2, broken)
  // The sidebar is as wide as it was, with no sideways scroll.
  const now = await sizes()
  expect(now.width).toBe(bare.width)
  expect(now.beyond).toBeLessThanOrEqual(0)
})

test('project menu: saving an empty description takes the line away, and the dialog shows what was saved when it opens again', async ({
  page,
}) => {
  const data = model()
  for (const shown of [data.projects[0], data.boards[1].project]) {
    shown.description = 'Moves the cargo'
  }
  await open(page, data)
  const shown = projectButton(page, 1).locator('.project-description')
  await expect(shown).toHaveText('Moves the cargo')
  const dialog = await dialogFrom(page, 1, 'Description…', 'Project description')
  const line = dialog.getByLabel('Description')
  // It opens on the line the project has.
  await expect(line).toHaveValue('Moves the cargo')
  await line.fill('')
  await line.press('Enter')
  await expect
    .poll(() => calls(page, 'project.describe'))
    .toEqual([{ project: 1, description: '' }])
  await expect(dialog).toBeHidden()
  await expect(shown).toHaveCount(0)
  await expect(page.locator('#status')).toHaveText('harbour has no description.')
  await expect(projectButton(page, 1)).toBeFocused()
  // Opened again, it is empty; spaces alone are no line either.
  await dialogFrom(page, 1, 'Description…', 'Project description')
  await expect(line).toHaveValue('')
  await line.fill('   ')
  await line.press('Enter')
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'project.describe')).toEqual([
    { project: 1, description: '' },
    { project: 1, description: '' },
  ])
  await expect(shown).toHaveCount(0)
})

test('project menu: a line the daemon refuses is said in the dialog, and a dialog opens on the project as it is now', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Description…', 'Project description')
  const line = dialog.getByLabel('Description')
  const refusal = dialog.getByRole('alert')
  // The field stops at two hundred; one that gets past it is refused in the daemon's words.
  await line.evaluate((input) => input.removeAttribute('maxlength'))
  await line.fill('a'.repeat(201))
  await line.press('Enter')
  await expect(refusal).toHaveText(
    "a project's description is one line of plain text: it is at most 200 characters",
  )
  await expect(dialog).toBeVisible()
  await expect(line).toHaveValue('a'.repeat(201))
  await line.fill('First')
  await line.press('Enter')
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1).locator('.project-description')).toHaveText('First')
  // The daemon has another line by the time it opens again: it shows that one, not the last drawn.
  await changed(page, () => {
    for (const shown of [window.__model.projects[0], window.__model.boards[1].project]) {
      shown.description = 'Elsewhere'
    }
  })
  await dialogFrom(page, 1, 'Description…', 'Project description')
  await expect(line).toHaveValue('Elsewhere')
  await expect(refusal).toBeHidden()
})

test('project menu: a closed project is renamed and described from its menu as an open one is', async ({
  page,
}) => {
  await open(page)
  const rename = await dialogFrom(page, 2, 'Rename…', 'Rename project')
  await rename.getByLabel('Name').fill('forge')
  await rename.getByLabel('Name').press('Enter')
  await expect.poll(() => calls(page, 'project.rename')).toEqual([{ project: 2, name: 'forge' }])
  await expect(projectButton(page, 2).locator('.project-name')).toHaveText('forge')
  await expect(page.getByRole('button', { name: 'Resume forge' })).toBeVisible()
  const describe = await dialogFrom(page, 2, 'Description…', 'Project description')
  await describe.getByLabel('Description').fill('Where the iron goes')
  await describe.getByLabel('Description').press('Enter')
  await expect(projectButton(page, 2).locator('.project-description')).toHaveText(
    'Where the iron goes',
  )
  // The project shown is the one it was, whatever was done to the other.
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect(projectButton(page, 1).locator('.project-name')).toHaveText('harbour')
})

/** twoOpen's foundry with a staff of its own: athena, and nobody else. */
function twoWithStaffs() {
  const data = twoOpen()
  data.boards[2].lanes.push({
    participant: {
      ...participant(11, 'athena', 'worker', { harness: 'pi', tier: 'light' }),
      projectId: 2,
    },
    tasks: [],
    activity: { state: 'closed' },
    pane: null,
  })
  return data
}

test('project menu: Project settings… opens the dialog of the board’s Project settings button for the project the board shows', async ({
  page,
}) => {
  await open(page)
  const dialog = await dialogFrom(page, 1, 'Project settings…', 'Project settings')
  await expect(page.locator('#project-title')).toHaveText('harbour')
  // The same dialog the button opens: its sections, and the project's own staff and calls.
  // Computer use is last, and there since the project has Claude Code members.
  await expect(dialog.locator('fieldset.project-section > legend')).toHaveText([
    'Staff',
    'Rules for the staff',
    'Compaction',
    'Advisor tool',
    'Human approval',
    'Wake-up calls',
    'Specs',
    'Computer use',
  ])
  await expect(dialog.getByRole('table', { name: 'On the staff' }).locator('tbody tr')).toHaveText([
    /@zeus.*Worker/,
    /@diana.*Worker/,
  ])
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 1 }])
  // Left, it gives the keyboard back to the project it was asked of.
  await dialog.getByRole('button', { name: 'Close' }).click()
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 1)).toBeFocused()
})

test('project menu: Project settings… on a project the board does not show shows it first, and opens the dialog on it', async ({
  page,
}) => {
  await open(page, twoWithStaffs())
  await expect(page.locator('#project-title')).toHaveText('harbour')
  const dialog = await dialogFrom(page, 2, 'Project settings…', 'Project settings')
  // foundry is the project shown, as if it had been chosen in the list.
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await expect(projectButton(page, 2)).toHaveAttribute('aria-current', 'true')
  await expect(projectButton(page, 1)).toHaveAttribute('aria-current', 'false')
  // The dialog is foundry's, not harbour's: its calls were read, its staff is in it, and it acts on it.
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 2 }])
  await expect(dialog.getByRole('table', { name: 'On the staff' }).locator('tbody tr')).toHaveText([
    /@athena.*Worker/,
  ])
  await dialog
    .getByRole('group', { name: 'Human approval', exact: true })
    .getByRole('checkbox')
    .check()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 2, gate: true }])
  // Left with Escape, the keyboard is on foundry's card.
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(projectButton(page, 2)).toBeFocused()
  // And the board's own button opens the same dialog for the project shown now, without that asked again.
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog.getByRole('table', { name: 'On the staff' }).locator('tbody tr')).toHaveText([
    /@athena.*Worker/,
  ])
  await page.keyboard.press('Escape')
  await expect(page.getByRole('button', { name: 'Project settings' })).toBeFocused()
})

test('project menu: Project settings… on a project while the board of another is being read opens it on the project asked for', async ({
  page,
}) => {
  await open(page, twoWithStaffs())
  // harbour's board is read again, slowly, when the human asks for foundry's settings.
  await page.evaluate(() => {
    window.__delay['board.get'] = 300
  })
  const boards = (await calls(page, 'board.get')).length
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(boards)
  await menuOn(page, 2)
  await itemNamed(page, 'Project settings…').click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeVisible()
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await expect(dialog.getByRole('table', { name: 'On the staff' }).locator('tbody tr')).toHaveText([
    /@athena.*Worker/,
  ])
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 2 }])
})

test('project menu: a closed project has Project settings… to open as an open one has, and the keys reach it', async ({
  page,
}) => {
  await open(page)
  // foundry is closed.
  await menuOn(page, 2)
  await expect(projectMenu(page).getByRole('menuitem')).toHaveText([
    'Rename…',
    'Description…',
    'Project settings…',
    'Specs…',
  ])
  await expect(itemNamed(page, 'Project settings…')).toBeEnabled()
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Description…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Project settings…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Specs…')).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(itemNamed(page, 'Rename…')).toBeFocused()
  await page.keyboard.press('End')
  await expect(itemNamed(page, 'Specs…')).toBeFocused()
  await page.keyboard.press('ArrowUp')
  await expect(itemNamed(page, 'Project settings…')).toBeFocused()
  // Chosen, it shows foundry, as it does for any project the board does not show, and opens
  // the dialog on it.
  await page.keyboard.press('Enter')
  await expect(page.getByRole('dialog', { name: 'Project settings' })).toBeVisible()
  await expect(page.locator('#project-title')).toHaveText('foundry')
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 2 }])
  // Nothing of the project is resumed by it: the board is still a closed project's.
  await expect(page.locator('main.main')).toHaveAttribute('data-suspended', 'true')
  expect(await calls(page, 'project.resume')).toEqual([])
})

/**
 * The daemon's next answer to `operation` is held until `letGo`: what the page asked
 * is on its way, and the page goes on meanwhile. `answers` counts what it answered.
 */
const hold = (page, operation) =>
  page.evaluate((operation) => {
    window.__hold[operation] = new Promise((go) => {
      window.__release[operation] = go
    })
  }, operation)
const letGo = (page, operation) =>
  page.evaluate((operation) => window.__release[operation](), operation)
const answers = (page, operation) =>
  page.evaluate(
    (operation) => window.__answered.filter((answered) => answered === operation).length,
    operation,
  )

/**
 * What differs between the two dialogs where the human saves and cancels before the daemon
 * answers: the item that opens each; what is typed for harbour, which is saved, and for
 * foundry, which is not; a text the daemon refuses (spaces for a name, over two hundred
 * characters for a line: the field's own limit is lifted for it); what the list shows of
 * a save; what the status line says of harbour's; and how the human leaves the dialog.
 */
const CANCELLED = [
  {
    what: 'rename',
    item: 'Rename…',
    title: 'Rename project',
    label: 'Name',
    operation: 'project.rename',
    body: (project, text) => ({ project, name: text }),
    saved: 'Quay',
    typed: 'forge',
    refused: '   ',
    told: 'harbour is now Quay.',
    leaves: 'Cancel',
    shows: (page, project, text) =>
      expect(projectButton(page, project).locator('.project-name')).toHaveText(text),
  },
  {
    what: 'description',
    item: 'Description…',
    title: 'Project description',
    label: 'Description',
    operation: 'project.describe',
    body: (project, text) => ({ project, description: text }),
    saved: 'Moves the cargo',
    typed: 'Where the iron goes',
    refused: 'a'.repeat(201),
    told: 'The description of harbour is saved.',
    leaves: 'Escape',
    shows: (page, project, text) =>
      expect(projectButton(page, project).locator('.project-description')).toHaveText(text),
  },
]

/** The dialog of `spec` opened on project `id`, with `text` typed in its field. */
async function typedIn(page, spec, id, text, { unlimited = false } = {}) {
  const dialog = await dialogFrom(page, id, spec.item, spec.title)
  const field = dialog.getByLabel(spec.label)
  if (unlimited) await field.evaluate((input) => input.removeAttribute('maxlength'))
  await field.fill(text)
  return { dialog, field }
}

/** The human leaves the dialog without saving, as `spec` says. */
async function leave(page, spec, dialog) {
  if (spec.leaves === 'Cancel') await dialog.getByRole('button', { name: 'Cancel' }).click()
  else await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
}

for (const spec of CANCELLED) {
  test(`project menu: a ${spec.what} saved and cancelled before the daemon answers does not close the dialog opened next on another project`, async ({
    page,
  }) => {
    await open(page)
    await hold(page, spec.operation)
    const first = await typedIn(page, spec, 1, spec.saved)
    await first.field.press('Enter')
    await expect.poll(() => calls(page, spec.operation)).toEqual([spec.body(1, spec.saved)])
    await leave(page, spec, first.dialog)
    // foundry's dialog, opened after it, with its own text typed and not saved.
    const next = await typedIn(page, spec, 2, spec.typed)
    // The daemon answers harbour's save now, and the list shows what it did.
    await letGo(page, spec.operation)
    await spec.shows(page, 1, spec.saved)
    await settled(page)
    // foundry's dialog is as the human left it, and theirs to save. Cancelling did not
    // take harbour's change back, and the status line tells it.
    await expect(next.dialog).toBeVisible()
    await expect(next.field).toHaveValue(spec.typed)
    await expect(next.field).toBeFocused()
    await expect(page.locator('#status')).toHaveText(spec.told)
    await next.field.press('Enter')
    await expect
      .poll(() => calls(page, spec.operation))
      .toEqual([spec.body(1, spec.saved), spec.body(2, spec.typed)])
    await expect(next.dialog).toBeHidden()
    await spec.shows(page, 2, spec.typed)
  })

  test(`project menu: a ${spec.what} that the daemon refuses after the human cancelled it is not said in the dialog opened next on another project`, async ({
    page,
  }) => {
    await open(page)
    await hold(page, spec.operation)
    const first = await typedIn(page, spec, 1, spec.refused, { unlimited: true })
    await first.field.press('Enter')
    await expect
      .poll(() => calls(page, spec.operation))
      .toEqual([spec.body(1, spec.refused.trim())])
    await leave(page, spec, first.dialog)
    const next = await typedIn(page, spec, 2, spec.typed)
    await letGo(page, spec.operation)
    await expect.poll(() => answers(page, spec.operation)).toBe(1)
    await settled(page)
    // The refusal was of a dialog the human had left: foundry's has nothing of it, and its text.
    await expect(next.dialog).toBeVisible()
    await expect(next.dialog.getByRole('alert')).toBeHidden()
    await expect(next.field).toHaveValue(spec.typed)
    await next.field.press('Enter')
    await expect(next.dialog).toBeHidden()
    await spec.shows(page, 2, spec.typed)
  })

  test(`project menu: a ${spec.what} saved and cancelled before the daemon answers does not close the dialog opened next on the same project`, async ({
    page,
  }) => {
    await open(page)
    await hold(page, spec.operation)
    const first = await typedIn(page, spec, 1, spec.saved)
    await first.field.press('Enter')
    await expect.poll(() => calls(page, spec.operation)).toEqual([spec.body(1, spec.saved)])
    await leave(page, spec, first.dialog)
    // The same dialog on the same project: an opening of its own, with its own text.
    const again = await typedIn(page, spec, 1, spec.typed)
    await letGo(page, spec.operation)
    await spec.shows(page, 1, spec.saved)
    await settled(page)
    await expect(again.dialog).toBeVisible()
    await expect(again.field).toHaveValue(spec.typed)
    await expect(again.field).toBeFocused()
    await again.field.press('Enter')
    await expect
      .poll(() => calls(page, spec.operation))
      .toEqual([spec.body(1, spec.saved), spec.body(1, spec.typed)])
    await expect(again.dialog).toBeHidden()
    await spec.shows(page, 1, spec.typed)
  })

  test(`project menu: a ${spec.what} saved and cancelled, not yet answered, does not keep the dialog opened next from saving`, async ({
    page,
  }) => {
    await open(page)
    await hold(page, spec.operation)
    const first = await typedIn(page, spec, 1, spec.saved)
    await first.field.press('Enter')
    await expect.poll(() => calls(page, spec.operation)).toEqual([spec.body(1, spec.saved)])
    await leave(page, spec, first.dialog)
    // harbour's save is still not answered: foundry's dialog saves all the same.
    const next = await typedIn(page, spec, 2, spec.typed)
    await next.field.press('Enter')
    await expect
      .poll(() => calls(page, spec.operation))
      .toEqual([spec.body(1, spec.saved), spec.body(2, spec.typed)])
    await expect(next.dialog).toBeHidden()
    await spec.shows(page, 2, spec.typed)
    expect(await answers(page, spec.operation)).toBe(1)
    // And harbour's answer, when it comes, is the list's as well.
    await letGo(page, spec.operation)
    await spec.shows(page, 1, spec.saved)
    await expect(page.locator('#status')).toHaveText(spec.told)
  })
}

// --- Project settings are for one project, from the moment they are asked for until they close. ---

/**
 * twoOpen's two projects with settings of their own in every section of Project settings, so that
 * a dialog that mixes the two shows it: staff (harbour's zeus is a worker and a reviewer, and
 * foundry has a zeus of its own), approval, specs and wake-up calls.
 */
function twoWithOwnSettings() {
  const data = twoOpen()
  const member = (id, handle, extra) => ({
    participant: { ...participant(id, handle, 'worker', extra), projectId: 2 },
    tasks: [],
    activity: { state: 'closed' },
    pane: null,
  })
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus').participant.roles = [
    'worker',
    'reviewer',
  ]
  data.boards[2].lanes.push(
    member(11, 'zeus'),
    member(12, 'athena', { harness: 'pi', tier: 'light' }),
  )
  data.boards[2].project.gate = true
  data.boards[1].project.specs = { on: true, folder: 'plans', variant: 'tdd' }
  data.boards[2].project.specs = { on: false, folder: 'docs', variant: 'core-html' }
  data.wakeUps = {
    1: { on: true, everyMs: 7_200_000, text: 'Harbour: anything left?' },
    2: { on: false, everyMs: 1_800_000, text: null },
  }
  return data
}

/** What each of those projects has, as the dialog of Project settings shows it. */
const HARBOURS = {
  staff: ['zeus', 'diana', 'zeus'],
  approval: false,
  specs: ['on', 'plans', 'tdd'],
  calls: [true, '2', 'hours', 'Harbour: anything left?'],
}
const FOUNDRYS = {
  staff: ['zeus', 'athena'],
  approval: true,
  specs: ['off', 'docs', 'core-html'],
  calls: [false, '30', 'minutes', DEFAULT_WAKE_UP_WORDS],
}

/** What the dialog shows in each of its sections now, whichever project it is for. */
const shownIn = (dialog) =>
  dialog.evaluate((node) => {
    const control = (name) => node.querySelector(`[name="${name}"]`)
    return {
      staff: [...node.querySelectorAll('#staff-members tr[data-handle]')].map(
        (row) => row.dataset.handle,
      ),
      approval: control('gate').checked,
      specs: [control('specs').value, control('specsFolder').value, control('specsVariant').value],
      calls: [
        control('wakeup-on').checked,
        control('wakeup-every').value,
        control('wakeup-unit').value,
        control('wakeup-text').value,
      ],
    }
  })

test('Project settings: asked for from a project’s menu while another is being chosen, whose board is on its way, they are the menu’s project’s, and that board is not drawn in them when it comes', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  // The human chooses foundry, whose board is on its way, and asks for harbour's settings from
  // harbour's menu: the project they asked for last is the one they are on.
  await hold(page, 'board.get')
  await chooseProject(page, 'foundry')
  const dialog = await dialogFrom(page, 1, 'Project settings…', 'Project settings')
  await expect(page.locator('#project-title')).toHaveText('harbour')
  expect(await shownIn(dialog)).toEqual(HARBOURS)
  // foundry's board comes, late: the page is on harbour, which is read again, and the dialog is
  // not drawn from a board that is not its own.
  await letGo(page, 'board.get')
  await settled(page)
  expect(await shownIn(dialog)).toEqual(HARBOURS)
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect(projectButton(page, 1)).toHaveAttribute('aria-current', 'true')
  await expect(projectButton(page, 2)).toHaveAttribute('aria-current', 'false')
  // What is saved there is harbour's, whole, as the dialog shows it.
  await dialog
    .getByRole('group', { name: 'Human approval', exact: true })
    .getByRole('checkbox')
    .check()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 1, gate: true }])
  await dialog.getByRole('button', { name: 'Save wake-up calls' }).click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([{ project: 1, on: true, every: 2, unit: 'hours', text: 'Harbour: anything left?' }])
})

test('Project settings: asked for a project the human has left before its calls came, they are not opened, and the calls that came show nowhere', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  // foundry's settings are asked for, and its calls are on their way.
  await hold(page, 'wakeup.get')
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect.poll(() => calls(page, 'wakeup.get')).toEqual([{ project: 2 }])
  // The human goes to harbour, whose board comes at once. The calls of foundry come after.
  await chooseProject(page, 'harbour')
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await letGo(page, 'wakeup.get')
  await expect.poll(() => answers(page, 'wakeup.get')).toBe(1)
  await settled(page)
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeHidden()
  // The next asked for is the project the human is on, with nothing of foundry's in it.
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog).toBeVisible()
  expect(await shownIn(dialog)).toEqual(HARBOURS)
  expect(await calls(page, 'wakeup.get')).toEqual([{ project: 2 }, { project: 1 }])
})

/** Every lane of the harbour's board runs on `harness`, the chief's too. */
function allOn(data, harness) {
  for (const lane of data.boards[1].lanes) {
    if (lane.participant.role !== 'human') lane.participant.harness = harness
  }
}

/**
 * What the Computer use section of Project settings shows (T-1266): each block in its order, a
 * harness's (by its word) or the permissions', with its heading, the status on its heading's
 * line, its muted lines, and for the permissions the columns (the apps) and each pane's row:
 * its name, each app's status, and its one button.
 */
const computerUse = (dialog) =>
  dialog.locator('fieldset.computer-use').evaluate((section) => {
    const text = (node) => node.textContent.replace(/\s+/g, ' ').trim()
    return [...section.querySelectorAll('.computer-use-block')]
      .filter((block) => block.closest('[hidden]') === null)
      .map((block) => {
        const shown = { block: block.dataset.harness ?? 'permissions' }
        shown.heading = text(block.querySelector('.computer-use-head'))
        const status = block.querySelector('.computer-use-title .computer-use-status')
        if (status !== null) shown.status = text(status)
        shown.lines = [...block.querySelectorAll('.dialog-help')]
          .filter((line) => line.closest('[hidden]') === null && !line.hidden)
          .map(text)
        const columns = [...block.querySelectorAll('thead th')].map(text).filter(Boolean)
        if (columns.length > 0) shown.columns = columns
        const rows = [...block.querySelectorAll('tbody tr')].map((row) =>
          [...row.children].map((cell) => text(cell)),
        )
        if (rows.length > 0) shown.rows = rows
        return shown
      })
  })

/** The distance from each block of the section to the next, in px. */
const computerUseGaps = (dialog) =>
  dialog.locator('fieldset.computer-use').evaluate((section) => {
    const blocks = [...section.querySelectorAll('.computer-use-block')].filter(
      (block) => block.closest('[hidden]') === null,
    )
    return blocks
      .slice(1)
      .map(
        (block, at) =>
          block.getBoundingClientRect().top - blocks[at].getBoundingClientRect().bottom,
      )
  })

/**
 * What Project settings and ConsensFlow's note say when a permission reads missing and
 * ConsensFlow is on in the list all the same (T-903): macOS keeps a grant for one copy of
 * the app, and the copy that was allowed may not be the installed one.
 */
const MEND =
  'If ConsensFlow is already on in that list, macOS may hold the grant for another copy of the app, so remove it with − and add /Applications/ConsensFlow.app with +.'

/** The words of ConsensFlow's own note of a missing permission, as the engine writes them. */
const noteAbout = (names) =>
  `Claude Code's computer use is on for this project, but ConsensFlow can't control the screen until you allow it ${names} in System Settings → Privacy & Security. ${MEND}`

const CLAUDE_OFF =
  'Turn it on in a Claude Code window of this project: /mcp → computer-use → Enable.'
const CLAUDE_ON = "It works through ConsensFlow's permissions, below."
const CODEX_SET_UP =
  "Set up in the ChatGPT app. It works through its own app's permissions, below, which ConsensFlow can't read."
const CODEX_NOT_SET_UP = 'Set it up in the ChatGPT app, then allow Codex Computer Use below.'
const KEPT =
  "These are the Mac's, for every project. A permission you change in System Settings may need ConsensFlow to be quit and opened again."
const OPEN = 'Open System Settings'
const ACCESSIBILITY = 'Accessibility'
const SCREEN = 'Screen & System Audio Recording'

test('Project settings: a project with no member on a harness with computer use, the chief included, shows no computer use', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'devin')
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeVisible()
  await expect(dialog.locator('fieldset.computer-use')).toBeHidden()
  expect(await calls(page, 'computer_use.get')).toEqual([{ project: 1 }])
})

test('Project settings: computer use appears with a Claude Code member and goes when the last one goes', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'devin')
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const section = dialog.getByRole('group', { name: 'Computer use' })
  await expect(dialog.locator('fieldset.computer-use')).toBeHidden()
  // A Claude Code worker joins: the section is there, while the dialog is open.
  await page.evaluate(() => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.role === 'worker')
    lane.participant.harness = 'claude-code'
    window.__listeners.get('state-changed')()
  })
  await expect(section).toBeVisible()
  await expect(section.locator('[data-harness="claude-code"] .computer-use-head')).toHaveText(
    'Claude Code',
  )
  // It leaves the staff: the section goes.
  await page.evaluate(() => {
    const board = window.__model.boards[1]
    board.lanes = board.lanes.filter((l) => l.participant.harness !== 'claude-code')
    window.__listeners.get('state-changed')()
  })
  await expect(dialog.locator('fieldset.computer-use')).toBeHidden()
  // A Claude Code chief alone is enough.
  await page.evaluate(() => {
    const chief = window.__model.boards[1].lanes.find((l) => l.participant.role === 'chief')
    chief.participant.harness = 'claude-code'
    window.__listeners.get('state-changed')()
  })
  await expect(section).toBeVisible()
})

test('Project settings: computer use says where Claude Code turns it on, and each macOS permission opens its pane', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = { permissions: { accessibility: true, screenRecording: false } }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'Off for this project',
      lines: [CLAUDE_OFF],
    },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [MEND, KEPT],
      columns: ['Permission', 'ConsensFlow'],
      rows: [
        [ACCESSIBILITY, 'allowed', OPEN],
        [SCREEN, 'not allowed', OPEN],
      ],
    },
  ])
  // Only what needs the human's hand is marked: the one that is not allowed.
  await expect(dialog.locator('.computer-use-status[data-state="missing"]')).toHaveText([
    'not allowed',
  ])
  await dialog
    .getByRole('button', { name: 'Open System Settings for Screen & System Audio Recording' })
    .click()
  await dialog.getByRole('button', { name: 'Open System Settings for Accessibility' }).click()
  await expect
    .poll(() => calls(page, 'computer_use.open'))
    .toEqual([{ pane: 'screen-recording' }, { pane: 'accessibility' }])
})

test('Project settings: computer use that is on says so, and on Windows says Claude Code has none there', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = { on: true, permissions: { accessibility: true, screenRecording: true } }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'On for this project',
      lines: [CLAUDE_ON],
    },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [KEPT],
      columns: ['Permission', 'ConsensFlow'],
      rows: [
        [ACCESSIBILITY, 'allowed', OPEN],
        [SCREEN, 'allowed', OPEN],
      ],
    },
  ])
  // No path of the project is shown: the section is of the project the dialog is for.
  await expect(dialog.locator('fieldset.computer-use')).not.toContainText('/work/harbour')
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.computerUse = { system: 'windows', permissions: null }
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'Not available on Windows',
      lines: [],
    },
  ])
  await expect(dialog.locator('.computer-use-permissions')).toBeHidden()
  await expect(dialog.locator('fieldset.computer-use').getByRole('button')).toHaveCount(0)
})

test("Project settings: a ConsensFlow started from another app says the permissions shown are that app's", async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = {
    grantsOf: 'com.apple.Terminal',
    permissions: { accessibility: true, screenRecording: false },
  }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'Off for this project',
      lines: [CLAUDE_OFF],
    },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [
        "This ConsensFlow was started from com.apple.Terminal, so these are that app's permissions, not ConsensFlow's.",
        MEND,
        KEPT,
      ],
      columns: ['Permission', 'ConsensFlow'],
      rows: [
        [ACCESSIBILITY, 'allowed', OPEN],
        [SCREEN, 'not allowed', OPEN],
      ],
    },
  ])
})

test("a member's note with the words of ConsensFlow's computer-use note gets no System Settings button", async ({
  page,
}) => {
  const data = model()
  data.inbox[1].push({
    id: 18,
    kind: 'note',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: null,
    body: noteAbout('Accessibility'),
    questions: null,
    createdAt: at(1),
  })
  await open(page, data)
  const note = page
    .getByRole('region', { name: 'For you' })
    .getByRole('list', { name: 'Notes for you' })
    .locator('li[data-message="18"]')
  await expect(note).toBeVisible()
  await expect(note.getByRole('button', { name: /Open System Settings/ })).toHaveCount(0)
})

test("Project settings: Codex's computer use says whether the ChatGPT app set it up, and leads to Codex Computer Use's own permissions", async ({
  page,
}) => {
  const data = model()
  allOn(data, 'codex')
  // Run from a terminal, ConsensFlow's permissions are that app's: said where they are shown, and
  // nowhere else, so not for Codex, whose block has none of ConsensFlow's. One of them is not
  // allowed, and the words on mending an entry (T-903) name ConsensFlow.app: they are not said
  // under a column that is another app's.
  data.computerUse = {
    grantsOf: 'com.apple.Terminal',
    permissions: { accessibility: true, screenRecording: false },
  }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  // ConsensFlow's own permissions are not Codex's: they are not shown for it, and Codex's are
  // read by nobody, so each says unknown, with its pane to open as every row has.
  expect(await computerUse(dialog)).toEqual([
    { block: 'codex', heading: 'Codex', status: 'Not set up', lines: [CODEX_NOT_SET_UP] },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [KEPT],
      columns: ['Permission', 'Codex Computer Use'],
      rows: [
        [ACCESSIBILITY, 'unknown', OPEN],
        [SCREEN, 'unknown', OPEN],
      ],
    },
  ])
  await dialog.getByRole('button', { name: 'Open System Settings for Accessibility' }).click()
  await dialog
    .getByRole('button', { name: 'Open System Settings for Screen & System Audio Recording' })
    .click()
  await expect
    .poll(() => calls(page, 'computer_use.open'))
    .toEqual([{ pane: 'accessibility' }, { pane: 'screen-recording' }])
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.computerUse = { codexSetUp: true, permissions: null }
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    { block: 'codex', heading: 'Codex', status: 'Set up', lines: [CODEX_SET_UP] },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [KEPT],
      columns: ['Permission', 'Codex Computer Use'],
      rows: [
        [ACCESSIBILITY, 'unknown', OPEN],
        [SCREEN, 'unknown', OPEN],
      ],
    },
  ])
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.computerUse = { system: 'windows', codexSetUp: false, permissions: null }
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    { block: 'codex', heading: 'Codex', status: 'Not available on Windows', lines: [] },
  ])
  await expect(dialog.locator('fieldset.computer-use').getByRole('button')).toHaveCount(0)
})

test('Project settings: a project with Claude Code and Codex members shows both blocks, and one permissions block with a column for each app and one button for each pane', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'codex')
  data.boards[1].lanes.find((l) => l.participant.role === 'chief').participant.harness =
    'claude-code'
  data.computerUse = {
    codexSetUp: true,
    permissions: { accessibility: false, screenRecording: true },
  }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'Off for this project',
      lines: [CLAUDE_OFF],
    },
    { block: 'codex', heading: 'Codex', status: 'Set up', lines: [CODEX_SET_UP] },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [MEND, KEPT],
      columns: ['Permission', 'ConsensFlow', 'Codex Computer Use'],
      rows: [
        [ACCESSIBILITY, 'not allowed', 'unknown', OPEN],
        [SCREEN, 'allowed', 'unknown', OPEN],
      ],
    },
  ])
  // The two panes both apps need are opened by one button each, not by one for each app.
  const section = dialog.locator('fieldset.computer-use')
  await expect(section.getByRole('button')).toHaveText([OPEN, OPEN])
  await section.getByRole('button', { name: 'Open System Settings for Accessibility' }).click()
  await section
    .getByRole('button', { name: 'Open System Settings for Screen & System Audio Recording' })
    .click()
  await expect
    .poll(() => calls(page, 'computer_use.open'))
    .toEqual([{ pane: 'accessibility' }, { pane: 'screen-recording' }])
})

test('Project settings: the blocks of computer use sit the same distance apart, however many there are', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'codex')
  data.boards[1].lanes.find((l) => l.participant.role === 'chief').participant.harness =
    'claude-code'
  data.computerUse = {
    codexSetUp: true,
    permissions: { accessibility: true, screenRecording: false },
  }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  const gaps = await computerUseGaps(dialog)
  expect(gaps).toHaveLength(2)
  // Room between them, the same each time.
  expect(gaps[0]).toBeGreaterThanOrEqual(8)
  expect(Math.abs(gaps[0] - gaps[1])).toBeLessThan(1)
  // Windows has the two blocks and no more, as far apart.
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.computerUse = { system: 'windows', permissions: null }
  })
  await page.getByRole('button', { name: 'Project settings' }).click()
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  const windows = await computerUseGaps(dialog)
  expect(windows).toHaveLength(1)
  expect(Math.abs(windows[0] - gaps[0])).toBeLessThan(1)
})

test('Project settings: the sections are the same distance apart and spaced alike inside, and each that saves does so with the same button in the same place', async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = { permissions: { accessibility: true, screenRecording: false } }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  const sections = await dialog.evaluate((node) => {
    const all = [...node.querySelectorAll('fieldset.project-section')].filter(
      (section) => section.closest('[hidden]') === null && !section.hidden,
    )
    return all.map((section, at) => {
      const box = section.getBoundingClientRect()
      const style = getComputedStyle(section)
      const save = [...section.querySelectorAll('button')].find((button) =>
        /^Save /.test(button.textContent),
      )
      const inner =
        box.right -
        Number.parseFloat(style.paddingRight) -
        Number.parseFloat(style.borderRightWidth)
      return {
        name: section.querySelector('legend').textContent,
        above: at === 0 ? null : box.top - all[at - 1].getBoundingClientRect().bottom,
        inside: style.rowGap,
        save:
          save === undefined
            ? null
            : {
                text: save.textContent,
                primary: save.classList.contains('primary-button'),
                inActions: save.parentElement.classList.contains('dialog-actions'),
                fromEdge: Math.round(inner - save.getBoundingClientRect().right),
              },
      }
    })
  })
  expect(sections.map(({ name }) => name)).toEqual([
    'Staff',
    'Rules for the staff',
    'Compaction',
    'Advisor tool',
    'Human approval',
    'Wake-up calls',
    'Specs',
    'Computer use',
  ])
  // The same room between the sections, and inside each, so no group runs into the next.
  expect(new Set(sections.slice(1).map(({ above }) => Math.round(above))).size).toBe(1)
  expect(sections[1].above).toBeGreaterThanOrEqual(12)
  expect(new Set(sections.map(({ inside }) => inside))).toEqual(new Set(['12px']))
  // A section that saves has one primary button, at the end of its own line of actions.
  const saves = sections.filter(({ save }) => save !== null).map(({ name, save }) => [name, save])
  expect(saves).toEqual([
    ['Rules for the staff', { text: 'Save rules', primary: true, inActions: true, fromEdge: 0 }],
    ['Compaction', { text: 'Save compaction', primary: true, inActions: true, fromEdge: 0 }],
    ['Advisor tool', { text: 'Save advisor tool', primary: true, inActions: true, fromEdge: 0 }],
    ['Wake-up calls', { text: 'Save wake-up calls', primary: true, inActions: true, fromEdge: 0 }],
    ['Specs', { text: 'Save specs', primary: true, inActions: true, fromEdge: 0 }],
  ])
})

test("Project settings: the compaction's two choices read as the approval's checkbox does, in the same colour and size", async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeVisible()
  const looks = (labels) =>
    labels.evaluateAll((all) =>
      all.map((label) => {
        const style = getComputedStyle(label)
        return [style.color, style.fontSize]
      }),
    )
  const [approval] = await looks(dialog.locator('label:has([name="gate"])'))
  const choices = await looks(
    dialog.getByRole('group', { name: 'Compaction' }).locator('label:has([type="radio"])'),
  )
  expect(choices).toEqual([approval, approval])
})

test("Project settings: a Claude Code member on a Mac whose permissions can't be read shows them unknown, each with its pane to open", async ({
  page,
}) => {
  const data = model()
  allOn(data, 'claude-code')
  data.computerUse = { on: true, permissions: null }
  await open(page, data)
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog.getByRole('group', { name: 'Computer use' })).toBeVisible()
  expect(await computerUse(dialog)).toEqual([
    {
      block: 'claude-code',
      heading: 'Claude Code',
      status: 'On for this project',
      lines: [CLAUDE_ON],
    },
    {
      block: 'permissions',
      heading: 'Permissions in System Settings',
      lines: [KEPT],
      columns: ['Permission', 'ConsensFlow'],
      rows: [
        [ACCESSIBILITY, 'unknown', OPEN],
        [SCREEN, 'unknown', OPEN],
      ],
    },
  ])
})

test("a note that ConsensFlow lacks computer use's permissions opens the System Settings pane of each it names", async ({
  page,
}) => {
  const data = model()
  data.inbox[1].push({
    id: 17,
    kind: 'note',
    state: 'queued',
    sender: null,
    recipient: 'human',
    taskNumber: null,
    body: noteAbout('Accessibility and Screen & System Audio Recording'),
    questions: null,
    createdAt: at(1),
  })
  await open(page, data)
  const note = page
    .getByRole('region', { name: 'For you' })
    .getByRole('list', { name: 'Notes for you' })
    .locator('li[data-message="17"]')
  await note.getByRole('button', { name: 'Open System Settings for Accessibility' }).click()
  await note
    .getByRole('button', { name: 'Open System Settings for Screen & System Audio Recording' })
    .click()
  await expect
    .poll(() => calls(page, 'computer_use.open'))
    .toEqual([{ pane: 'accessibility' }, { pane: 'screen-recording' }])
  // Any other note has no such button.
  const other = page.locator('li[data-message]:not([data-message="17"])')
  await expect(other.getByRole('button', { name: /Open System Settings/ })).toHaveCount(0)
})

test('a note of computer use that names one permission opens that pane alone, whatever else it says', async ({
  page,
}) => {
  const data = model()
  data.inbox[1].push({
    id: 19,
    kind: 'note',
    state: 'queued',
    sender: null,
    recipient: 'human',
    taskNumber: null,
    body: noteAbout('Accessibility'),
    questions: null,
    createdAt: at(1),
  })
  await open(page, data)
  const note = page
    .getByRole('region', { name: 'For you' })
    .getByRole('list', { name: 'Notes for you' })
    .locator('li[data-message="19"]')
  await expect(note).toContainText(MEND)
  await expect(note.getByRole('button', { name: /Open System Settings/ })).toHaveCount(1)
  await expect(
    note.getByRole('button', { name: 'Open System Settings for Accessibility' }),
  ).toBeVisible()
  // The permissions a note names are the ones it says to allow, not any name after them.
  await page.evaluate(
    (body) => {
      window.__model.inbox[1].push({
        id: 20,
        kind: 'note',
        state: 'queued',
        sender: null,
        recipient: 'human',
        taskNumber: null,
        body,
        questions: null,
        createdAt: 1,
      })
      window.__listeners.get('state-changed')()
    },
    `${noteAbout('Accessibility')} Screen & System Audio Recording has a list of its own.`,
  )
  const later = page
    .getByRole('region', { name: 'For you' })
    .getByRole('list', { name: 'Notes for you' })
    .locator('li[data-message="20"]')
  await expect(later).toBeVisible()
  await expect(later.getByRole('button', { name: /Open System Settings/ })).toHaveCount(1)
})

test('Project settings: asked for while the human chooses another project, whose board is on its way, they are not opened, nor when the human is back on the project asked for and its board is on its way', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const title = page.locator('#project-title')
  const settings = page.getByRole('button', { name: 'Project settings' })
  // harbour's settings are asked for; the human chooses foundry, whose board is on its way, and
  // the board shown is still harbour's when harbour's calls come.
  await hold(page, 'wakeup.get')
  await settings.click()
  await expect.poll(() => calls(page, 'wakeup.get')).toEqual([{ project: 1 }])
  await hold(page, 'board.get')
  await chooseProject(page, 'foundry')
  await expect(title).toHaveText('harbour')
  await letGo(page, 'wakeup.get')
  await expect.poll(() => answers(page, 'wakeup.get')).toBe(1)
  await settled(page)
  await expect(dialog).toBeHidden()
  await letGo(page, 'board.get')
  await expect(title).toHaveText('foundry')
  await expect(dialog).toBeHidden()
  // foundry's are asked for; the human goes to harbour, whose board comes, and back to foundry,
  // whose board is on its way again: the board shown is harbour's when foundry's calls come.
  await hold(page, 'wakeup.get')
  await settings.click()
  await expect.poll(() => calls(page, 'wakeup.get')).toHaveLength(2)
  await chooseProject(page, 'harbour')
  await expect(title).toHaveText('harbour')
  await hold(page, 'board.get')
  await chooseProject(page, 'foundry')
  await expect(title).toHaveText('harbour')
  await letGo(page, 'wakeup.get')
  await expect.poll(() => answers(page, 'wakeup.get')).toBe(2)
  await settled(page)
  await expect(dialog).toBeHidden()
  await letGo(page, 'board.get')
  await expect(title).toHaveText('foundry')
  await expect(dialog).toBeHidden()
})

test('Project settings: open for a project they show and save its own, whole, while the page is taken to another project', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeVisible()
  expect(await shownIn(dialog)).toEqual(HARBOURS)
  // The dialog is modal, and the human cannot choose a project under it. A page taken to
  // another all the same (a shortcut, a project gone from the list) leaves it as it is.
  await page.evaluate(() =>
    document.querySelector('li.project[data-project="2"] .project-select').click(),
  )
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await settled(page)
  expect(await shownIn(dialog)).toEqual(HARBOURS)
  // Every save is harbour's, by its id, whichever project the page shows.
  const section = (name) => dialog.getByRole('group', { name, exact: true })
  await section('Human approval').getByRole('checkbox').check()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 1, gate: true }])
  await section('Wake-up calls').getByLabel('Every').fill('3')
  await section('Wake-up calls').getByRole('button', { name: 'Save wake-up calls' }).click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([{ project: 1, on: true, every: 3, unit: 'hours', text: 'Harbour: anything left?' }])
  await section('Specs').locator('[name="specsFolder"]').fill('docs')
  await section('Specs').getByRole('button', { name: 'Save specs' }).click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 1, on: true, folder: 'docs', variant: 'tdd' }])
  await pickMember(dialog, { harness: 'Codex', model: 'gpt-6-astra', tier: 'critical' })
  await section('Staff').getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 1,
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: null,
        tier: 'critical',
        roles: ['worker'],
      },
    ])
  // foundry has a zeus too, with one role: it is harbour's, with two, that loses one.
  await dialog.getByRole('button', { name: 'Remove Reviewer @zeus' }).click()
  await expect.poll(() => calls(page, 'member.roles')).toHaveLength(1)
  expect((await calls(page, 'member.roles'))[0]).toEqual({
    project: 1,
    agent: 'zeus',
    roles: ['worker'],
  })
  // And the one that leaves the staff altogether is harbour's.
  await dialog.getByRole('button', { name: 'Remove Worker @diana' }).click()
  await dialog.getByRole('button', { name: 'Remove @diana', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ project: 1, agent: 'diana' }])
  await expect(page.locator('#project-title')).toHaveText('foundry')
})

test('Project settings: asked for twice while the calls are on their way, they open once, and the answer that comes second leaves what the human typed', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  const settings = page.getByRole('button', { name: 'Project settings' })
  // The first ask is held; the second is answered at once, and opens the dialog.
  await page.evaluate(() => {
    window.__hold['wakeup.get'] = new Promise((go) => {
      window.__first = go
    })
  })
  await settings.click()
  await expect.poll(() => calls(page, 'wakeup.get')).toHaveLength(1)
  await settings.click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  await expect(dialog).toBeVisible()
  const folder = dialog.locator('[name="specsFolder"]')
  await folder.fill('typed')
  // The first answer comes with the dialog open: it is for a dialog that is up already.
  await page.evaluate(() => window.__first())
  await expect.poll(() => answers(page, 'wakeup.get')).toBe(2)
  await settled(page)
  await expect(folder).toHaveValue('typed')
})

/** What the daemon says of the saves below when it refuses them (blank words, a folder outside). */
const REFUSED = {
  calls: 'the wake-up text must be text, not empty, at most 1000000 characters',
  specs: 'the specs folder is a folder inside the project: it stays inside it, with no `..`',
}

/**
 * harbour's calls (with `words`, which the daemon refuses when they are blank) and specs saved
 * and not yet answered, and the dialog left; with `next`, foundry's settings are asked for from
 * its menu then. Gives the dialog and its two sections.
 */
async function leftWithSavesOnTheirWay(page, words, { next = false } = {}) {
  await page.getByRole('button', { name: 'Project settings' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project settings' })
  const calling = dialog.getByRole('group', { name: 'Wake-up calls', exact: true })
  const specs = dialog.getByRole('group', { name: 'Specs', exact: true })
  await expect.poll(() => shownIn(dialog)).toEqual(HARBOURS)
  await hold(page, 'wakeup.set')
  await hold(page, 'project.specs')
  await calling.getByLabel('Every').fill('3')
  await calling.getByLabel('What it says').fill(words)
  await calling.getByRole('button', { name: 'Save wake-up calls' }).click()
  await specs.locator('[name="specsFolder"]').fill('late')
  await specs.getByRole('button', { name: 'Save specs' }).click()
  await expect.poll(() => calls(page, 'wakeup.set')).toHaveLength(1)
  await expect.poll(() => calls(page, 'project.specs')).toHaveLength(1)
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  if (next) {
    await dialogFrom(page, 2, 'Project settings…', 'Project settings')
    expect(await shownIn(dialog)).toEqual(FOUNDRYS)
  }
  return { dialog, calling, specs }
}

test('Project settings: a save answered after the dialog was left for another project’s shows nothing in that one, and the status line says it', async ({
  page,
}) => {
  // foundry's chief has no window open, harbour's has: what the save says is harbour's.
  const data = twoWithOwnSettings()
  data.boards[2].lanes.find((lane) => lane.participant.role === 'chief').pane = null
  await open(page, data)
  const left = await leftWithSavesOnTheirWay(page, 'Harbour: anything left?', { next: true })
  const { calling, specs, dialog } = left
  // harbour's answers come now: nothing of them is in foundry's dialog.
  await letGo(page, 'wakeup.set')
  await expect(page.locator('#status')).toHaveText(
    'Wake-up calls are on: the chief is asked every 3 hours.',
  )
  await letGo(page, 'project.specs')
  // harbour's chief has a window open: it is told now, though the dialog was left for foundry's
  // (whose chief has none) before the daemon answered.
  await expect(page.locator('#status')).toHaveText('Saved: the chief is told now.')
  await settled(page)
  expect(await shownIn(dialog)).toEqual(FOUNDRYS)
  await expect(specs.locator('.specs-note')).toBeHidden()
  // The next save from it is foundry's own, as its dialog shows it.
  await calling.getByRole('button', { name: 'Save wake-up calls' }).click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([
      { project: 1, on: true, every: 3, unit: 'hours', text: 'Harbour: anything left?' },
      { project: 2, on: false, every: 30, unit: 'minutes', text: DEFAULT_WAKE_UP_WORDS },
    ])
})

test('Project settings: a save the daemon refuses after the dialog was left for another project’s is said on the status line, and nowhere in that one', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  const { specs, calling, dialog } = await leftWithSavesOnTheirWay(page, '   ', { next: true })
  await page.evaluate((words) => {
    window.__model.specsRefusal = words
  }, REFUSED.specs)
  // A refusal is said as one, each on its own: not as a note.
  const status = page.locator('#status')
  await letGo(page, 'wakeup.set')
  await expect(status).toHaveText(REFUSED.calls)
  await expect(status).toHaveAttribute('data-tone', 'error')
  await letGo(page, 'project.specs')
  await expect(status).toHaveText(REFUSED.specs)
  await expect(status).toHaveAttribute('data-tone', 'error')
  await settled(page)
  expect(await shownIn(dialog)).toEqual(FOUNDRYS)
  await expect(calling.locator('.wakeup-hint')).toBeHidden()
  await expect(specs.locator('.specs-note')).toBeHidden()
})

test('Project settings: a save the daemon refuses after the dialog was left, with none opened since, is said on the status line, and not where nobody looks', async ({
  page,
}) => {
  await open(page, twoWithOwnSettings())
  await leftWithSavesOnTheirWay(page, '   ')
  await page.evaluate((words) => {
    window.__model.specsRefusal = words
  }, REFUSED.specs)
  const status = page.locator('#status')
  await letGo(page, 'wakeup.set')
  await expect(status).toHaveText(REFUSED.calls)
  await expect(status).toHaveAttribute('data-tone', 'error')
  await letGo(page, 'project.specs')
  await expect(status).toHaveText(REFUSED.specs)
  await expect(status).toHaveAttribute('data-tone', 'error')
  await settled(page)
  // The shut dialog has no refusal in it either: what it shows is read again when it opens.
  for (const words of ['.wakeup-hint', '.specs-note']) {
    await expect(page.locator(`#project-settings-dialog ${words}`)).toHaveJSProperty('hidden', true)
  }
})

// --- A closed project's settings open as an open project's do, and its board stays read-only. ---

/**
 * twoWithOwnSettings with foundry closed, as every project is after a start: no window, nothing
 * at work, its Resume waiting.
 */
function closedWithOwnSettings() {
  const data = twoWithOwnSettings()
  data.projects[1].state = 'suspended'
  const board = data.boards[2]
  board.project.state = 'suspended'
  for (const lane of board.lanes) {
    lane.activity = { state: 'closed' }
    lane.pane = null
  }
  board.resume = { tasks: 0, messages: 0 }
  return data
}

test('Project settings: open for a closed project from its menu and from the board’s button, and every section saves for it, while its board stays a closed project’s', async ({
  page,
}) => {
  await open(page, closedWithOwnSettings())
  const main = page.locator('main.main')
  const banner = page.getByRole('status').filter({ hasText: 'Waiting for Resume.' })
  // From its menu: foundry is shown, closed, and the dialog is its own.
  const dialog = await dialogFrom(page, 2, 'Project settings…', 'Project settings')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await expect(main).toHaveAttribute('data-suspended', 'true')
  expect(await shownIn(dialog)).toEqual(FOUNDRYS)
  const section = (name) => dialog.getByRole('group', { name, exact: true })
  // Its specs and its calls are saved as an open project's are, and asked of foundry.
  await section('Specs').locator('[name="specs"]').selectOption('on')
  await section('Specs').locator('[name="specsFolder"]').fill('plans')
  await section('Specs').getByRole('button', { name: 'Save specs' }).click()
  await expect
    .poll(() => calls(page, 'project.specs'))
    .toEqual([{ project: 2, on: true, folder: 'plans', variant: 'core-html' }])
  await section('Wake-up calls').getByRole('checkbox').check()
  await section('Wake-up calls').getByLabel('Every').fill('45')
  await section('Wake-up calls').getByRole('button', { name: 'Save wake-up calls' }).click()
  await expect
    .poll(() => calls(page, 'wakeup.set'))
    .toEqual([{ project: 2, on: true, every: 45, unit: 'minutes', text: DEFAULT_WAKE_UP_WORDS }])
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  // From the board's own button, with foundry shown: the same dialog, with what was saved in it.
  const button = page.getByRole('button', { name: 'Project settings' })
  await expect(button).toBeEnabled()
  await button.click()
  await expect(dialog).toBeVisible()
  expect(await shownIn(dialog)).toEqual({
    ...FOUNDRYS,
    specs: ['on', 'plans', 'core-html'],
    calls: [true, '45', 'minutes', DEFAULT_WAKE_UP_WORDS],
  })
  // Its approval and its staff are saved too, for foundry.
  await section('Human approval').getByRole('checkbox').uncheck()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 2, gate: false }])
  await pickMember(dialog, { harness: 'Codex', model: 'gpt-6-astra', tier: 'critical' })
  await section('Staff').getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      {
        project: 2,
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: null,
        tier: 'critical',
        roles: ['worker'],
      },
    ])
  await dialog.getByRole('button', { name: 'Remove Worker @athena' }).click()
  await dialog.getByRole('button', { name: 'Remove @athena', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ project: 2, agent: 'athena' }])
  await dialog.getByRole('button', { name: 'Close' }).click()
  // Nothing of it resumed the project: its board is read-only still, and its banner waits.
  await expect(main).toHaveAttribute('data-suspended', 'true')
  await expect(banner).toBeVisible()
  await expect(page.locator('.terminal-card')).toHaveCount(0)
  expect(await calls(page, 'project.resume')).toEqual([])
})

test("counts the human's unread notes on Inbox, and opens For you with it, from a folded board too", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 900 })
  const data = model()
  // An older ledger's question to the human is no note: For you shows none, Inbox counts none.
  data.inbox[1].push({
    id: 12,
    kind: 'question',
    state: 'queued',
    sender: 'chief',
    recipient: 'human',
    taskNumber: 1,
    body: 'Ship it?',
    questions: null,
    createdAt: at(2),
  })
  await open(page, data)
  const inbox = page.getByRole('button', { name: 'Inbox (1)' })
  await expect(inbox).toHaveAttribute('data-waiting', 'true')
  const board = page.getByRole('region', { name: 'Board' })
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect(board).toBeHidden()
  await inbox.click()
  await expect(board).toBeVisible()
  await expect(page.getByRole('region', { name: 'For you' })).toBeInViewport()
})

test('folds the board away to the left for the windows, and moves the divider with the keys', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 900 })
  await open(page)
  const board = page.getByRole('region', { name: 'Board' })
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const width = (locator) => locator.evaluate((node) => node.getBoundingClientRect().width)
  const room = await page.locator('.main').evaluate((node) => node.getBoundingClientRect().width)
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect(board).toBeHidden()
  await expect.poll(() => width(dock)).toBeGreaterThan(room - 40)
  // The board and the windows cannot both fold: folding the windows brings the board back.
  await dock.getByRole('button', { name: 'Hide terminals' }).click()
  await expect(board).toBeVisible()
  await page.getByRole('button', { name: 'Show terminals' }).click()
  // The divider, moved with the keys, sets the board's width, kept across a reload.
  const before = await width(board)
  const divider = page.getByRole('separator', { name: 'Board width' })
  await divider.focus()
  for (let press = 0; press < 3; press += 1) await divider.press('ArrowLeft')
  const near = async (locator) => Math.abs((await width(locator)) - (before - 96)) < 1.5
  await expect.poll(() => near(board)).toBe(true)
  await page.reload()
  await expect.poll(() => near(page.getByRole('region', { name: 'Board' }))).toBe(true)
  // It stops where the windows keep 300px.
  for (let press = 0; press < 40; press += 1) await page.getByRole('separator').press('ArrowRight')
  await expect
    .poll(() => width(page.getByRole('complementary', { name: 'Terminal dock' })))
    .toBeGreaterThanOrEqual(300)
})

test('folds the projects sidebar and the terminal dock away, and remembers it in this browser', async ({
  page,
}) => {
  await open(page)
  const projects = page.getByTestId('projects')
  const stage = page.getByRole('region', { name: 'Terminals' })
  await expect(projects).toBeVisible()
  await expect(stage).toBeVisible()
  // The icons sit in the panels' own header lines and stay when folded.
  const sidebar = page.getByRole('navigation', { name: 'Projects' })
  await sidebar.getByRole('button', { name: 'Hide projects' }).click()
  await expect(projects).toBeHidden()
  await expect(sidebar.getByRole('button', { name: 'Show projects' })).toBeVisible()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  // Folding the projects leaves the dock's header alone: at its right edge, its arrow pointing right.
  await expect
    .poll(() =>
      dock
        .locator('.dock-head')
        .evaluate((head) => [
          getComputedStyle(head).justifyContent,
          getComputedStyle(head.querySelector('.fold-icon svg')).transform,
        ]),
    )
    .toEqual(['flex-end', 'none'])
  await dock.getByRole('button', { name: 'Hide terminals' }).click()
  await expect(stage).toBeHidden()
  await page.reload()
  await expect(page.getByTestId('projects')).toBeHidden()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeHidden()
  await page.getByRole('button', { name: 'Show projects' }).click()
  await page.getByRole('button', { name: 'Show terminals' }).click()
  await expect(page.getByTestId('projects')).toBeVisible()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeVisible()
})

test('resizes no terminal when the projects fold away and come back: the board takes the room', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, { ...model(), realTerminals: true })
  const resizes = () =>
    page.evaluate(
      () =>
        window.__calls.filter(
          ([command, args]) => command === 'pane_resize' && args.id === 'p1-chief',
        ).length,
    )
  // The first size goes at once; it is the only one.
  await expect.poll(resizes).toBe(1)
  const widths = () =>
    page.evaluate(() => [
      document.querySelector('#board').getBoundingClientRect().width,
      document.querySelector('.dock').getBoundingClientRect().width,
    ])
  const [board, dock] = await widths()
  await page.getByRole('button', { name: 'Hide projects' }).click()
  await expect.poll(async () => (await widths())[0]).toBeGreaterThan(board + 100)
  expect((await widths())[1]).toBe(dock)
  await page.getByRole('button', { name: 'Show projects' }).click()
  await expect.poll(async () => (await widths())[0]).toBe(board)
  expect((await widths())[1]).toBe(dock)
  // Past the time a size settles in: still the first size alone.
  await page.waitForTimeout(600)
  expect(await resizes()).toBe(1)
})

test('folds the panels for this page when the browser keeps no storage', async ({ page }) => {
  // A browser that refuses storage (its data blocked, say) throws at every touch.
  await page.addInitScript(() => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      get() {
        throw new DOMException('The operation is insecure.', 'SecurityError')
      },
    })
  })
  await open(page)
  const projects = page.getByTestId('projects')
  const board = page.getByRole('region', { name: 'Board' })
  const stage = page.getByRole('region', { name: 'Terminals' })
  await page.getByRole('button', { name: 'Hide projects' }).click()
  await expect(projects).toBeHidden()
  await page.getByRole('button', { name: 'Show projects' }).click()
  await expect(projects).toBeVisible()
  // Folding the board, then the windows, brings the board back.
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect(board).toBeHidden()
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await expect(board).toBeVisible()
  await expect(stage).toBeHidden()
  await page.getByRole('button', { name: 'Show terminals' }).click()
  await expect(stage).toBeVisible()
  // Inbox unfolds the board for the notes.
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect(board).toBeHidden()
  await page.getByRole('button', { name: 'Inbox (1)' }).click()
  await expect(board).toBeVisible()
})

test('starts a project in a chosen folder with the chosen chief, the staff ticked from the last one', async ({
  page,
}) => {
  const data = model()
  data.lastStaff = [
    lastMember('zeus', 'claude-code', ['worker', 'reviewer']),
    lastMember('diana', 'codex', ['worker'], 'light'),
    lastMember('hera', 'codex', ['worker'], 'critical'),
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await expect(dialog.getByLabel('Project folder')).toHaveValue('/work/fresh')
  const table = dialog.getByRole('table', { name: 'Members for the staff' })
  // The last staff, one row per member and role, by role and then by tier, each named by its
  // model and saying what it runs on.
  await expect(table.locator('tbody tr')).toHaveText([
    /^gpt-6-astra.*Codex · max · critical.*Worker/,
    /^claude-sonnet-5.*Claude Code · high · standard.*Worker/,
    /^gpt-5.6-luna.*Codex · low · light.*Worker/,
    /^claude-sonnet-5.*Reviewer/,
  ])
  await expect(dialog.getByLabel('Second review of')).toHaveCount(0)
  await pickChief(dialog, 'Pi', 'meta/muse-spark')
  // A member as Add member picks one: a role, a harness, a model it lists and a tier.
  await pickMember(dialog, {
    role: 'advisor',
    harness: 'Pi',
    model: 'meta/muse-spark',
    tier: 'light',
  })
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  // The row of the member just picked names its model by its id, as its harness takes it (Pi's
  // with its provider), never by what the harness's list calls the model.
  await expect(
    table.locator('tbody tr', { hasText: 'Advisor' }).locator('.member-name'),
  ).toHaveText('meta/muse-spark')
  // An image designer draws on Codex's own model: it takes no model, effort or tier, and is
  // named by the default it runs on (designer P2).
  await pickDesigner(dialog)
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  const drawing = table.locator('tbody tr', { hasText: 'Image designer' })
  await expect(drawing.locator('.member-name')).toHaveText('Codex default')
  await expect(drawing.locator('.member-meta')).toHaveText('Codex')
  await dialog.getByRole('button', { name: 'Remove Worker gpt-5.6-luna' }).click()
  await expect(table.locator('tbody tr')).toHaveCount(5)
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'pi',
        model: 'meta/muse-spark',
        gate: false,
        compaction: 300000,
        staff: [
          {
            harness: 'claude-code',
            model: 'claude-sonnet-5',
            effort: 'high',
            tier: 'standard',
            roles: ['worker', 'reviewer'],
          },
          {
            harness: 'codex',
            model: 'gpt-6-astra',
            effort: 'max',
            tier: 'critical',
            roles: ['worker'],
          },
          {
            harness: 'pi',
            model: 'meta/muse-spark',
            effort: null,
            tier: 'light',
            roles: ['advisor'],
          },
          { harness: 'codex', roles: ['designer'] },
        ],
      },
    ])
})

test('New project keeps an image designer a member of its own beside a worker, in either order', async ({
  page,
}) => {
  await open(page)
  for (const order of [
    ['worker', 'designer'],
    ['designer', 'worker'],
  ]) {
    await page.getByRole('button', { name: 'New project' }).click()
    const dialog = page.getByRole('dialog', { name: 'New project' })
    await pickChief(dialog, 'Codex', 'gpt-6-astra')
    for (const role of order) {
      if (role === 'designer') await pickDesigner(dialog)
      else {
        await pickMember(dialog, { role, harness: 'Codex', model: 'gpt-5.6-luna', tier: 'light' })
      }
      await dialog.getByRole('button', { name: 'Add', exact: true }).click()
    }
    await dialog.getByRole('button', { name: 'Start project' }).click()
    const one = (role) =>
      role === 'designer'
        ? { harness: 'codex', roles: ['designer'] }
        : { harness: 'codex', model: 'gpt-5.6-luna', effort: null, tier: 'light', roles: [role] }
    await expect
      .poll(async () => (await calls(page, 'project.open')).at(-1)?.staff)
      .toEqual(order.map(one))
    await page.evaluate(() => {
      window.__calls = []
    })
  }
})

test("New project carries the last staff's image designer as it runs: on Codex's own model, in no tier", async ({
  page,
}) => {
  const data = model()
  data.lastStaff = [
    lastMember('zeus', 'claude-code', ['worker']),
    // As the daemon reads a designer, whatever model its row once held: none, no effort.
    {
      harness: 'codex',
      model: null,
      modelName: null,
      effort: null,
      designer: true,
      tier: 'light',
      role: 'designer',
      roles: ['designer'],
    },
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const table = dialog.getByRole('table', { name: 'Members for the staff' })
  await expect(table.locator('tbody tr')).toHaveText([
    /^claude-sonnet-5.*Worker/,
    /^Codex default.*Codex.*Image designer/,
  ])
  await pickChief(dialog, 'Codex', 'gpt-6-astra')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(async () => (await calls(page, 'project.open')).at(-1)?.staff)
    .toEqual([
      {
        harness: 'claude-code',
        model: 'claude-sonnet-5',
        effort: 'high',
        tier: 'standard',
        roles: ['worker'],
      },
      { harness: 'codex', roles: ['designer'] },
    ])
})

test("docks the chief's terminal and a member's own, offers no Open terminal for them, and feeds them their output", async ({
  page,
}) => {
  await open(page)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'chief',
  )
  await expect(page.getByRole('button', { name: "Show @zeus's terminal" })).toHaveCount(0)
  await expect(page.getByRole('button', { name: "Show Chief of Staff's terminal" })).toHaveCount(0)
  // A member without a window of its own is a heading: nothing to view on its row.
  await expect(page.locator('tr[data-handle="diana"] .row-tools button')).toHaveCount(0)
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-zeus', generation: 7, seq: 1, bytes: [104, 105] }),
  )
  await expect
    .poll(() =>
      page.evaluate(() => window.__calls.filter(([c]) => c === 'pane_ack').map(([, a]) => a)),
    )
    .toEqual([{ id: 'p1-zeus', generation: 7, seq: 1 }])
  const subscriptions = await page.evaluate(
    () => window.__calls.filter(([c]) => c === 'subscribe_output').length,
  )
  expect(subscriptions).toBe(1)
})

/** The stand-in emulator of the card of `handle` in the dock, by its index in `__emulators`. */
const emulatorOf = (page, handle) =>
  page.evaluate((handle) => {
    const card = document.querySelector(`#stage .terminal-card[data-handle="${handle}"]`)
    return window.__emulators.findIndex((e) => card?.contains(e.host) && !e.disposed)
  }, handle)

test('writes each message to its terminal as it comes and acknowledges up to the last one drawn', async ({
  page,
}) => {
  await open(page)
  const zeus = await emulatorOf(page, 'zeus')
  expect(zeus).toBeGreaterThanOrEqual(0)
  // The terminal is slow to draw (a page throttled behind a hidden window or a locked
  // screen draws once a tick): what comes meanwhile goes to it at once, not one message
  // a tick behind an acknowledgement each.
  await page.evaluate(() => {
    window.__writeGate = new Promise((open) => {
      window.__openWrites = open
    })
    for (const seq of [1, 2, 3]) {
      window.__output.onmessage({ id: 'p1-zeus', generation: 7, seq, bytes: [96 + seq] })
    }
  })
  await expect.poll(() => page.evaluate((i) => window.__emulators[i].writes, zeus)).toBe(3)
  expect(await acked(page, 'p1-zeus'), 'nothing acknowledged before it is drawn').toEqual([])
  await page.evaluate(() => window.__openWrites())
  // One acknowledgement frees everything up to its number: the last drawn is acknowledged,
  // with no acknowledgement a message.
  await expect.poll(async () => (await acked(page, 'p1-zeus')).at(-1)).toBe(3)
  const acks = await acked(page, 'p1-zeus')
  expect(acks.length, `${acks}`).toBeLessThan(3)
  expect(acks, 'in order').toEqual([...acks].sort((a, b) => a - b))
  expect(await page.evaluate((i) => window.__emulators[i].written, zeus)).toEqual([97, 98, 99])
})

test('acknowledges what comes for a window the page let go of while it still runs, and draws none of it', async ({
  page,
}) => {
  await open(page)
  // The board says zeus's window is gone; its program, still running, prints on. Unread
  // and unacknowledged, its output would hold the window to a stop once 1 MiB of it is owed.
  await page.evaluate(() => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
    lane.pane = null
    lane.activity = { state: 'closed' }
    window.__listeners.get('state-changed')()
  })
  await expect.poll(() => disposed(page)).toBe(1)
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-zeus', generation: 7, seq: 4, bytes: [104, 105] }),
  )
  await expect.poll(() => acked(page, 'p1-zeus')).toEqual([4])
  const drawn = await page.evaluate(() =>
    window.__emulators.filter((e) => e.disposed).map((e) => e.written.length),
  )
  expect(drawn).toEqual([0])
})

/** Both projects open, each with its chief's window live; harbour as `data` has it. */
function twoOpen(data = model()) {
  data.projects[1].state = 'open'
  data.boards[2].project.state = 'open'
  data.boards[2].lanes.push({
    participant: { ...participant(10, 'chief', 'chief'), projectId: 2 },
    tasks: [],
    activity: { state: 'idle' },
    pane: { id: 'p2-chief', generation: 1 },
  })
  return data
}

const chooseProject = (page, name) => page.locator('.project-select', { hasText: name }).click()

/**
 * Whether a window is still served: output for it is acknowledged, and keys
 * typed into its card in the dock (found by `handle`) reach its pane.
 */
async function served(page, pane, handle) {
  const count = (command) =>
    page.evaluate(
      ([command, id]) =>
        window.__calls.filter(([c, args]) => c === command && args.id === id).length,
      [command, pane.id],
    )
  const [acks, keys] = [await count('pane_ack'), await count('pane_input_enqueue')]
  await page.evaluate(
    ({ id, generation }) =>
      window.__output.onmessage({ id, generation, seq: Date.now(), bytes: [104, 105] }),
    pane,
  )
  const typed = await page.evaluate((handle) => {
    const card = document.querySelector(`#stage .terminal-card[data-handle="${handle}"]`)
    const emulator = window.__emulators.find((e) => card?.contains(e.host) && !e.disposed)
    emulator?.type('x')
    return emulator !== undefined
  }, handle)
  await expect.poll(() => count('pane_ack')).toBeGreaterThan(acks)
  expect(typed, `a live emulator in ${handle}'s card`).toBe(true)
  await expect.poll(() => count('pane_input_enqueue')).toBeGreaterThan(keys)
}

const disposed = (page) =>
  page.evaluate(() => window.__emulators.filter((emulator) => emulator.disposed).length)

test("keeps each project's terminals, scrollback and all, when the human switches projects", async ({
  page,
}) => {
  await open(page, twoOpen())
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  const emulators = () => page.evaluate(() => window.__emulators.length)
  const before = await emulators()
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-chief', generation: 5, seq: 1, bytes: [104, 105] }),
  )
  await page.locator('.project-select', { hasText: 'foundry' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await expect(dock.locator('.terminal-card[data-handle="chief"]')).toHaveCount(1)
  await page.locator('.project-select', { hasText: 'harbour' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  expect(await emulators()).toBe(before + 1, "only foundry's chief got a new terminal")
  expect(
    await page.evaluate(() => window.__emulators.some((emulator) => emulator.written.length > 0)),
  ).toBe(true)
})

test('draws a board only under its own project: one that comes late ends no live window', async ({
  page,
}) => {
  await open(page, twoOpen())
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  // Each project shown once, so each has its cards.
  await chooseProject(page, 'foundry')
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await chooseProject(page, 'harbour')
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  // A redraw of harbour waits on its board (a busy core), and the human
  // chooses foundry meanwhile.
  await page.evaluate(() => {
    window.__delay['board.get'] = 300
  })
  const boards = (await calls(page, 'board.get')).length
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(boards)
  await chooseProject(page, 'foundry')
  await page.evaluate(() => {
    window.__delay['board.get'] = 0
  })
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await page.waitForTimeout(400)
  expect(await disposed(page), 'no live window was retired').toBe(0)
  await served(page, { id: 'p2-chief', generation: 1 }, 'chief')
  await chooseProject(page, 'harbour')
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await served(page, { id: 'p1-chief', generation: 5 }, 'chief')
  await served(page, { id: 'p1-zeus', generation: 7 }, 'zeus')
  expect(await disposed(page)).toBe(0)
})

test('starts a project while the board shown is on its way, and its windows go on', async ({
  page,
}) => {
  const data = model()
  data.boards[3] = {
    project: { id: 3, name: 'new', directory: '/work/fresh', state: 'open' },
    open: [],
    lanes: [
      {
        participant: { ...participant(30, 'chief', 'chief'), projectId: 3 },
        tasks: [],
        activity: { state: 'starting' },
        pane: { id: 'p3-chief', generation: 1 },
      },
    ],
  }
  await open(page, data)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  // The project takes a moment to open; a change it causes meanwhile brings
  // a redraw of harbour, whose board answers slowly.
  await page.evaluate(() => {
    window.__delay['project.open'] = 300
    window.__delay['board.get'] = 250
    setTimeout(() => {
      window.__model.projects.push({
        id: 3,
        name: 'new',
        directory: '/work/fresh',
        state: 'open',
      })
      window.__listeners.get('state-changed')()
    }, 100)
  })
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await pickChief(dialog, 'Claude Code', 'claude-sonnet-5')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect(page.locator('#project-title')).toHaveText('new')
  await page.evaluate(() => {
    window.__delay = {}
  })
  await page.waitForTimeout(400)
  expect(await disposed(page), 'no live window was retired').toBe(0)
  await chooseProject(page, 'harbour')
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await served(page, { id: 'p1-chief', generation: 5 }, 'chief')
  await served(page, { id: 'p1-zeus', generation: 7 }, 'zeus')
})

/** Fires a state change and waits for the redraw it brings to have read `project`'s board. */
async function redrawn(page, project) {
  const reads = () =>
    page.evaluate(
      (project) =>
        window.__calls.filter(
          ([command, args]) =>
            command === 'daemon_request' &&
            args.operation === 'board.get' &&
            args.body.project === project,
        ).length,
      project,
    )
  const before = await reads()
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(reads).toBeGreaterThan(before)
}

test("lets a window of a project not shown go when that project's own board says it ended", async ({
  page,
}) => {
  const data = twoOpen()
  const foundry = data.boards[2]
  const member = { ...participant(11, 'zeus', 'worker'), projectId: 2 }
  foundry.lanes.push(
    { participant: member, tasks: [], activity: { state: 'closed' }, pane: null },
    {
      participant: { ...session(12, member, 'amber-pine'), projectId: 2 },
      tasks: [],
      activity: { state: 'working' },
      pane: { id: 'p2-zeus-amber-pine', generation: 3 },
    },
  )
  await open(page, data)
  // Nothing of foundry's in the dock yet: only the board shown is read.
  await redrawn(page, 1)
  expect(await calls(page, 'board.get')).not.toContainEqual({ project: 2 })
  // foundry's session prints while harbour is shown: its window gets an
  // emulator, off screen, and foundry's board is read to say whose it is.
  const emulators = await page.evaluate(() => window.__emulators.length)
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p2-zeus-amber-pine', generation: 3, seq: 1, bytes: [104] }),
  )
  await expect.poll(() => page.evaluate(() => window.__emulators.length)).toBe(emulators + 1)
  await redrawn(page, 2)
  expect(await disposed(page)).toBe(0)
  // The task done, the window closes; the human is still on harbour.
  await page.evaluate(() => {
    const lane = window.__model.boards[2].lanes.find(
      (l) => l.participant.handle === 'zeus-amber-pine',
    )
    lane.pane = null
    lane.activity = { state: 'closed' }
  })
  await redrawn(page, 2)
  await expect.poll(() => disposed(page)).toBe(1)
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await served(page, { id: 'p1-chief', generation: 5 }, 'chief')
})

test('takes the windows of a project closed or deleted while not shown out of the dock', async ({
  page,
}) => {
  await open(page, twoOpen())
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  // foundry shown once: its chief's window has its card, kept off screen.
  await chooseProject(page, 'foundry')
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await chooseProject(page, 'harbour')
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  // foundry is closed (from another of the human's windows, say): its windows go.
  await page.evaluate(() => {
    const { projects, boards } = window.__model
    projects[1].state = 'suspended'
    boards[2].project.state = 'suspended'
    for (const lane of boards[2].lanes) lane.pane = null
    window.__listeners.get('state-changed')()
  })
  await expect.poll(() => disposed(page)).toBe(1)
  // Resumed, its chief has a new window; then the project is deleted.
  await page.evaluate(() => {
    const { projects, boards } = window.__model
    projects[1].state = 'open'
    boards[2].project.state = 'open'
    boards[2].lanes.find((lane) => lane.participant.handle === 'chief').pane = {
      id: 'p2-chief',
      generation: 2,
    }
  })
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p2-chief', generation: 2, seq: 1, bytes: [104] }),
  )
  await redrawn(page, 2)
  expect(await disposed(page)).toBe(1)
  await page.evaluate(() => {
    window.__model.projects.splice(1, 1)
    window.__listeners.get('state-changed')()
  })
  await expect.poll(() => disposed(page)).toBe(2)
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await served(page, { id: 'p1-chief', generation: 5 }, 'chief')
})

test("takes a closed window's card out of the dock: its lane says so and opens it again", async ({
  page,
}) => {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  Object.assign(zeusLane, { tasks: [], activity: { state: 'closed' }, pane: null })
  data.boards[1].lanes.push({
    participant: session(20, zeusLane.participant, 'amber-pine'),
    tasks: [task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 9 },
  })
  await open(page, data)
  await unfold(page)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')).toHaveCount(1)
  // Its task accepted, the window closes: a last frame left in the dock
  // would still show the agent's prompt and read as open.
  await page.evaluate(() => {
    const lane = window.__model.boards[1].lanes.find(
      (l) => l.participant.handle === 'zeus-amber-pine',
    )
    lane.tasks[0].state = 'accepted'
    lane.pane = null
    lane.activity = { state: 'closed' }
    window.__listeners.get('state-changed')()
  })
  await expect(dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')).toHaveCount(0)
  await expect(dock.locator('.terminal-card[data-handle="chief"]')).toHaveAttribute(
    'data-focused',
    'true',
  )
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect(row.locator('.row-status')).toHaveCount(0)
  await expect(row.getByTestId('lamp')).toHaveAttribute('data-state', 'closed')
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  // The chief's window opens and closes with the project: its card has no Open, only its
  // History, Switch chief and Start meeting.
  await expect
    .poll(() => named(page.locator('#stage .terminal-card[data-handle="chief"] button')))
    .toEqual(['History', 'Switch chief', 'Start meeting'])
})

/** A message for the chief that its meeting with the human holds: queued, as the daemon lists it. */
const heldForChief = (id, kind, sender, taskNumber, body) => ({
  id,
  kind,
  state: 'queued',
  sender,
  recipient: 'chief',
  taskNumber,
  body,
  questions: null,
  choices: null,
  createdAt: at(2),
})

test('meets the chief from its card: Start holds what comes for it, the button counts it, its card and For you list it, and Stop lets it go', async ({
  page,
}) => {
  const data = model()
  // An approval project: what waits for the human's approval is a list of its own.
  data.boards[1].project.gate = true
  data.boards[1].gated = [
    {
      id: 30,
      kind: 'task',
      state: 'gated',
      sender: 'chief',
      recipient: 'zeus',
      taskNumber: 4,
      body: 'Add the tests',
      questions: null,
      choices: null,
      createdAt: at(3),
    },
  ]
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  const status = card.locator('.terminal-status')
  await card.getByRole('button', { name: 'Start meeting' }).click()
  await expect.poll(() => calls(page, 'project.meeting')).toEqual([{ project: 1, on: true }])
  await expect
    .poll(() => named(card.locator('button')))
    .toEqual(['History', 'Switch chief', 'Stop meeting (0)'])
  await expect(status).toHaveText('In a meeting · nothing held yet')
  await expect(status).toHaveAttribute('data-state', 'meeting')
  // What comes for the chief now waits, as the daemon's next board says: a member's question
  // first, then the oldest.
  await page.evaluate(
    (held) => {
      window.__model.boards[1].held = held
      window.__listeners.get('state-changed')()
    },
    [
      heldForChief(41, 'question', 'zeus', 3, 'Which parser?'),
      heldForChief(40, 'result', 'zeus', 2, 'Lexer done; 14 tests pass.'),
    ],
  )
  await expect
    .poll(() => named(card.locator('button')))
    .toEqual(['History', 'Switch chief', 'Stop meeting (2)'])
  await expect(status).toHaveText('In a meeting · 2 messages held')
  const held = page.getByRole('list', { name: 'Held for your meeting' })
  await expect
    .poll(() => held.locator('.strip').evaluateAll((all) => all.map((s) => s.dataset.message)))
    .toEqual(['41', '40'])
  await expect(held.locator('.strip-route')).toHaveText([
    'Question from @zeus · T-3 · held',
    'Result from @zeus · T-2 · held',
  ])
  // A held message only reads: marked read, it would never reach the chief.
  await expect.poll(() => named(held.locator('button'))).toEqual(['Open task', 'Open task'])
  // What waits for approval stays apart, and each is counted once.
  const waiting = page.getByRole('list', { name: 'Waiting for you' })
  await expect
    .poll(() => waiting.locator('.strip').evaluateAll((all) => all.map((s) => s.dataset.message)))
    .toEqual(['30'])
  await expect(page.locator('.foryou-status')).toHaveText('1 waiting · 2 held · 1 note')

  await card.getByRole('button', { name: 'Stop meeting (2)' }).click()
  await expect
    .poll(() => calls(page, 'project.meeting'))
    .toEqual([
      { project: 1, on: true },
      { project: 1, on: false },
    ])
  await expect
    .poll(() => named(card.locator('button')))
    .toEqual(['History', 'Switch chief', 'Start meeting'])
  await expect(held).toHaveCount(0)
  await expect(status).toHaveText('Working')
  await expect(page.locator('.foryou-status')).toHaveText('1 waiting · 1 note')
})

test("keeps the chief's head inside its card in a meeting, however narrow, and draws no meeting on a closed project", async ({
  page,
}) => {
  const data = closedFoundry()
  data.boards[1].project.meeting = at(5)
  data.boards[1].held = [
    heldForChief(41, 'question', 'zeus', 3, 'Which parser?'),
    heldForChief(40, 'result', 'zeus', 2, 'Lexer done; 14 tests pass.'),
  ]
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.getByRole('button', { name: 'Stop meeting (2)' })).toBeVisible()
  // Every part of the head inside the card: the head wraps (name and buttons, then what the
  // chief is doing), and nothing runs past its edge.
  const outside = () =>
    card.evaluate((card) => {
      const edge = card.getBoundingClientRect()
      return [...card.querySelectorAll('.terminal-head > *, .terminal-head button')]
        .filter((part) => {
          const box = part.getBoundingClientRect()
          return box.left < edge.left || box.right > edge.right
        })
        .map((part) => part.textContent)
    })
  // The chief's buttons keep together, each line of them ending at the head's end: wrapped
  // one by one, History stayed by the name and the other two went under it, at the left.
  const shortLines = () =>
    card.evaluate((card) => {
      const ends = new Map()
      for (const button of card.querySelectorAll('.terminal-head button')) {
        const box = button.getBoundingClientRect()
        const line = Math.round(box.top)
        ends.set(line, Math.max(ends.get(line) ?? 0, box.right))
      }
      const end = Math.max(...ends.values())
      return [...ends.values()].filter((right) => right < end - 1).length
    })
  for (const width of [700, 880, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    await expect.poll(outside, { message: `the head at ${width}px` }).toEqual([])
    await expect.poll(shortLines, { message: `the buttons' lines at ${width}px` }).toBe(0)
  }
  // A closed project draws no card: nothing on it acts, and its Close ended any meeting.
  await chooseProject(page, 'foundry')
  await expect(page.locator('main.main')).toHaveAttribute('data-suspended', 'true')
  await expect(page.getByRole('button', { name: /meeting/ })).toHaveCount(0)
  await expect(page.getByRole('list', { name: 'Held for your meeting' })).toHaveCount(0)
})

/**
 * harbour as the daemon has it while a worker is at work: zeus between tasks,
 * and one session of it with a live window beside the chief's.
 */
function atWork() {
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  Object.assign(zeusLane, { tasks: [], activity: { state: 'closed' }, pane: null })
  data.boards[1].lanes.push({
    participant: session(20, zeusLane.participant, 'amber-pine'),
    tasks: [task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 9 },
  })
  return data
}

/** Every member's row unfolded, its sessions' rows under it, as the human unfolds them. */
async function unfold(page) {
  const folded = page.locator('button.fold-sessions[aria-expanded="false"]')
  while ((await folded.count()) > 0) await folded.first().click()
}

/** The handles of the terminals in the dock, in their order there. */
const docked = (page) =>
  page
    .locator('#stage .terminal-card')
    .evaluateAll((cards) => cards.map((card) => card.dataset.handle))

/** The output of pane `id` the page acknowledged, by its sequence numbers. */
const acked = (page, id) =>
  page.evaluate(
    (id) =>
      window.__calls
        .filter(([command, args]) => command === 'pane_ack' && args.id === id)
        .map(([, args]) => args.seq),
    id,
  )

test("keeps a session's terminal out of the dock until the human shows it, and the chief's in it", async ({
  page,
}) => {
  const data = atWork()
  const zeus = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus').participant
  zeus.roles = ['worker', 'reviewer']
  // A reviewer's window waits for the human: its row says so, and that is all.
  data.boards[1].lanes.push({
    participant: session(22, zeus, 'brisk-birch', 'reviewer'),
    tasks: [
      task(23, 'Review T-21', 'working', 'chief', 'zeus-brisk-birch', 2, {
        pool: 'reviewer',
        tier: 'standard',
      }),
    ],
    activity: { state: 'waiting', reason: 'permission to run a command' },
    pane: { id: 'p1-zeus-brisk-birch', generation: 2 },
  })
  data.tasks['1:21'] = {
    ...task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3),
    messages: [],
  }
  await open(page, data)
  await unfold(page)
  await expect.poll(() => docked(page)).toEqual(['chief'])
  const worker = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect(worker.locator('.row-status')).toHaveText('Working')
  await expect.poll(() => toolsOf(worker)).toEqual(['Show terminal', 'Delete session'])
  const reviewer = page.locator('tr[data-handle="zeus-brisk-birch"]')
  await expect(reviewer.locator('.row-status')).toHaveText('Waiting: permission to run a command')
  await expect(
    reviewer.getByRole('button', { name: "Show @zeus · brisk-birch's terminal" }),
  ).toBeVisible()
  // The chief's terminal has nothing to show or hide, on its row or its card:
  // its row has no tools, and its card reads its history, switches the chief and meets it.
  await expect(page.locator('tr[data-handle="chief"] .row-head button')).toHaveCount(0)
  await expect
    .poll(() => named(page.locator('#stage .terminal-card[data-handle="chief"] button')))
    .toEqual(['History', 'Switch chief', 'Start meeting'])
  // Out of the dock, a session's window is fed all the same: what it prints
  // is taken and acknowledged, so it never waits for a reader.
  expect(await page.evaluate(() => window.__emulators.length)).toBe(3)
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-zeus-amber-pine', generation: 9, seq: 1, bytes: [104] }),
  )
  await expect.poll(() => acked(page, 'p1-zeus-amber-pine')).toEqual([1])
  // Reading its task is not asking to see its window; its drawer's Show terminal is.
  await worker.locator('button.card[data-task="21"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-21' })
  await expect(drawer).toBeVisible()
  expect(await docked(page)).toEqual(['chief'])
  await drawer.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  // The drawer covers the dock: it closes, and the task's window is in front.
  await expect(drawer).toBeHidden()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await expect(
    page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]'),
  ).toHaveAttribute('data-focused', 'true')
  expect(await calls(page, 'session.open')).toEqual([])
})

test("opens a task's closed window from its drawer, and offers none for a task no window has", async ({
  page,
}) => {
  const data = atWork()
  const zeus = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus').participant
  data.boards[1].lanes.push({
    participant: session(22, zeus, 'brisk-birch'),
    tasks: [task(23, 'Write the docs', 'done', 'chief', 'zeus-brisk-birch', 30)],
    activity: { state: 'closed' },
    pane: null,
  })
  data.tasks['1:23'] = {
    ...task(23, 'Write the docs', 'done', 'chief', 'zeus-brisk-birch', 30),
    messages: [],
  }
  data.tasks['1:6'] = {
    ...task(6, 'Write the docs', 'open', 'chief', null, 1, { pool: 'worker', tier: 'standard' }),
    messages: [],
  }
  await open(page, data)
  await unfold(page)
  await page.locator('tr[data-handle="zeus-brisk-birch"] button.card[data-task="23"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-23' })
  await drawer.getByRole('button', { name: "Show @zeus · brisk-birch's terminal" }).click()
  await expect(drawer).toBeHidden()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-brisk-birch' }])
  // A task on the board for any worker has no window yet.
  await page.locator('button.card[data-task="6"]').click()
  const open6 = page.getByRole('complementary', { name: 'Task T-6' })
  await expect(open6).toBeVisible()
  await expect(open6.getByRole('button', { name: /terminal/ })).toHaveCount(0)
})

test("shows a session's terminal in front, the dock unfolded, and hides it again from its row or its card", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  const card = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
  // Asked for with the dock folded away, it unfolds the dock.
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeVisible()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await expect(card).toHaveAttribute('data-focused', 'true')
  await expect.poll(() => toolsOf(row)).toEqual(['Hide terminal', 'Delete session'])
  // Hidden from its row: the card goes and the chief's is in front again.
  await row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await expect(page.locator('#stage .terminal-card[data-handle="chief"]')).toHaveAttribute(
    'data-focused',
    'true',
  )
  await expect(
    row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }),
  ).toBeVisible()
  // Shown again, and hidden from its own card.
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await card.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await expect(
    row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }),
  ).toBeVisible()
  // Showing a window that is open asks the daemon nothing; each Hide, from the
  // row and from the card, tells it, and it closes the window once it is free.
  // This one holds a task, so the board still has the same window throughout.
  expect(await calls(page, 'session.open')).toEqual([])
  await expect
    .poll(() => calls(page, 'session.hide'))
    .toEqual([
      { project: 1, handle: 'zeus-amber-pine' },
      { project: 1, handle: 'zeus-amber-pine' },
    ])
  expect(await disposed(page)).toBe(0)
})

test('makes a window the human hid theirs again when they show it before it closes', async ({
  page,
}) => {
  const data = atWork()
  // Hidden while its agent was still at work on a turn: the daemon has it open yet.
  const hid = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus-amber-pine')
  Object.assign(hid, { tasks: [], activity: { state: 'working' }, hidden: true })
  await open(page, data)
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await expect(
    page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]'),
  ).toHaveAttribute('data-focused', 'true')
})

test("offers Hide alone on a session's card, which tells the daemon: the window it holds at work stays", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  const card = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  // Its History, and Hide: an icon, named by its tip like the row's tools; nothing on the
  // card ends the window.
  await expect.poll(() => named(card.locator('button'))).toEqual(['History', 'Hide terminal'])
  await expect(card.getByRole('button', { name: /^Close/ })).toHaveCount(0)
  await card.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await expect
    .poll(() => calls(page, 'session.hide'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  // Hidden, its window is the same one, at work on its task: what it prints is still taken.
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-zeus-amber-pine', generation: 9, seq: 1, bytes: [104] }),
  )
  await expect.poll(() => acked(page, 'p1-zeus-amber-pine')).toEqual([1])
  expect(await disposed(page)).toBe(0)
  await expect(row.getByRole('button', { name: /^Close/ })).toHaveCount(0)
})

test("keeps a shown session's card head on one line, Hide in it, at the default window and on a wider screen", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const card = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
  await expect.poll(() => named(card.locator('button'))).toEqual(['History', 'Hide terminal'])
  // Every part of the head inside its 30px: a name wrapped onto three lines ran out of it.
  const outside = () =>
    card.evaluate((card) => {
      const head = card.querySelector('.terminal-head').getBoundingClientRect()
      return [...card.querySelectorAll('.terminal-head > *')]
        .filter((part) => {
          const box = part.getBoundingClientRect()
          return (
            box.top < head.top ||
            box.bottom > head.bottom ||
            box.left < head.left ||
            box.right > head.right
          )
        })
        .map((part) => part.className)
    })
  for (const width of [880, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    await expect.poll(outside, { message: `the head at ${width}px` }).toEqual([])
  }
})

test("keeps a hidden terminal's scrollback: its output goes on arriving, and showing it shows all of it, across project switches too", async ({
  page,
}) => {
  await open(page, twoOpen(atWork()))
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  const print = (seq, text) =>
    page.evaluate(
      ([seq, text]) =>
        window.__output.onmessage({
          id: 'p1-zeus-amber-pine',
          generation: 9,
          seq,
          bytes: [...text].map((c) => c.charCodeAt(0)),
        }),
      [seq, text],
    )
  // The emulators made for the session's window: what each was written, and
  // whether its card is on screen.
  const lexer = () =>
    page.evaluate(() =>
      window.__emulators
        .filter((e) => e.host.closest('.terminal-card')?.dataset.handle === 'zeus-amber-pine')
        .map((e) => [String.fromCharCode(...e.written), e.host.isConnected, e.disposed]),
    )
  await print(1, 'parsing ')
  await expect.poll(() => acked(page, 'p1-zeus-amber-pine')).toEqual([1])
  expect(await lexer()).toEqual([['parsing ', false, false]])
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(lexer).toEqual([['parsing ', true, false]])
  // Hidden, it keeps taking what its window prints.
  await row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await print(2, 'done')
  await expect.poll(() => acked(page, 'p1-zeus-amber-pine')).toEqual([1, 2])
  await expect.poll(lexer).toEqual([['parsing done', false, false]])
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(lexer).toEqual([['parsing done', true, false]])
  // Away to foundry and back: shown as it was, every line still there.
  await chooseProject(page, 'foundry')
  await unfold(page)
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await chooseProject(page, 'harbour')
  await unfold(page)
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  expect(await lexer()).toEqual([['parsing done', true, false]])
  // Hidden, it stays hidden there and back.
  await row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await chooseProject(page, 'foundry')
  await unfold(page)
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await chooseProject(page, 'harbour')
  await unfold(page)
  await expect(page.locator('#project-title')).toHaveText('harbour')
  await expect.poll(() => docked(page)).toEqual(['chief'])
  expect(await lexer()).toEqual([['parsing done', false, false]])
})

test("takes a shown terminal out with its window; the session's next window waits to be shown, unless the human opened it", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  /** The session's window as the board has it: `generation`, or none. */
  const windowOf = (generation) =>
    changed(
      page,
      (generation) => {
        const lane = window.__model.boards[1].lanes.find(
          (l) => l.participant.handle === 'zeus-amber-pine',
        )
        lane.pane = generation === null ? null : { id: 'p1-zeus-amber-pine', generation }
        lane.activity = { state: generation === null ? 'closed' : 'working' }
      },
      generation,
    )
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  // Its task done, the window ends: its card and emulator go with it.
  await windowOf(null)
  await expect.poll(() => docked(page)).toEqual(['chief'])
  expect(await disposed(page)).toBe(1)
  await expect(row.locator('.row-status')).toHaveCount(0)
  // A follow-up opens a window of its own accord: it waits to be shown.
  await windowOf(10)
  await expect(
    row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }),
  ).toBeVisible()
  expect(await docked(page)).toEqual(['chief'])
  // That one ends too, and the human opens the terminal: the window that
  // comes is shown, in front, as asked.
  await windowOf(null)
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  await windowOf(11)
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await expect(
    page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]'),
  ).toHaveAttribute('data-focused', 'true')
  await expect(
    row.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }),
  ).toBeVisible()
})

test("forgets on a reload which terminals were shown: the dock starts again with the chief's", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await page.reload()
  await unfold(page)
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true')
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await expect(
    page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }),
  ).toBeVisible()
})

test("keeps the chief's card first in the dock while its window is down and a session works on, its terminal once it is up", async ({
  page,
}) => {
  const data = atWork()
  const chief = data.boards[1].lanes.find((l) => l.participant.handle === 'chief')
  Object.assign(chief, { activity: { state: 'starting' }, pane: null })
  await open(page, data)
  await unfold(page)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.locator('.terminal-status')).toHaveText('Starting')
  await expect(
    card.getByRole('button', { name: 'Switch the chief to another harness or model' }),
  ).toBeVisible()
  await expect(page.locator('#stage .stage-empty')).toHaveCount(0)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  // Its window up, the chief's card holds its terminal, where the card without one was.
  await changed(page, () => {
    const chief = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    Object.assign(chief, { activity: { state: 'idle' }, pane: { id: 'p1-chief', generation: 6 } })
  })
  await expect(card.locator('.terminal-host')).toHaveCount(1)
  await expect(card.locator('.terminal-status')).toHaveText('Idle')
  expect(await docked(page)).toEqual(['chief', 'zeus-amber-pine'])
})

/**
 * harbour with the chief's window alone in the dock and three sessions of
 * zeus at work, their terminals waiting to be shown. Their ids run the other
 * way from the order the tests show them in: a card put in lane order would
 * go before those shown first.
 */
function threeSessions() {
  const data = atWork()
  const zeus = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus').participant
  for (const [id, name] of [
    [19, 'brisk-birch'],
    [18, 'calm-cedar'],
  ]) {
    data.boards[1].lanes.push({
      participant: session(id, zeus, name),
      tasks: [],
      activity: { state: 'working' },
      pane: { id: `p1-zeus-${name}`, generation: 9 },
    })
  }
  return data
}

/** The room the dock gives its cards: the stage less its padding, and the gap between two cards. */
const roomOf = (page) =>
  page.locator('#stage').evaluate((stage) => {
    const style = getComputedStyle(stage)
    const [left, right, top, bottom, gap] = [
      'paddingLeft',
      'paddingRight',
      'paddingTop',
      'paddingBottom',
      'columnGap',
    ].map((name) => Number.parseFloat(style[name]))
    return {
      width: stage.clientWidth - left - right,
      height: stage.clientHeight - top - bottom,
      gap,
    }
  })

/**
 * The dock's cards, left to right: each one's handle, its left edge in the
 * row's own coordinates (the row scrolls), its size, and whether it is
 * wholly in view.
 */
const cardsOf = (page) =>
  page.locator('#stage').evaluate((stage) => {
    const row = stage.getBoundingClientRect()
    return [...stage.querySelectorAll('.terminal-card')].map((card) => {
      const box = card.getBoundingClientRect()
      return {
        handle: card.dataset.handle,
        x: box.left - row.left + stage.scrollLeft,
        width: box.width,
        height: box.height,
        inView: box.left >= row.left - 1 && box.right <= row.right + 1,
      }
    })
  })

const cardOf = (cards, handle) => cards.find((card) => card.handle === handle)
const sizesOf = (cards) => cards.map(({ handle, width, height }) => [handle, width, height])

/** A card is at `x` in the row, to the pixel: a scroll may land half a pixel off. */
const expectPlace = (card, x) =>
  expect(Math.abs(card.x - x), `${card.handle} is at ${card.x}, not ${x}`).toBeLessThan(1)

/** The handle of the card whose terminal has the keyboard, if one does. */
const typingIn = (page) =>
  page.evaluate(() => document.activeElement?.closest('.terminal-card')?.dataset.handle ?? null)

/** Starts a record of the cards the page puts into the dock and takes out of it. */
const watchStage = (page) =>
  page.evaluate(() => {
    window.__touched = []
    new MutationObserver((records) => {
      for (const { addedNodes, removedNodes } of records) {
        for (const node of addedNodes) window.__touched.push(`in ${node.dataset.handle}`)
        for (const node of removedNodes) window.__touched.push(`out ${node.dataset.handle}`)
      }
    }).observe(document.querySelector('#stage'), { childList: true })
  })

/** The cards put into the dock and taken out of it since this was last asked. */
const touched = (page) => page.evaluate(() => window.__touched.splice(0))

test('opens a terminal shown to the right of the open ones, in view and with the keyboard, and moves or resizes none of them', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, { ...threeSessions(), realTerminals: true })
  await unfold(page)
  await watchStage(page)
  const room = await roomOf(page)
  // Beside the board each card but the chief's is two-thirds of the dock.
  const other = ((room.width - room.gap) * 2) / 3
  expect(other).toBeGreaterThan(320)
  const show = async (name) => {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
    await expect(page.locator(`#stage .terminal-card[data-handle="zeus-${name}"]`)).toHaveAttribute(
      'data-focused',
      'true',
    )
    await expect.poll(() => typingIn(page)).toBe(`zeus-${name}`)
    return cardsOf(page)
  }
  // The chief alone: as tall and as wide as the dock.
  const [chief, ...others] = await cardsOf(page)
  expect([chief.handle, others]).toEqual(['chief', []])
  expect(chief.height).toBeCloseTo(room.height, 0)
  expect(chief.width).toBeCloseTo(room.width, 0)
  // A session shown opens to the chief's right, two-thirds as wide, in view
  // and with the keyboard; the chief keeps its size and its place.
  const two = await show('amber-pine')
  expect(sizesOf(two)[0]).toEqual(['chief', chief.width, chief.height])
  expect(two[1].width).toBeCloseTo(other, 0)
  expect(two[1].height).toBe(chief.height)
  expectPlace(two[0], chief.x)
  expectPlace(two[1], chief.x + chief.width + room.gap)
  expect(two[1].inView).toBe(true)
  // A third, whose lane comes before the second's: still to the right of both,
  // the row scrolled to it, and the others where they were, as big as they were.
  const three = await show('brisk-birch')
  expect(sizesOf(three)).toEqual([
    ['chief', chief.width, chief.height],
    ['zeus-amber-pine', two[1].width, chief.height],
    ['zeus-brisk-birch', two[1].width, chief.height],
  ])
  expectPlace(three[0], chief.x)
  expectPlace(three[1], two[1].x)
  expectPlace(three[2], two[1].x + two[1].width + room.gap)
  expect(three.map((card) => card.inView)).toEqual([false, false, true])
  // Each card that came in was put into the row, and none was taken out and put back.
  expect(await touched(page)).toEqual(['in zeus-amber-pine', 'in zeus-brisk-birch'])
})

test('sends no resize to a terminal already open when another is shown', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, { ...threeSessions(), realTerminals: true })
  await unfold(page)
  const sent = () =>
    page.evaluate(() =>
      window.__calls
        .filter(([command]) => command === 'pane_resize')
        .map(([, { id, cols, rows }]) => `${id} ${cols}x${rows}`),
    )
  // A terminal takes its size once, when its card is first laid out.
  await expect.poll(sent).toHaveLength(1)
  for (const [shown, name] of [
    [2, 'amber-pine'],
    [3, 'brisk-birch'],
  ]) {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
    await expect.poll(async () => (await sent()).length).toBe(shown)
    // A size waits 200 ms to hold before it is sent: none follows for the cards already open.
    await page.waitForTimeout(600)
  }
  expect((await sent()).map((size) => size.split(' ')[0])).toEqual([
    'p1-chief',
    'p1-zeus-amber-pine',
    'p1-zeus-brisk-birch',
  ])
})

test('closes the gap of a terminal hidden from the middle of the dock: those to its right move left and none changes size', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, threeSessions())
  await unfold(page)
  const names = ['amber-pine', 'brisk-birch', 'calm-cedar']
  for (const name of names) {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
  }
  await expect.poll(() => docked(page)).toEqual(['chief', ...names.map((name) => `zeus-${name}`)])
  const before = await cardsOf(page)
  await watchStage(page)
  // Hidden from the middle: the one to its right takes its place.
  const row = page.locator('tr[data-handle="zeus-brisk-birch"]')
  await row.getByRole('button', { name: "Hide @zeus · brisk-birch's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine', 'zeus-calm-cedar'])
  const after = await cardsOf(page)
  expect(sizesOf(after)).toEqual(
    sizesOf(before).filter(([handle]) => handle !== 'zeus-brisk-birch'),
  )
  expectPlace(cardOf(after, 'chief'), cardOf(before, 'chief').x)
  expectPlace(cardOf(after, 'zeus-amber-pine'), cardOf(before, 'zeus-amber-pine').x)
  expectPlace(cardOf(after, 'zeus-calm-cedar'), cardOf(before, 'zeus-brisk-birch').x)
  // Only the card that left was taken out of the row.
  expect(await touched(page)).toEqual(['out zeus-brisk-birch'])
  // Shown again, it comes in at the right end, in view.
  await row.getByRole('button', { name: "Show @zeus · brisk-birch's terminal" }).click()
  await expect
    .poll(() => docked(page))
    .toEqual(['chief', 'zeus-amber-pine', 'zeus-calm-cedar', 'zeus-brisk-birch'])
  const last = cardOf(await cardsOf(page), 'zeus-brisk-birch')
  expectPlace(last, cardOf(before, 'zeus-calm-cedar').x)
  expect(last.inView).toBe(true)
  expect(await touched(page)).toEqual(['in zeus-brisk-birch'])
})

test('keeps the order the cards came into the dock in across a switch to another project and back', async ({
  page,
}) => {
  await open(page, twoOpen(threeSessions()))
  await unfold(page)
  for (const name of ['amber-pine', 'brisk-birch']) {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
  }
  const shown = ['chief', 'zeus-amber-pine', 'zeus-brisk-birch']
  await expect.poll(() => docked(page)).toEqual(shown)
  await chooseProject(page, 'foundry')
  await expect.poll(() => docked(page)).toEqual(['chief'])
  await chooseProject(page, 'harbour')
  await expect.poll(() => docked(page)).toEqual(shown)
})

test("sizes the dock's cards: beside the board the chief's the whole dock and each other two-thirds of it, with the board folded three-quarters and the last quarter", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, atWork())
  await unfold(page)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const divider = page.getByRole('separator', { name: 'Board width' })
  const squeeze = async (key, presses) => {
    await divider.focus()
    for (let press = 0; press < presses; press += 1) await divider.press(key)
  }
  const measure = async () => ({
    dock: (await dock.boundingBox()).width,
    room: await roomOf(page),
    cards: await cardsOf(page),
  })
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  // Half the window: the chief's the whole dock, the other two-thirds of it.
  let now = await measure()
  expect(now.dock).toBeCloseTo((1600 - 18) / 2, 0)
  expect(now.cards[0].width).toBeCloseTo(now.room.width, 0)
  expect(now.cards[1].width).toBeCloseTo(((now.room.width - now.room.gap) * 2) / 3, 0)
  // A dock whose two-thirds is under 320px: the other keeps 320px.
  await squeeze('ArrowRight', 12)
  now = await measure()
  expect(((now.room.width - now.room.gap) * 2) / 3).toBeLessThan(320)
  expect(now.room.width).toBeGreaterThan(320)
  expect(now.cards.map((card) => Math.round(card.width))).toEqual([Math.round(now.room.width), 320])
  // A dock under 320px: every card as wide as it is.
  await squeeze('ArrowRight', 40)
  now = await measure()
  expect(now.room.width).toBeLessThan(320)
  for (const card of now.cards) expect(card.width).toBeCloseTo(now.room.width, 0)
  // The board folded: the chief's three-quarters of the row, the other the
  // last quarter, whole beside it.
  await squeeze('ArrowLeft', 40)
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect.poll(async () => (await measure()).dock).toBeGreaterThan(1200)
  now = await measure()
  const [chief, other] = now.cards
  expect(chief.width).toBeCloseTo(((now.room.width - now.room.gap) * 3) / 4, 0)
  expect(other.width).toBeCloseTo((now.room.width - now.room.gap) / 4, 0)
  expect(other.inView).toBe(true)
  // Back beside the board, the cards are as they were.
  await page.getByRole('button', { name: 'Show board' }).click()
  await expect
    .poll(async () => (await cardsOf(page))[0].width)
    .toBeCloseTo((await roomOf(page)).width, 0)
})

test("with the board folded fills the row with the chief's card alone, three-quarters of it beside another window, and the row again when it leaves", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  // harbour keeps a session's terminal shown; foundry's dock is its chief's alone.
  await open(page, { ...twoOpen(atWork()), realTerminals: true })
  await unfold(page)
  const resizes = () =>
    page.evaluate(() =>
      window.__calls
        .filter(([command, args]) => command === 'pane_resize' && args.id === 'p1-chief')
        .map(([, args]) => args.cols),
    )
  await expect.poll(async () => (await resizes()).length).toBe(1)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  await page.getByRole('button', { name: 'Hide board' }).click()
  const room = await roomOf(page)
  const [threeQuarters, quarter] = [((room.width - room.gap) * 3) / 4, (room.width - room.gap) / 4]
  // Two cards beside the folded board: the chief's three-quarters of the row,
  // the other's the last quarter, whole beside it.
  let now = await cardsOf(page)
  expect(now[0].width).toBeCloseTo(threeQuarters, 0)
  expect(now[1].width).toBeCloseTo(quarter, 0)
  expect(now[1].inView).toBe(true)
  // Folding grew the chief's card: its terminal took the new size once, wider.
  await expect.poll(async () => (await resizes()).length).toBe(2)
  // Alone — the other project's dock is its chief's alone: the whole row.
  await chooseProject(page, 'foundry')
  await expect.poll(() => docked(page)).toEqual(['chief'])
  now = await cardsOf(page)
  expect(now[0].width).toBeCloseTo(room.width, 0)
  // Back to harbour's two: three-quarters and the last quarter again, as wide
  // as the card left them, so no new size for its terminal.
  await chooseProject(page, 'harbour')
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  now = await cardsOf(page)
  expect(now[0].width).toBeCloseTo(threeQuarters, 0)
  expect(now[1].width).toBeCloseTo(quarter, 0)
  await page.waitForTimeout(300)
  expect((await resizes()).length).toBe(2)
  // The session's card hidden from its own Hide: the chief's is the whole row
  // again, and its terminal takes that size once, wider still.
  await page
    .locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
    .getByRole('button', { name: "Hide @zeus · amber-pine's terminal" })
    .click()
  await expect.poll(() => docked(page)).toEqual(['chief'])
  now = await cardsOf(page)
  expect(now[0].width).toBeCloseTo(room.width, 0)
  await expect.poll(async () => (await resizes()).length).toBe(3)
  const [beside, folded, alone] = await resizes()
  expect(folded).toBeGreaterThan(beside)
  expect(alone).toBeGreaterThan(folded)
})

test('snaps the row of terminals to its padding: scrolled to its start, no sliver of the next card shows', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1366, height: 768 })
  await open(page, threeSessions())
  await unfold(page)
  for (const name of ['amber-pine', 'brisk-birch']) {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
  }
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine', 'zeus-brisk-birch'])
  const stage = page.getByRole('region', { name: 'Terminals' })
  // Snapped to the row's edge, the row sat 8px along, the next card's edge in view.
  await stage.evaluate((node) => {
    node.scrollLeft = 0
  })
  await page.waitForTimeout(300)
  expect(await stage.evaluate((node) => node.scrollLeft)).toBe(0)
  const [chief, next] = await cardsOf(page)
  expect(chief.inView).toBe(true)
  expect(next.inView).toBe(false)
})

test('gives the projects and the board half the window and the terminals the other half, whatever an older build kept', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  // What an older build kept was a share of another room: the windows start at half all the same.
  await page.addInitScript(() => {
    localStorage.setItem('cf.layout.dock-share', '0.3')
    localStorage.setItem('cf.layout.board-width', '900')
  })
  await open(page)
  const widths = () =>
    page.evaluate(() =>
      ['.sidebar', '#board', '.dock'].map(
        (selector) => document.querySelector(selector).getBoundingClientRect().width,
      ),
    )
  const [projects, board, dock] = await widths()
  expect(dock).toBeCloseTo((1600 - 18) / 2, 0)
  expect(projects + board).toBeCloseTo((1600 - 18) / 2, 0)
  // The terminals folded: the board takes the whole width beside the projects.
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await expect.poll(async () => (await widths())[1]).toBeCloseTo(1600 - projects - 34, 0)
})

test("makes a card the dock's whole width on a narrow window, the next one opening to its right", async ({
  page,
}) => {
  await page.setViewportSize({ width: 700, height: 900 })
  await open(page, atWork())
  await unfold(page)
  const room = await roomOf(page)
  const [chief] = await cardsOf(page)
  expect(chief.width).toBeCloseTo(room.width, 0)
  expect(chief.height).toBeCloseTo(room.height, 0)
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine'])
  const [kept, shown] = await cardsOf(page)
  expect(sizesOf([kept, shown])).toEqual([
    ['chief', chief.width, chief.height],
    ['zeus-amber-pine', chief.width, chief.height],
  ])
  expectPlace(kept, chief.x)
  expectPlace(shown, chief.x + chief.width + room.gap)
  expect(shown.inView).toBe(true)
})

test('keeps every card as tall as it was when the row first scrolls, where a scrollbar takes room', async () => {
  // The headless browser hides its scrollbars: this one draws them, as Windows
  // and a Mac with a mouse do.
  const browser = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] })
  try {
    const page = await browser.newPage({ viewport: { width: 1600, height: 900 } })
    await open(page, threeSessions())
    await unfold(page)
    const [chief] = await cardsOf(page)
    for (const name of ['amber-pine', 'brisk-birch']) {
      await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
      await expect.poll(() => docked(page)).toContain(`zeus-${name}`)
    }
    // The chief's card fills the dock and the next makes it scroll: a scrollbar
    // that came with it would take room from every card, a resize of each terminal.
    const taken = await page
      .locator('#stage')
      .evaluate((stage) => stage.offsetHeight - stage.clientHeight)
    // A Mac set to overlay scrollbars (the default without a mouse) draws them over
    // the content even here: nothing takes room, and there is nothing to hold.
    test.skip(taken === 0, 'this machine shows overlay scrollbars, which take no room')
    expect(taken).toBeGreaterThan(0)
    expect((await cardsOf(page)).map((card) => card.height)).toEqual([
      chief.height,
      chief.height,
      chief.height,
    ])
  } finally {
    await browser.close()
  }
})

test('brings a terminal already in the dock into view again when it is shown from its task, where it is', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  const data = threeSessions()
  data.tasks['1:21'] = {
    ...task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3),
    messages: [],
  }
  await open(page, data)
  await unfold(page)
  for (const name of ['amber-pine', 'brisk-birch']) {
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
  }
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus-amber-pine', 'zeus-brisk-birch'])
  const stage = page.getByRole('region', { name: 'Terminals' })
  const amber = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
  // Twice: the card shown last is another the first time, and this one the second.
  for (let time = 1; time <= 2; time += 1) {
    await stage.evaluate((node) => {
      node.scrollLeft = 0
    })
    expect(cardOf(await cardsOf(page), 'zeus-amber-pine').inView).toBe(false)
    await page.locator('tr[data-handle="zeus-amber-pine"] button.card[data-task="21"]').click()
    const drawer = page.getByRole('complementary', { name: 'Task T-21' })
    await drawer.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
    await expect(drawer).toBeHidden()
    await expect(amber).toHaveAttribute('data-focused', 'true')
    expect(cardOf(await cardsOf(page), 'zeus-amber-pine').inView).toBe(true)
    expect(await docked(page)).toEqual(['chief', 'zeus-amber-pine', 'zeus-brisk-birch'])
  }
})

test('leaves the keyboard in the terminal that has it when a window comes up that the human did not ask for', async ({
  page,
}) => {
  const data = withAdvisor()
  const athena = data.boards[1].lanes.find((l) => l.participant.handle === 'athena')
  Object.assign(athena, { activity: { state: 'closed' }, pane: null })
  await open(page, data)
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus'])
  const typing = page.locator('#stage .terminal-card[data-handle="chief"] .stub-input')
  await typing.focus()
  await expect(typing).toBeFocused()
  await changed(page, () => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'athena')
    Object.assign(lane, {
      activity: { state: 'working' },
      pane: { id: 'p1-athena', generation: 4 },
    })
  })
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus', 'athena'])
  await expect(typing).toBeFocused()
})

test('resizes a terminal once, when a drag that narrows it holds still', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 800 })
  await open(page, { ...model(), realTerminals: true })
  const sizes = () =>
    page.evaluate(() =>
      window.__calls
        .filter(([command, args]) => command === 'pane_resize' && args.id === 'p1-chief')
        .map(([, args]) => args.cols),
    )
  // The first size goes at once, and holds: a terminal wider than its card
  // once held the card open and lost a column at every fit.
  await expect.poll(async () => (await sizes()).length).toBe(1)
  await page.waitForTimeout(600)
  const [first] = await sizes()
  expect(await sizes()).toEqual([first])
  const grip = await page.locator('#board-resize').boundingBox()
  const x = grip.x + grip.width / 2
  const y = grip.y + 60
  await page.evaluate(() => {
    window.__moves = []
    window.addEventListener('pointermove', () => window.__moves.push(performance.now()), true)
  })
  await page.mouse.move(x, y)
  await page.mouse.down()
  // A slow drag to the right, the windows narrowing: every step outlasts a
  // frame, and none outlasts the settle.
  for (let step = 1; step <= 10; step += 1) {
    await page.mouse.move(x + step * 16, y)
    await page.waitForTimeout(40)
  }
  await page.mouse.up()
  // A runner too busy to keep each step under the 200 ms settle drags in
  // pauses, and the page rightly sends a size at each: not this case.
  const longest = await page.evaluate(() =>
    Math.max(...window.__moves.slice(1).map((at, step) => at - window.__moves[step])),
  )
  test.skip(longest >= 200, `a step took ${Math.round(longest)} ms, past the settle`)
  await expect.poll(async () => (await sizes()).length, { timeout: 3_000 }).toBe(2)
  await page.waitForTimeout(600)
  const after = await sizes()
  expect(after, 'one resize for the whole drag, and none after it').toHaveLength(2)
  expect(after[1]).toBeLessThan(first)
})

test('resizes a terminal while the board redraws faster than a size settles', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 800 })
  await open(page, { ...model(), realTerminals: true })
  const sizes = () =>
    page.evaluate(() =>
      window.__calls
        .filter(([command, args]) => command === 'pane_resize' && args.id === 'p1-chief')
        .map(([, args]) => args.cols),
    )
  await expect.poll(async () => (await sizes()).length).toBe(1)
  // A busy board: a state change every 100 ms, as fast as the daemon sends them,
  // each redraw asking every terminal to fit.
  await page.evaluate(() => {
    window.__busy = setInterval(() => window.__listeners.get('state-changed')(), 100)
  })
  const grip = await page.locator('#board-resize').boundingBox()
  const x = grip.x + grip.width / 2
  const y = grip.y + 60
  await page.mouse.move(x, y)
  await page.mouse.down()
  await page.mouse.move(x + 160, y, { steps: 4 })
  await page.mouse.up()
  await expect.poll(async () => (await sizes()).length, { timeout: 2_000 }).toBe(2)
  const [first, narrowed] = await sizes()
  expect(narrowed).toBeLessThan(first)
  await page.evaluate(() => clearInterval(window.__busy))
})

test("gives a card's title room to read, at the default window and on a wider screen", async ({
  page,
}) => {
  const data = model()
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  data.boards[1].lanes.push({
    participant: session(20, zeus.participant, 'amber-pine'),
    tasks: [task(21, 'Write the lexer', 'working', 'chief', 'zeus-amber-pine', 3)],
    activity: { state: 'working' },
    pane: { id: 'p1-zeus-amber-pine', generation: 9 },
  })
  await open(page, data)
  const titles = () =>
    page
      .locator('button.card .card-title')
      .evaluateAll((nodes) => nodes.map((node) => Math.round(node.getBoundingClientRect().width)))
  for (const width of [880, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    await expect
      .poll(async () => Math.min(...(await titles())), { message: `titles at ${width}px` })
      .toBeGreaterThanOrEqual(60)
  }
  // At 1440 the board beside the windows has half the window, the projects'
  // panel in it, and scrolls sideways; with the windows folded it has every
  // column without scrolling.
  const board = page.getByRole('region', { name: 'Board' })
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await expect
    .poll(() => board.evaluate((node) => node.scrollWidth - node.clientWidth))
    .toBeLessThanOrEqual(0)
})

/**
 * harbour with a session of zeus whose Finished cell holds `count` accepted
 * tasks, T-31 up, each with a result of two lines.
 */
function crowded(count) {
  const data = model()
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  const finished = Array.from({ length: count }, (_, at) =>
    task(31 + at, `Old task ${31 + at}`, 'accepted', 'chief', 'zeus-amber-pine', 60 - at, {
      result: `Result ${31 + at}\nIts details.`,
    }),
  )
  data.boards[1].lanes.push({
    participant: session(20, zeus.participant, 'amber-pine'),
    tasks: finished,
    activity: { state: 'closed' },
    pane: null,
  })
  for (const done of finished) data.tasks[`1:${done.number}`] = { ...done, messages: [] }
  return data
}

test("folds a member's sessions under its row until the human unfolds them, their cards on its row meanwhile", async ({
  page,
}) => {
  await open(page, atWork())
  const member = page.locator('tr[data-handle="zeus"]')
  const session = page.locator('tr[data-handle="zeus-amber-pine"]')
  // Folded at first: the session's row is off the board, its card on the member's.
  await expect(member).toHaveAttribute('data-folded', 'true')
  await expect(session).toHaveCount(0)
  await expect(member.locator('button.card[data-task="21"]')).toHaveCount(1)
  const toggle = member.getByRole('button', { name: "Show @zeus's 1 session" })
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')
  // Unfolded: the session's row is under it, holding its card and its tools.
  await toggle.click()
  await expect(member).toHaveAttribute('data-folded', 'false')
  await expect(session.locator('button.card[data-task="21"]')).toHaveCount(1)
  await expect(member.locator('button.card[data-task="21"]')).toHaveCount(0)
  await expect(
    session.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }),
  ).toBeVisible()
  // It stays unfolded across a redraw, and folds again on its arrow.
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect(session).toHaveCount(1)
  const unfolded = member.getByRole('button', { name: "Hide @zeus's 1 session" })
  await expect(unfolded).toHaveAttribute('aria-expanded', 'true')
  await unfolded.click()
  await expect(session).toHaveCount(0)
  await expect(member.locator('button.card[data-task="21"]')).toHaveCount(1)
})

test('puts Delete finished beside its heading, the headings level', async ({ page }) => {
  const data = model()
  const zeus = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  zeus.tasks.push(task(5, 'Old spike', 'accepted', 'chief', 'zeus', 90))
  await open(page, data)
  const finished = page.locator('th[data-state="finished"]')
  const button = finished.getByRole('button', { name: 'Delete finished' })
  const [heading, done, icon] = [
    await finished.boundingBox(),
    await page.locator('th[data-state="done"]').boundingBox(),
    await button.boundingBox(),
  ]
  expect(Math.abs(heading.height - done.height)).toBeLessThan(2)
  // Beside the word, on its line: its middle at the heading's.
  expect(Math.abs(icon.y + icon.height / 2 - (heading.y + heading.height / 2))).toBeLessThan(3)
  expect(icon.x).toBeGreaterThan(heading.x + 40)
})

test('stacks a cell of four cards or more into one tile: a stack, and how many', async ({
  page,
}) => {
  await open(page, crowded(3))
  await unfold(page)
  const cell = page.locator('tr[data-handle="zeus-amber-pine"] td[data-state="finished"]')
  await expect(cell.locator('button.card')).toHaveCount(3)
  /** Task `number`, finished on the session's row. */
  const finish = (number) =>
    changed(
      page,
      (number) => {
        const lane = window.__model.boards[1].lanes.find(
          (l) => l.participant.handle === 'zeus-amber-pine',
        )
        lane.tasks.push({ ...lane.tasks[0], id: number, number, title: `Old task ${number}` })
      },
      number,
    )
  await finish(34)
  await expect(cell.locator('button.card')).toHaveCount(0)
  const tile = cell.getByRole('button', { name: '4 finished tasks of @zeus · amber-pine' })
  await expect(tile).toHaveText('4')
  await expect(tile.locator('svg')).toHaveCount(1)
  // Every other cell draws its cards as before.
  await expect(
    page.locator('tr[data-handle="zeus"] td[data-state="finished"] button.card[data-task="5"]'),
  ).toHaveCount(1)
  // The tile keeps the keyboard while its count changes.
  await tile.focus()
  await finish(35)
  await expect(
    cell.getByRole('button', { name: '5 finished tasks of @zeus · amber-pine' }),
  ).toBeFocused()
})

test("lists a stack's tasks in a dialog, the newest first, and opens the one chosen in the drawer", async ({
  page,
}) => {
  await open(page, crowded(5))
  await unfold(page)
  const tile = page.getByRole('button', { name: '5 finished tasks of @zeus · amber-pine' })
  await tile.click()
  const dialog = page.getByRole('dialog', { name: 'Finished tasks of @zeus · amber-pine' })
  await expect(dialog).toBeVisible()
  await expect(dialog.locator('.card-number')).toHaveText(['T-35', 'T-34', 'T-33', 'T-32', 'T-31'])
  const card = dialog.locator('button.card[data-task="33"]')
  await expect(card.locator('.card-title')).toHaveText('Old task 33')
  await expect(card.locator('.card-route')).toHaveText('from @chief')
  // A result reads by its first line.
  await expect(card.locator('.card-result')).toHaveText('Result 33')
  // The keyboard is on the newest; Esc closes the dialog and gives it back to the tile.
  await expect(dialog.locator('button.card[data-task="35"]')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(tile).toBeFocused()
  await tile.click()
  await dialog.getByRole('button', { name: 'Close', exact: true }).click()
  await expect(dialog).toBeHidden()
  // Open, it follows the board: a task gone from the cell leaves the list.
  await tile.click()
  await changed(page, () => {
    const lane = window.__model.boards[1].lanes.find(
      (l) => l.participant.handle === 'zeus-amber-pine',
    )
    lane.tasks = lane.tasks.filter((t) => t.number !== 35)
  })
  await expect(dialog.locator('.card-number')).toHaveText(['T-34', 'T-33', 'T-32', 'T-31'])
  // Choosing a task closes the dialog and opens the task, as its card on the board does.
  await card.click()
  await expect(dialog).toBeHidden()
  await expect(page.getByRole('complementary', { name: 'Task T-33' })).toBeVisible()
})

test("gives For you's strips room to read, at the default window and on wider screens", async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.gate = true
  const gated = (id, kind, sender, recipient, taskNumber, body) => ({
    id,
    kind,
    state: 'gated',
    sender,
    recipient,
    taskNumber,
    body,
    questions: null,
    choices: null,
    createdAt: at(3),
  })
  data.boards[1].gated = [
    gated(30, 'task', 'chief', 'zeus-amber-pine', 4, 'Add the tests\nCover every error path.'),
    gated(31, 'result', 'zeus-amber-pine', 'chief', 2, 'Lexer done; 14 tests pass.'),
  ]
  await open(page, data)
  // Each strip's text, and whether its buttons are all inside it.
  const strips = () =>
    page.locator('.foryou .strip').evaluateAll((nodes) =>
      nodes.map((strip) => {
        const edge = strip.getBoundingClientRect().right
        return {
          text: Math.round(strip.querySelector('.strip-title').getBoundingClientRect().width),
          inside: [...strip.querySelectorAll('.strip-actions button')].every(
            (button) => button.getBoundingClientRect().right <= edge,
          ),
        }
      }),
    )
  // At the default window the board is at its narrowest, 280px, beside the
  // projects' panel and the windows' half.
  for (const [width, least] of [
    [880, 200],
    [1440, 240],
    [2200, 240],
  ]) {
    await page.setViewportSize({ width, height: 900 })
    await expect
      .poll(async () => Math.min(...(await strips()).map((strip) => strip.text)), {
        message: `strip text at ${width}px`,
      })
      .toBeGreaterThanOrEqual(least)
    expect(
      (await strips()).every((strip) => strip.inside),
      `buttons at ${width}px`,
    ).toBe(true)
  }
})

test('keeps every button of a lane inside its column, however narrow the board', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1100, height: 800 })
  const data = model()
  const zeusLane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  Object.assign(zeusLane, { tasks: [], activity: { state: 'closed' }, pane: null })
  data.boards[1].lanes.push(
    {
      participant: session(20, zeusLane.participant, 'lively-comet'),
      tasks: [task(21, 'Write the lexer', 'accepted', 'chief', 'zeus-lively-comet', 3)],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: session(22, zeusLane.participant, 'amber-pine'),
      tasks: [task(23, 'Write the parser', 'working', 'chief', 'zeus-amber-pine', 2)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 3 },
    },
  )
  await open(page, data)
  await unfold(page)
  // The board at its narrowest, the windows given the rest.
  const grip = page.locator('#board-resize')
  await grip.focus()
  for (let press = 0; press < 30; press += 1) await grip.press('ArrowLeft')
  await expect(grip).toHaveAttribute('aria-valuenow', '280')
  const row = page.locator('tr[data-handle="zeus-lively-comet"]')
  await expect.poll(() => toolsOf(row)).toEqual(['Show terminal', 'Delete session'])
  const live = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect.poll(() => toolsOf(live)).toEqual(['Show terminal', 'Delete session'])
  const outside = () =>
    page.locator('table[aria-label="Tasks"] tbody th').evaluateAll((heads) =>
      heads.flatMap((head) => {
        const column = head.getBoundingClientRect()
        return [...head.querySelectorAll('button')]
          .filter((button) => {
            const box = button.getBoundingClientRect()
            return box.left < column.left || box.right > column.right + 0.5
          })
          .map((button) => button.textContent)
      }),
    )
  expect(await outside()).toEqual([])
  // Its terminal shown, the live lane offers to hide it, in the same room.
  await live.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => toolsOf(live)).toEqual(['Hide terminal', 'Delete session'])
  expect(await outside()).toEqual([])
})

test("gives a member's heading row no buttons, and a closed session's row no Transcript: its card opens the task", async ({
  page,
}) => {
  const data = model()
  const diana = data.boards[1].lanes.find((l) => l.participant.handle === 'diana').participant
  data.boards[1].lanes.push({
    participant: session(24, diana, 'amber-pine'),
    tasks: [task(3, 'hostile', 'failed', 'chief', 'diana-amber-pine', 5)],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  await unfold(page)
  await expect(page.locator('tr[data-handle="diana"] .row-tools button')).toHaveCount(0)
  await expect
    .poll(() => toolsOf(page.locator('tr[data-handle="diana-amber-pine"]')))
    .toEqual(['Show terminal', 'Delete session'])
  // Each tool is an icon, its name in a tip on hover.
  const showTerminal = page.getByRole('button', { name: "Show @diana · amber-pine's terminal" })
  await expect(showTerminal.locator('svg.icon')).toHaveCount(1)
  await showTerminal.hover()
  await expect
    .poll(() => showTerminal.evaluate((node) => getComputedStyle(node, '::after').content))
    .toBe('"Show terminal"')
  await page.locator('tr[data-handle="diana-amber-pine"] button.card[data-task="3"]').click()
  await expect(page.getByRole('complementary', { name: 'Task T-3' })).toBeVisible()
})

test('switches the chief from its card to a harness installed here and a model of it, after its turn or now', async ({
  page,
}) => {
  const data = model()
  data.missing = ['devin']
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await expect(dialog).toBeVisible()
  const harness = dialog.locator('[name="chiefHarness"]')
  const models = dialog.locator('[name="chiefModel"]')
  // A harness is picked, never a default: nothing is chosen yet; Devin is not installed here.
  await expect(harness).toHaveValue('')
  expect(await offered(harness)).toEqual([
    ['Pick a harness', true],
    ['Claude Code', false],
    ['Codex', false],
    ['Pi', false],
    ['Muse Code', false],
  ])
  // Only the harness and model the chief is on are no switch: another model of its
  // harness is a fresh conversation with the handoff.
  await harness.selectOption({ label: 'Claude Code' })
  await expect(models).toBeEnabled()
  expect(await offered(models)).toEqual([
    ['Pick a model', true],
    ['claude-opus-5-5', true],
    ['claude-sonnet-5', false],
  ])
  await pickChief(dialog, 'Codex', 'gpt-6-astra')
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the chief to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 1, harness: 'codex', model: 'gpt-6-astra', when: 'now', note: true }])
  await expect(dialog).toBeHidden()
})

test('opens Switch chief letting the chief finish its turn, whatever was picked last time', async ({
  page,
}) => {
  await open(page)
  const switchChief = page.getByRole('button', {
    name: 'Switch the chief to another harness or model',
  })
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await switchChief.click()
  await pickChief(dialog, 'Codex', 'gpt-5.6-luna')
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the chief to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect(dialog).toBeHidden()
  await switchChief.click()
  await expect(dialog.locator('[name="chiefHarness"]')).toHaveValue('')
  await expect(dialog.locator('[name="chiefModel"]')).toBeDisabled()
  await expect(dialog.getByLabel('Let it finish its turn, then switch')).toBeChecked()
  await expect(
    dialog.getByLabel('First ask the chief to write down where things stand'),
  ).not.toBeChecked()
})

test("the chief's card says when a switch waits for its turn", async ({ page }) => {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').switching = {
    harness: 'codex',
    model: 'gpt-6-astra',
  }
  await open(page, data)
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-status'),
  ).toHaveText('Switching the chief to Codex (gpt-6-astra) after this turn')
  await expect(page.locator('.row-status[data-state="switching"]')).toHaveCount(0)
})

test('says on a card and a row when a message waits for what the human typed there and has not sent', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').holding = true
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus').holding = true
  await open(page, data)
  const words = 'A message waits until you send or erase what you typed here'
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-status'),
  ).toHaveText(words)
  await expect(page.locator('tr[data-handle="zeus"] .row-status')).toHaveText(words)
})

test('says a message waits for what the human typed in a window out of quota too', async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  chief.holding = true
  chief.participant.outUntil = new Date(Date.now() + 90 * 60_000).toISOString()
  await open(page, data)
  const status = page.locator('#stage .terminal-card[data-handle="chief"] .terminal-status')
  await expect(status).toHaveAttribute('data-state', 'out')
  await expect(status).toHaveText(
    /^Out of quota until (\S+ )?\d\d:\d\d\. A message waits until you send or erase what you typed here$/,
  )
})

test('offers a chief from before it had a model every model, and moves it to one', async ({
  page,
}) => {
  // A project opened before a chief was a harness and a model: its chief names none.
  const data = model()
  delete data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.model
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Claude Code' })
  await expect(dialog.locator('[name="chiefModel"]')).toBeEnabled()
  expect(await offered(dialog.locator('[name="chiefModel"]'))).toEqual([
    ['Pick a model', true],
    ['claude-opus-5-5', false],
    ['claude-sonnet-5', false],
  ])
  await pickChief(dialog, 'Claude Code', 'claude-sonnet-5')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([
      { project: 1, harness: 'claude-code', model: 'claude-sonnet-5', when: 'turn', note: false },
    ])
})

test('refuses Switch chief when no harness is installed here, and says where to get one', async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'codex', 'pi', 'devin', 'muse']
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  await expect(page.locator('#status')).toHaveText(
    'No harness is installed here: install one from Agents, Harnesses.',
  )
  await expect(page.getByRole('dialog', { name: 'Switch the chief' })).toBeHidden()
})

test('opens a task only over its own project, and every redraw after still draws', async ({
  page,
}) => {
  const data = twoOpen()
  data.tasks['1:4'] = { ...task(4, 'Add the tests', 'queued', 'chief', 'zeus', 1), messages: [] }
  await open(page, data)
  // harbour's T-4 is asked for and is slow to come; the human chooses foundry meanwhile.
  await page.evaluate(() => {
    window.__delay['task.get'] = 300
    window.__delay['task.transcript'] = 300
  })
  await page.locator('button.card[data-task="4"]').click()
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await page.waitForTimeout(500)
  await expect(page.locator('#task-drawer')).toBeHidden()
  // The next change still draws, and nothing was refused on the way.
  await page.evaluate(() => {
    const [, chief] = window.__model.boards[2].lanes
    window.__model.boards[2].lanes.push({
      participant: { ...chief.participant, id: 11, handle: 'ares', role: 'worker' },
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    })
    window.__listeners.get('state-changed')()
  })
  await expect(page.locator('tr[data-handle="ares"]')).toHaveCount(1)
  await expect(page.locator('#status')).toHaveText('')
})

test('closes a drawer whose task is gone, and still draws the board', async ({ page }) => {
  const data = model()
  data.tasks['1:4'] = { ...task(4, 'Add the tests', 'queued', 'chief', 'zeus', 1), messages: [] }
  await open(page, data)
  await page.locator('button.card[data-task="4"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-4' })
  await expect(drawer).toBeVisible()
  await page.evaluate(() => {
    delete window.__model.tasks['1:4']
    const zeus = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
    zeus.activity = { state: 'idle' }
    window.__listeners.get('state-changed')()
  })
  await expect(page.locator('tr[data-handle="zeus"]').getByTestId('lamp')).toHaveAttribute(
    'data-state',
    'idle',
  )
  await expect(drawer).toBeHidden()
  await expect(page.locator('#status')).toHaveText('no task T-4 in this project')
})

test("acts on the project a control was drawn for, while another one's board is on its way", async ({
  page,
}) => {
  const data = twoOpen()
  const zeus = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  data.boards[1].lanes.push({
    participant: session(20, zeus.participant, 'amber-pine'),
    tasks: [task(21, 'Write the lexer', 'done', 'chief', 'zeus-amber-pine', 3)],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  await unfold(page)
  await page.evaluate(() => {
    window.__delay['board.get'] = 400
  })
  await chooseProject(page, 'foundry')
  await unfold(page)
  // harbour's board is still the one shown: what is done on it is done in harbour.
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
})

test('switches the chief only of the project it was asked for', async ({ page }) => {
  await open(page, twoOpen())
  // What is installed comes slowly; the human chooses foundry before the dialog is up.
  await page.evaluate(() => {
    window.__delay['harnesses.missing'] = 300
  })
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await page.waitForTimeout(500)
  await expect(page.getByRole('dialog', { name: 'Switch the chief' })).toBeHidden()
  // Asked for again on foundry's own chief card, it switches foundry's chief.
  await page.evaluate(() => {
    window.__delay = {}
  })
  await page.getByRole('button', { name: 'Switch the chief to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await pickChief(dialog, 'Codex', 'gpt-5.6-luna')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 2, harness: 'codex', model: 'gpt-5.6-luna', when: 'turn', note: false }])
})

test('says the daemon is down while it is, why, and what comes next, and reads everything again once it is back', async ({
  page,
}) => {
  await open(page)
  // The app gives the daemon's state when the output is subscribed to: the page listens first.
  const order = await page.evaluate(() =>
    window.__calls.map(([command, args]) => (command === 'listen' ? args.name : command)),
  )
  expect(order).toContain('daemon-status')
  expect(order.indexOf('daemon-status')).toBeLessThan(order.indexOf('subscribe_output'))
  const banner = page.getByRole('alert')
  await expect(banner).toBeHidden()
  const daemonStatus = (payload) =>
    page.evaluate(
      (payload) => window.__listeners.get('daemon-status')({ event: 'daemon-status', payload }),
      payload,
    )
  await page.evaluate(() => {
    window.__down = 'the Node bridge is not running'
  })
  await daemonStatus({
    available: false,
    cause: 'the daemon stopped (exit code 1)',
    retrying: true,
  })
  await expect(banner).toHaveText(
    'The daemon is not running. The daemon stopped (exit code 1). Starting it again…',
  )
  // What the human does meanwhile is refused with why, not with a code.
  await page.getByRole('button', { name: 'Close harbour' }).click()
  await expect(page.locator('#status')).toHaveText('the Node bridge is not running')
  await daemonStatus({ available: false, cause: 'it stopped again.', retrying: false })
  await expect(banner).toHaveText(
    'The daemon is not running. It stopped again. Quit ConsensFlow and open it again.',
  )
  // Back: the banner goes, and the agents and the board are read again.
  await page.evaluate(() => {
    window.__down = null
  })
  const [agents, boards] = [
    (await calls(page, 'harnesses.missing')).length,
    (await calls(page, 'board.get')).length,
  ]
  await daemonStatus({ available: true, cause: null, retrying: false })
  await expect(banner).toBeHidden()
  await expect
    .poll(async () => (await calls(page, 'harnesses.missing')).length)
    .toBeGreaterThan(agents)
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(boards)
  await expect(page.locator('tr[data-handle="zeus"]')).toHaveCount(1)
})

test('says a restart of the daemon in its steps, and each thing only once it is true', async ({
  page,
}) => {
  await open(page)
  const banner = page.getByRole('alert')
  const daemonStatus = (payload) =>
    page.evaluate(
      (payload) => window.__listeners.get('daemon-status')({ event: 'daemon-status', payload }),
      payload,
    )
  // The windows are being closed: nothing is said to be closed, and nothing to be started.
  await daemonStatus({
    available: false,
    cause: "ConsensFlow's daemon stopped; closing its windows…",
    retrying: true,
    closing: true,
  })
  await expect(banner).toHaveText(
    "The daemon is not running. ConsensFlow's daemon stopped; closing its windows…",
  )
  // They are proven gone: now it is closed, and the daemon is being started.
  await daemonStatus({
    available: false,
    cause: "ConsensFlow's daemon stopped and its windows are closed.",
    retrying: true,
  })
  await expect(banner).toHaveText(
    "The daemon is not running. ConsensFlow's daemon stopped and its windows are closed. Starting it again…",
  )
  // Their end could not be confirmed: that is said, and the person is asked to quit.
  await daemonStatus({
    available: false,
    cause:
      "ConsensFlow's daemon stopped, and the end of its windows could not be confirmed, so it was not started again while one of them might still run.",
    retrying: false,
  })
  await expect(banner).toHaveText(
    "The daemon is not running. ConsensFlow's daemon stopped, and the end of its windows could not be confirmed, so it was not started again while one of them might still run. Quit ConsensFlow and open it again.",
  )
  // Up again: the banner goes, and what became of the windows and the projects is said,
  // in the status line, long enough to be read.
  const notice =
    "ConsensFlow's daemon was started again. Its windows were closed, and the projects that were open are paused until you resume them."
  await daemonStatus({ available: true, cause: null, retrying: false, notice })
  await expect(banner).toBeHidden()
  await expect(page.locator('#status')).toHaveText(notice)
})

test('says what to do with no project yet: start one; there is no board to read, no staff to open', async ({
  page,
}) => {
  await open(page, { ...model(), projects: [] })
  await expect(page.locator('#project-title')).toHaveText('No project')
  await expect(page.getByTestId('projects')).toHaveText('No projects yet.')
  await expect(page.getByRole('region', { name: 'Board' })).toHaveText(
    'Start a project to see its board: choose New project and pick the project folder.',
  )
  await expect(page.getByRole('region', { name: 'Terminals' })).toHaveText(
    'No terminal is open yet.',
  )
  await expect(page.getByRole('button', { name: 'Project settings' })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Inbox', exact: true })).toHaveAttribute(
    'data-waiting',
    'false',
  )
  expect(await calls(page, 'board.get')).toEqual([])
})

test('says so when the page runs outside ConsensFlow, and never reads as ready', async ({
  page,
}) => {
  await page.goto(`${ui.origin}/index.html`)
  await expect(page.locator('#status')).toHaveText('ConsensFlow is not running this page.')
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'false')
  await expect(page.locator('#project-title')).toHaveText('No project')
})

// A file dropped on a terminal window types its path there, as a terminal does (2026-10-15).
// Tauri catches a drop over the whole window, so the page is told in four events of its own
// (enter, over, leave, drop), each with the paths and where the pointer is. These tests say them
// as the app does, in the places each system says them in (a Mac's in points, Windows' in device
// pixels), on a display twice as dense as the page, with the programs in real terminals.

/** The app's event `name`, said to the page as Tauri says it. */
const emitted = (page, name, payload = null) =>
  page.evaluate(
    ([name, payload]) => window.__listeners.get(name)({ event: name, id: 0, payload }),
    [name, payload],
  )

/**
 * Files dragged over the window: each event as Tauri emits it, and the drop as the app does
 * (`__drop`, with its number), `position` as the system says it.
 */
const dragged = {
  enter: (page, paths, position) => emitted(page, 'tauri://drag-enter', { paths, position }),
  over: (page, position) => emitted(page, 'tauri://drag-over', { position }),
  leave: (page) => emitted(page, 'tauri://drag-leave'),
  drop: (page, paths, position) =>
    page.evaluate(([paths, position]) => window.__drop(paths, position), [paths, position]),
}

/** A place in the page's pixels, as a system that counts `factor` of them to a pixel says it. */
const told = ({ x, y }, factor = 1) => ({ x: x * factor, y: y * factor })

/** The middle of `locator`, in the page's pixels. */
async function centreOf(locator) {
  const box = await locator.boundingBox()
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
}

/** The middle of the dock's card of `handle`, in the page's pixels. */
const middleOf = (page, handle) =>
  centreOf(page.locator(`#stage .terminal-card[data-handle="${handle}"]`))

/** harbour's chief and zeus in real terminals side by side: the board folded away, the window wide. */
async function sideBySide(page, data = model()) {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page, { ...data, realTerminals: true })
  await page.getByRole('button', { name: 'Hide board' }).click()
  await expect(page.getByRole('region', { name: 'Board' })).toBeHidden()
  await expect.poll(() => docked(page)).toEqual(['chief', 'zeus'])
}

const CHIEF_PANE = { id: 'p1-chief', generation: 5 }

/** The program in `pane` turns bracketed paste on or off, as a harness does when it starts and ends. */
async function bracketedPaste(page, pane, on) {
  const seq = ((await acked(page, pane.id)).at(-1) ?? 0) + 1
  const bytes = [...new TextEncoder().encode(`\x1b[?2004${on ? 'h' : 'l'}`)]
  await page.evaluate((message) => window.__output.onmessage(message), { ...pane, seq, bytes })
  await expect.poll(() => acked(page, pane.id)).toContain(seq)
}

/** What the page typed into pane `id`, as text: one entry for each message it sent the pane host. */
const typedInto = (page, id) =>
  page.evaluate(
    (id) =>
      window.__calls
        .filter(([command, args]) => command === 'pane_input_enqueue' && args.id === id)
        .map(([, args]) => new TextDecoder().decode(new Uint8Array(args.bytes))),
    id,
  )

/**
 * The commands the page sent from the call numbered `from` on, less the sizes and the
 * acknowledgements that the layout and the output cause.
 */
const sentSince = (page, from) =>
  page.evaluate(
    (from) =>
      window.__calls
        .slice(from)
        .map(([command]) => command)
        .filter((command) => command !== 'pane_resize' && command !== 'pane_ack'),
    from,
  )

/** The human pastes `text` into the terminal of `handle`: the paste event the browser fires at its keyboard field. */
const humanPaste = (page, handle, text) =>
  page.evaluate(
    ([handle, text]) => {
      const keyboard = document.querySelector(
        `#stage .terminal-card[data-handle="${handle}"] .xterm-helper-textarea`,
      )
      const clipboardData = new DataTransfer()
      clipboardData.setData('text/plain', text)
      keyboard.dispatchEvent(
        new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }),
      )
    },
    [handle, text],
  )

/** The handles of the dock's cards that show a drop goes to them. */
const marked = (page) =>
  page
    .locator('#stage .terminal-card[data-drop="true"]')
    .evaluateAll((cards) => cards.map((card) => card.dataset.handle))

/** The handle of the dock's card at `point`, whatever is drawn there: null where there is none. */
const cardAt = (page, { x, y }) =>
  page.evaluate(
    ([x, y]) => document.elementFromPoint(x, y)?.closest('.terminal-card')?.dataset.handle ?? null,
    [x, y],
  )

/** From now on, every line the status line says, in order: the same words said twice are two. */
const hearStatus = (page) =>
  page.evaluate(() => {
    window.__said = []
    new MutationObserver((records) => {
      for (const { addedNodes } of records) {
        for (const node of addedNodes) window.__said.push(node.textContent)
      }
    }).observe(document.querySelector('#status'), { childList: true })
  })

/** What the status line said since `hearStatus`. */
const heard = (page) => page.evaluate(() => window.__said)

test.describe('file drop on a Mac', () => {
  test.use({ userAgent: MAC_AGENT, deviceScaleFactor: 2 })

  const PATHS = [
    '/Users/me/Desktop/Screenshot 2026-10-15 at 10.30.00\u202fAM (2).png',
    "/Users/me/it's here/$HOME & [x]*.txt",
    '/Users/me/plain.txt',
  ]
  const WORDS =
    "/Users/me/Desktop/Screenshot\\ 2026-10-15\\ at\\ 10.30.00\u202fAM\\ \\(2\\).png /Users/me/it\\'s\\ here/\\$HOME\\ \\&\\ \\[x\\]\\*.txt /Users/me/plain.txt"

  test('types the paths of the files dropped on a window, each as one word of the shell, as the human’s paste is typed', async ({
    page,
  }) => {
    await sideBySide(page)
    await bracketedPaste(page, CHIEF_PANE, true)
    const before = await page.evaluate(() => window.__calls.length)
    await dragged.drop(page, PATHS, await middleOf(page, 'chief'))
    // Between the markers the program asked for, as one message, and nothing in the other window.
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual([`\x1b[200~${WORDS}\x1b[201~`])
    expect(await typedInto(page, 'p1-zeus')).toEqual([])
    // The pane host is sent those words and nothing else: no reply, nothing for the daemon, and
    // the app's window is left where a Mac puts it.
    await expect
      .poll(() => sentSince(page, before))
      .toEqual(['pane_input_enqueue', 'pane_input_wait'])
    // The human's paste of the same words gives the same bytes.
    await humanPaste(page, 'chief', WORDS)
    await expect.poll(async () => (await typedInto(page, 'p1-chief')).length).toBe(2)
    const [dropped, pasted] = await typedInto(page, 'p1-chief')
    expect(dropped).toBe(pasted)
  })

  test('types them with no markers where the program asked for none, as the human’s paste does', async ({
    page,
  }) => {
    await sideBySide(page)
    await dragged.drop(page, PATHS, await middleOf(page, 'chief'))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual([WORDS])
    await humanPaste(page, 'chief', WORDS)
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual([WORDS, WORDS])
    // Asked for and then given up again, the markers go.
    await bracketedPaste(page, CHIEF_PANE, true)
    await bracketedPaste(page, CHIEF_PANE, false)
    await dragged.drop(page, ['/a b/c.png'], await middleOf(page, 'chief'))
    await expect.poll(async () => (await typedInto(page, 'p1-chief')).at(-1)).toBe('/a\\ b/c.png')
  })

  test('goes to the window under the pointer, whose place a Mac says in points, not in device pixels', async ({
    page,
  }) => {
    await sideBySide(page)
    const [chief, zeus] = [await middleOf(page, 'chief'), await middleOf(page, 'zeus')]
    // Halved, zeus's place is in the chief's window: a Mac's points are the page's pixels already.
    expect(await cardAt(page, told(zeus, 0.5))).toBe('chief')
    await dragged.drop(page, ['/a/zeus.png'], told(zeus))
    await expect.poll(() => typedInto(page, 'p1-zeus')).toEqual(['/a/zeus.png'])
    await dragged.drop(page, ['/a/chief.png'], told(chief))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(['/a/chief.png'])
    expect(await typedInto(page, 'p1-zeus')).toEqual(['/a/zeus.png'])
    // The head of a window is the window's too: its name and its buttons are on the card.
    const head = page.locator('#stage .terminal-card[data-handle="zeus"] .terminal-head')
    await dragged.drop(page, ['/a/head.png'], told(await centreOf(head)))
    await expect.poll(() => typedInto(page, 'p1-zeus')).toEqual(['/a/zeus.png', '/a/head.png'])
  })

  test('types a path into a Devin window as it is: only Windows’ console changes what it takes', async ({
    page,
  }) => {
    const data = model()
    data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.harness =
      'devin'
    await sideBySide(page, data)
    await dragged.drop(page, ['/Users/me/a — b.png'], await middleOf(page, 'chief'))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(['/Users/me/a\\ —\\ b.png'])
    expect(await page.evaluate(() => window.__calls.some(([c]) => c === 'short_paths'))).toBe(false)
  })

  test('types nothing for a file whose name holds a control character, a line break, a tab or a delete, and names it', async ({
    page,
  }) => {
    await sideBySide(page)
    await hearStatus(page)
    const chief = await middleOf(page, 'chief')
    for (const path of ['/Users/me/a\nb.png', '/Users/me/a\tb.png', '/Users/me/a\x7fb.png']) {
      await dragged.drop(page, ['/c d', path], chief)
    }
    await expect
      .poll(() => heard(page))
      .toEqual(
        ['/Users/me/a␊b.png', '/Users/me/a␉b.png', '/Users/me/a␡b.png'].map(
          (shown) =>
            `Nothing was typed: the name of ${shown} holds a control character, which no window can take.`,
        ),
      )
    expect(await typedInto(page, 'p1-chief')).toEqual([])
  })

  test('gives the keyboard to the window it went to, so what the human types next follows the path', async ({
    page,
  }) => {
    await sideBySide(page)
    await page.locator('#stage .terminal-card[data-handle="chief"] .xterm-helper-textarea').focus()
    await expect.poll(() => typingIn(page)).toBe('chief')
    await dragged.drop(page, ['/a/zeus.png'], await middleOf(page, 'zeus'))
    await expect.poll(() => typingIn(page)).toBe('zeus')
  })

  test('does nothing outside a window: on the board, the projects, the dock’s own head, or under a dialog', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1000, height: 700 })
    await open(page, { ...model(), realTerminals: true })
    await expect.poll(() => docked(page)).toEqual(['chief', 'zeus'])
    const paths = ['/a/b.png']
    const elsewhere = [
      await centreOf(page.getByRole('region', { name: 'Board' })),
      await centreOf(page.getByTestId('projects')),
      await centreOf(page.locator('.dock-head')),
    ]
    for (const place of elsewhere) {
      expect(await cardAt(page, place)).toBeNull()
      await dragged.enter(page, paths, told(place))
      expect(await marked(page)).toEqual([])
      await dragged.drop(page, paths, told(place))
    }
    // Under a dialog, with the chief's window drawn behind it: a place the dialog covers, and a
    // place on its backdrop, which cover the window all the same.
    const chief = page.locator('#stage .terminal-card[data-handle="chief"]')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: 'Settings', exact: true })
    await expect(dialog).toBeVisible()
    const [card, over] = [await chief.boundingBox(), await dialog.boundingBox()]
    const left = Math.max(card.x, over.x)
    const right = Math.min(card.x + card.width, over.x + over.width)
    const top = Math.max(card.y, over.y)
    const bottom = Math.min(card.y + card.height, over.y + over.height)
    expect(right - left, 'the dialog covers part of the chief’s window').toBeGreaterThan(20)
    const covered = { x: (left + right) / 2, y: (top + bottom) / 2 }
    const backdrop = { x: card.x + card.width - 8, y: card.y + card.height - 8 }
    for (const place of [covered, backdrop]) {
      expect(await cardAt(page, place)).toBeNull()
      await dragged.enter(page, paths, told(place))
      expect(await marked(page)).toEqual([])
      await dragged.drop(page, paths, told(place))
    }
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    expect(await typedInto(page, 'p1-zeus')).toEqual([])
    await expect(page.locator('#status')).toHaveText('')
  })

  test('types nothing where a window is not running, and says so; the others still take a drop', async ({
    page,
  }) => {
    const data = model()
    // The chief's window is down: its card is there, with no terminal.
    const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
    Object.assign(chief, { activity: { state: 'closed' }, pane: null })
    await sideBySide(page, data)
    await expect(
      page.locator('#stage .terminal-card[data-handle="chief"] .terminal-host'),
    ).toHaveCount(0)
    const place = await middleOf(page, 'chief')
    // No window, so nothing to show a drop goes there.
    await dragged.enter(page, ['/a/b.png'], told(place))
    expect(await marked(page)).toEqual([])
    await dragged.drop(page, ['/a/b.png'], told(place))
    await expect(page.locator('#status')).toHaveText(
      "Nothing was typed: Chief of Staff's window is not running.",
    )
    await expect(page.locator('#status')).toHaveAttribute('data-tone', 'error')
    expect(await typedInto(page, 'p1-zeus')).toEqual([])
    // The window beside it takes a drop as ever.
    await dragged.drop(page, ['/a/zeus.png'], await middleOf(page, 'zeus'))
    await expect.poll(() => typedInto(page, 'p1-zeus')).toEqual(['/a/zeus.png'])
  })

  test('types nothing where a window ended under the file, and says so; nothing where none did', async ({
    page,
  }) => {
    await sideBySide(page)
    await hearStatus(page)
    /** zeus's window ends, or comes up again as its `generation`. */
    const zeusWindow = (generation) =>
      changed(
        page,
        (generation) => {
          const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
          Object.assign(
            lane,
            generation === null
              ? { activity: { state: 'closed' }, pane: null }
              : { activity: { state: 'idle' }, pane: { id: 'p1-zeus', generation } },
          )
        },
        generation,
      )
    const zeus = await middleOf(page, 'zeus')
    await dragged.enter(page, ['/a/b.png'], told(zeus))
    expect(await marked(page)).toEqual(['zeus'])
    // Its window ends: its card goes, the chief's taking the row the place is
    // in — the drop was meant for the window that went, not the card there now.
    await zeusWindow(null)
    await expect.poll(() => docked(page)).toEqual(['chief'])
    expect(await cardAt(page, zeus)).toBe('chief')
    await dragged.over(page, told(zeus))
    expect(await marked(page)).toEqual([])
    // Dropped where it was: nothing is typed, and the status line says the window went.
    await dragged.drop(page, ['/a/b.png'], told(zeus))
    const went = "Nothing was typed: @zeus's window has closed."
    await expect.poll(() => heard(page)).toEqual([went])
    await expect(page.locator('#status')).toHaveAttribute('data-tone', 'error')
    // The next drag, dropped there with no window ending under it, is the
    // chief's card's like anywhere on it: it takes the drop and says nothing.
    await dragged.enter(page, ['/a/b.png'], told(zeus))
    expect(await marked(page)).toEqual(['chief'])
    await dragged.drop(page, ['/a/b.png'], told(zeus))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(['/a/b.png'])
    // Nor does a drop elsewhere, though a window ended under the file before it moved on.
    await zeusWindow(8)
    await expect.poll(() => docked(page)).toEqual(['chief', 'zeus'])
    await dragged.enter(page, ['/a/b.png'], told(await middleOf(page, 'zeus')))
    expect(await marked(page)).toEqual(['zeus'])
    await zeusWindow(null)
    await expect.poll(() => docked(page)).toEqual(['chief'])
    const head = told(await centreOf(page.locator('.dock-head')))
    await dragged.over(page, head)
    await dragged.drop(page, ['/a/b.png'], head)
    expect(await heard(page)).toEqual([went])
    expect(await typedInto(page, 'p1-zeus')).toEqual([])
    expect(await typedInto(page, 'p1-chief')).toEqual(['/a/b.png'])
  })

  test('types nothing on a closed project’s dock, though an open one’s windows live on off screen', async ({
    page,
  }) => {
    await open(page, { ...model(), realTerminals: true })
    await expect.poll(() => docked(page)).toEqual(['chief', 'zeus'])
    await hearStatus(page)
    await chooseProject(page, 'foundry')
    const stage = page.getByRole('region', { name: 'Terminals' })
    await expect(stage).toHaveText('No terminal is open yet.')
    const place = await centreOf(stage)
    expect(await cardAt(page, place)).toBeNull()
    await dragged.enter(page, ['/a/b.png'], told(place))
    await dragged.drop(page, ['/a/b.png'], told(place))
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    expect(await typedInto(page, 'p1-zeus')).toEqual([])
    expect(await heard(page)).toEqual([])
  })

  test('marks the window a drop would go to, from the enter to the leave or the drop', async ({
    page,
  }) => {
    await sideBySide(page)
    const [chief, zeus] = [await middleOf(page, 'chief'), await middleOf(page, 'zeus')]
    const cards = ['chief', 'zeus'].map((name) =>
      page.locator(`#stage .terminal-card[data-handle="${name}"]`),
    )
    const ring = /rgb\(99, 199, 178\)/
    expect(await marked(page)).toEqual([])
    await expect(cards[0]).toHaveCSS('box-shadow', 'none')
    // Entered over the chief's window: it shows, and its ring is the app's own seafoam.
    await dragged.enter(page, ['/a/b.png'], told(chief))
    expect(await marked(page)).toEqual(['chief'])
    await expect(cards[0]).toHaveCSS('box-shadow', ring)
    await expect(cards[1]).toHaveCSS('box-shadow', 'none')
    // Its tint covers the whole card, and lets the pointer through.
    const tint = await cards[0].evaluate((card) => {
      const style = getComputedStyle(card, '::after')
      return {
        pointer: style.pointerEvents,
        size: [Number.parseFloat(style.width), Number.parseFloat(style.height)],
        card: [card.clientWidth, card.clientHeight],
      }
    })
    expect(tint.pointer).toBe('none')
    expect(Math.abs(tint.size[0] - tint.card[0])).toBeLessThan(1)
    expect(Math.abs(tint.size[1] - tint.card[1])).toBeLessThan(1)
    // Moved over zeus's: the mark moves with it, and is on one window at a time.
    await dragged.over(page, told(zeus))
    expect(await marked(page)).toEqual(['zeus'])
    await expect(cards[0]).toHaveCSS('box-shadow', 'none')
    await expect(cards[1]).toHaveCSS('box-shadow', ring)
    // Over the dock's head, which is no window: no mark.
    await dragged.over(page, told(await centreOf(page.locator('.dock-head'))))
    expect(await marked(page)).toEqual([])
    // Back over a window, then gone from the app's window: the mark goes with the leave.
    await dragged.over(page, told(chief))
    expect(await marked(page)).toEqual(['chief'])
    await dragged.leave(page)
    expect(await marked(page)).toEqual([])
    // An over that follows the leave, with nothing entered, marks nothing.
    await dragged.over(page, told(chief))
    expect(await marked(page)).toEqual([])
    // The mark goes with a drop too, which no leave comes before.
    await dragged.enter(page, ['/a/b.png'], told(zeus))
    expect(await marked(page)).toEqual(['zeus'])
    await dragged.drop(page, ['/a/b.png'], told(zeus))
    expect(await marked(page)).toEqual([])
    await expect(cards[1]).toHaveCSS('box-shadow', 'none')
    // So does one that follows the drop.
    await dragged.over(page, told(zeus))
    expect(await marked(page)).toEqual([])
  })

  test('marks nothing and types nothing for a drag that carries no file', async ({ page }) => {
    await sideBySide(page)
    const chief = await middleOf(page, 'chief')
    await dragged.enter(page, [], told(chief))
    await dragged.over(page, told(chief))
    expect(await marked(page)).toEqual([])
    await dragged.drop(page, [], told(chief))
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    // Not even the markers of an empty paste.
    await bracketedPaste(page, CHIEF_PANE, true)
    await dragged.drop(page, [], told(chief))
    expect(await typedInto(page, 'p1-chief')).toEqual([])
  })
})

test.describe('file drop on Windows', () => {
  test.use({ userAgent: WINDOWS_AGENT, deviceScaleFactor: 2 })

  const PATHS = [
    'C:\\Users\\me\\My Pictures\\a b.png',
    'C:\\Users\\me\\plain.txt',
    '\\\\server\\share\\x y (2).txt',
  ]
  const WORDS =
    '"C:\\Users\\me\\My Pictures\\a b.png" C:\\Users\\me\\plain.txt "\\\\server\\share\\x y (2).txt"'

  test('types the paths of the files dropped on a window, each quoted if it has a space, as the human’s paste is typed', async ({
    page,
  }) => {
    await sideBySide(page)
    await bracketedPaste(page, CHIEF_PANE, true)
    const before = await page.evaluate(() => window.__calls.length)
    await dragged.drop(page, PATHS, told(await middleOf(page, 'chief'), 2))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual([`\x1b[200~${WORDS}\x1b[201~`])
    // The app's window comes in front, so the keys pressed next go to it and not to Explorer.
    await expect
      .poll(async () => (await sentSince(page, before)).sort())
      .toEqual(['focus_after_drop', 'pane_input_enqueue', 'pane_input_wait'])
    await humanPaste(page, 'chief', WORDS)
    await expect.poll(async () => (await typedInto(page, 'p1-chief')).length).toBe(2)
    const [dropped, pasted] = await typedInto(page, 'p1-chief')
    expect(dropped).toBe(pasted)
  })

  test('goes to the window under the pointer, whose place Windows says in device pixels', async ({
    page,
  }) => {
    await sideBySide(page)
    const [chief, zeus] = [await middleOf(page, 'chief'), await middleOf(page, 'zeus')]
    // As it is said, zeus's place is outside the page: it takes the display's scale to bring it in.
    expect(await cardAt(page, told(zeus, 2))).toBeNull()
    await dragged.drop(page, ['C:\\a\\zeus.png'], told(zeus, 2))
    await expect.poll(() => typedInto(page, 'p1-zeus')).toEqual(['C:\\a\\zeus.png'])
    await dragged.enter(page, ['C:\\a\\chief.png'], told(chief, 2))
    expect(await marked(page)).toEqual(['chief'])
    await dragged.drop(page, ['C:\\a\\chief.png'], told(chief, 2))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(['C:\\a\\chief.png'])
    expect(await typedInto(page, 'p1-zeus')).toEqual(['C:\\a\\zeus.png'])
  })

  /** harbour with its chief on `harness`, the short names Windows keeps as `shortNames` says. */
  const chiefOn = (harness, shortNames = {}) => {
    const data = model()
    data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.harness =
      harness
    return { ...data, shortNames }
  }
  /** A path with marks, which every window takes whole, and one with a character Codex drops. */
  const DASHED = 'C:\\Users\\me\\a — b “c”.png'
  const SMILING = 'C:\\Users\\me\\a 🙂 b.png'
  const ALIASED = { [SMILING]: { short: 'C:\\Users\\me\\A-B~1.PNG' } }
  /** The paths the page asked the short names of, each time it asked. */
  const askedShort = (page) =>
    page.evaluate(() =>
      window.__calls.filter(([command]) => command === 'short_paths').map(([, args]) => args.paths),
    )
  /** The number of the drop the page asked `command` of, and the paths where it named some, each time it asked. */
  const askedOf = (page, command) =>
    page.evaluate(
      (command) =>
        window.__calls
          .filter(([asked]) => asked === command)
          .map(([, args]) => (args?.paths === undefined ? args?.id : [args.id, args.paths])),
      command,
    )
  /** How many times the page asked the app to come in front after a drop. */
  const focusAsked = (page) =>
    page.evaluate(() => window.__calls.filter(([c]) => c === 'focus_after_drop').length)

  test('types a path with marks into a Devin window whole, and asks no short name', async ({
    page,
  }) => {
    await sideBySide(page, chiefOn('devin', ALIASED))
    await dragged.drop(page, [DASHED, SMILING], told(await middleOf(page, 'chief'), 2))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual([`"${DASHED}" "${SMILING}"`])
    expect(await askedShort(page)).toEqual([])
  })

  test('types a path into a Codex window by its short name where the long one holds a character Codex drops', async ({
    page,
  }) => {
    await sideBySide(page, chiefOn('codex', ALIASED))
    const paths = [
      SMILING,
      DASHED,
      'C:\\Users\\me\\My Pictures\\plain.txt',
      'C:\\Users\\me\\résumé.txt',
    ]
    await dragged.drop(page, paths, told(await middleOf(page, 'chief'), 2))
    // The others as they are: marks, letters and ASCII reach it whole. Only the one is asked for.
    await expect
      .poll(() => typedInto(page, 'p1-chief'))
      .toEqual([
        `C:\\Users\\me\\A-B~1.PNG "${DASHED}" "C:\\Users\\me\\My Pictures\\plain.txt" C:\\Users\\me\\résumé.txt`,
      ])
    expect(await askedShort(page)).toEqual([[SMILING]])
    // The human did nothing meanwhile: the window has the keyboard, and the app comes in front.
    await expect.poll(() => typingIn(page)).toBe('chief')
    await expect.poll(() => focusAsked(page)).toBe(1)
    // Both asked of the drop by the number the app told it by.
    expect(await askedOf(page, 'short_paths')).toEqual([[1, [SMILING]]])
    expect(await askedOf(page, 'focus_after_drop')).toEqual([1])
    // A human paste into it goes as it was typed (T-548).
    await humanPaste(page, 'chief', 'a — b')
    await expect.poll(async () => (await typedInto(page, 'p1-chief')).at(-1)).toBe('a — b')
  })

  // What a drop may carry is the harness the window was launched with, which the daemon says with
  // its pane (T-1336): a member edited to another harness runs it from its next window.
  for (const [launched, configured, typed, asked] of [
    ['devin', 'codex', [`"${SMILING}"`], []],
    ['codex', 'devin', ['C:\\Users\\me\\A-B~1.PNG'], [[SMILING]]],
  ]) {
    test(`goes by ${launched}, which a window was launched with, and not ${configured}, which its member is set to run now`, async ({
      page,
    }) => {
      const data = chiefOn(configured, ALIASED)
      const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
      chief.pane = { ...CHIEF_PANE, harness: launched }
      await sideBySide(page, data)
      await dragged.drop(page, [SMILING], told(await middleOf(page, 'chief'), 2))
      await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(typed)
      expect(await askedShort(page)).toEqual(asked)
    })
  }

  test('types nothing into a Codex window for a path with a character Codex drops and no short name it takes, and says why in the app’s words', async ({
    page,
  }) => {
    const KEPT = 'C:\\Users\\me\\x🙂y.png'
    const SHARED = '\\\\server\\share\\a 𝐀 b.png'
    // A volume that keeps no short names (the stand-in's answer where it is told none), an
    // alias that still holds what Codex drops, and a share that did not answer.
    const answered = {
      [KEPT]: { short: 'C:\\Users\\me\\X🙂Y~1.PNG' },
      [SHARED]: { error: 'its network share did not answer' },
    }
    await sideBySide(page, chiefOn('codex', answered))
    await hearStatus(page)
    const chief = told(await middleOf(page, 'chief'), 2)
    await dragged.drop(page, ['C:\\Users\\me\\plain.txt', SMILING], chief)
    await dragged.drop(page, [KEPT], chief)
    await dragged.drop(page, [SHARED], chief)
    const cannot = "Nothing was typed: Chief of Staff's window cannot take"
    await expect
      .poll(() => heard(page))
      .toEqual([
        `${cannot} 🙂 (U+1F642) of ${SMILING}, and Windows keeps no short name for it.`,
        `${cannot} 🙂 (U+1F642) of ${KEPT}, and its short name holds them too.`,
        `${cannot} 𝐀 (U+1D400) of ${SHARED}, and its network share did not answer.`,
      ])
    await expect(page.locator('#status')).toHaveAttribute('data-tone', 'error')
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    expect(await focusAsked(page)).toBe(0)
  })

  test('types a drop typed after the human acted into the window it was dropped on, and takes no focus', async ({
    page,
  }) => {
    await sideBySide(page, chiefOn('codex', ALIASED))
    const chief = told(await middleOf(page, 'chief'), 2)
    const zeusKeys = page.locator(
      '#stage .terminal-card[data-handle="zeus"] .xterm-helper-textarea',
    )
    // What the human may do while the short names are read: click another window, press a
    // key, or leave the app's window for another program.
    const acts = [
      [
        'a click on another window',
        () => page.locator('#stage .terminal-card[data-handle="zeus"] .terminal-host').click(),
      ],
      ['a key pressed', () => page.keyboard.press('Shift')],
      ['the app left', () => page.evaluate(() => window.dispatchEvent(new Event('blur')))],
    ]
    for (const [at, [name, act]] of acts.entries()) {
      await zeusKeys.focus()
      await hold(page, 'short_paths')
      await dragged.drop(page, [SMILING], chief)
      await expect.poll(() => askedShort(page)).toHaveLength(at + 1)
      await act()
      await letGo(page, 'short_paths')
      await expect.poll(() => typedInto(page, 'p1-chief'), name).toHaveLength(at + 1)
      expect(await typingIn(page), name).toBe('zeus')
    }
    expect(await focusAsked(page)).toBe(0)
  })

  test('types the paths of the drops in the order they came, one waiting for the short names of the one before', async ({
    page,
  }) => {
    await sideBySide(page, chiefOn('codex', ALIASED))
    await hold(page, 'short_paths')
    const chief = told(await middleOf(page, 'chief'), 2)
    await dragged.drop(page, [SMILING], chief)
    await dragged.drop(page, ['C:\\Users\\me\\second.png'], chief)
    await page.waitForTimeout(100)
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    await letGo(page, 'short_paths')
    await expect
      .poll(() => typedInto(page, 'p1-chief'))
      .toEqual(['C:\\Users\\me\\A-B~1.PNG', 'C:\\Users\\me\\second.png'])
  })

  test('types every drop that came while one waited for its short names, each looked up as the drop it is', async ({
    page,
  }) => {
    const B = 'C:\\Users\\me\\b 🙂 2.png'
    const C = 'C:\\Users\\me\\c 🙂 3.png'
    const data = chiefOn('codex', {
      ...ALIASED,
      [B]: { short: 'C:\\Users\\me\\B-2~1.PNG' },
      [C]: { short: 'C:\\Users\\me\\C-3~1.PNG' },
    })
    data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus').participant.harness =
      'codex'
    await sideBySide(page, data)
    await hearStatus(page)
    const chief = told(await middleOf(page, 'chief'), 2)
    const zeus = told(await middleOf(page, 'zeus'), 2)
    // The first drop's short names are slow to come (a share far away); meanwhile one more
    // file is dropped on zeus, and another on the chief.
    await hold(page, 'short_paths')
    await dragged.drop(page, [SMILING], chief)
    await expect.poll(() => askedShort(page)).toHaveLength(1)
    await dragged.drop(page, [B], zeus)
    await dragged.drop(page, [C], chief)
    await letGo(page, 'short_paths')
    await expect
      .poll(() => typedInto(page, 'p1-chief'))
      .toEqual(['C:\\Users\\me\\A-B~1.PNG', 'C:\\Users\\me\\C-3~1.PNG'])
    await expect.poll(() => typedInto(page, 'p1-zeus')).toEqual(['C:\\Users\\me\\B-2~1.PNG'])
    expect(await askedOf(page, 'short_paths')).toEqual([
      [1, [SMILING]],
      [2, [B]],
      [3, [C]],
    ])
    expect(await heard(page)).toEqual([])
  })

  test('types a path whose short names the app would not look up while another lookup ran, once it can', async ({
    page,
  }) => {
    // A page before a reload left a lookup reading a share far away: the app is busy
    // with it for the first two asks.
    await sideBySide(page, { ...chiefOn('codex', ALIASED), busyLookups: 2 })
    await hearStatus(page)
    await dragged.drop(page, [SMILING], told(await middleOf(page, 'chief'), 2))
    await expect.poll(() => typedInto(page, 'p1-chief')).toEqual(['C:\\Users\\me\\A-B~1.PNG'])
    expect(await askedShort(page)).toEqual([[SMILING], [SMILING], [SMILING]])
    expect(await heard(page)).toEqual([])
  })

  test('types nothing into a window that closed while the short names were read, and says so', async ({
    page,
  }) => {
    await sideBySide(page, chiefOn('codex', ALIASED))
    await hearStatus(page)
    await hold(page, 'short_paths')
    await dragged.drop(page, [SMILING], told(await middleOf(page, 'chief'), 2))
    await expect.poll(() => askedShort(page)).toEqual([[SMILING]])
    await changed(page, () => {
      const chief = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
      Object.assign(chief, { activity: { state: 'closed' }, pane: null })
    })
    await expect(
      page.locator('#stage .terminal-card[data-handle="chief"] .terminal-host'),
    ).toHaveCount(0)
    await letGo(page, 'short_paths')
    await expect
      .poll(() => heard(page))
      .toEqual(["Nothing was typed: Chief of Staff's window has closed."])
    expect(await typedInto(page, 'p1-chief')).toEqual([])
  })

  test('types nothing for a path with a control character, which no Windows name holds, and names it', async ({
    page,
  }) => {
    await sideBySide(page)
    await hearStatus(page)
    await dragged.drop(page, ['C:\\Users\\me\\a\tb.txt'], told(await middleOf(page, 'chief'), 2))
    await expect
      .poll(() => heard(page))
      .toEqual([
        'Nothing was typed: the name of C:\\Users\\me\\a\u2409b.txt holds a control character, which no window can take.',
      ])
    expect(await typedInto(page, 'p1-chief')).toEqual([])
    expect(await focusAsked(page)).toBe(0)
  })
})

test('hears each event of the app with one listener, as the app keeps every one it is given', async ({
  page,
}) => {
  await open(page)
  const listeners = await page.evaluate(() =>
    Object.fromEntries([...window.__handlers].map(([name, all]) => [name, all.length])),
  )
  expect(listeners).toEqual({
    'state-changed': 1,
    'daemon-status': 1,
    'tauri://drag-enter': 1,
    'tauri://drag-over': 1,
    'tauri://drag-leave': 1,
    'file-drop': 1,
    'update-state-changed': 1,
    'check-updates': 1,
  })
})

/**
 * Models are shown by their id (spec harness-models, HMD-12): every model select offers each
 * model as its harness takes it, never the name its harness's list gives it (`Opus`); Pi's
 * with its provider, Devin's by the id its list holds with the effort picked beside it.
 */
const IDS = {
  'Claude Code': ['claude-opus-5-5', 'claude-fable-5-1', 'claude-sonnet-5-5', 'claude-haiku-5-5'],
  Codex: ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6.1-terra', 'gpt-6-sol'],
  Devin: ['adaptive', 'swe-2', 'swe-2-lightning', 'claude-opus-5-5', 'claude-opus-5-5-fast'],
  'Muse Code': ['muse-wonder-1.0', 'muse-spark-1.3', 'muse-spark-1.3-contributor'],
  Pi: [
    'anthropic/claude-fable-5',
    'anthropic/claude-fable-5-1',
    'anthropic/claude-haiku-4-5',
    'kimi-coding/k3',
  ],
}

test("the model selects of New project, Switch chief, Add member and Edit member show each model's id, as its harness takes it", async ({
  page,
}) => {
  const data = shotData('after')
  await open(page, data)
  const ids = async (select) =>
    (await offered(select)).map(([text]) => text).filter((text) => text !== 'Pick a model')
  // A listed id is there, in each harness's list, and no display name is.
  const known = (harness) =>
    JSON.parse(
      readFileSync(join(import.meta.dirname, 'fixtures', 'models', `${harness}.json`), 'utf8'),
    ).models
  const listedModels = ['claude', 'codex', 'devin', 'muse', 'pi'].flatMap(known)
  const idsOf = new Set(
    listedModels.flatMap((model) => [
      model.id,
      model.provider ? `${model.provider}/${model.id}` : model.id,
    ]),
  )
  // The names the lists give their models, which are no ids (`Opus`, `GPT-6.1-Sol`).
  const labels = new Set(
    listedModels.map((model) => model.label).filter((label) => !idsOf.has(label)),
  )
  for (const [button, name] of [
    ['New project', 'New project'],
    ['Switch the chief to another harness or model', 'Switch the chief'],
  ]) {
    await page.getByRole('button', { name: button }).click()
    const dialog = page.getByRole('dialog', { name })
    for (const [harness, wanted] of Object.entries(IDS)) {
      await dialog.locator('[name="chiefHarness"]').selectOption({ label: harness })
      const models = dialog.locator('[name="chiefModel"]')
      await expect(models).toBeEnabled()
      const offeredIds = await ids(models)
      expect(offeredIds.slice(0, wanted.length), `${name}: ${harness}`).toEqual(wanted)
      for (const text of offeredIds)
        expect(labels.has(text), `${name}: ${text} is a label`).toBe(false)
    }
    // Devin: the listed id, and its effort picked beside it.
    await dialog.locator('[name="chiefHarness"]').selectOption({ label: 'Devin' })
    await dialog.locator('[name="chiefModel"]').selectOption({ label: 'swe-2-lightning' })
    expect(await offered(dialog.locator('[name="chiefEffort"]'))).toEqual([
      ['Pick an effort', true],
      ['medium', false],
      ['high', false],
      ['max', false],
    ])
    await dialog.getByRole('button', { name: 'Cancel' }).click()
  }
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  const add = settings.locator('.staff-add')
  for (const [harness, wanted] of Object.entries(IDS)) {
    await add.locator('[name="memberHarness"]').selectOption({ label: harness })
    await expect(add.locator('[name="memberModel"]')).toBeEnabled()
    expect((await ids(add.locator('[name="memberModel"]'))).slice(0, wanted.length)).toEqual(wanted)
  }
  // Edit member: the select holds the member's own id, picked, and the others by theirs.
  await settings.getByRole('button', { name: 'Edit @claude-opus-5-5-high' }).first().click()
  await expect(add.locator('[name="memberModel"]')).toHaveValue('claude-opus-5-5')
  expect((await ids(add.locator('[name="memberModel"]'))).slice(0, 4)).toEqual(IDS['Claude Code'])
})

/**
 * A member's name holds its model and effort (`@claude-opus-5-5-high`), so what stands under
 * it is its role, its tier and its harness, as the harness is called; a name that holds none
 * (the image designer on its harness's default) is followed by what it runs on too.
 */
test("a member's card says its role, tier and harness under a name that holds its model, and what it runs on only under one that does not", async ({
  page,
}) => {
  await open(page, shotData('after'))
  const meta = (handle, role) =>
    page.locator(`tr[data-handle="${handle}"][data-role="${role}"] .row-meta`)
  await expect(meta('claude-opus-5-5-high', 'worker')).toHaveText('worker · complex · Claude Code')
  await expect(meta('claude-opus-5-5-high', 'tester')).toHaveText('tester · complex · Claude Code')
  await expect(meta('claude-fable-5-1-max', 'reviewer')).toHaveText(
    'reviewer · critical · Claude Code',
  )
  await expect(meta('gpt-6-astra-xhigh', 'advisor')).toHaveText('advisor · critical · Codex')
  await expect(meta('swe-2-max', 'worker')).toHaveText('worker · light · Devin')
  // A window of one: whose it is, and its harness.
  await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
  await expect(meta('claude-opus-5-5-high-amber-pine', 'worker')).toHaveText(
    'worker session of @claude-opus-5-5-high · Claude Code',
  )
  // The designer's name holds no model: what it runs on is said (its harness's default). A
  // design task has no tier, so none is said of it (designer P2).
  await expect(meta('codex-default', 'designer')).toHaveText('designer · Codex · Codex default')
  // The same two lines in Project settings' table, where the role is a column of its own.
  await page.getByRole('button', { name: 'Project settings' }).click()
  const table = page
    .getByRole('dialog', { name: 'Project settings' })
    .getByRole('table', { name: 'On the staff' })
  const rows = table.locator('tbody tr')
  await expect(rows.filter({ hasText: '@gpt-6-astra-xhigh' }).locator('.member-meta')).toHaveText(
    'critical · Codex',
  )
  await expect(rows.filter({ hasText: '@codex-default' }).locator('.member-meta')).toHaveText(
    'Codex default · Codex',
  )
  // It has nothing to edit: Codex's own model, no effort, no tier. A member has an Edit.
  const designer = rows.filter({ hasText: '@codex-default' })
  await expect(
    designer.getByRole('button', { name: 'Remove Image designer @codex-default' }),
  ).toBeVisible()
  await expect(designer.getByRole('button', { name: /^Edit/ })).toHaveCount(0)
  await expect(
    rows.filter({ hasText: '@gpt-6-astra-xhigh' }).getByRole('button', { name: /^Edit/ }),
  ).toHaveCount(1)
})

/**
 * The name holds the model (HMD-12): Edit member to another model or effort renames the member,
 * and the board shows the new name on its rows and its windows', the status line says it.
 */
test('Edit member to another model renames the member: its rows and its windows take the new name, and the status line says so', async ({
  page,
}) => {
  await open(page, shotData('after'))
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  await settings.getByRole('button', { name: 'Edit @claude-opus-5-5-high' }).first().click()
  await pickMember(settings, {
    harness: 'Codex',
    model: 'gpt-6.1-sol',
    effort: 'max',
    tier: 'critical',
  })
  await settings.getByRole('button', { name: 'Save member' }).click()
  await expect(page.locator('#status')).toHaveText(
    '@claude-opus-5-5-high is @gpt-6-1-sol-max now, and runs gpt-6.1-sol from its next window.',
  )
  expect(await calls(page, 'member.edit')).toEqual([
    {
      project: 1,
      member: 'claude-opus-5-5-high',
      harness: 'codex',
      model: 'gpt-6.1-sol',
      effort: 'max',
      tier: 'critical',
    },
  ])
  await settings.getByRole('button', { name: 'Close' }).click()
  await expect(page.locator('tr[data-handle="claude-opus-5-5-high"]')).toHaveCount(0)
  await expect(page.locator('tr[data-handle="gpt-6-1-sol-max"] .row-name')).toHaveText([
    '@gpt-6-1-sol-max',
    '@gpt-6-1-sol-max',
  ])
  await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
  await expect(page.locator('tr[data-handle="gpt-6-1-sol-max-amber-pine"] .row-name')).toHaveText(
    '@gpt-6-1-sol-max · amber-pine',
  )
})

/**
 * A window at work when its member is edited (HMD-12): the daemon renames the member and the
 * window, and the window goes on. The page shows its terminal by the window's pane, never by the
 * name it had: after the edit the card is there under the new name, its emulator is not
 * disposed, nothing says the window is gone, and it is still in the dock the human put it in.
 */
test('Edit member renames a member whose window is at work and its terminal stays: the same card, emulator and place, under the new name', async ({
  page,
}) => {
  await open(page, shotData('after'))
  await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
  await page
    .getByRole('button', { name: "Show @claude-opus-5-5-high · amber-pine's terminal" })
    .click()
  const card = (handle) => page.locator(`#stage .terminal-card[data-handle="${handle}"]`)
  await expect(card('claude-opus-5-5-high-amber-pine')).toHaveCount(1)
  const emulator = () =>
    page.evaluate(() => {
      const found = window.__emulators.filter(
        (one) => document.querySelector('#stage')?.contains(one.host) || one.disposed,
      )
      return found.map((one) => one.disposed)
    })
  expect(await emulator()).toEqual([false, false])
  await page.getByRole('button', { name: 'Project settings' }).click()
  const settings = page.getByRole('dialog', { name: 'Project settings' })
  await settings.getByRole('button', { name: 'Edit @claude-opus-5-5-high' }).first().click()
  await pickMember(settings, {
    harness: 'Codex',
    model: 'gpt-6.1-sol',
    effort: 'max',
    tier: 'critical',
  })
  await settings.getByRole('button', { name: 'Save member' }).click()
  await expect(page.locator('#status')).toContainText('is @gpt-6-1-sol-max now')
  await settings.getByRole('button', { name: 'Close' }).click()
  // The window's card is the one it was, under its new name: none went, none was made.
  await expect(card('gpt-6-1-sol-max-amber-pine')).toHaveCount(1)
  await expect(card('claude-opus-5-5-high-amber-pine')).toHaveCount(0)
  await expect(page.locator('#stage .terminal-gone, #stage .stage-empty')).toHaveCount(0)
  expect(await emulator()).toEqual([false, false])
})

/**
 * Shots for the review of the model ids (HMD-12): the staff list, New project's and Switch
 * chief's selects, and a member's card before and after an edit, in dark and light. Not a
 * check: it writes PNGs into `CF_SHOTS_DIR` where that is set, and skips where it is not.
 * `CF_SHOTS_VARIANT` says the board of the shot: `before` is alpha.2's (names and labels from
 * the harness's list), `after` this build's (ids), on the recorded lists of the harnesses.
 */
const SHOTS = process.env.CF_SHOTS_DIR
const SHOT_VARIANT = process.env.CF_SHOTS_VARIANT ?? 'after'

/** The recorded list of `harness`, as a harness gives one. */
const recorded = (harness) =>
  listed(
    JSON.parse(
      readFileSync(join(import.meta.dirname, 'fixtures', 'models', `${harness}.json`), 'utf8'),
    ).models,
  )

/** The staff of a real board (kluris's), as the variant names it. */
function shotData(variant) {
  const after = variant === 'after'
  const name = (before, id) => (after ? id : before)
  const by = (label, id) => (after ? id : label)
  const data = model()
  data.models.claude.listing = recorded('claude')
  data.models.codex.listing = recorded('codex')
  data.models.devin.listing = recorded('devin')
  data.models.muse.listing = recorded('muse')
  data.models.pi.listing = recorded('pi')
  const staff = (id, handle, roles, tier, extra) =>
    participant(id, handle, roles[0], {
      roles,
      tier,
      ...(after ? { nameApart: false } : { agent: null }),
      ...extra,
    })
  const opus = staff(
    3,
    name('opus-high', 'claude-opus-5-5-high'),
    ['worker', 'tester'],
    'complex',
    {
      harness: 'claude-code',
      model: 'claude-opus-5-5',
      effort: 'high',
      modelName: by('Opus', 'claude-opus-5-5'),
    },
  )
  const fable = staff(4, name('fable-max', 'claude-fable-5-1-max'), ['reviewer'], 'critical', {
    harness: 'claude-code',
    model: 'claude-fable-5-1',
    effort: 'max',
    modelName: by('Fable', 'claude-fable-5-1'),
  })
  const astra = staff(5, name('gpt-6-astra-xhigh', 'gpt-6-astra-xhigh'), ['advisor'], 'critical', {
    harness: 'codex',
    model: 'gpt-6-astra',
    effort: 'xhigh',
    modelName: by('GPT-6-Astra', 'gpt-6-astra'),
  })
  const swe = staff(6, 'swe-2-max', ['worker'], 'light', {
    harness: 'devin',
    model: 'swe-2-max',
    effort: 'max',
    modelName: by('SWE-2', 'swe-2'),
  })
  const sonnet = staff(
    7,
    name('sonnet-xhigh', 'claude-sonnet-5-5-xhigh'),
    ['worker', 'reviewer'],
    'standard',
    {
      harness: 'claude-code',
      model: 'claude-sonnet-5-5',
      effort: 'xhigh',
      modelName: by('Sonnet', 'claude-sonnet-5-5'),
    },
  )
  const designer = staff(8, 'codex-default', ['designer'], 'light', {
    harness: 'codex',
    designer: true,
    ...(after ? { nameApart: true } : {}),
  })
  const window = (member, id, words) =>
    session(id, member, words, 'worker', after ? { nameApart: false } : {})
  const apart = (member) => (after ? member : member)
  const lanes = data.boards[1].lanes
  lanes.splice(
    2,
    2,
    {
      participant: apart(opus),
      tasks: [task(2, 'Write the parser', 'working', 'chief', opus.handle, 4)],
      activity: { state: 'working' },
      pane: null,
    },
    {
      participant: window(opus, 10, 'amber-pine'),
      tasks: [task(7, 'Fix the build', 'working', 'chief', `${opus.handle}-amber-pine`, 2)],
      activity: { state: 'working' },
      pane: { id: 'p1-w', generation: 1 },
    },
    {
      participant: fable,
      tasks: [task(3, 'Review the parser', 'done', 'chief', fable.handle, 6)],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: astra,
      tasks: [task(4, 'Advise on the schema', 'queued', 'chief', astra.handle, 1)],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: swe,
      tasks: [task(5, 'Add the tests', 'working', 'chief', swe.handle, 3)],
      activity: { state: 'idle' },
      pane: null,
    },
    {
      participant: sonnet,
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: designer,
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  lanes[1].participant.model = 'claude-opus-5-5'
  lanes[1].participant.effort = 'xhigh'
  data.editRenames = after
  return data
}

for (const scheme of ['dark', 'light']) {
  test(`shots: the staff list, New project, Switch chief and an edit, ${scheme}`, async ({
    page,
  }, testInfo) => {
    test.skip(!SHOTS, 'CF_SHOTS_DIR names where the shots go')
    test.setTimeout(60_000)
    await page.setViewportSize({ width: 1500, height: 940 })
    await page.emulateMedia({ colorScheme: scheme })
    await open(page, shotData(SHOT_VARIANT))
    const shot = (name, target = page) =>
      target.screenshot({ path: join(SHOTS, `${name}-${SHOT_VARIANT}-${scheme}.png`) })
    await expect(page.getByRole('heading', { name: 'harbour' })).toBeVisible()
    await shot('staff-list')
    // The selects, listed open: a native pop-up is no part of a page's shot.
    for (const [button, name, harness, file] of [
      ['New project', 'New project', 'Claude Code', 'new-project'],
      ['Switch the chief to another harness or model', 'Switch the chief', 'Codex', 'switch-chief'],
    ]) {
      await page.getByRole('button', { name: button }).click()
      const dialog = page.getByRole('dialog', { name })
      await dialog.locator('[name="chiefHarness"]').selectOption({ label: harness })
      const models = dialog.locator('[name="chiefModel"]')
      await expect(models).toBeEnabled()
      await models.evaluate((select) => {
        select.size = Math.min(select.options.length, 9)
      })
      await shot(file, dialog)
      await dialog.getByRole('button', { name: 'Cancel' }).click()
    }
    // A member's card before and after an edit: the name follows the model it runs on.
    const row = (handle) => page.locator(`tr[data-handle="${handle}"][data-role="worker"]`).first()
    const before = row(SHOT_VARIANT === 'after' ? 'claude-opus-5-5-high' : 'opus-high')
    await shot('member-card-before-edit', before)
    await page.getByRole('button', { name: 'Project settings' }).click()
    const settings = page.getByRole('dialog', { name: 'Project settings' })
    await shot('project-settings', settings)
    if (SHOT_VARIANT === 'after') {
      await settings.getByRole('button', { name: 'Edit @claude-opus-5-5-high' }).first().click()
      await pickMember(settings, {
        harness: 'Codex',
        model: 'gpt-6.1-sol',
        effort: 'max',
        tier: 'critical',
      })
      await settings.getByRole('button', { name: 'Save member' }).click()
      await expect(page.locator('#status')).toContainText('is @gpt-6-1-sol-max now')
      await shot('project-settings-after-edit', settings)
      await settings.getByRole('button', { name: 'Close' }).click()
      await shot('member-card-after-edit', row('gpt-6-1-sol-max'))
    }
    testInfo.annotations.push({ type: 'shots', description: SHOTS })
  })
}
