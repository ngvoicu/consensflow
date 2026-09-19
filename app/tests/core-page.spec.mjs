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
  tags: [],
  outUntil: null,
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
  tags: [],
  purpose: null,
  kind: 'work',
  reviewOf: null,
  round: 0,
  verdict: null,
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
    agents: [
      { name: 'zeus', harness: 'claude', model: 'claude-sonnet-5' },
      { name: 'diana', harness: 'codex', model: 'gpt-5.6-luna' },
      { name: 'athena', harness: 'opencode', model: 'muse-spark' },
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
          task(6, 'Write the docs', 'open', 'lead', null, 1, {
            pool: 'worker',
            tier: 'standard',
            tags: ['docs'],
          }),
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
            participant: participant(3, 'zeus', 'worker', { tags: ['coding', 'rust'] }),
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
      'inbox.get': ({ project }) => answer({ messages: data.inbox[project] ?? [] }),
      'task.get': ({ project, task }) => answer({ task: data.tasks[`${project}:${task}`] }),
      'task.add': ({ to, tier, pool, body }) =>
        answer({
          task: { number: 9, assignee: to ?? null, tier: tier ?? null, pool: pool ?? null, body },
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

test("shows a task waiting for a member in its requester's backlog, with the tier it waits for", async ({
  page,
}) => {
  await open(page)
  const foryou = page.getByRole('region', { name: 'For you' })
  await expect(foryou.locator('.foryou-status')).toHaveText('2 waiting')
  const card = page.locator(
    'tr[data-handle="lead"] td[data-state="open"] button.card[data-task="6"]',
  )
  await expect(card.locator('.card-route')).toHaveText('for a standard worker · docs')
  await expect(page.locator('tr[data-handle="zeus"] .row-meta')).toHaveText(
    'worker · standard · coding, rust · claude-code · claude-sonnet-5',
  )
})

test('gives a task to a tier of member, never to a member by name', async ({ page }) => {
  await open(page)
  await expect(page.getByRole('button', { name: 'Give @zeus a task' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Give Lead a task' })).toHaveCount(1)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  await composer.getByLabel('For').selectOption('worker:light')
  await composer.getByLabel('Tags').fill('docs, review')
  await expect(composer.getByLabel('Purpose')).toBeHidden()
  await composer.getByLabel('Task').fill('Profile the parser on the large fixture.')
  await composer.getByRole('button', { name: 'Put on the board' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([
      {
        project: 1,
        pool: 'worker',
        tier: 'light',
        tags: ['docs', 'review'],
        body: 'Profile the parser on the large fixture.',
      },
    ])
  await expect(page.locator('#status')).toHaveText('T-9 is on the board for a light worker.')
})

test('asks for the purpose of critical work, and offers only the tiers the team has', async ({
  page,
}) => {
  const data = model()
  data.boards[1].lanes.push({
    participant: participant(8, 'calliope', 'worker', { tier: 'critical' }),
    tasks: [],
    activity: { state: 'closed' },
    pane: null,
  })
  await open(page, data)
  const backlog = page.getByRole('region', { name: 'For you' })
  await backlog.getByRole('button', { name: 'New task' }).click()
  const composer = backlog.locator('form.composer')
  await expect(composer.getByLabel('For').locator('option')).toHaveText([
    'Lead',
    'A critical worker (calliope)',
    'A standard worker (zeus)',
    'A light worker (diana)',
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
        tags: [],
        purpose: 'architecture',
        body: 'Why does the parser leak memory?',
      },
    ])
})

test('gives a coordinator a task by name from its bay, and keeps a half-written one when the board redraws', async ({
  page,
}) => {
  await open(page)
  const lead = page.locator('tr[data-handle="lead"]')
  await lead.getByRole('button', { name: 'Give Lead a task' }).click()
  await lead.getByLabel('Task for Lead').fill('Half a thought')
  await page.evaluate(() => window.__listeners.get('state-changed')())
  await expect.poll(async () => (await calls(page, 'board.get')).length).toBeGreaterThan(1)
  await expect(lead.getByLabel('Task for Lead')).toHaveValue('Half a thought')
  await lead.getByRole('button', { name: 'Queue task' }).click()
  await expect
    .poll(() => calls(page, 'task.add'))
    .toEqual([{ project: 1, to: 'lead', body: 'Half a thought' }])
  await expect(page.locator('#status')).toHaveText('T-9 queued for the lead.')
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
  const hera = page.locator('tr[data-handle="hera"]')
  await expect(hera.locator('button.card')).toHaveCount(0)
  await expect(hera.locator('td[data-state="working"] .reviewing')).toHaveText('Reviewing T-8')
})

test('asks for a review of finished work from the drawer', async ({ page }) => {
  await open(page)
  await page.locator('button.card[data-task="2"]').click()
  const drawer = page.getByRole('complementary', { name: 'Task T-2' })
  await drawer.getByRole('button', { name: 'Ask for a review' }).click()
  await expect.poll(() => calls(page, 'task.review')).toEqual([{ project: 1, task: 2 }])
})

test("opens a card's drawer with the result apart from the brief and the reviews under it, and accepts", async ({
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
  await drawer.getByRole('button', { name: 'Accept' }).click()
  await expect.poll(() => calls(page, 'task.accept')).toEqual([{ project: 1, task: 2 }])
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

test('shows the team as a table of roles, and adds a saved agent with the roles ticked', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  const table = dialog.getByRole('table', { name: 'On the team' })
  await expect(table.locator('tbody tr')).toHaveCount(2)
  await expect(table.locator('tbody tr').first()).toContainText('@zeus')
  await expect(table.locator('tbody tr').first()).toContainText('standard')
  await expect(table.locator('tbody tr').first()).toContainText('coding, rust')
  await expect(dialog.getByRole('checkbox', { name: 'Worker @zeus' })).toBeChecked()
  await expect(dialog.getByRole('checkbox', { name: 'Reviewer @zeus' })).not.toBeChecked()
  await expect(dialog.getByLabel('Agent').locator('option')).toHaveText([
    'athena · opencode · muse-spark',
  ])
  const roles = dialog.getByRole('group', { name: 'Roles for the new member' })
  await roles.getByRole('checkbox', { name: 'Worker' }).uncheck()
  await roles.getByRole('checkbox', { name: 'Advisor' }).check()
  await roles.getByRole('checkbox', { name: 'Reviewer' }).check()
  await dialog.getByRole('button', { name: 'Add to team' }).click()
  await expect
    .poll(() => calls(page, 'member.add'))
    .toEqual([{ project: 1, agent: 'athena', roles: ['advisor', 'reviewer'] }])
})

test("changes a member's roles from its row, and keeps at least one", async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByRole('checkbox', { name: 'Reviewer @zeus' }).check()
  await expect
    .poll(() => calls(page, 'member.roles'))
    .toEqual([{ project: 1, agent: 'zeus', roles: ['worker', 'reviewer'] }])
  // A plain click: the page puts the tick back at once, which `uncheck` would count as a failure.
  await dialog.getByRole('checkbox', { name: 'Worker @diana' }).click()
  await expect(dialog.getByRole('checkbox', { name: 'Worker @diana' })).toBeChecked()
  await expect(page.getByRole('status')).toContainText('@diana needs at least one role.')
  expect(await calls(page, 'member.roles')).toHaveLength(1)
})

test('takes a member off the team once the human confirms', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Keep @zeus' }).click()
  await expect(dialog.getByText('Remove @zeus? Its open tasks are cancelled.')).toHaveCount(0)
  expect(await calls(page, 'member.remove')).toEqual([])

  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
  await dialog.getByRole('button', { name: 'Remove @zeus', exact: true }).click()
  await expect.poll(() => calls(page, 'member.remove')).toEqual([{ project: 1, agent: 'zeus' }])
})

test('keeps a pending removal and the chosen agent when the core redraws the team', async ({
  page,
}) => {
  const data = model()
  data.agents.push({ name: 'hera', harness: 'pi', model: 'muse-spark' })
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByLabel('Agent').selectOption('hera')
  await dialog.getByRole('button', { name: 'Remove @zeus from the team' }).click()
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

test('puts a tick back when the core refuses the change', async ({ page }) => {
  const data = model()
  const lane = data.boards[1].lanes.find((l) => l.participant.handle === 'diana')
  lane.participant.roles = ['worker', 'reviewer']
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await dialog.getByRole('checkbox', { name: 'Reviewer @diana' }).click()
  await expect(page.getByRole('status')).toContainText(
    '@diana is the last reviewer: the review policy needs one',
  )
  await expect(dialog.getByRole('checkbox', { name: 'Reviewer @diana' })).toBeChecked()
  await expect(dialog.getByLabel('Second review of')).toHaveValue('members')
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
  await dialog.getByLabel('Second review of').selectOption('all')
  await expect.poll(() => calls(page, 'project.review')).toEqual([{ project: 1, review: 'all' }])
})

test('holds the review choices until a reviewer is on the team', async ({ page }) => {
  const data = model()
  data.boards[1].project.review = 'none'
  await open(page, data)
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await expect(dialog.getByLabel('Second review of')).toHaveValue('none')
  await expect(dialog.locator('.team-warning')).toHaveText(
    'Reviews need a reviewer on the team: tick Reviewer for one of the agents.',
  )
  await expect(
    dialog.getByLabel('Second review of').locator('option[value="members"]'),
  ).toHaveJSProperty('disabled', true)
  await expect(
    dialog.getByLabel('Second review of').locator('option[value="all"]'),
  ).toHaveJSProperty('disabled', true)
  expect(await calls(page, 'project.review')).toEqual([])
})

test('adds a PM on the harness the human picks', async ({ page }) => {
  await open(page)
  await expect(page.getByRole('button', { name: 'PM', exact: true })).toBeHidden()
  await page.getByRole('button', { name: 'Team' }).click()
  const dialog = page.getByRole('dialog', { name: 'Project team' })
  await expect(dialog.getByText('No PM yet.')).toBeVisible()
  await dialog.getByLabel('The PM runs in').selectOption('codex')
  await dialog.getByRole('button', { name: 'Add a PM' }).click()
  await expect.poll(() => calls(page, 'pm.add')).toEqual([{ project: 1, harness: 'codex' }])
})

/** The harbour board with a PM and its advisor, the advisor added first. */
function withPm() {
  const data = model()
  data.boards[1].lanes.push(
    {
      participant: participant(5, 'athena', 'advisor', { harness: 'opencode' }),
      tasks: [task(6, 'Research the market', 'working', 'pm', 'athena', 4)],
      activity: { state: 'working' },
      pane: { id: 'p1-athena', generation: 4 },
    },
    {
      participant: participant(6, 'pm', 'pm', { harness: 'codex' }),
      tasks: [],
      activity: { state: 'idle' },
      pane: { id: 'p1-pm', generation: 3 },
    },
  )
  return data
}

test("groups the PM's team after the lead's, on the board and in the dock's strip of windows", async ({
  page,
}) => {
  await open(page, withPm())
  await expect(page.locator('.board-group')).toHaveText(["Lead's team", "PM's team"])
  expect(
    await page
      .locator('tbody tr[data-handle]')
      .evaluateAll((rows) => rows.map((row) => row.dataset.handle)),
  ).toEqual(['human', 'lead', 'zeus', 'diana', 'pm', 'athena'])

  // The dock on the right is a strip of every window, the lead first, then
  // the PM, then the members; a row's Terminal button brings its card into view.
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  const cards = () =>
    dock.locator('.terminal-card').evaluateAll((cards) => cards.map((c) => c.dataset.handle))
  await expect.poll(cards).toEqual(['lead', 'zeus', 'pm', 'athena'])
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'lead',
  )
  await page.getByRole('button', { name: "Open @athena's terminal" }).click()
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'athena',
  )
  expect(await page.evaluate(() => window.__emulators.length)).toBe(4)
  expect(await page.locator('tbody tr[data-handle]').count()).toBe(
    6,
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
  await page.getByRole('button', { name: 'Your agents' }).click()
  await expect.poll(opened).toEqual([''])
  await page.getByRole('button', { name: 'Agent library' }).click()
  await page.getByRole('button', { name: 'Harnesses' }).click()
  await expect.poll(opened).toEqual(['', 'library', 'harnesses'])
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
  await expect(row.locator('.row-status')).toHaveText('Free: a window opens with its next task')
  await expect(row.getByTestId('lamp')).toHaveAttribute('data-state', 'closed')
})

test('closes an open project from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Close harbour' }).click()
  await expect.poll(() => calls(page, 'project.close')).toEqual([{ project: 1 }])
  await expect(page.getByRole('button', { name: 'Close foundry' })).toHaveCount(0)
})

test('resumes a suspended project from the list', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Resume foundry' }).click()
  await expect.poll(() => calls(page, 'project.resume')).toEqual([{ project: 2 }])
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
  await expect(table.locator('tbody tr')).toHaveCount(3)
  await expect(dialog.getByRole('checkbox', { name: 'Reviewer zeus' })).toBeChecked()
  await expect(dialog.getByRole('checkbox', { name: 'Worker diana' })).toBeChecked()
  await expect(dialog.getByRole('checkbox', { name: 'Worker athena' })).not.toBeChecked()
  await expect(dialog.getByLabel('Second review of')).toHaveValue('members')
  await dialog.getByLabel('The lead runs in').selectOption('opencode')
  await dialog.getByRole('checkbox', { name: 'Advisor athena' }).check()
  await dialog.getByRole('checkbox', { name: 'Worker diana' }).uncheck()
  await dialog.getByLabel('Second review of').selectOption('all')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([
      {
        directory: '/work/fresh',
        harness: 'opencode',
        review: 'all',
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
    'Reviews need a reviewer on the team: tick Reviewer for one of the agents.',
  )
  await expect(
    dialog.getByLabel('Second review of').locator('option[value="all"]'),
  ).toHaveJSProperty('disabled', true)
  await dialog.getByRole('checkbox', { name: 'Reviewer athena' }).check()
  await expect(dialog.locator('.team-warning')).toBeHidden()
  await dialog.getByLabel('Second review of').selectOption('members')
  await dialog.getByRole('checkbox', { name: 'Reviewer athena' }).uncheck()
  await expect(dialog.getByLabel('Second review of')).toHaveValue('none')
  await dialog.getByRole('button', { name: 'Start project' }).click()
  await expect
    .poll(() => calls(page, 'project.open'))
    .toEqual([{ directory: '/work/fresh', harness: 'claude-code', review: 'none', team: [] }])
})

test('shows every live window in the strip, brings the asked one into view, and feeds them their output', async ({
  page,
}) => {
  await open(page)
  const dock = page.getByRole('complementary', { name: 'Terminal dock' })
  await expect(dock.locator('.terminal-card')).toHaveCount(2)
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'lead',
  )
  await page.getByRole('button', { name: "Open @zeus's terminal" }).click()
  await expect(dock.locator('.terminal-card[data-focused="true"]')).toHaveAttribute(
    'data-handle',
    'zeus',
  )
  await expect(page.getByRole('button', { name: "Open @diana's terminal" })).toBeDisabled()
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
  await ended.getByRole('button', { name: "Close @zeus's ended window" }).click()
  await expect(dock.locator('.terminal-card[data-handle="zeus"]')).toHaveCount(0)
  await expect(page.getByRole('button', { name: "Open @zeus's terminal" })).toBeDisabled()
})

test('offers no terminal for a participant without a window', async ({ page }) => {
  await open(page)
  await expect(page.getByRole('button', { name: "Open @diana's terminal" })).toBeDisabled()
})
