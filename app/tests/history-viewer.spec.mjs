import { expect, test } from '@playwright/test'

import { refusedByPolicy, serveUi } from './serve-ui.mjs'

/**
 * The history viewer (core/history-viewer.js) and the History button on every
 * window's head (core/terminals.js), against a stand-in for the app: the
 * daemon's `history.get` is answered from an in-page model of each history,
 * paged from a place as the daemon pages it (a few items a page), searched and
 * with the tools' outputs left out as the daemon does; `history.part` gives a
 * long item's parts; every call is recorded. The page is served under the
 * app's own Content-Security-Policy, which refuses nothing here.
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

/** Item `id` of conversation `conversation`, the `n`th copied there. */
const item = (conversation, n, id, role, text, extra = {}) => ({
  conversation,
  row: conversation * 100 + n,
  id,
  role,
  text,
  complete: true,
  at: '2026-10-16T09:30:00.000Z',
  ...extra,
})

const DELIVERY = '[ConsensFlow m-12 · T-1 · result from @zeus]\nParser done, 14 tests.'

/**
 * The chief's history: its Claude Code conversation on ares, which a Switch chief to
 * Codex on maia ended, and the one going on now. Four items a page, the tools' left
 * out: the most recent page holds the summary onwards, the one before it the rest.
 */
function chiefHistory() {
  return {
    conversations: [
      {
        id: 1,
        harness: 'claude-code',
        agent: 'ares',
        startedAt: '2026-10-16T09:00:00.000Z',
        endedAt: '2026-10-16T11:00:00.000Z',
        endedBy: { by: 'switch', harness: 'codex', agent: 'maia' },
      },
      {
        id: 2,
        harness: 'codex',
        agent: 'maia',
        startedAt: '2026-10-16T11:01:00.000Z',
        endedAt: null,
        endedBy: null,
      },
    ],
    items: [
      item(1, 0, 'u-1', 'user', 'Ship the parser'),
      item(1, 1, 'u-2', 'user', `half a thought${DELIVERY}`, {
        delivery: {
          line: `· m-12: @zeus's result on T-1 (cf task get T-1) ${HOSTILE}`,
          at: 'half a thought'.length,
        },
      }),
      item(1, 2, 'a-1', 'assistant', 'On it'),
      item(1, 3, 't-1', 'tool', 'ok 12 passed'),
      item(1, 4, 'u-3', 'user', 'This session is being continued from a previous conversation.', {
        compaction: true,
      }),
      item(1, 5, 'a-2', 'assistant', HOSTILE),
      item(2, 0, 'u-5', 'user', 'Where are we?'),
      item(2, 1, 't-2', 'tool', 'ls: 3 files'),
      item(2, 2, 'a-5', 'assistant', 'Taken over: the parser is done.'),
    ],
  }
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
    pageSize: 4,
    histories: {
      chief: chiefHistory(),
      // The session's: a short item, and a long one shown by its first part.
      'zeus-amber-pine': {
        conversations: [
          {
            id: 7,
            harness: 'pi',
            agent: 'zeus',
            startedAt: '2026-10-16T10:00:00.000Z',
            endedAt: null,
            endedBy: null,
          },
        ],
        items: [
          item(7, 0, 'w-1', 'user', 'Write the lexer'),
          item(7, 1, 'w-2', 'assistant', 'The first part', {
            cut: { from: 0, bytes: 700_000, revision: 1 },
          }),
        ],
      },
    },
    // A long item's parts, as `history.part` reads them.
    parts: { 'w-2': ['The first part', ' and the rest, ', 'to its end.'] },
  }
}

async function open(page, data = model()) {
  await page.addInitScript((data) => {
    window.__calls = []
    window.__model = data
    window.__listeners = new Map()
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    const order = (a, b) => a.conversation - b.conversation || a.row - b.row
    const placeOf = (it) => it && { conversation: it.conversation, row: it.row }
    const operations = {
      'projects.list': () => answer({ projects: data.projects }),
      'agents.list': () => answer({ agents: [], missing: [] }),
      'board.get': () => answer({ board: window.__model.board }),
      'inbox.get': () => answer({ messages: [], total: 0, shown: 0 }),
      // The daemon's pages: the most recent, the one before a place or the one after it;
      // a search keeps the items that hold it in any case, and the conversations it found
      // something in; the tools' outputs are left out, and counted, unless asked for. A
      // search the model says stops short (`partial`) finds nothing, and says where it stopped.
      'history.get': ({ handle, before, after, find, tools }) => {
        const model = window.__model
        if (model.historyRefusal) return { ok: false, error: model.historyRefusal }
        const history = model.histories[handle] ?? { conversations: [], items: [] }
        const size = model.pageSize
        const holds = (it) => !find || it.text.toLowerCase().includes(find.toLowerCase())
        const shown = history.items
          .filter((it) => tools || it.role !== 'tool')
          .filter((it) => holds(it) && !(find && model.partial))
        let items
        let older
        let newer
        if (after) {
          const rest = shown.filter((it) => order(it, after) > 0)
          items = rest.slice(0, size)
          older = placeOf(items[0]) ?? after
          newer = rest.length > size ? placeOf(items.at(-1)) : null
        } else {
          const rest = before ? shown.filter((it) => order(it, before) < 0) : shown
          items = rest.slice(-size)
          older = rest.length > size ? placeOf(items[0]) : null
          newer = before ? (placeOf(items.at(-1)) ?? before) : null
        }
        if (find && model.partial) older = { conversation: 1, row: 0 }
        const last = items.at(-1)?.conversation ?? -Infinity
        // The tools' outputs a heading counts, so far as the model says it counts them.
        const counted = model.toolsCounted ?? Infinity
        const conversations = history.conversations
          .map((conversation) => {
            const count = history.items.filter(
              (it) => it.conversation === conversation.id && it.role === 'tool',
            ).length
            return {
              ...conversation,
              toolsLeftOut: tools ? 0 : Math.min(count, counted),
              ...(!tools && count >= counted ? { moreToolsLeftOut: true } : {}),
              items: items
                .filter((it) => it.conversation === conversation.id)
                .map(({ conversation: _, ...shownItem }) => shownItem),
            }
          })
          .filter(
            (conversation) =>
              conversation.items.length > 0 || (!find && newer === null && conversation.id > last),
          )
        return answer({
          history: { conversations, older, newer, partial: !!(find && model.partial) },
        })
      },
      // A long item's text, a part at a time, as the daemon reads it, by the item's
      // place, with the revision the copy made of it. An item the model says is copied
      // again as it is read (`copies`) is, once a part: the next copy is read, the last kept.
      'history.part': ({ conversation, row, from }) => {
        const model = window.__model
        const it = Object.values(model.histories)
          .flatMap((history) => history.items)
          .find((it) => it.conversation === conversation && it.row === row)
        if (!it) return { ok: false, error: `no item at ${conversation}:${row}` }
        const copies = model.copies?.[it.id]
        const { revision, parts } =
          copies?.length > 1
            ? copies.shift()
            : (copies?.[0] ?? { revision: 1, parts: model.parts[it.id] })
        let at = 0
        for (const [index, text] of parts.entries()) {
          if (at === from) {
            const next = index + 1 < parts.length ? at + text.length : null
            return answer({ part: { text, from, next, revision } })
          }
          at += text.length
        }
        return { ok: false, error: `no part of ${it.id} from ${from}` }
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

/** What the page's requests for `operation` asked. */
const asked = (page, operation = 'history.get') =>
  page.evaluate(
    (operation) =>
      window.__calls
        .filter(([command, args]) => command === 'daemon_request' && args.operation === operation)
        .map(([, args]) => args.body),
    operation,
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

/** Opens a session's terminal, and its history from its card. */
async function openSession(page) {
  await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  await page
    .locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
    .getByRole('button', { name: "Read everything kept of @zeus · amber-pine's conversations" })
    .click()
  const viewer = page.getByRole('dialog', { name: 'History of @zeus · amber-pine' })
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

/** The ids the stand-in gave the items shown, by their text, in order. */
const texts = (viewer) => viewer.locator('.history-item > .transcript-body').allTextContents()

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
    const viewer = await openSession(page)
    await expect(viewer.locator('.history-who')).toHaveText('Pi · zeus')
    expect(await asked(page)).toEqual([{ project: 1, handle: 'zeus-amber-pine', tools: false }])
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
    expect(await asked(page)).toEqual([{ project: 1, handle: 'chief', tools: false }])
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
      /^\d{4}-\d\d-\d\d \d\d:\d\d to \d{4}-\d\d-\d\d \d\d:\d\d · ended by Switch chief, to Codex · maia · 1 tool output left out$/,
    )
    expect(said[1][1]).toMatch(
      /^since \d{4}-\d\d-\d\d \d\d:\d\d, going on now · 1 tool output left out$/,
    )
    // The words for each end the ledger knows, and for one it does not.
    await viewer.getByRole('button', { name: 'Close' }).click()
    await page.evaluate(() => {
      const { conversations } = window.__model.histories.chief
      const [claude, codex] = conversations
      claude.endedBy = { by: 'window', resumed: false }
      codex.endedAt = '2026-10-16T12:00:00.000Z'
      codex.endedBy = { by: 'window', resumed: true }
      conversations.push(
        { ...codex, id: 3, endedBy: { by: 'fresh' } },
        { ...codex, id: 4, endedBy: null },
      )
    })
    await openChief(page)
    const ends = (await headings(viewer)).map(([, about]) =>
      about.split(' · ').slice(1).join(' · '),
    )
    expect(ends).toEqual([
      'ended when the window went to another conversation (/clear, /new or /resume) · 1 tool output left out',
      'ended when the window resumed an earlier conversation (/resume) · 1 tool output left out',
      'ended when a window opened on a fresh conversation',
      'ended (the ledger does not say why)',
    ])
    // A heading counts the tools' outputs so far, and says when there may be more.
    await viewer.getByRole('button', { name: 'Close' }).click()
    await page.evaluate(() => {
      window.__model.toolsCounted = 1
    })
    await openChief(page)
    expect((await headings(viewer))[0][1]).toMatch(/ · 1 or more tool outputs left out$/)
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

  test('goes older from the oldest item shown and newer from the newest, never repeating or skipping one', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const status = viewer.locator('.history-page')
    const older = viewer.getByRole('button', { name: 'Older' })
    const newer = viewer.getByRole('button', { name: 'Newer' })
    await expect(status).toHaveText('The most recent')
    await expect(newer).toBeDisabled()
    const first = await texts(viewer)
    // Meanwhile the chief says more: a page already read is not moved by it.
    await page.evaluate(() => {
      const { items } = window.__model.histories.chief
      items.push({ ...items.at(-1), row: 203, id: 'a-6', text: 'And more.' })
    })
    await older.click()
    await expect(status).toHaveText('Earlier items')
    await expect(older).toBeDisabled()
    const before = await texts(viewer)
    expect([...before, ...first]).toEqual([
      'Ship the parser',
      'half a thought',
      'On it',
      'This session is being continued from a previous conversation.',
      HOSTILE,
      'Where are we?',
      'Taken over: the parser is done.',
    ])
    await newer.click()
    await expect(status).toHaveText('Earlier items')
    await expect(viewer.locator('.history-item')).toHaveCount(4)
    await newer.click()
    await expect(status).toHaveText('The most recent')
    expect(await texts(viewer)).toEqual(['And more.'])
    const places = (await asked(page)).map(({ before, after }) => ({ before, after }))
    expect(places).toEqual([
      { before: undefined, after: undefined },
      { before: { conversation: 1, row: 104 }, after: undefined },
      { before: undefined, after: { conversation: 1, row: 102 } },
      { before: undefined, after: { conversation: 2, row: 202 } },
    ])
  })

  test('finds words in any case, says when nothing holds them, and where a search stopped short', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Find in the history' })
    await find.fill('PARSER')
    await find.press('Enter')
    // The stored text is searched: the delivery holds "Parser done", under what was typed with it.
    expect(await texts(viewer)).toEqual([
      'Ship the parser',
      'half a thought',
      'Taken over: the parser is done.',
    ])
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      tools: false,
      find: 'PARSER',
    })
    await expect(viewer.locator('.history-page')).toHaveText('The most recent')
    await find.fill('zzz')
    await viewer.getByRole('button', { name: 'Find' }).click()
    await expect(viewer.locator('.history-note')).toHaveText(
      `Nothing in Chief of Staff's history holds "zzz".`,
    )
    await expect(viewer.getByRole('button', { name: 'Older' })).toBeDisabled()
    await expect(viewer.getByRole('button', { name: 'Newer' })).toBeDisabled()
    // A search through a long history stops at what one read may go through, and goes on.
    await page.evaluate(() => {
      window.__model.partial = true
    })
    await viewer.getByRole('button', { name: 'Find' }).click()
    await expect(viewer.locator('.history-note')).toHaveText(
      'Nothing found yet that holds "zzz": Older searches further back.',
    )
    await expect(viewer.locator('.history-page')).toHaveText(
      'The most recent; the search stopped here to stay quick: Older searches further back',
    )
    await viewer.getByRole('button', { name: 'Older' }).click()
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      tools: false,
      find: 'zzz',
      before: { conversation: 1, row: 0 },
    })
  })

  test("shows the tools' outputs on the switch", async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-item[data-role="tool"]')).toHaveCount(0)
    await viewer.getByRole('checkbox', { name: 'Show tool output' }).check()
    await expect(viewer.locator('.history-item[data-role="tool"]')).toHaveText(/ls: 3 files/)
    expect((await asked(page)).at(-1)).toEqual({ project: 1, handle: 'chief', tools: true })
    expect(await labels(viewer)).toContain('Tool output')
    expect((await headings(viewer)).at(-1)[1]).not.toContain('left out')
  })

  test('shows a long item by a part, and the whole of it on asking, read a part at a time', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openSession(page)
    const long = viewer.locator('.history-item').filter({ hasText: 'The first part' })
    await expect(long.locator('.history-cut')).toContainText('Its first part')
    await long.getByRole('button', { name: 'Show the whole item (684 KB)' }).click()
    const whole = viewer.locator('.history-item').filter({ hasText: 'to its end.' })
    await expect(whole.locator('.transcript-body')).toHaveText(
      'The first part and the rest, to its end.',
    )
    await expect(whole.locator('.history-cut')).toHaveCount(0)
    const read = (await asked(page, 'history.part')).map(({ from }) => from)
    expect(read).toEqual([0, 14, 29])
    expect((await asked(page, 'history.part'))[0]).toEqual({
      project: 1,
      handle: 'zeus-amber-pine',
      conversation: 7,
      row: 701,
      from: 0,
    })
    // A search shows the part where it found its words, and says from where.
    await page.evaluate(() => {
      window.__model.histories['zeus-amber-pine'].items[1].cut = { from: 5_000, bytes: 700_000 }
    })
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(viewer.locator('.history-cut')).toContainText('… from character 5001')
  })

  test('reads a long item again from its start when it is copied again meanwhile, and says so when it keeps changing', async ({
    page,
  }) => {
    const data = model()
    const first = ['The first part', ' and the rest, ', 'to its end.']
    // The new copy's every part differs, its first as long as the old copy's.
    const grown = ['The FIRST part', ' grown, ', 'and now ends.']
    // Copied again once its first part is read: the second part is the new copy's.
    data.copies = {
      'w-2': [
        { revision: 1, parts: first },
        { revision: 2, parts: grown },
      ],
    }
    await open(page, data)
    const viewer = await openSession(page)
    const long = viewer.locator('.history-item').filter({ hasText: 'The first part' })
    await long.getByRole('button', { name: 'Show the whole item (684 KB)' }).click()
    const whole = viewer.locator('.history-item').filter({ hasText: 'now ends.' })
    await expect(whole.locator('.transcript-body')).toHaveText(
      'The FIRST part grown, and now ends.',
    )
    // Never a text of two copies: the second copy is read again from its start.
    expect((await asked(page, 'history.part')).map(({ from }) => from)).toEqual([0, 14, 0, 14, 22])
    // An item copied again at every part, as one still being written fast may be:
    // three reads, then the viewer says so and may be asked again.
    await page.evaluate((first) => {
      window.__model.copies['w-2'] = Array.from({ length: 20 }, (_, at) => ({
        revision: 10 + at,
        parts: first,
      }))
    }, first)
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    const again = viewer.locator('.history-item').filter({ hasText: 'The first part' })
    const button = again.getByRole('button', { name: 'Show the whole item (684 KB)' })
    await button.click()
    await expect(again.locator('.history-refusal')).toHaveText(
      'It changed each time it was read: show it again to read it as it is now.',
    )
    await expect(button).toBeEnabled()
    expect((await asked(page, 'history.part')).length).toBe(5 + 6)
  })

  test('keeps a long delivery folded when its whole is the copy the page showed, and not one copied again since', async ({
    page,
  }) => {
    const data = model()
    const header = '[ConsensFlow m-12 · T-1 · result from @zeus]\n'
    const line = "· m-12: @zeus's result on T-1 (cf task get T-1)"
    // A long delivery, shown by its first part and folded to its line.
    data.histories['zeus-amber-pine'].items.push(
      item(7, 2, 'w-3', 'user', `${header}Parser done`, {
        cut: { from: 0, bytes: 700_000, revision: 1 },
        delivery: { line, at: 0 },
      }),
    )
    data.parts['w-3'] = [`${header}Parser done`, ', all of it.']
    await open(page, data)
    const viewer = await openSession(page)
    const long = () => viewer.locator('.history-item').nth(2)
    await expect(long().locator('.history-delivery summary')).toHaveText(line)
    await long().getByRole('button', { name: 'Show the whole item (684 KB)' }).click()
    await expect(long().locator('.history-cut')).toHaveCount(0)
    await expect(long().locator('.history-delivery summary')).toHaveText(line)
    await expect(long().locator('.history-delivery pre')).toContainText('Parser done, all of it.')
    // Copied again since the page was read, the human's words in its place: the whole of
    // the new copy is drawn as it is, under no line of the message it no longer holds.
    await page.evaluate(() => {
      window.__model.copies = {
        'w-3': [{ revision: 2, parts: ['Typed instead', ' by the human.'] }],
      }
    })
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(long().locator('.history-cut')).toHaveCount(1)
    await long().getByRole('button', { name: 'Show the whole item (684 KB)' }).click()
    await expect(long().locator('.history-cut')).toHaveCount(0)
    await expect(long().locator('.history-delivery')).toHaveCount(0)
    await expect(long().locator('.transcript-body')).toHaveText('Typed instead by the human.')
  })

  test('holds a search to 1,000 characters, and says so of a longer one', async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Find in the history' })
    await expect(find).toHaveAttribute('maxlength', '1000')
    const searches = async () => (await asked(page)).filter((asking) => asking.find).length
    // A longer one put in the box past its bound, as a paste or a script may.
    await find.evaluate((input) => {
      input.value = 'a'.repeat(1_001)
    })
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText(
      'A search holds at most 1,000 characters; this one has 1,001.',
    )
    expect(await searches()).toBe(0)
    await find.fill('🙂'.repeat(500))
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText(
      `Nothing in Chief of Staff's history holds "${'🙂'.repeat(500)}".`,
    )
    expect(await searches()).toBe(1)
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
