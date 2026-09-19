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
        pathname === '/' ? 'core.html' : decodeURIComponent(pathname.slice(1)),
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
const participant = (id, handle, role, extra = {}) => ({
  id,
  sessionId: 1,
  handle,
  role,
  agent: role === 'human' || role === 'lead' || role === 'pm' ? null : handle,
  harness: role === 'human' ? null : 'claude-code',
  ...extra,
})
const task = (number, title, state, requester, assignee, minutesAgo = 3) => ({
  id: number,
  sessionId: 1,
  number,
  title,
  body: title,
  state,
  requester,
  assignee,
  createdAt: at(minutesAgo + 1),
  updatedAt: at(minutesAgo),
})

function model() {
  return {
    sessions: [
      { id: 1, name: 'harbour', directory: '/work/harbour', state: 'open', resumeOnStart: false },
      {
        id: 2,
        name: 'foundry',
        directory: '/work/foundry',
        state: 'suspended',
        resumeOnStart: false,
      },
    ],
    agents: [
      { name: 'zeus', harness: 'claude', model: 'claude-sonnet-5' },
      { name: 'diana', harness: 'codex', model: 'gpt-5.6-luna' },
      { name: 'athena', harness: 'opencode', model: 'muse-spark' },
    ],
    boards: {
      1: {
        session: { id: 1, name: 'harbour', directory: '/work/harbour', state: 'open' },
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
            pane: { id: 's1-lead', generation: 5 },
          },
          {
            participant: participant(3, 'zeus', 'worker'),
            tasks: [
              task(2, 'Write the parser', 'done', 'lead', 'zeus', 2),
              task(4, 'Add the tests', 'queued', 'lead', 'zeus', 1),
              task(5, 'Old spike', 'accepted', 'lead', 'zeus', 90),
            ],
            activity: { state: 'waiting', reason: 'permission to run a command' },
            pane: { id: 's1-zeus', generation: 7 },
          },
          {
            participant: participant(4, 'diana', 'worker', { harness: 'codex' }),
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
        session: { id: 2, name: 'foundry', directory: '/work/foundry', state: 'suspended' },
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
          createdAt: at(2),
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
    window.__listeners = new Map()
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    const operations = {
      'sessions.list': () => answer({ sessions: data.sessions }),
      'agents.list': () => answer({ agents: data.agents }),
      'board.get': ({ session }) => answer({ board: data.boards[session] }),
      'inbox.get': ({ session }) => answer({ messages: data.inbox[session] ?? [] }),
      'task.get': ({ session, task }) => answer({ task: data.tasks[`${session}:${task}`] }),
      'task.add': ({ to, body }) => answer({ task: { number: 9, assignee: to, body } }),
      'session.open': ({ directory }) => answer({ session: { id: 3, name: 'new', directory } }),
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
  await page.goto(`${origin}/core.html`)
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

test('draws a bay per participant with its lamp, and its open tasks as strips', async ({
  page,
}) => {
  await open(page)
  await expect(page.getByRole('heading', { name: 'harbour' })).toBeVisible()
  const bays = page.locator('.bay')
  await expect(bays).toHaveCount(4)
  await expect(bays.locator('.bay-name')).toHaveText(['You', 'Lead', '@zeus', '@diana'])
  const zeus = page.locator('.bay[data-handle="zeus"]')
  await expect(zeus.getByTestId('lamp')).toHaveAttribute('data-state', 'waiting')
  await expect(zeus.locator('.bay-status')).toHaveText('Waiting: permission to run a command')
  // The live task first, then the queue, then what is finished and waits for a decision.
  await expect(zeus.locator('button.strip .strip-number')).toHaveText(['T-4', 'T-2'])
  await expect(zeus.locator('.bay-cleared')).toHaveText('1 cleared (accepted or cancelled)')
  await expect(page.locator('.bay[data-handle="lead"] .strip')).toHaveAttribute(
    'data-state',
    'working',
  )
  await expect(page.locator('#inbox-button')).toHaveText('Inbox (2)')
})

test('shows an agent-written title as text, never as markup', async ({ page }) => {
  await open(page)
  const strip = page.locator('.bay[data-handle="diana"] .strip-title')
  await expect(strip).toHaveText('<img src=x onerror=window.__pwned=1> hostile title')
  expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  await expect(page.locator('.bay[data-handle="diana"] img')).toHaveCount(0)
})

test("answers a question in the human's bay and routes it back", async ({ page }) => {
  await open(page)
  const question = page.locator('.strip-message[data-kind="question"]')
  await expect(question.locator('.strip-route')).toHaveText('Question from @lead · T-1')
  await question.getByLabel('Answer to m-12').fill('Yes, after the smoke passes.')
  await question.getByRole('button', { name: 'Send answer' }).click()
  await expect
    .poll(() => calls(page, 'message.answer'))
    .toEqual([{ question: 12, body: 'Yes, after the smoke passes.' }])
  await expect.poll(() => calls(page, 'message.read')).toEqual([{ message: 12 }])
  await expect(page.locator('#status')).toHaveText('Answer sent to @lead.')
})

test('queues a task for a worker from its bay', async ({ page }) => {
  await open(page)
  const zeus = page.locator('.bay[data-handle="zeus"]')
  await zeus.getByRole('button', { name: 'Give @zeus a task' }).click()
  await zeus.getByLabel('Task for @zeus').fill('Profile the parser on the large fixture.')
  await zeus.getByRole('button', { name: 'Queue task' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([{ session: 1, to: 'zeus', body: 'Profile the parser on the large fixture.' }])
  await expect(page.locator('#status')).toHaveText('T-9 queued for @zeus.')
})

test('keeps a half-written task when the board redraws', async ({ page }) => {
  await open(page)
  const zeus = page.locator('.bay[data-handle="zeus"]')
  await zeus.getByRole('button', { name: 'Give @zeus a task' }).click()
  await zeus.getByLabel('Task for @zeus').fill('Half a thought')
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(1)
  await expect(zeus.getByLabel('Task for @zeus')).toHaveValue('Half a thought')
})

test("opens a strip's thread and accepts the result", async ({ page }) => {
  await open(page)
  await page.locator('.bay[data-handle="zeus"] button.strip[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await expect(drawer.locator('.thread-body')).toHaveText([
    'Write the parser',
    'Parser done, 14 tests.',
  ])
  await drawer.getByRole('button', { name: 'Accept' }).click()
  await expect.poll(() => calls(page, 'task.accept')).toEqual([{ session: 1, task: 2 }])
})

test('sends a failed task back with a follow-up', async ({ page }) => {
  await open(page)
  await page.locator('.bay[data-handle="diana"] button.strip').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-3' })
  await drawer.getByLabel('Follow-up for T-3').fill('Try again with the smaller model.')
  await drawer.getByRole('button', { name: 'Send back' }).click()
  await expect
    .poll(() => calls(page, 'task.reopen'))
    .toEqual([{ session: 1, task: 3, body: 'Try again with the smaller model.' }])
})

test('adds a saved agent to the session team', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Session team' })
  await expect(
    dialog.getByRole('list', { name: 'On the team' }).locator('.member-line'),
  ).toHaveText(['@zeus · worker · claude-code', '@diana · worker · codex'])
  await expect(dialog.getByLabel('Add an agent').locator('option')).toHaveText([
    'athena · opencode · muse-spark',
  ])
  await dialog.getByLabel('As').selectOption('advisor')
  await dialog.getByRole('button', { name: 'Add to team' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ session: 1, agent: 'athena', role: 'advisor' }])
})

test('takes a member off the team once the human confirms', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Session team' })
  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Keep @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toHaveCount(0)
  expect(await calls(page, 'member.remove')).toEqual([])

  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
  await dialog.getByRole('button', { name: 'Remove @zeus', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ session: 1, agent: 'zeus' }])
})

test('keeps a pending removal and the chosen agent when the core redraws the team', async ({
  page,
}) => {
  const data = model()
  data.agents.push({ name: 'hera', harness: 'pi', model: 'muse-spark' })
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Session team' })
  await dialog.getByLabel('Add an agent').selectOption('hera')
  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
  const boards = () =>
    page.evaluate(() => window.__calls.filter(([, args]) => args.operation === 'board.get').length)
  for (let redraw = 0; redraw < 2; redraw += 1) {
    const before = await boards()
    await page.evaluate(() => window.__listeners.get('state-changed')())
    await expect.poll(boards).toBeGreaterThan(before)
  }
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await expect(dialog.getByLabel('Add an agent')).toHaveValue('hera')
})

test('adds a PM on the harness the human picks', async ({ page }) => {
  await open(page)
  await expect(page.getByRole('button', { name: 'PM', exact: true })).toBeHidden()
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Session team' })
  await expect(dialog.getByText('No PM yet.')).toBeVisible()
  await dialog.getByLabel('The PM runs in').selectOption('codex')
  await dialog.getByRole('button', { name: 'Add a PM' }).click()
  await expect.poll(() => calls(page, 'pm.add')).toEqual([{ session: 1, harness: 'codex' }])
})

/** The harbour board with a PM and its advisor, the advisor added first. */
function withPm() {
  const data = model()
  data.boards[1].lanes.push(
    {
      participant: participant(5, 'athena', 'advisor', { harness: 'opencode' }),
      tasks: [task(6, 'Research the market', 'working', 'pm', 'athena', 4)],
      activity: { state: 'working' },
      pane: { id: 's1-athena', generation: 4 },
    },
    {
      participant: participant(6, 'pm', 'pm', { harness: 'codex' }),
      tasks: [],
      activity: { state: 'idle' },
      pane: { id: 's1-pm', generation: 3 },
    },
  )
  return data
}

test("groups the PM's team after the lead's, on the board and in its own windows", async ({
  page,
}) => {
  await open(page, withPm())
  await expect(page.locator('.board-group')).toHaveText(["Lead's team", "PM's team"])
  expect(
    await page.locator('.bay').evaluateAll((bays) => bays.map((bay) => bay.dataset.handle)),
  ).toEqual(['human', 'lead', 'zeus', 'diana', 'pm', 'athena'])

  const stage = page.getByRole('region', { name: 'Terminals' })
  const staged = () =>
    stage.locator('.terminal-card').evaluateAll((cards) => cards.map((c) => c.dataset.handle))
  await page.getByRole('button', { name: 'PM', exact: true }).click()
  await expect.poll(staged).toEqual(['pm', 'athena'])
  await page.getByRole('button', { name: 'Lead', exact: true }).click()
  await expect.poll(staged).toEqual(['lead', 'zeus'])
  await page.getByRole('button', { name: 'Board', exact: true }).click()
  await page.getByRole('button', { name: "Open @athena's terminal" }).click()
  await expect.poll(staged).toEqual(['pm', 'athena'])
  await expect(stage.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'athena',
  )
  expect(await page.evaluate(() => window.__emulators.length)).toBe(4)
})

test('resumes a suspended session from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Resume foundry' }).click()
  await expect.poll(() => calls(page, 'session.resume')).toEqual([{ session: 2 }])
})

test('starts a session in a chosen folder with the chosen lead', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'New session' }).click()
  const dialog = page.getByRole('dialog', { name: 'New session' })
  await expect(dialog.getByLabel('Project folder')).toHaveValue('/work/fresh')
  await dialog.getByLabel('The lead runs in').selectOption('opencode')
  await dialog.getByRole('button', { name: 'Start session' }).click()
  await expect
    .poll(() => calls(page, 'session.open'))
    .toEqual([{ directory: '/work/fresh', harness: 'opencode' }])
})

test('shows the live windows, focused on the one asked for, and feeds them their output', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: "Open @zeus's terminal" }).click()
  const stage = page.getByRole('region', { name: 'Terminals' })
  await expect(stage.locator('.terminal-card')).toHaveCount(2)
  await expect(stage.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'zeus',
  )
  await expect(page.getByRole('button', { name: "Open @diana's terminal" })).toHaveCount(0)
  await page.evaluate(() =>
    window.__output.onmessage({ id: 's1-zeus', generation: 7, seq: 1, bytes: [104, 105] }),
  )
  await expect
    .poll(() =>
      page.evaluate(() => window.__calls.filter(([c]) => c === 'pane_ack').map(([, a]) => a)),
    )
    .toEqual([{ id: 's1-zeus', generation: 7, seq: 1 }])
  const subscriptions = await page.evaluate(
    () => window.__calls.filter(([c]) => c === 'subscribe_output').length,
  )
  expect(subscriptions).toBe(1)
})

test('offers no terminal for a participant without a window', async ({ page }) => {
  await open(page)
  await expect(page.getByRole('button', { name: "Open @diana's terminal" })).toBeDisabled()
})
