import { expect, test } from '@playwright/test'

import { refusedByPolicy, serveUi } from './serve-ui.mjs'

/**
 * The history viewer (core/history-viewer.js) and the History button on every
 * window's head (core/terminals.js), against a stand-in for the app: the
 * daemon's `history.get` is answered from an in-page model of pages, searched
 * and with the tools' outputs left out as the daemon does, and every call is
 * recorded. The page is served under the app's own Content-Security-Policy,
 * which refuses nothing here.
 */
let ui
let refused

test.setTimeout(30_000)

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

const participant = (id, handle, role, extra = {}) => ({
  id,
  projectId: 1,
  handle,
  role,
  roles: [role],
  agent: null,
  harness: null,
  designer: false,
  tier: null,
  outUntil: null,
  memberId: null,
  member: null,
  session: null,
  ...extra,
})

/** What an XSS would look like, were text ever taken as markup. */
const HOSTILE = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=2</script>'

const item = (id, role, text, extra = {}) => ({
  id,
  role,
  text,
  complete: true,
  at: '2026-10-16T09:30:00.000Z',
  ...extra,
})

const DELIVERY = '[ConsensFlow m-12 · T-1 · result from @zeus]\nParser done, 14 tests.'

/**
 * The chief's history in two pages, newest first, as the daemon pages it: its
 * Claude Code conversation on ares, which a Switch chief to Codex on maia ended,
 * across both, and the one going on now.
 */
function chiefPages() {
  const claude = {
    id: 1,
    harness: 'claude-code',
    agent: 'ares',
    startedAt: '2026-10-16T09:00:00.000Z',
    endedAt: '2026-10-16T11:00:00.000Z',
    endedBy: { by: 'switch', harness: 'codex', agent: 'maia' },
    toolsLeftOut: 0,
    earlier: false,
    later: false,
  }
  return [
    [
      {
        ...claude,
        earlier: true,
        items: [
          item('u-3', 'user', 'This session is being continued from a previous conversation.', {
            compaction: true,
          }),
          item('a-2', 'assistant', HOSTILE),
        ],
      },
      {
        id: 2,
        harness: 'codex',
        agent: 'maia',
        startedAt: '2026-10-16T11:01:00.000Z',
        endedAt: null,
        endedBy: null,
        toolsLeftOut: 0,
        earlier: false,
        later: false,
        items: [
          item('u-5', 'user', 'Where are we?'),
          item('t-2', 'tool', 'ls: 3 files'),
          item('a-5', 'assistant', 'Taken over: the parser is done.'),
        ],
      },
    ],
    [
      {
        ...claude,
        later: true,
        items: [
          item('u-1', 'user', 'Ship the parser'),
          item('u-2', 'user', `half a thought${DELIVERY}`, {
            delivery: {
              line: `· m-12: @zeus's result on T-1 (cf task get T-1) ${HOSTILE}`,
              at: 'half a thought'.length,
            },
          }),
          item('a-1', 'assistant', 'On it'),
          item('t-1', 'tool', 'ok 12 passed'),
        ],
      },
    ],
  ]
}

function model() {
  const chief = participant(2, 'chief', 'chief', { agent: 'ares', harness: 'claude-code' })
  const zeus = participant(3, 'zeus', 'worker', { agent: 'zeus', harness: 'pi', tier: 'standard' })
  const session = participant(4, 'zeus-amber-pine', 'worker', {
    agent: 'zeus',
    harness: 'pi',
    tier: 'standard',
    memberId: 3,
    member: 'zeus',
    session: 'amber-pine',
  })
  const project = { id: 1, name: 'harbour', directory: '/work/harbour', state: 'open' }
  return {
    projects: [project],
    board: {
      project,
      open: [],
      lanes: [
        {
          participant: participant(1, 'human', 'human'),
          tasks: [],
          activity: { state: 'closed' },
          pane: null,
        },
        {
          participant: chief,
          tasks: [],
          activity: { state: 'idle' },
          pane: { id: 'p1-chief', generation: 1 },
        },
        { participant: zeus, tasks: [], activity: { state: 'closed' }, pane: null },
        {
          participant: session,
          tasks: [],
          activity: { state: 'working' },
          pane: { id: 'p1-zeus-amber-pine', generation: 2 },
        },
      ],
    },
    histories: {
      chief: chiefPages(),
      'zeus-amber-pine': [
        [
          {
            id: 7,
            harness: 'pi',
            agent: 'zeus',
            startedAt: '2026-10-16T10:00:00.000Z',
            endedAt: null,
            endedBy: null,
            toolsLeftOut: 0,
            earlier: false,
            later: false,
            items: [item('w-1', 'user', 'Write the lexer')],
          },
        ],
      ],
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
      'agents.list': () => answer({ agents: [], missing: [] }),
      'board.get': () => answer({ board: window.__model.board }),
      'inbox.get': () => answer({ messages: [], total: 0, shown: 0 }),
      // The daemon's pages: a search keeps the items that hold it in any case, in their text
      // or a delivery's line, and the conversations it found something in; the tools' outputs
      // are left out, and counted, unless asked for.
      'history.get': ({ handle, page = 1, find, tools }) => {
        if (window.__model.historyRefusal)
          return { ok: false, error: window.__model.historyRefusal }
        const holds = (it) =>
          `${it.text}\n${it.delivery?.line ?? ''}`.toLowerCase().includes(find.toLowerCase())
        const pages = (window.__model.histories[handle] ?? [])
          .map((conversations) =>
            conversations
              .map((conversation) => ({
                ...conversation,
                toolsLeftOut: tools
                  ? 0
                  : conversation.items.filter((it) => it.role === 'tool').length,
                items: conversation.items
                  .filter((it) => tools || it.role !== 'tool')
                  .filter((it) => !find || holds(it)),
              }))
              .filter((conversation) => !find || conversation.items.length > 0),
          )
          .filter((conversations) => conversations.length > 0)
        if (pages.length === 0) return answer({ history: { page: 0, pages: 0, conversations: [] } })
        if (page < 1 || page > pages.length) {
          return { ok: false, error: `there are ${pages.length} pages` }
        }
        return answer({ history: { page, pages: pages.length, conversations: pages[page - 1] } })
      },
    }
    const invoke = async (command, args = {}) => {
      window.__calls.push([command, JSON.parse(JSON.stringify(args))])
      if (command === 'daemon_request') {
        const handle = operations[args.operation]
        return handle ? handle(args.body) : { ok: true }
      }
      if (command.startsWith('update_')) return { ok: false, error: 'not in this test' }
      return { ok: true }
    }
    window.__TAURI__ = {
      core: { invoke, Channel: class {} },
      event: {
        listen: async (name, handler) => {
          window.__listeners.set(name, handler)
          return () => window.__listeners.delete(name)
        },
      },
      test: {
        createEmulator: (host) => ({
          host,
          write: async () => {},
          onData: () => ({ dispose() {} }),
          fit() {},
          dispose() {},
        }),
      },
    }
  }, data)
  await page.goto(`${ui.origin}/index.html`)
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true')
}

/** What the page's requests for `history.get` asked. */
const asked = (page) =>
  page.evaluate(() =>
    window.__calls
      .filter(([command, args]) => command === 'daemon_request' && args.operation === 'history.get')
      .map(([, args]) => args.body),
  )

/** The daemon says something changed: the page reads the board again. */
const changed = (page) =>
  page.evaluate(() => window.__listeners.get('state-changed')({ payload: {} }))

const chiefCard = (page) => page.locator('#stage .terminal-card[data-handle="chief"]')

/** Opens the chief's history from its card. */
async function openChief(page) {
  await chiefCard(page)
    .getByRole('button', { name: "Read everything kept of Chief of Staff's conversations" })
    .click()
  const viewer = page.getByRole('dialog', { name: 'History of Chief of Staff' })
  await expect(viewer).toBeVisible()
  return viewer
}

/** The viewer's conversations, each its heading and the line under it. */
const headings = (viewer) =>
  viewer
    .locator('.history-conversation')
    .evaluateAll((sections) =>
      sections.map((section) => [
        section.querySelector('.history-heading').textContent,
        section.querySelector('.history-about').textContent,
      ]),
    )

/** The labels of the items shown, in order. */
const labels = (viewer) =>
  viewer
    .locator('.history-item > .transcript-head')
    .evaluateAll((heads) => heads.map((head) => head.textContent.split(' · ')[0]))

test.describe('the History button', () => {
  test("is on the chief's head beside Switch chief, on a session's, and on the chief's card with no window", async ({
    page,
  }) => {
    await open(page)
    await expect(chiefCard(page).locator('.terminal-head > button')).toHaveText([
      'History',
      'Switch chief',
    ])
    // A session's terminal, once the human shows it, reads its own history.
    await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
    await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
    const sessionCard = page.locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
    await sessionCard
      .getByRole('button', { name: "Read everything kept of @zeus · amber-pine's conversations" })
      .click()
    const viewer = page.getByRole('dialog', { name: 'History of @zeus · amber-pine' })
    await expect(viewer).toBeVisible()
    await expect(viewer.locator('.history-who')).toHaveText('Pi · zeus')
    expect(await asked(page)).toEqual([
      { project: 1, handle: 'zeus-amber-pine', page: 1, tools: false },
    ])
    await expect(viewer.locator('.history-body')).toContainText('Write the lexer')
    await viewer.getByRole('button', { name: 'Close' }).click()
    // The chief's window goes down: its card stays, with History.
    await page.evaluate(() => {
      const chief = window.__model.board.lanes.find((l) => l.participant.handle === 'chief')
      Object.assign(chief, { pane: null, activity: { state: 'closed' } })
    })
    await changed(page)
    await expect(chiefCard(page).locator('.terminal-host')).toHaveCount(0)
    await expect(chiefCard(page).locator('.terminal-head > button')).toHaveText([
      'History',
      'Switch chief',
    ])
    await openChief(page)
  })

  test('opens from the keyboard, and the viewer goes with Escape', async ({ page }) => {
    await open(page)
    const history = chiefCard(page).getByRole('button', { name: /^Read everything kept of/ })
    await history.focus()
    await page.keyboard.press('Enter')
    const viewer = page.getByRole('dialog', { name: 'History of Chief of Staff' })
    await expect(viewer).toBeVisible()
    expect(await asked(page)).toEqual([{ project: 1, handle: 'chief', page: 1, tools: false }])
    await page.keyboard.press('Escape')
    await expect(viewer).toBeHidden()
  })
})

test.describe('the history viewer', () => {
  test('heads each conversation with its harness and agent, from when until when, and what ended it', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-hint')).toHaveText('Not stored: tool calls and thinking.')
    await expect(viewer.locator('.history-who')).toHaveText('Claude Code · ares')
    const said = await headings(viewer)
    expect(said.map(([heading]) => heading)).toEqual(['Claude Code · ares', 'Codex · maia'])
    expect(said[0][1]).toMatch(
      /^\d{4}-\d\d-\d\d \d\d:\d\d to \d{4}-\d\d-\d\d \d\d:\d\d · ended by Switch chief, to Codex · maia$/,
    )
    expect(said[1][1]).toMatch(
      /^since \d{4}-\d\d-\d\d \d\d:\d\d, going on now · 1 tool output left out$/,
    )
    await expect(viewer.locator('.history-conversation').first()).toContainText(
      'Its earlier items are on an older page.',
    )
    // The words for each end the ledger knows, and for one it does not.
    await viewer.getByRole('button', { name: 'Close' }).click()
    await page.evaluate(() => {
      const [conversations] = window.__model.histories.chief
      const [claude, codex] = conversations
      claude.endedBy = { by: 'window', resumed: false }
      codex.endedAt = '2026-10-16T12:00:00.000Z'
      codex.endedBy = { by: 'window', resumed: true }
      conversations.push(
        { ...codex, id: 3, endedBy: { by: 'fresh' }, items: [] },
        { ...codex, id: 4, endedBy: null, items: [] },
      )
    })
    await openChief(page)
    const ends = (await headings(viewer)).map(([, about]) =>
      about.split(' · ').slice(1).join(' · '),
    )
    expect(ends).toEqual([
      'ended when the window went to another conversation (/clear, /new or /resume)',
      'ended when the window resumed an earlier conversation (/resume) · 1 tool output left out',
      'ended when a window opened on a fresh conversation',
      'ended (the ledger does not say why)',
    ])
  })

  test("labels items as the task drawer does, a compaction's summary as one, and shows every text as text", async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    expect(await labels(viewer)).toEqual([
      'Compaction summary',
      'The agent',
      'Sent to the window',
      'The agent',
    ])
    await expect(
      viewer.locator('.history-item[data-compaction="true"] .transcript-body'),
    ).toHaveText('This session is being continued from a previous conversation.')
    // Markup in what an agent wrote is its words: nothing of it is drawn or run.
    await expect(viewer.locator('.history-item[data-role="assistant"]').first()).toContainText(
      HOSTILE,
    )
    await viewer.getByRole('button', { name: 'Older' }).click()
    await expect(viewer.locator('.history-delivery summary')).toContainText(HOSTILE)
    await expect(viewer.locator('img, script')).toHaveCount(0)
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  })

  test('folds a delivery to its line, under what the human typed with it, and opens it on a click or a key', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    await viewer.getByRole('button', { name: 'Older' }).click()
    const delivered = viewer.locator('.history-item').filter({ has: page.locator('details') })
    await expect(delivered.locator('> .transcript-body')).toHaveText('half a thought')
    const fold = delivered.locator('details.history-delivery')
    await expect(fold.locator('summary')).toHaveText(
      `· m-12: @zeus's result on T-1 (cf task get T-1) ${HOSTILE}`,
    )
    const whole = fold.locator('.transcript-body')
    await expect(whole).toBeHidden()
    await fold.locator('summary').click()
    await expect(whole).toHaveText(DELIVERY)
    await fold.locator('summary').click()
    await expect(whole).toBeHidden()
    await fold.locator('summary').focus()
    await page.keyboard.press('Enter')
    await expect(whole).toBeVisible()
  })

  test('pages from the most recent to older and back', async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const status = viewer.locator('.history-page')
    const older = viewer.getByRole('button', { name: 'Older' })
    const newer = viewer.getByRole('button', { name: 'Newer' })
    await expect(status).toHaveText('Page 1 of 2, the most recent')
    await expect(newer).toBeDisabled()
    await older.click()
    await expect(status).toHaveText('Page 2 of 2')
    await expect(older).toBeDisabled()
    await expect(viewer.locator('.history-conversation')).toHaveCount(1)
    await expect(viewer.locator('.history-conversation')).toContainText(
      'It goes on on a newer page.',
    )
    await newer.click()
    await expect(status).toHaveText('Page 1 of 2, the most recent')
    expect((await asked(page)).map((body) => body.page)).toEqual([1, 2, 1])
  })

  test('finds words in any case, and says when nothing holds them', async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Find in the history' })
    await find.fill('PARSER')
    await find.press('Enter')
    // The most recent page of what holds it: the one item of it there.
    await expect(viewer.locator('.history-item .transcript-body')).toHaveText([
      'Taken over: the parser is done.',
    ])
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      page: 1,
      tools: false,
      find: 'PARSER',
    })
    await expect(viewer.locator('.history-page')).toHaveText('Page 1 of 2, the most recent')
    await find.fill('zzz')
    await viewer.getByRole('button', { name: 'Find' }).click()
    await expect(viewer.locator('.history-note')).toHaveText(
      `Nothing in Chief of Staff's history holds "zzz".`,
    )
    await expect(viewer.getByRole('button', { name: 'Older' })).toBeDisabled()
    await expect(viewer.getByRole('button', { name: 'Newer' })).toBeDisabled()
  })

  test("shows the tools' outputs on the switch", async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-item[data-role="tool"]')).toHaveCount(0)
    await viewer.getByRole('checkbox', { name: 'Show tool output' }).check()
    await expect(viewer.locator('.history-item[data-role="tool"]')).toHaveText(/ls: 3 files/)
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      page: 1,
      tools: true,
    })
    expect(await labels(viewer)).toContain('Tool output')
    expect((await headings(viewer))[1][1]).not.toContain('left out')
  })

  test("says the daemon's refusal in its words, and draws in the app's theme", async ({ page }) => {
    const data = model()
    data.historyRefusal = '"chief" is not in project 1'
    await open(page, data)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-refusal')).toHaveText('"chief" is not in project 1')
    // Its colours are the theme's: change the theme's, and the viewer's change with them.
    const colours = () =>
      viewer.evaluate((dialog) => {
        const body = getComputedStyle(dialog.querySelector('.history-body'))
        return [
          getComputedStyle(dialog).backgroundColor,
          getComputedStyle(dialog).color,
          body.backgroundColor,
        ]
      })
    expect(await colours()).toEqual(['rgb(17, 42, 49)', 'rgb(233, 241, 239)', 'rgb(12, 32, 38)'])
    await page.evaluate(() => {
      const root = document.documentElement.style
      root.setProperty('--panel', '#fafafa')
      root.setProperty('--foam', '#101010')
      root.setProperty('--deep', '#f0f0f0')
    })
    expect(await colours()).toEqual(['rgb(250, 250, 250)', 'rgb(16, 16, 16)', 'rgb(240, 240, 240)'])
  })
})
