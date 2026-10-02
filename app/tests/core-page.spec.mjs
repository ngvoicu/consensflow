test('a task held while its member is out of quota says when it goes on', async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((lane) => lane.participant.handle === 'zeus')
  lane.tasks.push(task(9, 'Write the docs', 'paused', 'chief', 'zeus', 4, { heldUntil: at(-25) }))
  await open(page, data)
  await expect(page.locator('button.card[data-task="9"] .card-route')).toHaveText(
    /^out of quota until \d\d:\d\d · from /,
  )
})

import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { dirname, extname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

/**
 * The board page of the new core (TEST-BDC-13), against a stand-in for the app:
 * `core_request` answers from an in-page model and records every call, the way
 * the Rust app forwards the page's requests to the core.
 */
const UI_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'ui')

let server
let origin

test.setTimeout(15_000)

test.beforeAll(async () => {
  server = createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname
      const file = resolve(
        UI_ROOT,
        pathname === '/' ? 'index.html' : decodeURIComponent(pathname.slice(1)),
      )
      if (file !== UI_ROOT && !file.startsWith(`${UI_ROOT}${sep}`)) {
        response.writeHead(403).end('forbidden')
        return
      }
      const type =
        { '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript' }[extname(file)] ??
        'application/octet-stream'
      response.writeHead(200, {
        'content-type': `${type}; charset=utf-8`,
        'cache-control': 'no-store',
      })
      response.end(await readFile(file))
    } catch {
      response.writeHead(404).end('not found')
    }
  })
  await new Promise((ready) => server.listen(0, '127.0.0.1', ready))
  origin = `http://127.0.0.1:${server.address().port}`
})

test.afterAll(async () => {
  await new Promise((closed) => server.close(closed))
})

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
  tier: MEMBER.includes(role) ? 'standard' : null,
  outUntil: null,
  memberId: null,
  member: null,
  session: null,
  ...extra,
})

/** A session of a member: its own participant, named after the member. */
const session = (id, member, name, role = 'worker', extra = {}) =>
  participant(id, `${member.handle}-${name}`, role, {
    memberId: member.id,
    member: member.handle,
    session: name,
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
  createdAt: at(minutesAgo + 1),
  updatedAt: at(minutesAgo),
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
          {
            id: 20,
            kind: 'task',
            sender: 'chief',
            recipient: 'zeus',
            state: 'delivered',
            reason: null,
            body: 'Write the parser',
          },
          {
            id: 21,
            kind: 'result',
            sender: 'zeus',
            recipient: 'chief',
            state: 'delivered',
            reason: null,
            body: 'Parser done, 14 tests.',
          },
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
      'task.resume': ({ project, task }) =>
        answer({ task: { ...data.tasks[`${project}:${task}`], state: 'queued' } }),
      // The last `limit` items, as the core gives them, and how many came.
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
      if (command === 'core_request') {
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
  await page.goto(`${origin}/index.html`)
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true')
}

const calls = (page, operation) =>
  page.evaluate(
    (operation) =>
      window.__calls
        .filter(([command, args]) => command === 'core_request' && args.operation === operation)
        .map(([, args]) => args.body),
    operation,
  )

test('draws the kanban: a row per participant, a column per state, and cards that stay when done', async ({
  page,
}) => {
  await open(page)
  await expect(page.getByRole('heading', { name: 'harbour' })).toBeVisible()
  const table = page.getByRole('table', { name: 'Tasks' })
  await expect(table.locator('thead th')).toHaveText([
    'Staff',
    'Backlog',
    'Queued',
    'Working',
    'Waiting',
    'Done',
    'Finished',
  ])
  const rows = table.locator('tbody tr')
  // Nothing assigns the human a task: what is for them is in For you, not a row.
  await expect(rows).toHaveCount(3)
  await expect(rows.locator('.row-name')).toHaveText(['Chief of Staff', '@zeus', '@diana'])
  const zeus = table.locator('tr[data-handle="zeus"]')
  await expect(zeus.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(zeus.locator('.row-status')).toHaveText('Waiting: permission to run a command')
  const chief = table.locator('tr[data-handle="chief"]')
  await expect(chief.locator('.row-status')).toHaveText('Working')
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

test("gives a session's lane the human's hand on its window: open, close, delete", async ({
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
  const live = page.locator('tr[data-handle="zeus-amber-pine"]')
  await expect(
    live.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }),
  ).toBeVisible()
  await expect(
    live.getByRole('button', { name: "Open @zeus · amber-pine's terminal" }),
  ).toHaveCount(0)
  const closed = page.locator('tr[data-handle="zeus-brisk-birch"]')
  await expect(closed.locator('.row-status')).toHaveText('Terminal closed')
  // Opening a terminal unfolds the dock it shows in.
  await page.getByRole('button', { name: 'Hide terminals' }).click()
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeHidden()
  await closed.getByRole('button', { name: "Open @zeus · brisk-birch's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-brisk-birch' }])
  await expect(page.getByRole('region', { name: 'Terminals' })).toBeVisible()
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const card = dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')
  await expect(card).toHaveCount(1)
  // The chief's window stays with the project: its card offers no Close.
  await expect(
    dock.locator('.terminal-card[data-handle="chief"]').getByRole('button', { name: /^Close/ }),
  ).toHaveCount(0)
  // The card's own header closes the window too, the same way as its row.
  await card.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.close'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  // Its card goes at once, even while the board still lists the window on its way out.
  await expect(dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')).toHaveCount(0)
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
  await expect(second.locator('.row-status')).toHaveText('Terminal closed')
  await expect(page.locator('tr[data-handle="zeus"] .row-status')).toHaveText(
    '1 terminal open, one per task',
  )
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
  await expect(bay.locator('.bay-empty')).toHaveText(
    'Nothing waits for you on the board: the chief asks in its terminal.',
  )
})

test('shows a question the chief left unanswered in For you as a notice, with nothing to write', async ({
  page,
}) => {
  const data = model()
  data.boards[1].overdue = [
    {
      id: 15,
      kind: 'question',
      state: 'delivered',
      sender: 'zeus',
      recipient: 'chief',
      taskNumber: 2,
      body: 'Which parser?',
      questions: null,
      createdAt: at(12),
    },
  ]
  await open(page, data)
  const strip = page.locator('.strip-message[data-message="15"]')
  await expect(strip).toHaveAttribute('data-overdue', 'true')
  await expect(strip.locator('.strip-route')).toHaveText(
    'Question from @zeus to @chief · T-2 · unanswered',
  )
  // The human tells the chief in its terminal; nothing here writes to an agent.
  await expect(strip.getByRole('textbox')).toHaveCount(0)
  await expect(strip.getByRole('button', { name: 'Mark m-15 read' })).toHaveCount(0)
  await strip.getByRole('button', { name: 'Open task' }).click()
  await expect(page.getByRole('complementary', { name: 'Task T-2' })).toBeVisible()
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
    'worker · standard · claude-code · claude-sonnet-5',
  )
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

test("opens a card's drawer with the result apart from the brief, and leaves accepting and reviews to the chief", async ({
  page,
}) => {
  await open(page)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await expect(drawer.locator('.drawer-brief')).toHaveText('Write the parser')
  await expect(drawer.getByRole('heading', { name: 'Result' })).toBeVisible()
  await expect(drawer.locator('.drawer-result')).toHaveText('Parser done, 14 tests.')
  // Accepting and asking for a review are the chief's, from its terminal.
  await expect(drawer.getByRole('button', { name: 'Accept' })).toHaveCount(0)
  await expect(drawer.getByRole('button', { name: /review/i })).toHaveCount(0)
  // Nothing here writes to the agent: that is done in its terminal.
  await expect(drawer.getByRole('textbox')).toHaveCount(0)
  // Each part is a panel of its own, headed by what it is.
  await expect(drawer.locator('.drawer-section-head')).toHaveText(['Brief', 'Result'])
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
  await open(page)
  await expect(table.locator('tr.board-empty')).toHaveCount(0)
})

test("keeps only what is new in a task's thread: questions, answers, follow-ups and an earlier result", async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  const brief = 'Tell a joke'
  const body = `${brief}\n\nReassigned from @gefjon-jolly-tundra (ran out of quota after starting); check the working tree for partial changes.`
  lane.tasks.push(task(14, 'Tell a joke', 'done', 'chief', 'zeus', 0, { body }))
  const message = (id, kind, sender, recipient, text, state = 'delivered') => ({
    id,
    kind,
    sender,
    recipient,
    state,
    reason: null,
    body: text,
  })
  data.tasks['1:14'] = {
    ...lane.tasks.find((t) => t.number === 14),
    messages: [
      message(40, 'task', 'chief', 'gefjon-jolly-tundra', brief),
      message(
        41,
        'note',
        null,
        'chief',
        'T-14 was taken back from @gefjon-jolly-tundra (ran out of quota after starting) and waits for another light worker.',
      ),
      message(42, 'task', 'chief', 'zeus', body),
      message(43, 'question', 'zeus', 'chief', 'About cats or code?'),
      message(44, 'answer', 'chief', 'zeus', 'Code.'),
      message(45, 'result', 'zeus', 'chief', 'Why do programmers mix up Halloween and Christmas?'),
      message(46, 'task', 'chief', 'zeus', 'Reopened: shorter, please.'),
      message(47, 'task', 'chief', 'zeus', 'Also no puns.', 'cancelled'),
      message(48, 'result', 'zeus', 'chief', 'Oct 31 == Dec 25.'),
    ],
  }
  await open(page, data)
  await page.locator('button.card[data-task="14"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-14' })
  await expect(drawer.locator('.drawer-result')).toHaveText('Oct 31 == Dec 25.')
  await expect(drawer.locator('.thread-body')).toHaveText([
    'About cats or code?',
    'Code.',
    'Why do programmers mix up Halloween and Christmas?',
    'Reopened: shorter, please.',
  ])
  await expect(drawer.locator('.drawer-meta')).toContainText('updated just now')
  await expect(drawer.locator('.drawer-meta')).not.toContainText('just now ago')
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
  // Folded until asked: the brief and the result come first.
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
  await expect(drawer.locator('.thread-body')).toHaveText(['Reopened: cover the errors too.'])
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
  // A session's Close, on its row and on its card in the dock, while its lamp changes.
  const row = page.locator('tr[data-handle="zeus-amber-pine"]')
  await row.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }).focus()
  await lampDrawn('zeus-amber-pine', 'idle')
  await expect(
    row.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }),
  ).toBeFocused()
  const card = page.locator('.terminal-card[data-handle="zeus-amber-pine"]')
  await card.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }).focus()
  await lampDrawn('zeus-amber-pine', 'working')
  await expect(card.getByTestId('lamp')).toHaveAttribute('data-state', 'working')
  await expect(
    card.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }),
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
  // Newest first, as the core reads them: only the two newest fit in its answer.
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

test('shows the staff as one row per member and role, and adds any saved agent in any role', async ({
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
  // An advisor: every agent, since any agent may take any role.
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
  // The image designer too: nothing on the card decides who may draw.
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveCount(4)
  await expect(dialog.getByRole('button', { name: 'Add to staff' })).toBeEnabled()
  await expect(dialog.locator('#staff-hint')).toBeHidden()
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

test('keeps a pending removal and the chosen agent when the core redraws the staff', async ({
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

test('starts a project with human approval required, and posts the checkbox with the staff', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  const gate = dialog.getByLabel('Human approval required', { exact: false })
  await expect(gate).not.toBeChecked()
  await gate.check()
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([{ directory: '/work/fresh', harness: 'claude-code', gate: true, staff: [] }])
})

test('offers a new project only the harnesses installed here, and no agent on another', async ({
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
  await expect(dialog.getByLabel('The chief runs in').locator('option')).toHaveText([
    'Codex',
    'OpenCode',
    'Pi',
  ])
  await expect(dialog.locator('tr[data-agent="ares"]')).toHaveCount(0)
  await expect(dialog.locator('option', { hasText: 'ares' })).toHaveCount(0)
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

  // The dock on the right is a strip of every open terminal, the chief first,
  // then the members; a row with its terminal open offers no Open terminal.
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const cards = () =>
    dock.locator('.terminal-card').evaluateAll((cards) => cards.map((c) => c.dataset.handle))
  await expect.poll(cards).toEqual(['chief', 'zeus', 'athena'])
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'chief',
  )
  await expect(page.getByRole('button', { name: "Open @athena's terminal" })).toHaveCount(0)
  expect(await page.evaluate(() => window.__emulators.length)).toBe(3)
  expect(await page.locator('tbody tr[data-handle]').count()).toBe(
    4,
    'the board stays beside the dock',
  )
})

test('opens the agents screens in their own window, and refreshes the agents when this one is back in front', async ({
  page,
}) => {
  await open(page)
  const opened = () =>
    page.evaluate(() =>
      window.__calls.filter(([c]) => c === 'open_agents_window').map(([, args]) => args.page),
    )
  const settings = page.getByRole('dialog', { name: 'Settings' })
  await page.getByRole('button', { name: 'Settings' }).click()
  await settings.getByRole('button', { name: 'Agents' }).click()
  await expect.poll(opened).toEqual([''])
  await expect(settings).toBeHidden()
  await page.getByRole('button', { name: 'Settings' }).click()
  await settings.getByRole('button', { name: 'Harnesses' }).click()
  await expect.poll(opened).toEqual(['', 'harnesses'])
  const listed = await page.evaluate(
    () => window.__calls.filter(([, args]) => args?.operation === 'agents.list').length,
  )
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await expect
    .poll(() =>
      page.evaluate(
        () => window.__calls.filter(([, args]) => args?.operation === 'agents.list').length,
      ),
    )
    .toBe(listed + 1)
  expect(await page.getByRole('dialog').count()).toBe(0)
})

test('shows a member between tasks as free, its window gone until the next task', async ({
  page,
}) => {
  const data = model()
  const diana = data.boards[1].lanes.find((lane) => lane.participant.handle === 'diana')
  diana.participant.outUntil = null
  await open(page, data)
  const row = page.locator('tr[data-handle="diana"]')
  await expect(row.locator('.row-status')).toHaveText('Free: a terminal opens with its next task')
  await expect(row.getByTestId('lamp')).toHaveAttribute('data-state', 'closed')
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
    messages: [
      {
        id: 32,
        kind: 'result',
        sender: 'zeus-amber-pine',
        recipient: 'chief',
        state: 'delivered',
        reason: null,
        body: 'Lexer done.',
      },
    ],
  }
  return data
}

test('shows a closed project read-only: it reads, nothing on it acts, and a banner resumes it', async ({
  page,
}) => {
  await open(page, closedFoundry())
  await chooseProject(page, 'foundry')
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
    'Switch the lead to another harness or model',
  ]) {
    await expect(board.getByRole('button', { name })).toHaveCount(0)
  }
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
  expect(reached).toEqual([
    'Open task',
    'Open task',
    "What @zeus · amber-pine's terminal wrote",
    'T-3, Write the lexer, Done, from @chief',
  ])
  // What it says is all there to read: a card opens its task, with nothing to do on it.
  await page.locator('button.card[data-task="3"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-3' })
  await expect(drawer.locator('.drawer-result')).toHaveText('Lexer done.')
  await expect(drawer.getByRole('button')).toHaveText(['Close'])
  await drawer.getByRole('button', { name: 'Close the task' }).click()
  await banner.getByRole('button', { name: 'Resume project' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
  await chooseProject(page, 'harbour')
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
  const addMember = (id, name) =>
    page.evaluate(
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
  // One member: a column of its own, as tall as the chief's.
  await expect(stage.locator('.terminal-card')).toHaveCount(2)
  let [chief, zeus] = [await box('chief'), await box('zeus')]
  expect(zeus.x).toBeGreaterThan(chief.x)
  expect(Math.abs(zeus.height - chief.height)).toBeLessThan(2)
  // Two: one column, one above the other.
  await addMember(40, 'amber-pine')
  await expect(stage.locator('.terminal-card')).toHaveCount(3)
  await expect.poll(async () => (await box('zeus')).height).toBeLessThan(chief.height / 2 + 1)
  zeus = await box('zeus')
  const second = await box('zeus-amber-pine')
  expect(second.x).toBe(zeus.x)
  expect(second.y).toBeGreaterThan(zeus.y)
  // Three: the third starts a column of its own, whole.
  await addMember(41, 'brisk-birch')
  await expect(stage.locator('.terminal-card')).toHaveCount(4)
  await expect.poll(async () => (await box('zeus-brisk-birch')).x).toBeGreaterThan(zeus.x)
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
  await dialog.getByLabel('The chief runs in').selectOption('opencode')
  await dialog.locator('[name="pickRole"]').selectOption('advisor')
  await expect(dialog.locator('[name="pickAgent"]').locator('option')).toHaveText([
    'hera · gpt-6-astra · codex · max',
    'zeus · claude-sonnet-5 · claude · high',
    'diana · gpt-5.6-luna · codex · low',
    'athena · muse-spark · opencode',
  ])
  await dialog.locator('[name="pickAgent"]').selectOption('athena')
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await dialog.getByRole('button', { name: 'Remove Worker diana' }).click()
  await expect(table.locator('tbody tr')).toHaveCount(4)
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'opencode',
        gate: false,
        staff: [
          { agent: 'zeus', roles: ['worker', 'reviewer'] },
          { agent: 'hera', roles: ['worker'] },
          { agent: 'athena', roles: ['advisor'] },
        ],
      },
    ])
})

test('shows every open terminal in the strip, offers no Open terminal for them, and feeds them their output', async ({
  page,
}) => {
  await open(page)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'chief',
  )
  await expect(page.getByRole('button', { name: "Open @zeus's terminal" })).toHaveCount(0)
  await expect(page.getByRole('button', { name: "Open Chief of Staff's terminal" })).toHaveCount(0)
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

/** Both projects open, each with its chief's window live. */
function twoOpen() {
  const data = model()
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
  await page
    .getByRole('dialog', { name: 'New project' })
    .getByRole('button', { name: 'Start project' })
    .click()
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
            command === 'core_request' &&
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
  await expect(row.locator('.row-status')).toHaveText('Terminal closed')
  await expect(row.getByTestId('lamp')).toHaveAttribute('data-state', 'closed')
  await expect(
    row.getByRole('button', { name: "What @zeus · amber-pine's terminal wrote" }),
  ).toBeEnabled()
  await row.getByRole('button', { name: "Open @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
  // The chief's window opens and closes with the project: its row has no Open.
  await expect(page.locator('tr[data-handle="chief"] .row-tools button')).toHaveText([
    'Switch lead',
  ])
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
  await page.mouse.move(x, y)
  await page.mouse.down()
  // A slow drag to the right, the windows narrowing: every step outlasts a
  // frame, and none outlasts the settle.
  for (let step = 1; step <= 10; step += 1) {
    await page.mouse.move(x + step * 16, y)
    await page.waitForTimeout(40)
  }
  await page.mouse.up()
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
  // A busy board: a state change every 100 ms, as fast as the core sends them,
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
  data.boards[1].lanes.push({
    participant: session(20, zeusLane.participant, 'lively-comet'),
    tasks: [task(21, 'Write the lexer', 'accepted', 'chief', 'zeus-lively-comet', 3)],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  // The board at its narrowest, the windows given the rest.
  const grip = page.locator('#board-resize')
  await grip.focus()
  for (let press = 0; press < 30; press += 1) await grip.press('ArrowLeft')
  await expect(grip).toHaveAttribute('aria-valuenow', '280')
  const row = page.locator('tr[data-handle="zeus-lively-comet"]')
  await expect(row.locator('.row-tools button')).toHaveText([
    'Open terminal',
    'Transcript',
    'Delete session',
  ])
  const outside = await page.locator('table[aria-label="Tasks"] tbody th').evaluateAll((heads) =>
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
  expect(outside).toEqual([])
})

test("opens a closed session's transcript from its lane, and gives a member's heading row no buttons", async ({
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
  await expect(page.locator('tr[data-handle="diana"] .row-tools button')).toHaveCount(0)
  await page.getByRole('button', { name: "What @diana · amber-pine's terminal wrote" }).click()
  await expect(page.getByRole('complementary', { name: 'Task T-3' })).toBeVisible()
  await expect
    .poll(async () => (await calls(page, 'task.transcript')).some((call) => call.task === 3))
    .toBe(true)
})

test('switches the lead from its row: an installed harness on its default, or a saved agent, after its turn or now', async ({
  page,
}) => {
  const data = model()
  data.missing = ['devin']
  await open(page, data)
  await page.getByRole('button', { name: 'Switch the lead to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the lead' })
  await expect(dialog).toBeVisible()
  const select = dialog.getByLabel('The lead runs on')
  const groups = await select
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
  // Devin is not installed here, and a hidden agent is never on offer.
  expect(groups).toEqual([
    [
      'Claude Code',
      [
        ['Claude Code, on its own default model', true],
        ['zeus', false],
      ],
    ],
    [
      'Codex',
      [
        ['Codex, on its own default model', false],
        ['diana', false],
        ['hera', false],
      ],
    ],
    [
      'OpenCode',
      [
        ['OpenCode, on its own default model', false],
        ['athena', false],
      ],
    ],
    ['Pi', [['Pi, on its own default model', false]]],
  ])
  await select.selectOption('agent:hera')
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the lead to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 1, agent: 'hera', when: 'now', note: true }])
  await expect(dialog).toBeHidden()
})

test('opens Switch lead letting the lead finish its turn, whatever was picked last time', async ({
  page,
}) => {
  await open(page)
  const switchLead = page.getByRole('button', {
    name: 'Switch the lead to another harness or model',
  })
  const dialog = page.getByRole('dialog', { name: 'Switch the lead' })
  await switchLead.click()
  await dialog.getByLabel('Switch now, cutting its turn off').check()
  await dialog.getByLabel('First ask the lead to write down where things stand').check()
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect(dialog).toBeHidden()
  await switchLead.click()
  await expect(dialog.getByLabel('Let it finish its turn, then switch')).toBeChecked()
  await expect(
    dialog.getByLabel('First ask the lead to write down where things stand'),
  ).not.toBeChecked()
})

test("the lead's row says when a switch waits for its turn", async ({ page }) => {
  const data = model()
  data.boards[1].lanes.find((lane) => lane.participant.handle === 'chief').switching = {
    harness: 'codex',
    agent: null,
  }
  await open(page, data)
  await expect(page.locator('.row-status[data-state="switching"]')).toHaveText(
    'Switching the lead to Codex after this turn',
  )
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
  await page.evaluate(() => {
    window.__delay['board.get'] = 400
  })
  await chooseProject(page, 'foundry')
  // harbour's board is still the one shown: what is done on it is done in harbour.
  await page.getByRole('button', { name: "Open @zeus · amber-pine's terminal" }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ project: 1, handle: 'zeus-amber-pine' }])
})

test('switches the lead only of the project it was asked for', async ({ page }) => {
  await open(page, twoOpen())
  // The agents come slowly; the human chooses foundry before the dialog is up.
  await page.evaluate(() => {
    window.__delay['agents.list'] = 300
  })
  await page.getByRole('button', { name: 'Switch the lead to another harness or model' }).click()
  await chooseProject(page, 'foundry')
  await expect(page.locator('#project-title')).toHaveText('foundry')
  await page.waitForTimeout(500)
  await expect(page.getByRole('dialog', { name: 'Switch the lead' })).toBeHidden()
  // Asked for again on foundry's own row, it switches foundry's lead.
  await page.evaluate(() => {
    window.__delay = {}
  })
  await page.getByRole('button', { name: 'Switch the lead to another harness or model' }).click()
  const dialog = page.getByRole('dialog', { name: 'Switch the lead' })
  await dialog.getByLabel('The lead runs on').selectOption('harness:codex')
  await dialog.getByRole('button', { name: 'Switch', exact: true }).click()
  await expect
    .poll(() => calls(page, 'chief.switch'))
    .toEqual([{ project: 2, harness: 'codex', when: 'turn', note: false }])
})

test('says the daemon is down while it is, why, and what comes next, and reads everything again once it is back', async ({
  page,
}) => {
  await open(page)
  // The app gives the core's state when the output is subscribed to: the page listens first.
  const order = await page.evaluate(() =>
    window.__calls.map(([command, args]) => (command === 'listen' ? args.name : command)),
  )
  expect(order).toContain('core-status')
  expect(order.indexOf('core-status')).toBeLessThan(order.indexOf('subscribe_output'))
  const banner = page.getByRole('alert')
  await expect(banner).toBeHidden()
  const coreStatus = (payload) =>
    page.evaluate(
      (payload) => window.__listeners.get('core-status')({ event: 'core-status', payload }),
      payload,
    )
  await page.evaluate(() => {
    window.__down = 'the Node bridge is not running'
  })
  await coreStatus({ available: false, cause: 'the daemon stopped (exit code 1)', retrying: true })
  await expect(banner).toHaveText(
    'The daemon is not running. The daemon stopped (exit code 1). Starting it again…',
  )
  // What the human does meanwhile is refused with why, not with a code.
  await page.getByRole('button', { name: 'Close harbour' }).click()
  await expect(page.locator('#status')).toHaveText('the Node bridge is not running')
  await coreStatus({ available: false, cause: 'it stopped again.', retrying: false })
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
  await coreStatus({ available: true, cause: null, retrying: false })
  await expect(banner).toBeHidden()
  await expect.poll(async () => (await calls(page, 'agents.list')).length).toBeGreaterThan(agents)
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(boards)
  await expect(page.locator('tr[data-handle="zeus"]')).toHaveCount(1)
})
