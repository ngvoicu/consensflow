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
          id: 12,
          kind: 'question',
          state: 'queued',
          sender: 'chief',
          recipient: 'human',
          taskNumber: 1,
          body: 'Ship it to production today?',
          questions: null,
          createdAt: at(2),
        },
        {
          id: 14,
          kind: 'question',
          state: 'queued',
          sender: 'chief',
          recipient: 'human',
          taskNumber: 1,
          body: 'Colour: Which colour?\n- red: Warm\n- blue\n\nTools: Which tools?\n- vite\n- esbuild',
          questions: [
            {
              question: 'Which colour?',
              header: 'Colour',
              options: [
                { label: 'red', description: 'Warm' },
                { label: 'blue', description: null },
              ],
              multiple: false,
            },
            {
              question: 'Which tools?',
              header: 'Tools',
              options: [
                { label: 'vite', description: null },
                { label: 'esbuild', description: null },
              ],
              multiple: true,
            },
          ],
          createdAt: at(1),
        },
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
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    const operations = {
      'projects.list': () => answer({ projects: data.projects }),
      'agents.list': () => answer({ agents: data.agents }),
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
      'inbox.get': ({ project }) => answer({ messages: data.inbox[project] ?? [] }),
      'task.get': ({ project, task }) => answer({ task: data.tasks[`${project}:${task}`] }),
      'task.resume': ({ project, task }) =>
        answer({ task: { ...data.tasks[`${project}:${task}`], state: 'queued' } }),
      'task.transcript': ({ project, task }) =>
        answer(data.transcripts?.[`${project}:${task}`] ?? { items: [], total: 0 }),
      'project.open': ({ directory }) => answer({ project: { id: 3, name: 'new', directory } }),
    }
    const invoke = async (command, args = {}) => {
      window.__calls.push([
        command,
        JSON.parse(JSON.stringify(args, (key, value) => (key === 'onOutput' ? 'channel' : value))),
      ])
      if (command === 'core_request') {
        const handle = operations[args.operation]
        return handle ? handle(args.body) : { ok: true }
      }
      if (command === 'roster_handle') return { url: 'http://127.0.0.1:1/', token: 'ui-token' }
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
          window.__listeners.set(name, handler)
          return () => window.__listeners.delete(name)
        },
      },
      dialog: { open: async () => '/work/fresh' },
      test: {
        createEmulator: (host) => {
          // The keyboard lives in a field inside the host, as xterm's does.
          host.append(
            Object.assign(document.createElement('textarea'), { className: 'stub-input' }),
          )
          const emulator = {
            host,
            written: [],
            write: async (bytes) => emulator.written.push(...bytes),
            onData: (callback) => {
              emulator.type = callback
              return { dispose() {} }
            },
            resize() {},
            fit() {},
            dispose() {},
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
  await expect(rows).toHaveCount(4)
  await expect(rows.locator('.row-name')).toHaveText(['You', 'Chief of Staff', '@zeus', '@diana'])
  const zeus = table.locator('tr[data-handle="zeus"]')
  await expect(zeus.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(zeus.locator('.row-status')).toHaveText('Waiting: permission to run a command')
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
    'human',
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

test('writes out a long one-line question in For you, where the human answers it', async ({
  page,
}) => {
  const data = model()
  const long =
    'I tried to put a joke task on the board for a light-tier worker, but the staff has no members yet, so the task was refused. Could you add one light worker in the app? Any tier would do for a joke.'
  const question = data.inbox[1].find((message) => message.id === 12)
  question.body = long
  await open(page, data)
  const strip = page.locator(`.strip-message[data-message="${question.id}"]`)
  await expect(strip.locator('.strip-title')).toHaveText(long)
  await expect(strip.locator('.strip-body')).toHaveCount(0)
  await expect(strip.getByLabel(`Answer to m-${question.id}`)).toBeVisible()
})

test('answers a question with options by picking, one pick per question at least', async ({
  page,
}) => {
  await open(page)
  const question = page.locator('.strip-message[data-message="14"]')
  await expect(question.locator('.strip-body')).toHaveCount(0)
  await expect(question.locator('.strip-title')).toHaveText('', {
    useInnerText: false,
  })
  const form = question.getByRole('form', { name: 'Answer to m-14' })
  await expect(form.getByRole('group', { name: 'Colour: Which colour?' })).toContainText('Warm')
  await form.getByRole('radio', { name: 'blue' }).check()
  await form.getByRole('button', { name: 'Send answer' }).click()
  expect(await calls(page, 'message.answer')).toEqual([])
  await expect(form.getByRole('group', { name: 'Tools: Which tools?' })).toHaveClass(
    /choice-missing/,
  )
  await form.getByRole('checkbox', { name: 'vite' }).check()
  await form.getByRole('checkbox', { name: 'esbuild' }).check()
  await form.getByLabel('Something else for Colour').fill('purple')
  await form.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([
      {
        question: 14,
        choices: [
          ['blue', 'purple'],
          ['vite', 'esbuild'],
        ],
      },
    ])
  await expect.poll(() => calls(page, 'message.read')).toEqual([{ message: 14 }])
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

test("answers a question in the human's bay and routes it back", async ({ page }) => {
  await open(page)
  const question = page.locator('.strip-message[data-message="12"]')
  await expect(question.locator('.strip-route')).toHaveText('Question from @chief · T-1')
  await question.getByLabel('Answer to m-12').fill('Yes, after the smoke passes.')
  await question.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([{ question: 12, body: 'Yes, after the smoke passes.' }])
  await expect.poll(() => calls(page, 'message.read')).toEqual([{ message: 12 }])
  await expect(page.locator('#status')).toHaveText('Answer sent to @chief.')
})

test("shows a task waiting for a member in its requester's backlog, with the tier it waits for", async ({
  page,
}) => {
  await open(page)
  const foryou = page.getByRole('region', { name: 'For you' })
  await expect(foryou.locator('.foryou-status')).toHaveText('2 waiting · 1 note')
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

test('a terminal keeps a visible scrollbar for its scrollback', async ({ page }) => {
  await open(page)
  const rules = await page.evaluate(() =>
    [...document.styleSheets]
      .flatMap((sheet) => [...sheet.cssRules])
      .map((rule) => rule.selectorText ?? '')
      .filter((selector) => selector.includes('.xterm-viewport::-webkit-scrollbar')),
  )
  expect(rules).toEqual([
    '.xterm-viewport::-webkit-scrollbar',
    '.xterm-viewport::-webkit-scrollbar-track',
    '.xterm-viewport::-webkit-scrollbar-thumb',
    '.xterm-viewport::-webkit-scrollbar-thumb:hover',
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
  await expect(bay.locator('.foryou-status')).toHaveText('2 waiting · 2 notes')
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

test('typed text is a draft the pane guards; arrows, mouse and Escape are not', async ({
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
          .map(([, args]) => [String.fromCharCode(...args.bytes).slice(0, 3), args.draft ?? true]),
      ),
    )
    .toEqual([
      ['h', true],
      ['\r', true],
      ['\x1b[A', false],
      ['\x1b[<', false],
      ['\x1b', false],
      ['\x1b[2', true],
    ])
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
  await expect(bay.locator('.foryou-status')).toHaveText('6 waiting · 1 note')
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
  ).toEqual(['human', 'chief', 'zeus', 'diana', 'athena'])
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
    5,
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

test('shows a closed project read-only: dimmed, no actions, no windows, and a Resume banner', async ({
  page,
}) => {
  await open(page)
  await page.locator('.project-select', { hasText: 'foundry' }).click()
  const main = page.locator('main.main')
  await expect(main).toHaveAttribute('data-suspended', 'true')
  const banner = page.getByRole('status').filter({ hasText: 'foundry is closed.' })
  await expect(banner).toContainText('nothing is delivered until you resume it')
  await expect(page.getByRole('button', { name: 'Staff' })).toBeDisabled()
  await expect(page.locator('table[aria-label="Tasks"]')).toHaveCSS('pointer-events', 'none')
  await expect(page.getByRole('complementary', { name: 'Terminal dock' })).toHaveCSS(
    'pointer-events',
    'none',
  )
  await expect(page.locator('.terminal-card')).toHaveCount(0)
  await banner.getByRole('button', { name: 'Resume project' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
  await page.locator('.project-select', { hasText: 'harbour' }).click()
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
})

test('resumes a suspended project from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Resume foundry' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
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

test("keeps each project's terminals, scrollback and all, when the human switches projects", async ({
  page,
}) => {
  const data = model()
  data.projects[1].state = 'open'
  data.boards[2].project.state = 'open'
  data.boards[2].lanes.push({
    participant: { ...participant(10, 'chief', 'chief'), projectId: 2 },
    tasks: [],
    activity: { state: 'idle' },
    pane: { id: 'p2-chief', generation: 1 },
  })
  await open(page, data)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  const emulators = () => page.evaluate(() => window.__emulators.length)
  const before = await emulators()
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-chief', generation: 5, seq: 1, bytes: [104, 105] }),
  )
  await page.locator('.project-select', { hasText: 'foundry' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await expect(dock.locator('.terminal-card[data-handle="chief"]')).toHaveAttribute(
    'data-ended',
    'false',
  )
  await page.locator('.project-select', { hasText: 'harbour' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await expect(dock.locator('.terminal-card[data-ended="true"]')).toHaveCount(0)
  expect(await emulators()).toBe(before + 1, "only foundry's chief got a new terminal")
  expect(
    await page.evaluate(() => window.__emulators.some((emulator) => emulator.written.length > 0)),
  ).toBe(true)
})

test("keeps an ended window's terminal in the strip until the human closes it", async ({
  page,
}) => {
  await open(page)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card[data-handle="zeus"]')).toHaveCount(1)
  await page.evaluate(() => {
    const lane = window.__model.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
    lane.pane = null
    lane.activity = { state: 'closed' }
    window.__listeners.get('state-changed')()
  })
  const ended = dock.locator('.terminal-card[data-handle="zeus"]')
  await expect(ended).toHaveAttribute('data-ended', 'true')
  await expect(ended.getByText('ended')).toBeVisible()
  await ended.getByRole('button', { name: "Close @zeus's ended terminal" }).click()
  await expect(dock.locator('.terminal-card[data-handle="zeus"]')).toHaveCount(0)
  await expect(page.locator('tr[data-handle="zeus"] .row-tools button')).toHaveCount(0)
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
