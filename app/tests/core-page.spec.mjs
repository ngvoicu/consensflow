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
  kind: 'work',
  reviewOf: null,
  round: 0,
  verdict: null,
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
    // Each saved agent carries the roles its model suits, as the Agents screen's pills say.
    agents: [
      {
        name: 'zeus',
        harness: 'claude',
        model: 'claude-sonnet-5',
        effort: 'high',
        profile: { categories: ['worker', 'reviewer'] },
      },
      {
        name: 'diana',
        harness: 'codex',
        model: 'gpt-5.6-luna',
        effort: 'low',
        profile: { categories: ['worker', 'reviewer'] },
      },
      {
        name: 'athena',
        harness: 'opencode',
        model: 'muse-spark',
        profile: { categories: ['advisor', 'worker', 'reviewer'] },
      },
    ],
    boards: {
      1: {
        project: {
          id: 1,
          name: 'harbour',
          directory: '/work/harbour',
          state: 'open',
          review: 'members',
        },
        open: [
          task(6, 'Write the docs', 'open', 'lead', null, 1, { pool: 'worker', tier: 'standard' }),
        ],
        lanes: [
          {
            participant: participant(1, 'human', 'human'),
            tasks: [],
            activity: { state: 'closed' },
            pane: null,
          },
          {
            participant: participant(2, 'lead', 'lead'),
            tasks: [task(1, 'Ship the release notes', 'working', 'human', 'lead', 12)],
            activity: { state: 'working' },
            pane: { id: 'p1-lead', generation: 5 },
          },
          {
            participant: participant(3, 'zeus', 'worker'),
            tasks: [
              task(2, 'Write the parser', 'done', 'lead', 'zeus', 2, {
                result: 'Parser done, 14 tests.',
              }),
              task(4, 'Add the tests', 'queued', 'lead', 'zeus', 1),
              task(5, 'Old spike', 'accepted', 'lead', 'zeus', 90),
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
                'lead',
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
          review: 'none',
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
          sender: 'lead',
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
          sender: 'lead',
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
        ...task(2, 'Write the parser', 'done', 'lead', 'zeus', 2),
        reviews: [
          {
            number: 10,
            round: 1,
            reviewer: 'hera',
            state: 'done',
            verdict: 'pass',
            findings: 'Looks right.\n\nVERDICT: pass',
          },
        ],
        messages: [
          {
            id: 20,
            kind: 'task',
            sender: 'lead',
            recipient: 'zeus',
            state: 'delivered',
            reason: null,
            body: 'Write the parser',
          },
          {
            id: 21,
            kind: 'result',
            sender: 'zeus',
            recipient: 'lead',
            state: 'delivered',
            reason: null,
            body: 'Parser done, 14 tests.',
          },
        ],
      },
      '1:3': {
        ...task(3, 'hostile', 'failed', 'lead', 'diana', 5),
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
      'team.last': () => answer({ team: data.lastTeam ?? [] }),
      'member.roles': ({ project, agent, roles }) => {
        const lane = data.boards[project].lanes.find((l) => l.participant.handle === agent)
        const board = data.boards[project]
        const reviewers = board.lanes.filter(
          (l) => l.participant.roles.includes('reviewer') && l !== lane,
        )
        if (
          board.project.review !== 'none' &&
          !roles.includes('reviewer') &&
          reviewers.length === 0
        )
          return { ok: false, error: `@${agent} is the last reviewer: the review policy needs one` }
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
      'task.add': ({ to, tier, pool, body, needs }) =>
        answer({
          task: {
            number: 9,
            assignee: to ?? null,
            tier: tier ?? null,
            pool: pool ?? null,
            body,
            blockedBy: needs ?? [],
          },
        }),
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
          const emulator = {
            host,
            written: [],
            write: async (bytes) => emulator.written.push(...bytes),
            onData: () => ({ dispose() {} }),
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
    'Team',
    'Backlog',
    'Queued',
    'Working',
    'Waiting',
    'In review',
    'Done',
    'Accepted',
    'Ended',
  ])
  const rows = table.locator('tbody tr')
  await expect(rows).toHaveCount(4)
  await expect(rows.locator('.row-name')).toHaveText(['You', 'Lead', '@zeus', '@diana'])
  const zeus = table.locator('tr[data-handle="zeus"]')
  await expect(zeus.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(zeus.locator('.row-status')).toHaveText('Waiting: permission to run a command')
  await expect(zeus.locator('td[data-state="queued"] .card-title')).toHaveText(['Add the tests'])
  const done = zeus.locator('td[data-state="done"] button.card[data-task="2"]')
  await expect(done.locator('.card-title')).toHaveText('Write the parser')
  await expect(done.locator('.card-result')).toHaveText('Parser done, 14 tests.')
  await expect(zeus.locator('td[data-state="accepted"] .card-title')).toHaveText(['Old spike'])
  await expect(
    page.locator('tr[data-handle="diana"] td[data-state="ended"] .card-title'),
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
      tasks: [task(21, 'Write the lexer', 'working', 'lead', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(22, zeus, 'brisk-birch'),
      tasks: [task(23, 'Write the docs', 'done', 'lead', 'zeus-brisk-birch', 30)],
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
  await expect(dock.locator('.terminal-card[data-handle="zeus-amber-pine"]')).toHaveCount(1)
  await live.getByRole('button', { name: "Close @zeus · amber-pine's terminal" }).click()
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
      task(31, 'Old parser', 'accepted', 'lead', 'zeus', 40, { pool: 'worker', tier: 'standard' }),
    ],
    activity: { state: 'closed' },
    pane: null,
  })
  data.boards[1].lanes.push(
    {
      participant: session(40, zeus, 'amber-pine', 'worker'),
      tasks: [task(41, 'Write the lexer', 'working', 'lead', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(42, zeus, 'brisk-birch', 'reviewer'),
      tasks: [
        task(43, 'Review T-2', 'working', 'lead', 'zeus-brisk-birch', 2, {
          kind: 'review',
          reviewOf: 2,
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
      tasks: [task(21, 'Write the lexer', 'working', 'lead', 'zeus-amber-pine', 3)],
      activity: { state: 'working' },
      pane: { id: 'p1-zeus-amber-pine', generation: 9 },
    },
    {
      participant: session(21, zeus, 'brisk-birch'),
      tasks: [
        task(22, 'Write the docs', 'done', 'lead', 'zeus-brisk-birch', 1, { result: 'Docs done.' }),
      ],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  const rows = page.locator('table[aria-label="Tasks"] tbody tr')
  await expect(rows.evaluateAll((nodes) => nodes.map((n) => n.dataset.handle))).resolves.toEqual([
    'human',
    'lead',
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
  await page.getByRole('button', { name: 'New task' }).click()
  const backlog = page.getByRole('region', { name: 'For you' })
  const composer = backlog.locator('form.composer')
  await expect(composer.getByLabel('For').locator('option')).toHaveText([
    'A standard worker (zeus)',
    'A light worker (diana)',
  ])
})

test('answers a question with options by picking, one pick per question at least', async ({
  page,
}) => {
  await open(page)
  const question = page.locator('.strip-message[data-message="14"]')
  await expect(question.locator('.strip-body')).toHaveCount(0)
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

test("shows a coordinator's unanswered question in For you and answers it without a read", async ({
  page,
}) => {
  const data = model()
  data.boards[1].overdue = [
    {
      id: 15,
      kind: 'question',
      state: 'delivered',
      sender: 'zeus',
      recipient: 'lead',
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
    'Question from @zeus to @lead · T-2 · unanswered',
  )
  await strip.getByLabel('Answer to m-15').fill('The recursive one.')
  await strip.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([{ question: 15, body: 'The recursive one.' }])
  expect(await calls(page, 'message.read')).toEqual([])
})

test("answers a question in the human's bay and routes it back", async ({ page }) => {
  await open(page)
  const question = page.locator('.strip-message[data-message="12"]')
  await expect(question.locator('.strip-route')).toHaveText('Question from @lead · T-1')
  await question.getByLabel('Answer to m-12').fill('Yes, after the smoke passes.')
  await question.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([{ question: 12, body: 'Yes, after the smoke passes.' }])
  await expect.poll(() => calls(page, 'message.read')).toEqual([{ message: 12 }])
  await expect(page.locator('#status')).toHaveText('Answer sent to @lead.')
})

test("shows a task waiting for a member in its requester's backlog, with the tier it waits for", async ({
  page,
}) => {
  await open(page)
  const foryou = page.getByRole('region', { name: 'For you' })
  await expect(foryou.locator('.foryou-status')).toHaveText('3 waiting')
  const card = page.locator(
    'tr[data-handle="lead"] td[data-state="open"] button.card[data-task="6"]',
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
    task(7, 'Wire the parser', 'open', 'lead', null, 1, {
      pool: 'worker',
      tier: 'standard',
      needs: [
        { number: 2, state: 'done' },
        { number: 6, state: 'open' },
      ],
      blockedBy: [2, 6],
    }),
  )
  data.tasks['1:7'] = { ...data.boards[1].open[1], messages: [], reviews: [] }
  await open(page, data)
  const card = page.locator(
    'tr[data-handle="lead"] td[data-state="open"] button.card[data-task="7"]',
  )
  await expect(card.locator('.card-route')).toHaveText(
    'blocked by T-2, T-6 · for a standard worker',
  )
  await card.click()
  const drawer = page.getByRole('complementary', { name: 'Task T-7' })
  await expect(drawer.locator('.drawer-meta')).toContainText('needs T-2 (done), T-6 (open)')
})

test('gives a task to a tier of member, never to a member by name', async ({ page }) => {
  await open(page)
  // Nobody is given a task by name from the board: the lead is talked to in its terminal.
  await expect(page.getByRole('button', { name: /^Give .* a task$/ })).toHaveCount(0)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  await composer.getByLabel('For').selectOption('worker:light')
  await expect(composer.getByLabel('Purpose')).toBeHidden()
  await expect(composer.getByLabel('Only after')).toBeVisible()
  await composer.getByLabel('Task').fill('Profile the parser on the large fixture.')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([
      {
        project: 1,
        pool: 'worker',
        tier: 'light',
        body: 'Profile the parser on the large fixture.',
      },
    ])
  await expect(page.locator('#status')).toHaveText('T-9 is on the board for a light worker.')
})

test('keeps what the human chose in New task when the board redraws under it', async ({ page }) => {
  await open(page)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  await composer.getByLabel('For').selectOption('worker:standard')
  await composer.getByLabel('Only after').fill('T-2')
  await composer.getByLabel('Task').fill('Wire the parser.')
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(() => calls(page, 'board.get').then((list) => list.length)).toBeGreaterThan(1)
  await expect(composer.getByLabel('For')).toHaveValue('worker:standard')
  await expect(composer.getByLabel('Only after')).toHaveValue('T-2')
  await expect(composer.getByLabel('Task')).toHaveValue('Wire the parser.')
})

test('says what to do instead of offering New task when the team has no worker or designer', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes = data.boards[1].lanes.filter((l) => l.participant.agent === null)
  await open(page, data)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  await expect(backlog.locator('form.composer')).toHaveCount(0)
  await expect(
    backlog.getByText('The team has no worker or image designer yet: add one in Team.'),
  ).toBeVisible()
})

test('puts a task on the board that waits for others, and refuses anything but task numbers', async ({
  page,
}) => {
  await open(page)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  await expect(composer.getByLabel('Only after')).toBeVisible()
  await composer.getByLabel('For').selectOption('worker:standard')
  await composer.getByLabel('Task').fill('Wire the parser into the CLI.')
  await composer.getByLabel('Only after').fill('T-2, 6')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([
      {
        project: 1,
        pool: 'worker',
        tier: 'standard',
        needs: [2, 6],
        body: 'Wire the parser into the CLI.',
      },
    ])
  await expect(page.locator('#status')).toHaveText(
    'T-9 is on the board for a standard worker. It waits until T-2, T-6 are accepted.',
  )
  await backlog.getByRole('button', { name: 'New task' }).click()
  await composer.getByLabel('For').selectOption('worker:standard')
  await composer.getByLabel('Task').fill('Second')
  await composer.getByLabel('Only after').fill('the parser')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect(composer.getByLabel('Only after')).toHaveJSProperty('validity.valid', false)
  expect(await calls(page, 'task.add')).toHaveLength(1)
})

test('asks for the purpose of critical work, and offers only the tiers of worker the team has', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push(
    {
      participant: participant(8, 'calliope', 'worker', { tier: 'critical' }),
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: participant(9, 'athena', 'advisor', { tier: 'complex' }),
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
    {
      participant: participant(10, 'pygmalion', 'designer', { harness: 'image', tier: 'light' }),
      tasks: [],
      activity: { state: 'closed' },
      pane: null,
    },
  )
  await open(page, data)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  // Advice is the lead's alone to ask, and the lead is talked to in its terminal:
  // neither athena, an advisor, nor the lead is an address here.
  await expect(composer.getByLabel('For').locator('option')).toHaveText([
    'A critical worker (calliope)',
    'A standard worker (zeus)',
    'A light worker (diana)',
    'An image designer (pygmalion)',
  ])
  await composer.getByLabel('For').selectOption('worker:critical')
  await composer.getByLabel('Purpose').selectOption('architecture')
  await composer.getByLabel('Task').fill('Why does the parser leak memory?')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([
      {
        project: 1,
        pool: 'worker',
        tier: 'critical',
        purpose: 'architecture',
        body: 'Why does the parser leak memory?',
      },
    ])
  await backlog.getByRole('button', { name: 'New task' }).click()
  await composer.getByLabel('For').selectOption('designer')
  await expect(composer.getByLabel('Purpose')).toBeHidden()
  await composer.getByLabel('Task').fill('A logo: a compass rose; save it as images/logo.png')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect
    .poll(async () => (await calls(page, 'task.add')).at(-1))
    .toEqual({
      project: 1,
      pool: 'designer',
      body: 'A logo: a compass rose; save it as images/logo.png',
    })
  await expect(page.locator('#status')).toHaveText('T-9 is on the board for an image designer.')
})

test("shows a member out of quota, and a task's reviews under it, never as cards of their own", async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes[2].tasks.push(
    task(8, 'Add the lexer', 'review', 'lead', 'zeus', 1, { round: 1 }),
  )
  data.boards[1].lanes.push({
    participant: participant(7, 'hera', 'reviewer', { harness: 'pi' }),
    tasks: [
      task(9, 'Review T-8', 'working', 'lead', 'hera', 1, {
        kind: 'review',
        reviewOf: 8,
        round: 2,
      }),
      task(10, 'Review T-2', 'done', 'lead', 'hera', 2, {
        kind: 'review',
        reviewOf: 2,
        round: 1,
        verdict: 'pass',
      }),
    ],
    activity: { state: 'working' },
    pane: { id: 'p1-hera', generation: 2 },
  })
  await open(page, data)
  const diana = page.locator('tr[data-handle="diana"]')
  await expect(diana.getByTestId('lamp')).toHaveAttribute('data-state', 'out')
  await expect(diana.locator('.row-status')).toHaveText(/^Out of quota until \d\d:\d\d$/)
  const lexer = page.locator(
    'tr[data-handle="zeus"] td[data-state="review"] li.card-item:has(button[data-task="8"])',
  )
  await expect(lexer.locator('.card-state')).toHaveText('In review · round 2')
  await expect(lexer.locator('.reviews li')).toHaveText(['@hera is reviewing, round 2'])
  const parser = page.locator(
    'tr[data-handle="zeus"] td[data-state="done"] li.card-item:has(button[data-task="2"])',
  )
  await expect(parser.locator('.reviews li')).toHaveText(['Reviewed by @hera, round 1: pass'])
  // The review lives on the worker's lane; the reviewer's row only says what it is doing.
  const hera = page.locator('tr[data-handle="hera"]')
  await expect(hera.locator('button.card')).toHaveCount(0)
  await expect(hera.locator('td li')).toHaveCount(0)
  await expect(hera.locator('.row-status')).toHaveText('Reviewing T-8')
})

test('asks for a review of finished work from the drawer', async ({ page }) => {
  await open(page)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await drawer.getByRole('button', { name: 'Ask for a review' }).click()
  await expect.poll(() => calls(page, 'task.review')).toEqual([{ project: 1, task: 2 }])
})

test("opens a card's drawer with the result apart from the brief and the reviews under it, and leaves accepting to the lead", async ({
  page,
}) => {
  await open(page)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await expect(drawer.locator('.drawer-brief')).toHaveText('Write the parser')
  await expect(drawer.getByRole('heading', { name: 'Result' })).toBeVisible()
  await expect(drawer.locator('.drawer-result')).toHaveText('Parser done, 14 tests.')
  await expect(drawer.locator('.drawer-review .drawer-review-head')).toHaveText([
    'Reviewed by @hera, round 1: pass',
  ])
  await expect(drawer.locator('.drawer-review .drawer-review-body')).toHaveText([
    'Looks right.\n\nVERDICT: pass',
  ])
  // Accepting is the lead's call, in its terminal: the drawer offers a review and a send-back.
  await expect(drawer.getByRole('button', { name: 'Accept' })).toHaveCount(0)
  await expect(drawer.getByRole('button', { name: 'Ask for a review' })).toBeVisible()
  await expect(drawer.getByRole('button', { name: 'Send back' })).toBeVisible()
})

test('pauses a task from its drawer, shows it paused in the queue, and resumes it with words', async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.tasks.push(task(11, 'Add the lexer', 'paused', 'lead', 'zeus', 4))
  data.tasks['1:4'] = { ...lane.tasks.find((t) => t.number === 4), messages: [], reviews: [] }
  data.tasks['1:11'] = { ...lane.tasks.find((t) => t.number === 11), messages: [], reviews: [] }
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
  await drawer.getByLabel('Resume T-11 with').fill('Go on with the lexer.')
  await drawer.getByRole('button', { name: 'Resume' }).click()
  await expect
    .poll(() => calls(page, 'task.resume'))
    .toEqual([{ project: 1, task: 11, body: 'Go on with the lexer.' }])
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
          text: '[ConsensFlow m-20 · T-2 · task from @lead]\nWrite the parser',
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

test('sends a failed task back with a follow-up', async ({ page }) => {
  await open(page)
  await page.locator('tr[data-handle="diana"] button.card').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-3' })
  await drawer.getByLabel('Follow-up for T-3').fill('Try again with the smaller model.')
  await drawer.getByRole('button', { name: 'Send back' }).click()
  await expect
    .poll(() => calls(page, 'task.reopen'))
    .toEqual([{ project: 1, task: 3, body: 'Try again with the smaller model.' }])
})

test('shows the team as one row per member and role, and adds a saved agent in a role its model suits', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  const table = dialog.getByRole('table', { name: 'On the team' })
  await expect(table.locator('thead th')).toHaveText(['Member', 'Role', ''])
  await expect(table.locator('tbody tr')).toHaveCount(2)
  await expect(table.locator('tbody tr').first()).toContainText('@zeus')
  await expect(table.locator('tbody tr').first()).toContainText('standard')
  await expect(table.locator('tbody tr').first().locator('td').nth(1)).toHaveText('Worker')
  await expect(dialog.getByLabel('Role').locator('option')).toHaveText([
    'Worker',
    'Advisor',
    'Reviewer',
    'Image designer',
  ])
  // A worker: every saved agent that does not hold the role yet.
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'athena · opencode · muse-spark',
  ])
  // An advisor: only the agents whose model suits advising.
  await dialog.getByLabel('Role').selectOption('advisor')
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'athena · opencode · muse-spark',
  ])
  await dialog.getByRole('button', { name: 'Add to team' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ project: 1, agent: 'athena', roles: ['advisor'] }])
  // A second role for a member already on the team adds to its roles.
  await dialog.getByLabel('Role').selectOption('reviewer')
  // Each choice says what it runs, the model's effort level included.
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'zeus · claude · claude-sonnet-5 · high',
    'diana · codex · gpt-5.6-luna · low',
    'athena · opencode · muse-spark',
  ])
  await dialog.getByLabel('Agent').selectOption('zeus')
  await dialog.getByRole('button', { name: 'Add to team' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'zeus', roles: ['worker', 'reviewer'] }])
  // Nobody's model suits the image designer here, and the dialog says so.
  await dialog.getByLabel('Role').selectOption('designer')
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: 'Add to team' })).toBeDisabled()
  await expect(dialog.locator('#team-hint')).toHaveText(
    'No saved agent suits Image designer yet: add one under Settings, Agents.',
  )
  await dialog.getByLabel('Role').selectOption('advisor')
  await expect(dialog.locator('#team-hint')).toBeHidden()
})

test("drops one role from a member's row, and asks before its last", async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'diana')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  const table = dialog.getByRole('table', { name: 'On the team' })
  await expect(table.locator('tbody tr[data-handle="diana"]')).toHaveCount(2)
  await dialog.getByRole('button', { name: 'Remove Worker @diana' }).click()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'diana', roles: ['reviewer'] }])
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  expect(await calls(page, 'member.remove')).toEqual([])
})

test('takes a member off the team once the human confirms', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Keep @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toHaveCount(0)
  expect(await calls(page, 'member.remove')).toEqual([])

  await dialog.getByRole('button', { name: 'Remove Worker @zeus' }).click()
  await dialog.getByRole('button', { name: 'Remove @zeus', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ project: 1, agent: 'zeus' }])
})

test('keeps a pending removal and the chosen agent when the core redraws the team', async ({
  page,
}) => {
  const data = model()
  data.agents.push({
    name: 'hera',
    harness: 'pi',
    model: 'muse-spark',
    profile: { categories: ['worker', 'reviewer'] },
  })
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
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

test('keeps the row when the core refuses to drop the last reviewer', async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'diana')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByRole('button', { name: 'Remove Reviewer @diana' }).click()
  await expect(page.getByRole('status')).toContainText(
    '@diana is the last reviewer: the review policy needs one',
  )
  await expect(dialog.getByRole('button', { name: 'Remove Reviewer @diana' })).toBeVisible()
  await expect(dialog.getByLabel('Second review of')).toHaveValue('members')
})

test('starts a project with human approval required, and posts the checkbox with the team', async ({
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
    .toEqual([
      { directory: '/work/fresh', harness: 'claude-code', review: 'none', gate: true, team: [] },
    ])
})

test('shows and sets human approval from the team dialog', async ({ page }) => {
  const data = model()
  data.boards[1].project.gate = true
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  const gate = dialog.getByLabel('Human approval required', { exact: false })
  await expect(gate).toBeChecked()
  await gate.uncheck()
  await expect.poll(() => calls(page, 'project.gate')).toEqual([{ project: 1, gate: false }])
  await expect(page.locator('#status')).toHaveText(
    'Messages between agents go straight through again.',
  )
})

test('lists what waits for approval in For you, and approves, declines, sends back or answers it', async ({
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
    gated(30, 'task', 'lead', 'zeus-amber-pine', 4, 'Add the tests\nCover the parser.'),
    gated(31, 'result', 'zeus-amber-pine', 'lead', 2, 'Parser done, 14 tests.'),
    gated(32, 'question', 'zeus-amber-pine', 'lead', 4, 'Which parser?'),
    gated(33, 'answer', 'lead', 'zeus-amber-pine', 4, 'The recursive one.', { replyTo: 32 }),
  ]
  await open(page, data)
  const bay = page.getByRole('region', { name: 'For you' })
  await expect(bay.locator('.foryou-status')).toHaveText('7 waiting')
  const brief = bay.locator('.strip-message[data-message="30"]')
  await expect(brief).toHaveAttribute('data-gated', 'true')
  await expect(brief.locator('.strip-route')).toHaveText(
    'Task from @lead to @zeus-amber-pine · T-4 · needs your approval',
  )
  await expect(brief.locator('.strip-body')).toHaveText('Add the tests\nCover the parser.')
  await brief.getByRole('button', { name: 'Approve m-30 for @zeus-amber-pine' }).click()
  await expect.poll(() => calls(page, 'message.approve')).toEqual([{ message: 30 }])
  await expect(page.locator('#status')).toHaveText('m-30 goes on to @zeus-amber-pine.')

  const answer = bay.locator('.strip-message[data-message="33"]')
  await expect(answer.locator('.strip-route')).toHaveText(
    'Answer from @lead to @zeus-amber-pine · T-4 · needs your approval',
  )
  await answer.getByLabel('Why m-33 is declined').fill('Say the iterative one')
  await answer.getByRole('button', { name: 'Decline' }).click()
  await expect
    .poll(() => calls(page, 'message.decline'))
    .toEqual([{ message: 33, reason: 'Say the iterative one' }])

  const result = bay.locator('.strip-message[data-message="31"]')
  await expect(result.locator('.strip-route')).toHaveText(
    'Result from @zeus-amber-pine to @lead · T-2 · needs your approval',
  )
  await result.getByLabel('Follow-up for T-2').fill('Add the lexer tests too.')
  await result.getByRole('button', { name: 'Send back' }).click()
  await expect
    .poll(() => calls(page, 'task.reopen'))
    .toEqual([{ project: 1, task: 2, body: 'Add the lexer tests too.' }])

  const question = bay.locator('.strip-message[data-message="32"]')
  await question.getByLabel('Answer to m-32').fill('The recursive one.')
  await question.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([{ question: 32, body: 'The recursive one.' }])
  expect(await calls(page, 'message.read')).toEqual([], 'a gated question was never in the inbox')
})

test('sets the review policy from the team dialog once a reviewer is on the team', async ({
  page,
}) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'zeus')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await expect(dialog.getByLabel('Second review of')).toHaveValue('members')
  await expect(dialog.locator('.team-warning')).toBeHidden()
  await expect(dialog.getByLabel('Second review of').locator('option')).toHaveText([
    'Nothing: results go straight to whoever asked',
    "Workers' work: what a worker finishes",
  ])
  await dialog.getByLabel('Second review of').selectOption('none')
  await expect.poll(() => calls(page, 'project.review')).toEqual([{ project: 1, review: 'none' }])
})

test('holds the review choices until a reviewer is on the team', async ({ page }) => {
  const data = model()
  data.boards[1].project.review = 'none'
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await expect(dialog.getByLabel('Second review of')).toHaveValue('none')
  await expect(dialog.locator('.team-warning')).toHaveText(
    'Reviews need a reviewer on the team: add one of the agents as Reviewer.',
  )
  await expect(
    dialog.getByLabel('Second review of').locator('option[value="members"]'),
  ).toHaveJSProperty('disabled', true)
  await expect(dialog.getByLabel('Second review of').locator('option')).toHaveCount(2)
  expect(await calls(page, 'project.review')).toEqual([])
})

/** The harbour board with an advisor on the team, its window open. */
function withAdvisor() {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(5, 'athena', 'advisor', { harness: 'opencode' }),
    tasks: [task(7, 'Research the market', 'working', 'lead', 'athena', 4)],
    activity: { state: 'working' },
    pane: { id: 'p1-athena', generation: 4 },
  })
  return data
}

test("lists every member under the lead, with no team groups, on the board and in the dock's strip of windows", async ({
  page,
}) => {
  await open(page, withAdvisor())
  await expect(page.locator('.board-group')).toHaveCount(0)
  expect(
    await page
      .locator('tbody tr[data-handle]')
      .evaluateAll((rows) => rows.map((row) => row.dataset.handle)),
  ).toEqual(['human', 'lead', 'zeus', 'diana', 'athena'])
  await expect(page.locator('tr[data-handle="athena"] .row-name')).toHaveText('@athena')
  await expect(page.locator('tr[data-handle="athena"] .row-meta')).toContainText('advisor')

  // The dock on the right is a strip of every open terminal, the lead first,
  // then the members; a row with its terminal open offers no Open terminal.
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const cards = () =>
    dock.locator('.terminal-card').evaluateAll((cards) => cards.map((c) => c.dataset.handle))
  await expect.poll(cards).toEqual(['lead', 'zeus', 'athena'])
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'lead',
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
  await expect(page.getByRole('button', { name: 'Team' })).toBeDisabled()
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
  await expect(page.getByRole('button', { name: 'Team' })).toBeEnabled()
})

test('deletes a closed project for good once the human confirms, never an open one', async ({
  page,
}) => {
  await open(page)
  await expect(page.getByRole('button', { name: 'Delete harbour' })).toHaveCount(0)
  await page.getByRole('button', { name: 'Delete foundry' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete foundry?' })
  await expect(dialog).toContainText('every message go for good')
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

test('starts a project in a chosen folder with the chosen lead, the team ticked from the last one', async ({
  page,
}) => {
  const data = model()
  data.lastTeam = [
    { agent: 'zeus', roles: ['worker', 'reviewer'] },
    { agent: 'diana', roles: ['worker'] },
  ]
  await open(page, data)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await expect(dialog.getByLabel('Project folder')).toHaveValue('/work/fresh')
  const table = dialog.getByRole('table', { name: 'Agents for the team' })
  // The last team, one row per agent and role.
  await expect(table.locator('tbody tr')).toHaveText([
    /zeus.*claude · claude-sonnet-5 · high.*Worker/,
    /zeus.*Reviewer/,
    /diana.*codex · gpt-5.6-luna · low.*Worker/,
  ])
  await expect(dialog.getByLabel('Second review of')).toHaveValue('members')
  await dialog.getByLabel('The lead runs in').selectOption('opencode')
  await dialog.locator('[name="pickRole"]').selectOption('advisor')
  await expect(dialog.locator('[name="pickAgent"]').locator('option')).toHaveText([
    'athena · opencode · muse-spark',
  ])
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await dialog.getByRole('button', { name: 'Remove Worker diana' }).click()
  await expect(table.locator('tbody tr')).toHaveCount(3)
  await dialog.getByLabel('Second review of').selectOption('none')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'opencode',
        review: 'none',
        gate: false,
        team: [
          { agent: 'zeus', roles: ['worker', 'reviewer'] },
          { agent: 'athena', roles: ['advisor'] },
        ],
      },
    ])
})

test('starts a project without reviews when nobody ticked is a reviewer', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'New project' }).click()
  const dialog = page.getByRole('dialog', { name: 'New project' })
  await expect(dialog.getByLabel('Second review of')).toHaveValue('none')
  await expect(dialog.locator('.team-warning')).toHaveText(
    'Reviews need a reviewer on the team: add one of the agents as Reviewer.',
  )
  await expect(
    dialog.getByLabel('Second review of').locator('option[value="members"]'),
  ).toHaveJSProperty('disabled', true)
  await dialog.locator('[name="pickRole"]').selectOption('reviewer')
  await dialog.locator('[name="pickAgent"]').selectOption('athena')
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await expect(dialog.locator('.team-warning')).toBeHidden()
  await dialog.getByLabel('Second review of').selectOption('members')
  await dialog.getByRole('button', { name: 'Remove Reviewer athena' }).click()
  await expect(dialog.getByLabel('Second review of')).toHaveValue('none')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      { directory: '/work/fresh', harness: 'claude-code', review: 'none', gate: false, team: [] },
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
    'lead',
  )
  await expect(page.getByRole('button', { name: "Open @zeus's terminal" })).toHaveCount(0)
  await expect(page.getByRole('button', { name: "Open Lead's terminal" })).toHaveCount(0)
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
    participant: { ...participant(10, 'lead', 'lead'), projectId: 2 },
    tasks: [],
    activity: { state: 'idle' },
    pane: { id: 'p2-lead', generation: 1 },
  })
  await open(page, data)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  const emulators = () => page.evaluate(() => window.__emulators.length)
  const before = await emulators()
  await page.evaluate(() =>
    window.__output.onmessage({ id: 'p1-lead', generation: 5, seq: 1, bytes: [104, 105] }),
  )
  await page.locator('.project-select', { hasText: 'foundry' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(1)
  await expect(dock.locator('.terminal-card[data-handle="lead"]')).toHaveAttribute(
    'data-ended',
    'false',
  )
  await page.locator('.project-select', { hasText: 'harbour' }).click()
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await expect(dock.locator('.terminal-card[data-ended="true"]')).toHaveCount(0)
  expect(await emulators()).toBe(before + 1, "only foundry's lead got a new terminal")
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
    tasks: [task(3, 'hostile', 'failed', 'lead', 'diana-amber-pine', 5)],
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
