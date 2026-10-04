import { mkdirSync } from 'node:fs'
import { expect, test } from '@playwright/test'

import { addAgent } from '../../src/roster.js'
import { tempEnv } from '../../tests/helpers.mjs'
import { agentsServer } from './agents-server.mjs'
import { serveUi } from './serve-ui.mjs'

/**
 * The board page (TEST-BDC-13), against a stand-in for the app:
 * `daemon_request` answers from an in-page model and records every call, the way
 * the Rust app forwards the page's requests to the core.
 */
let ui

test.setTimeout(15_000)

test.beforeAll(async () => {
  ui = await serveUi()
})

test.afterAll(() => ui?.close())

const at = (minutesAgo) => new Date(Date.now() - minutesAgo * 60_000).toISOString()
const MEMBER = ['worker', 'advisor', 'reviewer']
const participant = (id, handle, role, extra = {}) => ({
  id,
  projectId: 1,
  handle,
  role,
  roles: [role],
  agent: MEMBER.includes(role) ? handle : null,
  harness: role === 'human' ? null : 'claude-code',
  designer: false,
  tier: MEMBER.includes(role) ? 'standard' : null,
  outUntil: null,
  memberId: null,
  member: null,
  session: null,
  ...extra,
})

/** A session of a member: its own participant, named after the member, on the member's agent. */
const session = (id, member, name, role = 'worker', extra = {}) =>
  participant(id, `${member.handle}-${name}`, role, {
    memberId: member.id,
    member: member.handle,
    session: name,
    agent: member.agent,
    harness: member.harness,
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

function model() {
  return {
    projects: [
      { id: 1, name: 'harbour', directory: '/work/harbour', state: 'open', resumeOnStart: false },
      {
        id: 2,
        name: 'foundry',
        directory: '/work/foundry',
        state: 'suspended',
        resumeOnStart: false,
      },
    ],
    // Every agent, as the roster lists it: the catalog's and the human's own, each with its tier.
    agents: [
      {
        name: 'zeus',
        harness: 'claude',
        model: 'claude-sonnet-5',
        effort: 'high',
        profile: { workTier: 'standard' },
      },
      {
        name: 'diana',
        harness: 'codex',
        model: 'gpt-5.6-luna',
        effort: 'low',
        profile: { workTier: 'light' },
      },
      {
        name: 'athena',
        harness: 'opencode',
        model: 'muse-spark',
        profile: { workTier: 'light' },
      },
      // Last in the roster, first on offer: the pick lists go by tier.
      {
        name: 'hera',
        harness: 'codex',
        model: 'gpt-6-astra',
        effort: 'max',
        profile: { workTier: 'critical' },
      },
      // Hidden by the human's preference: never on offer, though a member on it still runs.
      {
        name: 'kronos',
        harness: 'pi',
        model: 'openrouter/anthropic/claude-opus-5.5',
        effort: 'xhigh',
        profile: { workTier: 'complex' },
        hidden: true,
      },
      // The image agent, a Codex agent that designs: an image designer, and nothing else,
      // never the chief.
      {
        name: 'pygmalion',
        harness: 'codex',
        designer: true,
        model: 'codex-image',
        profile: { workTier: 'light' },
      },
    ],
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
            participant: participant(2, 'chief', 'chief'),
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
    window.__listeners = new Map()
    // How long an operation takes to answer, in ms, set by a test while it runs:
    // the real core answers when it can, and a page that assumes at once races.
    window.__delay = {}
    // Why the daemon is down, while a test has it down: the app answers every
    // core request with this, as Rust does with no bridge to the daemon.
    window.__down = null
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    const operations = {
      'projects.list': () => answer({ projects: data.projects }),
      'agents.list': () => answer({ agents: data.agents, missing: data.missing ?? [] }),
      'staff.last': () => answer({ staff: data.lastStaff ?? [] }),
      'member.roles': ({ project, agent, roles }) => {
        const lane = data.boards[project].lanes.find((l) => l.participant.handle === agent)
        lane.participant.roles = roles
        return answer({ member: lane.participant })
      },
      'board.get': ({ project }) => answer({ board: data.boards[project] }),
      'project.gate': ({ project, gate }) => {
        data.boards[project].project.gate = gate
        return answer({ project: data.boards[project].project })
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
      // The last `limit` items, as the daemon gives them, and how many came.
      'task.transcript': ({ project, task, limit = Number.POSITIVE_INFINITY }) => {
        const { items, total } = data.transcripts?.[`${project}:${task}`] ?? { items: [], total: 0 }
        const last = items.slice(Math.max(0, items.length - limit))
        return answer({ items: last, total, shown: last.length })
      },
      'project.open': ({ directory }) => answer({ project: { id: 3, name: 'new', directory } }),
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
        if (window.__down !== null) {
          return {
            ok: false,
            error: 'not-available-yet',
            operation: args.operation,
            detail: window.__down,
          }
        }
        return handle ? handle(args.body) : { ok: true }
      }
      if (command === 'subscribe_output') {
        window.__output = args.onOutput
        return { ok: true }
      }
      // An agents screen's address, as Rust hands it: the daemon's page with
      // the UI token, or why there is none.
      if (command === 'agents_screen') {
        return data.screens
          ? { ok: true, url: `${data.screens.url}/${args.page}?token=${data.screens.token}` }
          : { ok: false, error: 'the agents screens are not available: the daemon is not up' }
      }
      if (command === 'pane_input_enqueue' || command === 'pane_reply_enqueue')
        return { ok: true, ticket: 't' }
      if (command.startsWith('update_')) return { ok: false, error: 'not in this test' }
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
          window.__listeners.set(name, handler)
          return () => window.__listeners.delete(name)
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
                // Retired by the page: its window is over for good.
                disposed: false,
                write: async (bytes) => emulator.written.push(...bytes),
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

const calls = (page, operation) =>
  page.evaluate(
    (operation) =>
      window.__calls
        .filter(([command, args]) => command === 'daemon_request' && args.operation === operation)
        .map(([, args]) => args.body),
    operation,
  )

/** What a chief pick list offers: each harness group's agents, and whether each is disabled. */
const chiefGroups = (select) =>
  select
    .locator('optgroup')
    .evaluateAll((all) =>
      all.map((group) => [
        group.label,
        [...group.querySelectorAll('option')].map((option) => [
          option.textContent.split(' · ')[0],
          option.disabled,
        ]),
      ]),
    )

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
    "@zeus-brisk-birch is gone; its tasks stay on @zeus's lane.",
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
    'worker · standard · claude-code · claude-sonnet-5 · high',
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

test("says the effort an agent runs at, after its model: on a member's row, a session's, and the chief's card", async ({
  page,
}) => {
  const data = model()
  const { lanes } = data.boards[1]
  Object.assign(lanes.find((lane) => lane.participant.handle === 'chief').participant, {
    agent: 'hera',
    harness: 'codex',
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
      participant: participant(7, 'athena', 'advisor', { harness: 'opencode', tier: 'light' }),
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  await unfold(page)
  const meta = (handle) => page.locator(`tr[data-handle="${handle}"] .row-meta`)
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-meta'),
  ).toHaveText('codex · gpt-6-astra · max')
  await expect(meta('zeus')).toHaveText('worker · standard · claude-code · claude-sonnet-5 · high')
  await expect(meta('zeus-amber-pine')).toHaveText(
    'worker session of @zeus · claude-code · claude-sonnet-5 · high',
  )
  // An agent with no effort shows none.
  await expect(meta('athena')).toHaveText('advisor · light · opencode · muse-spark')
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
  await expect(diana.locator('.row-status')).toHaveText(/^Out of quota until \d\d:\d\d$/)
  const hera = page.locator('tr[data-handle="hera"]')
  await expect(hera.locator('td[data-state="working"] button.card[data-task="9"]')).toHaveCount(1)
  await expect(hera.locator('td[data-state="done"] button.card[data-task="10"]')).toHaveCount(1)
  // A review hangs under no other card: it is a task of its own.
  await expect(page.locator('.reviews')).toHaveCount(0)
})

test('a task held while its member is out of quota says when it goes on', async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  lane.tasks.push(task(9, 'Write the docs', 'paused', 'chief', 'zeus', 4, { heldUntil: at(-25) }))
  await open(page, data)
  await expect(page.locator('button.card[data-task="9"] .card-route')).toHaveText(
    /^out of quota until \d\d:\d\d · from /,
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
        '| `src/lexer.js` | tokens carry their position |',
        '| src/errors.js | a new **LexError** |',
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
    'src/lexer.js',
    'tokens carry their position',
    'src/errors.js',
    'a new LexError',
  ])
  await expect(table.locator('td code')).toHaveText(['src/lexer.js'])
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
  ).toHaveText(['T-3 stays: T-9 still needs it.'])
  await dialog.getByRole('button', { name: 'Keep' }).click()
  await expect(dialog).toBeHidden()
  expect(await calls(page, 'tasks.delete')).toEqual([])
  await heading.getByRole('button', { name: 'Delete finished' }).press('Enter')
  await dialog.getByRole('button', { name: 'Delete for good' }).click()
  await expect.poll(() => calls(page, 'tasks.delete')).toEqual([{ project: 1, tasks: [5, 8] }])
  await expect(page.locator('#status')).toHaveText('2 finished tasks are deleted.')
  await expect(drawer).toBeHidden()
  await expect(page.locator('td[data-state="finished"] button.card')).toHaveText([/^T-3/])
  // What is left is kept for T-9: nothing more may go, so the heading offers nothing.
  await expect(heading.getByRole('button')).toHaveCount(0)
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
  data.deleteRefusal = 'T-7 still needs T-3: it stays on the board until T-7 is finished'
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
    'T-7 still needs T-3: it stays on the board until T-7 is finished',
  )
  await expect(status).toHaveAttribute('data-tone', 'error')
  await expect(drawer).toBeVisible()
  await drawer.getByRole('button', { name: 'Close the task' }).click()
  await page.evaluate(() => {
    window.__model.deleteRefusal =
      'T-8 still needs T-5: it stays on the board until T-8 is finished'
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
    'T-8 still needs T-5: it stays on the board until T-8 is finished',
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
  board.lanes = board.lanes.filter((lane) => lane.participant.agent === null)
  board.open = []
  await open(page, data)
  const table = page.getByRole('table', { name: 'Tasks' })
  await expect(table.locator('tr.board-empty')).toHaveText(
    'No members yet: add the agents this project may use under Staff.',
  )
  await page.getByRole('button', { name: 'Staff' }).click()
  await expect(
    page.getByRole('dialog', { name: 'Project staff' }).locator('tbody tr.staff-empty'),
  ).toHaveText('Nobody yet: add the agents this project may use.')
  await open(page)
  await expect(table.locator('tr.board-empty')).toHaveCount(0)
})

test('takes a chief switched to a saved agent for no member: the board says it has none, and that agent joins the staff', async ({
  page,
}) => {
  const data = model()
  const board = data.boards[1]
  board.lanes = board.lanes.filter((lane) => ['human', 'chief'].includes(lane.participant.role))
  board.open = []
  // Switch chief to a saved agent: the chief runs on hera now.
  Object.assign(board.lanes[1].participant, { agent: 'hera', harness: 'codex' })
  await open(page, data)
  await expect(page.locator('tr.board-empty')).toHaveText(
    'No members yet: add the agents this project may use under Staff.',
  )
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  await expect(dialog.locator('tbody tr')).toHaveText([
    'Nobody yet: add the agents this project may use.',
  ])
  await dialog.getByLabel('Agent').selectOption('hera')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ project: 1, agent: 'hera', roles: ['worker'] }])
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

test('a member whose agent is gone says so on the board and in the staff, with Remove at hand', async ({
  page,
}) => {
  const data = model()
  data.agents = data.agents.filter((agent) => agent.name !== 'diana')
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana').agentMissing = true
  await open(page, data)
  const row = page.locator('tr[data-handle="diana"]')
  await expect(row.locator('.row-status')).toHaveText(
    'No agent named diana any more: define one under Agents, or remove @diana from the staff',
  )
  await expect(row.locator('.row-status')).toHaveAttribute('data-state', 'missing')
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  const member = dialog.locator('tr[data-handle="diana"]')
  await expect(member.locator('.member-meta')).toHaveText(
    'no agent named diana any more: define one under Agents, or remove it',
  )
  await expect(member.getByRole('button', { name: 'Remove Worker @diana' })).toBeVisible()
})

test("says on the chief's card in the dock what the chief is doing and runs on, and switches it from there", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { agent: 'hera', harness: 'codex' })
  chief.activity = { state: 'waiting', reason: 'permission to run a command' }
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(card.locator('.terminal-status')).toHaveText('Waiting: permission to run a command')
  await expect(card.locator('.terminal-meta')).toHaveText('codex · gpt-6-astra · max')
  // Out of quota, it says until when, whatever its window does.
  await changed(page, () => {
    const chief = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'chief')
    chief.participant.outUntil = new Date(Date.now() + 90 * 60_000).toISOString()
  })
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'out')
  await expect(card.locator('.terminal-status')).toHaveText(/^Out of quota until \d\d:\d\d$/)
  await expect(card.locator('.terminal-status')).toHaveAttribute('data-state', 'out')
  // Switch chief is on the card: none on the board, though the chief's row is there for its tasks.
  await expect(page.locator('tr[data-handle="chief"]')).toHaveCount(1)
  const board = page.getByRole('region', { name: 'Board' })
  const switchChief = { name: 'Switch the chief to another agent' }
  await expect(board.getByRole('button', switchChief)).toHaveCount(0)
  await card.getByRole('button', switchChief).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await expect(dialog).toBeVisible()
  // What the chief runs on now is no switch.
  expect(await chiefGroups(dialog.getByLabel('The chief runs on'))).toContainEqual([
    'Codex',
    [
      ['diana', false],
      ['hera', true],
    ],
  ])
})

test("keeps the chief's card in the dock while its window is down: its agent gone, it says so, and Switch chief is the way on", async ({
  page,
}) => {
  const data = model()
  const chief = data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief')
  Object.assign(chief.participant, { agent: 'astraeus', harness: 'codex' })
  // A chief whose agent is gone has no window: it does not open without one.
  Object.assign(chief, { agentMissing: true, activity: { state: 'closed' }, pane: null })
  await open(page, data)
  const card = page.locator('#stage .terminal-card[data-handle="chief"]')
  const status = card.locator('.terminal-status')
  await expect(status).toHaveText(
    'No agent named astraeus any more: define one under Agents, or switch the chief',
  )
  await expect(status).toHaveAttribute('data-state', 'missing')
  await expect(card.locator('.terminal-host')).toHaveCount(0)
  await card.getByRole('button', { name: 'Switch the chief to another agent' }).click()
  await expect(page.getByRole('dialog', { name: 'Switch the chief' })).toBeVisible()
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
  // A member's Remove in the staff dialog.
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  await dialog.getByRole('button', { name: 'Remove Worker @diana' }).focus()
  await lampDrawn('chief', 'idle')
  expect(await focusedLabel(page)).toBe('Remove Worker @diana')
})

test('a click whose press and release straddle a redraw still lands', async ({ page }) => {
  await open(page)
  const card = await page.locator('button.card[data-task="2"]').boundingBox()
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
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  await dialog.getByLabel('Role').selectOption('reviewer')
  await dialog.getByLabel('Agent').selectOption('zeus')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect(dialog.locator('tr[data-handle="zeus"]')).toHaveCount(2)
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([
      { project: 1, agent: 'zeus', roles: ['worker', 'reviewer'] },
      { project: 1, agent: 'zeus', roles: ['reviewer'] },
    ])
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
  await stage.evaluate((node) => {
    node.scrollLeft = node.scrollWidth
  })
  const scrolled = await stage.evaluate((node) => node.scrollLeft)
  expect(scrolled).toBeGreaterThan(0)
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await page.waitForTimeout(100)
  expect(await stage.evaluate((node) => node.scrollLeft)).toBe(scrolled)
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

  // Windows' console drops a non-ASCII mark on its way to a window that reads
  // key presses (Devin, Codex); Claude reads its terminal as text.
  test("the human's marks reach a Devin window in ASCII", async ({ page }) => {
    expect(await sent(page, 'devin', 'a — “b” → €5, ăîș')).toBe('a -- "b" -> EUR5, ăîș')
  })

  test("the human's marks reach a Claude window as typed", async ({ page }) => {
    expect(await sent(page, 'claude-code', 'a — “b” → €5')).toBe('a — “b” → €5')
  })
})

test('shows the staff as one row per member and role, and adds a saved agent in a role it fits', async ({
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
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  const table = dialog.getByRole('table', { name: 'On the staff' })
  await expect(table.locator('thead th')).toHaveText(['Member', 'Role', ''])
  await expect(table.locator('tbody tr')).toHaveText([
    /@zeus.*Worker/,
    /@diana.*Worker/,
    /@hera.*gpt-6-astra · codex · max · critical.*Reviewer/,
  ])
  // Each member says what it runs: model, harness, effort and its tier.
  await expect(table.locator('tbody tr').first().locator('.member-meta')).toHaveText(
    'claude-sonnet-5 · claude · high · standard',
  )
  await expect(table.locator('tbody tr').first().locator('td').nth(1)).toHaveText('Worker')
  await expect(dialog.getByLabel('Role').locator('option')).toHaveText([
    'Worker',
    'Advisor',
    'Reviewer',
    'Image designer',
  ])
  // A worker: every agent that does not hold the role yet, grouped by work tier, the most critical first.
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'hera · gpt-6-astra · codex · max',
    'athena · muse-spark · opencode',
  ])
  await expect(dialog.getByLabel('Agent').locator('optgroup')).toHaveCount(2)
  // An advisor: every agent but the image agent, which only draws.
  await dialog.getByLabel('Role').selectOption('advisor')
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'hera · gpt-6-astra · codex · max',
    'zeus · claude-sonnet-5 · claude · high',
    'diana · gpt-5.6-luna · codex · low',
    'athena · muse-spark · opencode',
  ])
  expect(
    await dialog
      .getByLabel('Agent')
      .locator('optgroup')
      .evaluateAll((g) => g.map((n) => n.label)),
  ).toEqual(['T1 · Critical work', 'T3 · Standard work', 'T4 · Light work'])
  await dialog.getByLabel('Agent').selectOption('athena')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ project: 1, agent: 'athena', roles: ['advisor'] }])
  await expect(page.locator('#status')).toHaveText('@athena joined the staff as Advisor.')
  // A second role for a member already on the staff adds to its roles.
  await dialog.getByLabel('Role').selectOption('reviewer')
  // Each choice says what it runs, the model's effort level included; hera reviews already.
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'zeus · claude-sonnet-5 · claude · high',
    'diana · gpt-5.6-luna · codex · low',
    'athena · muse-spark · opencode',
  ])
  await dialog.getByLabel('Agent').selectOption('zeus')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'zeus', roles: ['worker', 'reviewer'] }])
  await expect(page.locator('#status')).toHaveText('@zeus is Reviewer now too.')
  // An image designer is an image agent, and an image agent is nothing else.
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'pygmalion · codex-image · codex',
  ])
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeEnabled()
  await expect(dialog.locator('#staff-hint')).toBeHidden()
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([
      { project: 1, agent: 'athena', roles: ['advisor'] },
      { project: 1, agent: 'pygmalion', roles: ['designer'] },
    ])
})

test('says why no image designer is on offer: Codex, which image agents run through, is not here, or each is one already', async ({
  page,
}) => {
  const data = model()
  // Codex is not installed here: the image agent goes with it.
  Object.assign(
    data.agents.find((agent) => agent.name === 'pygmalion'),
    { hidden: true, notInstalled: true },
  )
  await open(page, data)
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  const hint = dialog.locator('#staff-hint')
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(hint).toHaveText(
    'No image agent is on offer here: image agents run through Codex; install it from Agents, Harnesses.',
  )
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeDisabled()
  await dialog.getByRole('button', { name: 'Close' }).click()
  // Codex is back, and pygmalion draws for harbour already.
  await page.evaluate(() => {
    const pygmalion = window.__model.agents.find((agent) => agent.name === 'pygmalion')
    delete pygmalion.hidden
    delete pygmalion.notInstalled
    window.__model.boards[1].lanes.push({
      participant: {
        ...window.__model.boards[1].lanes[2].participant,
        id: 12,
        handle: 'pygmalion',
        role: 'designer',
        roles: ['designer'],
        agent: 'pygmalion',
        harness: 'codex',
        designer: true,
        tier: 'light',
      },
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    })
    window.__listeners.get('state-changed')()
  })
  await expect(page.locator('tr[data-handle="pygmalion"]')).toHaveCount(1)
  await page.getByRole('button', { name: 'Staff' }).click()
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(hint).toHaveText('Every image agent is on the staff as Image designer already.')
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeDisabled()
})

test("drops one role from a member's row, and asks before its last", async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'diana')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
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
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
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

test("offers, after a role is picked, the agents the staff shown does not hold it with: this project's, as it is now", async ({
  page,
}) => {
  await open(page, twoOpen())
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  const offered = () =>
    dialog
      .getByLabel('Agent')
      .locator('option')
      .evaluateAll((options) => options.map((option) => option.value))
  // harbour's staff seen first, then a reviewer role added there.
  await page.getByRole('button', { name: 'Staff' }).click()
  await dialog.getByLabel('Role').selectOption('reviewer')
  await dialog.getByLabel('Agent').selectOption('zeus')
  await dialog.getByRole('button', { name: 'Add to staff' }).click()
  await expect(dialog.locator('tr[data-handle="zeus"]')).toHaveCount(2)
  await dialog.getByLabel('Role').selectOption('advisor')
  await dialog.getByLabel('Role').selectOption('reviewer')
  expect(await offered()).not.toContain('zeus')
  await dialog.getByRole('button', { name: 'Close' }).click()
  // foundry has nobody on its staff: every agent may work there.
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await page.getByRole('button', { name: 'Staff' }).click()
  await dialog.getByLabel('Role').selectOption('advisor')
  await dialog.getByLabel('Role').selectOption('worker')
  expect(await offered()).toEqual(['hera', 'zeus', 'diana', 'athena'])
})

test('keeps a pending removal and the chosen agent when the daemon redraws the staff', async ({
  page,
}) => {
  const data = model()
  data.agents.push({
    name: 'hera',
    harness: 'pi',
    model: 'muse-spark',
    profile: { workTier: 'light' },
  })
  await open(page, data)
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  await dialog.getByLabel('Agent').selectOption('hera')
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  const boards = () =>
    page.evaluate(() => window.__calls.filter(([, args]) => args.operation === 'board.get').length)
  for (let redraw = 0; redraw < 2; redraw += 1) {
    const before = await boards()
    await page.evaluate(() => window.__listeners.get('state-changed')())
    await expect.poll(boards).toBeGreaterThan(before)
  }
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await expect(dialog.getByLabel('Agent')).toHaveValue('hera')
})

test('says why the staff picker offers nobody: every saved agent holds the role, or none is saved', async ({
  page,
}) => {
  const data = model()
  // Only the staff's own agents are saved, and both are workers already.
  data.agents = data.agents.filter((agent) => ['zeus', 'diana'].includes(agent.name))
  await open(page, data)
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
  const hint = dialog.locator('#staff-hint')
  await expect(hint).toHaveText('Every saved agent is on the staff as Worker already.')
  await expect(dialog.getByLabel('Agent')).toBeDisabled()
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeDisabled()
  // Another role: both are on offer again.
  await dialog.getByLabel('Role').selectOption('reviewer')
  await expect(hint).toBeHidden()
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeEnabled()
  await dialog.getByRole('button', { name: 'Close' }).click()
  await page.evaluate(() => {
    window.__model.agents = []
  })
  await page.getByRole('button', { name: 'Staff' }).click()
  await expect(hint).toHaveText('No saved agents yet: add one under Settings, Agents.')
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeDisabled()
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
  await dialog.getByLabel('The chief runs on').selectOption('zeus')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([{ directory: '/work/fresh', agent: 'zeus', gate: true, staff: [] }])
})

test('starts no project until its chief is picked: a chief runs on a saved agent, never on a default', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const chief = dialog.getByLabel('The chief runs on')
  await expect(chief).toHaveValue('')
  await expect(chief.locator('option').first()).toHaveText('Pick an agent')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect(dialog).toBeVisible()
  expect(await calls(page, 'project.open')).toEqual([])
  await chief.selectOption('hera')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([{ directory: '/work/fresh', agent: 'hera', gate: false, staff: [] }])
  await expect(dialog).toBeHidden()
})

test("offers a new project's chief only the agents on a harness installed here, harness by harness", async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'devin']
  data.agents.push({
    name: 'ares',
    harness: 'devin',
    model: 'swe-1-6-slow',
    profile: { workTier: 'light' },
    hidden: true,
    notInstalled: true,
  })
  data.lastStaff = [{ agent: 'ares', roles: ['worker'] }]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  // Claude Code and Devin are not installed here, and Pi's one agent is hidden; pygmalion
  // is a Codex agent, but an image agent is never the chief.
  expect(await chiefGroups(dialog.getByLabel('The chief runs on'))).toEqual([
    [
      'Codex',
      [
        ['diana', false],
        ['hera', false],
      ],
    ],
    ['OpenCode', [['athena', false]]],
  ])
  await expect(dialog.locator('tr[data-agent="ares"]')).toHaveCount(0)
  await expect(dialog.locator('option', { hasText: 'ares' })).toHaveCount(0)
})

test('refuses a new project when no harness is installed here, and says where to get one', async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'codex', 'opencode', 'pi', 'devin']
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

test('says in New project who may be picked: nobody yet, or no agent saved at all', async ({
  page,
}) => {
  await open(page)
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const rows = dialog.getByRole('table', { name: 'Agents for the staff' }).locator('tbody tr')
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(rows).toHaveText(['Nobody yet: pick a role, then an agent whose model suits it.'])
  await expect(dialog.locator('#new-project-hint')).toBeHidden()
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await page.evaluate(() => {
    window.__model.agents = []
  })
  await page.getByRole('button', { name: 'New project' }).click()
  await expect(rows).toHaveText(['No agents saved yet: add some under Agents first.'])
  await expect(dialog.locator('#new-project-hint')).toHaveText(
    'No saved agents yet: add one under Settings, Agents.',
  )
  await expect(dialog.locator('[name="pickAgent"]')).toBeDisabled()
})

test('shows and sets human approval from the staff dialog, which has no review policy', async ({
  page,
}) => {
  const data = model()
  data.boards[1].project.gate = true
  await open(page, data)
  await page.getByRole('button', { name: 'Staff' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project staff' })
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
    participant: participant(5, 'athena', 'advisor', { harness: 'opencode' }),
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
 * The agents screens behind Settings, served as the daemon serves them, for
 * a page that is handed their address; `env` is the daemon's, where its
 * agents are kept.
 */
async function agentsScreens(options) {
  const t = tempEnv()
  // A harness's version is asked in the human's home.
  mkdirSync(t.env.HOME, { recursive: true })
  const server = await agentsServer(t.env, options)
  return {
    env: t.env,
    screens: { url: server.url, token: server.token },
    close: async () => {
      await server.close()
      t.cleanup()
    },
  }
}

/** Settings, then one of its screens: the dialog it opens in. */
async function openScreen(page, name) {
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('dialog', { name: 'Settings' }).getByRole('button', { name }).click()
  return page.getByRole('dialog', { name })
}

const agentsRead = (page) =>
  page.evaluate(() => window.__calls.filter(([, args]) => args?.operation === 'agents.list').length)

test('opens Agents from Settings in a dialog over the board, the daemon’s agents in it; Close goes back to Settings and reads the agents again', async ({
  page,
}) => {
  const daemon = await agentsScreens()
  try {
    await open(page, { ...model(), screens: daemon.screens })
    const agents = await openScreen(page, 'Agents')
    await expect(agents).toBeVisible()
    await expect(page.getByRole('dialog', { name: 'Settings' })).toBeHidden()
    await expect(agents.getByRole('button', { name: 'Close' })).toBeFocused()
    const screen = agents.frameLocator('iframe')
    await expect(screen.locator('#agents-count')).toHaveText(/^(\d+) of \1 shown$/)
    await expect(screen.locator('#agents .member .callsign', { hasText: /^gefjon$/ })).toBeVisible()
    const read = await agentsRead(page)
    await agents.getByRole('button', { name: 'Close' }).click()
    await expect(agents).toBeHidden()
    await expect(page.getByRole('button', { name: 'Settings' })).toBeFocused()
    // What the screen changed (a model, a tier) is on the board's agents too.
    await expect.poll(() => agentsRead(page)).toBe(read + 1)
    expect(await page.getByRole('dialog').count()).toBe(0)
  } finally {
    await daemon.close()
  }
})

test('opens Harnesses from Settings in a dialog where a check runs against the daemon; Escape goes back to Settings', async ({
  page,
}) => {
  let checks = 0
  const daemon = await agentsScreens({
    harnessLatest: async () => {
      checks += 1
      return '1.2.4'
    },
  })
  try {
    await open(page, { ...model(), screens: daemon.screens })
    const harnesses = await openScreen(page, 'Harnesses')
    await expect(harnesses).toBeVisible()
    const screen = harnesses.frameLocator('iframe')
    await expect(screen.locator('.host')).toHaveCount(5)
    await expect(
      screen.locator('.host').filter({ has: screen.locator('strong', { hasText: /^pi$/ }) }),
    ).toContainText('Version 1.2.3, 1.2.4 is out')
    const before = checks
    await screen.getByRole('button', { name: 'Check all harnesses' }).click()
    await expect.poll(() => checks).toBeGreaterThan(before)
    await expect(screen.locator('#check-note')).toHaveText('')
    await harnesses.getByRole('button', { name: 'Close' }).focus()
    await page.keyboard.press('Escape')
    await expect(harnesses).toBeHidden()
    await expect(page.getByRole('button', { name: 'Settings' })).toBeFocused()
  } finally {
    await daemon.close()
  }
})

test('Escape pressed inside a screen closes its dialog, as it closes every other dialog', async ({
  page,
}) => {
  const daemon = await agentsScreens()
  try {
    await open(page, { ...model(), screens: daemon.screens })
    const agents = await openScreen(page, 'Agents')
    await agents.frameLocator('iframe').getByRole('searchbox').fill('gefjon')
    await page.keyboard.press('Escape')
    await expect(agents).toBeHidden()
    await expect(page.getByRole('button', { name: 'Settings' })).toBeFocused()
  } finally {
    await daemon.close()
  }
})

test('a screen opened again keeps what the human left on it, and lists the agents saved meanwhile', async ({
  page,
}) => {
  const daemon = await agentsScreens()
  try {
    await open(page, { ...model(), screens: daemon.screens })
    let agents = await openScreen(page, 'Agents')
    let screen = agents.frameLocator('iframe')
    await screen.getByLabel('Show', { exact: true }).selectOption('mine')
    await expect(screen.locator('#agents')).toHaveText('No agents match these filters.')
    await agents.getByRole('button', { name: 'Close' }).click()
    // Saved elsewhere while the screen was closed: `cf agent add`, say.
    addAgent({ name: 'newbie', harness: 'codex', model: 'gpt-6-astra' }, daemon.env)
    agents = await openScreen(page, 'Agents')
    screen = agents.frameLocator('iframe')
    await expect(screen.locator('#agents .member .callsign')).toHaveText(['newbie'])
    await expect(screen.getByLabel('Show', { exact: true })).toHaveValue('mine')
  } finally {
    await daemon.close()
  }
})

test('a screen that cannot open says why, and no dialog opens', async ({ page }) => {
  await open(page)
  await openScreen(page, 'Agents')
  await expect(page.locator('#status')).toHaveText(
    'the agents screens are not available: the daemon is not up',
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

test('shows a closed project read-only: it reads, nothing on it acts, and a banner resumes it', async ({
  page,
}) => {
  await open(page, closedFoundry())
  await unfold(page)
  await chooseProject(page, 'foundry')
  await unfold(page)
  const main = page.locator('main.main')
  await expect(main).toHaveAttribute('data-suspended', 'true')
  const banner = page.getByRole('status').filter({ hasText: 'foundry is closed.' })
  await expect(banner).toContainText('nothing is delivered until you resume it')
  await expect(page.getByRole('button', { name: 'Staff' })).toBeDisabled()
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
  const switchChief = page.getByRole('button', { name: 'Switch the chief to another agent' })
  await expect(switchChief).toHaveCount(0)
  await banner.getByRole('button', { name: 'Resume project' }).focus()
  const reached = []
  for (let press = 0; press < 4; press += 1) {
    await page.keyboard.press('Tab')
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
  await expect(page.getByRole('button', { name: 'Staff' })).toBeEnabled()
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

test('lays the windows out: the chief a whole column, the members two to a column', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 })
  await open(page)
  const stage = page.getByRole('region', { name: 'Terminals' })
  const box = (handle) => stage.locator(`.terminal-card[data-handle="${handle}"]`).boundingBox()
  // A session of zeus at work, its terminal shown.
  const addSession = async (id, name) => {
    await page.evaluate(
      ({ id, name }) => {
        const zeus = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
        window.__model.boards[1].lanes.push({
          participant: {
            ...zeus.participant,
            id,
            handle: `zeus-${name}`,
            memberId: zeus.participant.id,
            member: 'zeus',
            session: name,
          },
          tasks: [],
          activity: { state: 'working' },
          pane: { id: `p1-zeus-${name}`, generation: 1 },
        })
        window.__listeners.get('state-changed')()
      },
      { id, name },
    )
    // A member's sessions fold under its row, which the redraw gives an arrow.
    await page.locator('tr[data-handle="zeus"] button.fold-sessions').waitFor()
    await unfold(page)
    await page.getByRole('button', { name: `Show @zeus · ${name}'s terminal` }).click()
  }
  // One member: a column of its own, as tall as the chief's.
  await expect(stage.locator('.terminal-card')).toHaveCount(2)
  let [chief, zeus] = [await box('chief'), await box('zeus')]
  expect(zeus.x).toBeGreaterThan(chief.x)
  expect(Math.abs(zeus.height - chief.height)).toBeLessThan(2)
  // Two: one column, one above the other.
  await addSession(40, 'amber-pine')
  await expect(stage.locator('.terminal-card')).toHaveCount(3)
  await expect.poll(async () => (await box('zeus')).height).toBeLessThan(chief.height / 2 + 1)
  zeus = await box('zeus')
  const second = await box('zeus-amber-pine')
  expect(second.x).toBe(zeus.x)
  expect(second.y).toBeGreaterThan(zeus.y)
  // Three: the third starts a column of its own, whole. Shown, it is
  // scrolled into view: the columns are compared where they are now.
  await addSession(41, 'brisk-birch')
  await expect(stage.locator('.terminal-card')).toHaveCount(4)
  await expect
    .poll(async () => (await box('zeus-brisk-birch')).x - (await box('zeus')).x)
    .toBeGreaterThan(0)
  chief = await box('chief')
  expect(Math.abs((await box('zeus-brisk-birch')).height - chief.height)).toBeLessThan(2)
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
    { agent: 'zeus', roles: ['worker', 'reviewer'] },
    { agent: 'diana', roles: ['worker'] },
    { agent: 'hera', roles: ['worker'] },
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await expect(dialog.getByLabel('Project folder')).toHaveValue('/work/fresh')
  const table = dialog.getByRole('table', { name: 'Agents for the staff' })
  // The last staff, one row per agent and role, by role and then by tier.
  await expect(table.locator('tbody tr')).toHaveText([
    /hera.*gpt-6-astra · codex · max · critical.*Worker/,
    /zeus.*claude-sonnet-5 · claude · high · standard.*Worker/,
    /diana.*gpt-5.6-luna · codex · low · light.*Worker/,
    /zeus.*Reviewer/,
  ])
  await expect(dialog.getByLabel('Second review of')).toHaveCount(0)
  await dialog.getByLabel('The chief runs on').selectOption('athena')
  await dialog.locator('[name="pickRole"]').selectOption('advisor')
  await expect(dialog.locator('[name="pickAgent"]').locator('option')).toHaveText([
    'hera · gpt-6-astra · codex · max',
    'zeus · claude-sonnet-5 · claude · high',
    'diana · gpt-5.6-luna · codex · low',
    'athena · muse-spark · opencode',
  ])
  await dialog.locator('[name="pickAgent"]').selectOption('athena')
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  // The image designer: the image agent alone.
  await dialog.locator('[name="pickRole"]').selectOption('designer')
  await expect(dialog.locator('[name="pickAgent"]').locator('option')).toHaveText([
    'pygmalion · codex-image · codex',
  ])
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await dialog.getByRole('button', { name: 'Remove Worker diana' }).click()
  await expect(table.locator('tbody tr')).toHaveCount(5)
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        agent: 'athena',
        gate: false,
        staff: [
          { agent: 'zeus', roles: ['worker', 'reviewer'] },
          { agent: 'hera', roles: ['worker'] },
          { agent: 'athena', roles: ['advisor'] },
          { agent: 'pygmalion', roles: ['designer'] },
        ],
      },
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
        resumeOnStart: false,
      })
      window.__listeners.get('state-changed')()
    }, 100)
  })
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await dialog.getByLabel('The chief runs on').selectOption('zeus')
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
  // The chief's window opens and closes with the project: its card has no Open, only Switch chief.
  await expect
    .poll(() => named(page.locator('#stage .terminal-card[data-handle="chief"] button')))
    .toEqual(['Switch chief'])
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
  // its row has no tools, and its card switches the chief.
  await expect(page.locator('tr[data-handle="chief"] .row-head button')).toHaveCount(0)
  await expect
    .poll(() => named(page.locator('#stage .terminal-card[data-handle="chief"] button')))
    .toEqual(['Switch chief'])
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
  // None of it asks the daemon anything: the window works on, the same one throughout.
  expect(await calls(page, 'session.open')).toEqual([])
  expect(await disposed(page)).toBe(0)
})

test("offers Hide alone on a session's card: its window works on, hidden from the dock", async ({
  page,
}) => {
  await open(page, atWork())
  await unfold(page)
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  const card = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
  await row.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  // An icon, named by its tip like the row's tools; nothing on the card ends the window.
  await expect.poll(() => named(card.locator('button'))).toEqual(['Hide terminal'])
  await expect(card.getByRole('button', { name: /^Close/ })).toHaveCount(0)
  await card.getByRole('button', { name: "Hide @zeus · amber-pine's terminal" }).click()
  await expect.poll(() => docked(page)).toEqual(['chief'])
  // Hidden, its window is the same one, at work: what it prints is still taken.
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
  await expect.poll(() => named(card.locator('button'))).toEqual(['Hide terminal'])
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
    card.getByRole('button', { name: 'Switch the chief to another agent' }),
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
  // At 1440 the board, beside the windows, still has every column without scrolling.
  const board = page.getByRole('region', { name: 'Board' })
  expect(await board.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(0)
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
  for (const width of [880, 1440, 2200]) {
    await page.setViewportSize({ width, height: 900 })
    await expect
      .poll(async () => Math.min(...(await strips()).map((strip) => strip.text)), {
        message: `strip text at ${width}px`,
      })
      .toBeGreaterThanOrEqual(240)
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

test('switches the chief from its card to a saved agent on a harness installed here, after its turn or now', async ({
  page,
}) => {
  const data = model()
  data.missing = ['devin']
  // The chief runs on zeus: what it runs on now is no switch.
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').participant.agent =
    'zeus'
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the chief to another agent' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await expect(dialog).toBeVisible()
  const select = dialog.getByLabel('The chief runs on')
  // An agent is picked, never a default: nothing is chosen yet.
  await expect(select).toHaveValue('')
  await expect(select.locator('option').first()).toHaveText('Pick an agent')
  // Devin is not installed here, and a hidden agent (Pi's one) is never on offer.
  expect(await chiefGroups(select)).toEqual([
    ['Claude Code', [['zeus', true]]],
    [
      'Codex',
      [
        ['diana', false],
        ['hera', false],
      ],
    ],
    ['OpenCode', [['athena', false]]],
  ])
  await select.selectOption('hera')
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the chief to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 1, agent: 'hera', when: 'now', note: true }])
  await expect(dialog).toBeHidden()
})

test('opens Switch chief letting the chief finish its turn, whatever was picked last time', async ({
  page,
}) => {
  await open(page)
  const switchChief = page.getByRole('button', {
    name: 'Switch the chief to another agent',
  })
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await switchChief.click()
  await dialog.getByLabel('The chief runs on').selectOption('diana')
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the chief to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect(dialog).toBeHidden()
  await switchChief.click()
  await expect(dialog.getByLabel('The chief runs on')).toHaveValue('')
  await expect(dialog.getByLabel('Let it finish its turn, then switch')).toBeChecked()
  await expect(
    dialog.getByLabel('First ask the chief to write down where things stand'),
  ).not.toBeChecked()
})

test("the chief's card says when a switch waits for its turn", async ({ page }) => {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').switching = {
    harness: 'codex',
    agent: 'hera',
  }
  await open(page, data)
  await expect(
    page.locator('#stage .terminal-card[data-handle="chief"] .terminal-status'),
  ).toHaveText('Switching the chief to hera after this turn')
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

test("offers a chief still on its harness's own default every saved agent, and moves it to one", async ({
  page,
}) => {
  // A project opened before chiefs were agents: its chief names none.
  await open(page)
  await page.getByRole('button', { name: 'Switch the chief to another agent' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  const select = dialog.getByLabel('The chief runs on')
  const disabled = (await chiefGroups(select)).flatMap(([, options]) =>
    options.filter(([, off]) => off),
  )
  expect(disabled).toEqual([])
  await select.selectOption('zeus')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 1, agent: 'zeus', when: 'turn', note: false }])
})

test('refuses Switch chief when no harness is installed here, and says where to get one', async ({
  page,
}) => {
  const data = model()
  data.missing = ['claude', 'codex', 'opencode', 'pi', 'devin']
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the chief to another agent' }).click()
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
  // The agents come slowly; the human chooses foundry before the dialog is up.
  await page.evaluate(() => {
    window.__delay['agents.list'] = 300
  })
  await page.getByRole('button', { name: 'Switch the chief to another agent' }).click()
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await page.waitForTimeout(500)
  await expect(page.getByRole('dialog', { name: 'Switch the chief' })).toBeHidden()
  // Asked for again on foundry's own chief card, it switches foundry's chief.
  await page.evaluate(() => {
    window.__delay = {}
  })
  await page.getByRole('button', { name: 'Switch the chief to another agent' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the chief' })
  await dialog.getByLabel('The chief runs on').selectOption('diana')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 2, agent: 'diana', when: 'turn', note: false }])
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
    (await calls(page, 'agents.list')).length,
    (await calls(page, 'board.get')).length,
  ]
  await daemonStatus({ available: true, cause: null, retrying: false })
  await expect(banner).toBeHidden()
  await expect.poll(async () => (await calls(page, 'agents.list')).length).toBeGreaterThan(agents)
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(boards)
  await expect(page.locator('tr[data-handle="zeus"]')).toHaveCount(1)
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
  await expect(page.getByRole('button', { name: 'Staff' })).toBeDisabled()
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
