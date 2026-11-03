import { readFileSync } from 'node:fs'

import { expect, test } from '@playwright/test'

import { refusedByPolicy, serveUi } from './serve-ui.mjs'

/**
 * The history viewer (core/history-viewer.js) and the History button on every
 * window's head (core/terminals.js), against a stand-in for the app: the
 * daemon's `history.get` is answered from an in-page model of each history,
 * paged from a place as the daemon pages it, searched in an item's text and a
 * call's header as the daemon searches; `history.part` gives a long item's
 * parts; every call is recorded. The page is served under the app's own
 * Content-Security-Policy, which refuses nothing here.
 *
 * The view is drawn to the approved mock (chief-harness CHF-20,
 * `history-dark.png` and `history-light.png`): the human's prompts and
 * ConsensFlow's deliveries as blue blocks, the agent's words as markdown,
 * thinking dimmed, a tool's call as a block coloured by how it ended with its
 * output folded to its last lines, an edit's change as red and green rows, and
 * a compaction as a violet block.
 */
let ui
let refused

test.setTimeout(30_000)
// An action on what is not there fails its test, never times it out: a plant that renames a
// button is caught by the test, not taken for a broken run.
test.use({ actionTimeout: 5_000 })

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

/** A call's item: its output, and what its reader said of the call. */
const call = (conversation, n, id, header, text, extra = {}) =>
  item(conversation, n, id, 'tool', text, { kind: 'tool', name: 'Bash', header, ...extra })

const DELIVERY = '[ConsensFlow m-12 · T-1 · result from @zeus]\nParser done, 14 tests.'

/**
 * What Muse Code returned for a command that failed, as its live record wrote it (the
 * reader's fixture, line 11): JSON whose exit code is its fifth line of twelve.
 */
const MUSE_FAILED = JSON.parse(
  readFileSync(
    new URL('../../crates/cf-harness/tests/fixtures/history-items/muse.jsonl', import.meta.url),
    'utf8',
  ).split('\n')[10],
).payload.event.results[0].text

/** A failing test run's output: 35 lines of progress, then the failure and the exit line. */
const FAILING_RUN = [
  ...Array.from({ length: 35 }, (_, at) => `  ✓ case ${at + 1}`),
  'FAIL  filter.test.ts > empty query',
  'TypeError: Cannot read properties of undefined',
  'Command exited with code 1',
].join('\n')

const ANSWER = [
  '## Fixed the empty filter',
  '',
  '- Missing queries now use an empty string in `normalize`.',
  '- All 12 tests pass, including the regression case.',
  '',
  '```ts',
  'normalize(undefined) === ""',
  '```',
].join('\n')

const DIFF = [
  '@@ -2 +2 @@',
  '-  return query.trim().toLowerCase();',
  '+  return (query ?? "").trim().toLowerCase();',
].join('\n')

/**
 * The chief's history, the mock's own: a Claude Code conversation (1) that fixes a filter
 * and is compacted, then the one a Switch chief to Codex began (2).
 */
function chiefHistory() {
  return {
    items: [
      item(1, 0, 'u-1', 'user', 'Fix the crash when the task filter is empty.', {
        kind: 'prompt',
      }),
      item(1, 1, 'th-1', 'custom', 'I’ll check how the filter handles an empty query.', {
        kind: 'thinking',
      }),
      call(
        1,
        2,
        't-1',
        'read src/filter.ts:1-3',
        'export function normalize(query?: string) {\n  return query.trim().toLowerCase();\n}',
        { name: 'Read', status: 'ok', tookMs: 100 },
      ),
      call(1, 3, 't-2', '$ npm test', FAILING_RUN, { status: 'failed', tookMs: 1_200 }),
      call(1, 4, 't-3', 'edit src/filter.ts', 'The file src/filter.ts has been updated.', {
        name: 'Edit',
        status: 'ok',
        tookMs: 200,
        diff: DIFF,
      }),
      call(1, 5, 't-4', '$ npm test', 'PASS  filter.test.ts  12 tests passed', {
        status: 'ok',
        tookMs: 1_200,
      }),
      item(1, 6, 'a-1', 'assistant', ANSWER, { kind: 'text' }),
      item(1, 7, 'c-1', 'user', 'Summary: the empty filter is fixed, 12 tests pass.', {
        compaction: true,
        kind: 'compaction',
        tokens: 3_903,
      }),
      item(2, 0, 'u-2', 'user', 'Also cover a query containing only spaces.', { kind: 'prompt' }),
      item(2, 1, 'a-2', 'assistant', 'I’ll add a whitespace case to the same test file.', {
        kind: 'text',
      }),
    ],
  }
}

/**
 * A member's session: a delivery under what the human typed with it, a call still
 * running, rows an older build copied (no kind: drawn as plain blocks of their role),
 * and a long item shown by its first part.
 */
function sessionHistory() {
  return {
    items: [
      item(7, 0, 'w-1', 'user', `half a thought${DELIVERY}`, {
        kind: 'prompt',
        delivery: {
          line: "· m-12: @zeus's result on T-1 (cf task get T-1)",
          at: 'half a thought'.length,
        },
      }),
      call(7, 1, 'w-2', '$ cargo test', '', { status: 'running' }),
      item(7, 2, 'w-3', 'user', 'Write the lexer'),
      item(7, 3, 'w-4', 'assistant', 'On it, **now**.'),
      item(7, 4, 'w-5', 'tool', 'ok 12 passed'),
      item(7, 5, 'w-6', 'user', 'This session is being continued from a previous conversation.', {
        compaction: true,
      }),
      item(7, 6, 'w-7', 'assistant', 'The first part', {
        cut: { from: 0, bytes: 700_000, revision: 1 },
      }),
    ],
  }
}

function model() {
  const chief = participant(2, 'chief', 'chief', {
    agent: 'ares',
    harness: 'claude-code',
  })
  const zeus = participant(3, 'zeus', 'worker', {
    agent: 'zeus',
    harness: 'pi',
    tier: 'standard',
  })
  const session = participant(4, 'zeus-amber-pine', 'worker', {
    agent: 'zeus',
    harness: 'pi',
    tier: 'standard',
    memberId: 3,
    member: 'zeus',
    session: 'amber-pine',
  })
  const project = {
    id: 1,
    name: 'harbour',
    directory: '/work/harbour',
    state: 'open',
  }
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
        {
          participant: zeus,
          tasks: [],
          activity: { state: 'closed' },
          pane: null,
        },
        {
          participant: session,
          tasks: [],
          activity: { state: 'working' },
          pane: { id: 'p1-zeus-amber-pine', generation: 2 },
        },
      ],
    },
    pageSize: 20,
    histories: {
      chief: chiefHistory(),
      'zeus-amber-pine': sessionHistory(),
    },
    // A long item's parts, as `history.part` reads them.
    parts: { 'w-7': ['The first part', ' and the rest, ', 'to its end.'] },
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
      // The daemon's pages: the most recent, the one before a place or the one after it,
      // one stream of items across conversations, each with its conversation and row; a
      // search keeps the items whose text or call header holds it, in any case; the tools'
      // outputs are left out unless asked for. A search the model says stops short
      // (`partial`) finds nothing, and says where it stopped.
      'history.get': ({ handle, before, after, find, tools }) => {
        const model = window.__model
        if (model.historyRefusal) return { ok: false, error: model.historyRefusal }
        const history = model.histories[handle] ?? { items: [] }
        const size = model.pageSize
        const folded = (text) => (text ?? '').toLowerCase()
        const holds = (it) =>
          !find ||
          folded(it.text).includes(folded(find)) ||
          folded(it.header).includes(folded(find))
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
        // A search that stops short once, before anything it shows; Older goes on to the end.
        const once = !!(find && model.partialOnce && !before)
        if (once) {
          items = []
          older = { conversation: 3, row: 0 }
        }
        // A read the model says went through as much as one may before anything it shows.
        if (!find && model.readStops && !before) {
          items = []
          older = { conversation: 3, row: 0 }
        }
        const partial = !!(find ? model.partial || once : model.readStops && !before)
        return answer({ history: { items, older, newer, partial } })
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
    .getByRole('button', {
      name: "Read everything kept of Chief of Staff's conversations",
    })
    .click()
  const viewer = page.getByRole('dialog', { name: 'History', exact: true })
  await expect(viewer).toBeVisible()
  return viewer
}

/** Opens a session's terminal, and its history from its card. */
async function openSession(page) {
  await page.locator('button.fold-sessions[aria-expanded="false"]').first().click()
  await page.getByRole('button', { name: "Show @zeus · amber-pine's terminal" }).click()
  const history = page
    .locator('#stage .terminal-card[data-handle="zeus-amber-pine"]')
    .getByRole('button', {
      name: "Read everything kept of @zeus · amber-pine's conversations",
    })
  await expect(history).toBeVisible()
  await history.click()
  const viewer = page.getByRole('dialog', { name: 'History', exact: true })
  await expect(viewer).toBeVisible()
  return viewer
}

/** The items shown, in order, each by the first line it shows. */
const heads = (viewer) =>
  viewer
    .locator('.history-item')
    .evaluateAll((items) => items.map((it) => it.innerText.trim().split('\n')[0]))

/** The mock's items by the first line each shows. */
const HEAD = {
  'u-1': 'Fix the crash when the task filter is empty.',
  'th-1': 'I’ll check how the filter handles an empty query.',
  't-1': 'read src/filter.ts:1-3',
  't-2': '$ npm test',
  't-3': 'edit src/filter.ts',
  't-4': '$ npm test',
  'a-1': 'Fixed the empty filter',
  'c-1': '[compaction]',
  'u-2': 'Also cover a query containing only spaces.',
  'a-2': 'I’ll add a whitespace case to the same test file.',
  'a-6': 'And more.',
}
const headsOf = (...names) => names.map((name) => HEAD[name])

/** The last item shown. */
const lastItem = (viewer) => viewer.locator('.history-item').last()

/** The block that draws item `id`. */
const block = (viewer, id) => viewer.locator(`.history-item[data-id="${id}"]`)

/** The colour `property` of `locator`'s element computes to. */
const colour = (locator, property = 'background-color') =>
  locator.evaluate((element, property) => getComputedStyle(element)[property], property)

/** The quiet first row that loads what comes before the items shown. */
const earlier = (viewer) => viewer.locator('.history-earlier')

test.describe('the History button', () => {
  test("is on the chief's head beside Switch chief, on a session's, and on the chief's card with no window", async ({
    page,
  }) => {
    await open(page)
    await expect(chiefCard(page).locator('.terminal-head > button')).toHaveText([
      'History',
      'Switch chief',
    ])
    // A session's terminal, once the human shows it, reads its own history, tools' calls too.
    const viewer = await openSession(page)
    expect(await asked(page)).toEqual([{ project: 1, handle: 'zeus-amber-pine', tools: true }])
    await expect(viewer.locator('.history-body')).toContainText('Write the lexer')
    await viewer.getByRole('button', { name: 'Close' }).click()
    await expect(viewer).toBeHidden()
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
    const history = chiefCard(page).getByRole('button', {
      name: /^Read everything kept of/,
    })
    await history.focus()
    await page.keyboard.press('Enter')
    const viewer = page.getByRole('dialog', { name: 'History', exact: true })
    await expect(viewer).toBeVisible()
    expect(await asked(page)).toEqual([{ project: 1, handle: 'chief', tools: true }])
    await page.keyboard.press('Escape')
    await expect(viewer).toBeHidden()
  })
})

test.describe('the history viewer', () => {
  test('draws one stream of items across conversations, titled History and naming no conversation, harness or model', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-title')).toHaveText('History')
    // What the mock has above the stream: the title, the search, the close. Nothing says
    // what is not stored, and nothing switches the tools' calls off.
    await expect(viewer.getByRole('searchbox', { name: 'Search history' })).toBeVisible()
    await expect(viewer.locator('.history-hint')).toHaveCount(0)
    await expect(viewer.getByRole('checkbox')).toHaveCount(0)
    await expect(viewer.getByRole('button', { name: /^(Older|Newer)$/ })).toHaveCount(0)
    await expect(
      viewer.locator('.history-who, .history-conversation, .history-heading, .history-about'),
    ).toHaveCount(0)
    // Both conversations, in one list, oldest first.
    await expect(viewer.locator('.history-body ol.history-items')).toHaveCount(1)
    expect(await heads(viewer)).toEqual(
      headsOf('u-1', 'th-1', 't-1', 't-2', 't-3', 't-4', 'a-1', 'c-1', 'u-2', 'a-2'),
    )
    // No item is headed by a label or a time: the block's look says what it is.
    await expect(viewer.locator('.history-item .transcript-head')).toHaveCount(0)
    const said = await viewer.textContent()
    for (const name of ['Claude Code', 'Codex', 'ares', 'Chief of Staff', 'switch']) {
      expect(said).not.toContain(name)
    }
    // The newest item is in view.
    const body = viewer.locator('.history-body')
    expect(
      await body.evaluate((body) => body.scrollTop + body.clientHeight >= body.scrollHeight - 1),
    ).toBe(true)
    // A member's session is drawn the same way.
    await viewer.getByRole('button', { name: 'Close' }).click()
    const session = await openSession(page)
    await expect(session.locator('.history-title')).toHaveText('History')
    // A delivery is shown whole (O3), so its own header names its sender: only it may.
    const theirs = await session.evaluate((dialog) =>
      [...dialog.querySelectorAll('.history-head, .history-item:not([data-id="w-1"])')]
        .map((it) => it.textContent)
        .join('\n'),
    )
    for (const name of ['Pi', 'zeus', 'amber-pine']) expect(theirs).not.toContain(name)
  })

  test("draws the human's prompts as blue blocks, and ConsensFlow's delivery in full in the same block", async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const prompt = block(viewer, 'u-1')
    await expect(prompt).toHaveAttribute('data-kind', 'prompt')
    await expect(prompt).toHaveText('Fix the crash when the task filter is empty.')
    const blue = await colour(prompt)
    expect(blue).toBe(await colour(block(viewer, 'u-2')))
    expect(blue).not.toBe(await colour(viewer.locator('.history-body')))
    await viewer.getByRole('button', { name: 'Close' }).click()
    // A delivery is shown whole, after what the human typed with it, in one blue block:
    // nothing folds it to its line.
    const session = await openSession(page)
    const delivered = block(session, 'w-1')
    await expect(delivered).toHaveAttribute('data-kind', 'prompt')
    expect(await colour(delivered)).toBe(blue)
    await expect(delivered.locator('details, summary')).toHaveCount(0)
    await expect(delivered.locator('.history-text')).toHaveText(['half a thought', DELIVERY])
    await expect(delivered.locator('.history-text').nth(1)).toBeVisible()
  })

  test("draws the agent's words as markdown, in the mock's heading, marker and inline-code colours", async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const answer = block(viewer, 'a-1')
    await expect(answer).toHaveAttribute('data-kind', 'text')
    const heading = answer.locator('.md-heading')
    await expect(heading).toHaveText('Fixed the empty filter')
    await expect(answer.locator('li')).toHaveText([
      'Missing queries now use an empty string in normalize.',
      'All 12 tests pass, including the regression case.',
    ])
    await expect(answer.locator('li code')).toHaveText('normalize')
    await expect(answer.locator('pre code')).toHaveText('normalize(undefined) === ""')
    // Each its own colour, none the words' own.
    const words = await colour(answer.locator('li').first(), 'color')
    const marker = await answer
      .locator('li')
      .first()
      .evaluate((li) => getComputedStyle(li, '::marker').color)
    const seen = [
      await colour(heading, 'color'),
      marker,
      await colour(answer.locator('li code'), 'color'),
    ]
    expect(new Set([words, ...seen]).size).toBe(4)
    await expect(heading).toHaveCSS('text-decoration-line', 'underline')
    // The agent's words sit on the stream itself, in no block.
    expect(await colour(answer)).toBe('rgba(0, 0, 0, 0)')
    // A plain answer is words.
    await expect(block(viewer, 'a-2')).toHaveText(
      'I’ll add a whitespace case to the same test file.',
    )
  })

  test('draws thinking italic and dimmed, on the stream', async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const thinking = block(viewer, 'th-1')
    await expect(thinking).toHaveAttribute('data-kind', 'thinking')
    await expect(thinking).toHaveText('I’ll check how the filter handles an empty query.')
    await expect(thinking.locator('.history-text')).toHaveCSS('font-style', 'italic')
    const dim = await colour(thinking.locator('.history-text'), 'color')
    expect(dim).not.toBe(await colour(block(viewer, 'a-2').locator('p').first(), 'color'))
    expect(await colour(thinking)).toBe('rgba(0, 0, 0, 0)')
  })

  test('draws a call as a block: its header, its output folded to the last 5 lines, the time it took, green, red or grey', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const read = block(viewer, 't-1')
    await expect(read).toHaveAttribute('data-kind', 'tool')
    await expect(read).toHaveAttribute('data-status', 'ok')
    // The header: the verb, the path and the lines, each its own colour.
    const head = read.locator('.history-call')
    await expect(head).toHaveText('read src/filter.ts:1-3')
    await expect(head.locator('.history-verb')).toHaveText('read')
    await expect(head.locator('.history-path')).toHaveText('src/filter.ts')
    await expect(head.locator('.history-range')).toHaveText(':1-3')
    await expect(read.locator('.history-output')).toHaveText(
      'export function normalize(query?: string) {\n  return query.trim().toLowerCase();\n}',
    )
    await expect(read.locator('.history-fold')).toHaveCount(0)
    await expect(read.locator('.history-took')).toHaveText('Took 0.1s')
    // A failed run: red, its 38 lines folded to the last 5, the exit line among them.
    const failed = block(viewer, 't-2')
    await expect(failed).toHaveAttribute('data-status', 'failed')
    await expect(failed.locator('.history-call')).toHaveText('$ npm test')
    const fold = failed.getByRole('button', { name: '… (33 earlier lines, click to expand)' })
    await expect(fold).toBeVisible()
    await expect(failed.locator('.history-output')).toHaveText(
      [
        '  ✓ case 34',
        '  ✓ case 35',
        'FAIL  filter.test.ts > empty query',
        'TypeError: Cannot read properties of undefined',
        'Command exited with code 1',
      ].join('\n'),
    )
    await expect(failed.locator('.history-took')).toHaveText('Took 1.2s')
    const green = await colour(read)
    const red = await colour(failed)
    expect(red).not.toBe(green)
    expect(await colour(block(viewer, 't-4'))).toBe(green)
    // A click unfolds the whole output.
    await fold.click()
    await expect(failed.locator('.history-fold')).toHaveCount(0)
    await expect(failed.locator('.history-output')).toHaveText(FAILING_RUN)
    await viewer.getByRole('button', { name: 'Close' }).click()
    // A call still running, and an older build's call output with no status: grey both.
    const session = await openSession(page)
    const running = block(session, 'w-2')
    await expect(running).toHaveAttribute('data-status', 'running')
    await expect(running.locator('.history-call')).toHaveText('$ cargo test')
    await expect(running.locator('.history-took')).toHaveText('Still running')
    const grey = await colour(running)
    expect([green, red]).not.toContain(grey)
    expect(await colour(block(session, 'w-4'))).not.toBe(grey)
    expect(await colour(block(session, 'w-5'))).toBe(grey)
  })

  test('unfolds an output by the keyboard, and keeps an exit line in sight that the last 5 lines do not hold', async ({
    page,
  }) => {
    const data = model()
    // Claude Code says the exit first, then what the command wrote.
    const output = ['Exit code 2', ...Array.from({ length: 9 }, (_, at) => `line ${at + 1}`)]
    data.histories.chief.items.push(
      call(2, 2, 't-9', '$ make', `${output.join('\n')}\n\n`, { status: 'failed', tookMs: 61_500 }),
      call(
        2,
        3,
        't-10',
        '$ make check',
        ['Process exited with code 0', ...output.slice(1)].join('\n'),
        {
          status: 'ok',
        },
      ),
    )
    await open(page, data)
    const viewer = await openChief(page)
    const failed = block(viewer, 't-9')
    // Trailing blank lines are not lines of the output.
    const fold = failed.getByRole('button', { name: '… (5 earlier lines, click to expand)' })
    await expect(failed.locator('.history-output')).toHaveText(
      ['line 5', 'line 6', 'line 7', 'line 8', 'line 9'].join('\n'),
    )
    await expect(failed.locator('.history-exit')).toHaveText('Exit code 2')
    await expect(failed.locator('.history-took')).toHaveText('Took 1m 1.5s')
    await fold.focus()
    await page.keyboard.press('Enter')
    await expect(failed.locator('.history-output')).toHaveText(output.join('\n'))
    // Shown whole, the exit line is in it once.
    await expect(failed.locator('.history-exit')).toHaveCount(0)
    // A call that went well says no exit line, wherever its output says one.
    const ok = block(viewer, 't-10')
    await expect(ok.locator('.history-fold')).toHaveCount(1)
    await expect(ok.locator('.history-exit')).toHaveCount(0)
  })

  test("draws an edit's change as red and green rows", async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const edit = block(viewer, 't-3')
    await expect(edit.locator('.history-call')).toHaveText('edit src/filter.ts')
    await expect(edit.locator('.history-path')).toHaveText('src/filter.ts')
    const rows = edit.locator('.history-diff > *')
    await expect(rows).toHaveText([
      '@@ -2 +2 @@',
      '-  return query.trim().toLowerCase();',
      '+  return (query ?? "").trim().toLowerCase();',
    ])
    await expect(rows.nth(1)).toHaveAttribute('data-sign', 'removed')
    await expect(rows.nth(2)).toHaveAttribute('data-sign', 'added')
    const removed = await colour(rows.nth(1), 'color')
    const added = await colour(rows.nth(2), 'color')
    expect(removed).not.toBe(added)
    expect([removed, added]).not.toContain(await colour(rows.nth(0), 'color'))
    // The change says what the edit did: its tool's own words of it are not drawn.
    await expect(edit).not.toContainText('has been updated')
    await expect(edit.locator('.history-took')).toHaveText('Took 0.2s')
  })

  test('draws a compaction as a violet block, with its tokens, and its summary on a click', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const compaction = block(viewer, 'c-1')
    await expect(compaction).toHaveAttribute('data-kind', 'compaction')
    await expect(compaction.locator('.history-call')).toHaveText('[compaction]')
    const expand = compaction.getByRole('button', {
      name: 'Compacted from 3,903 tokens (click to expand)',
    })
    await expect(expand).toBeVisible()
    const summary = compaction.locator('.history-text')
    await expect(summary).toBeHidden()
    await expand.click()
    await expect(summary).toHaveText('Summary: the empty filter is fixed, 12 tests pass.')
    await expect(summary).toBeVisible()
    const violet = await colour(compaction)
    for (const other of ['u-1', 't-1', 't-2']) {
      expect(await colour(block(viewer, other))).not.toBe(violet)
    }
    await viewer.getByRole('button', { name: 'Close' }).click()
    // An older build's compaction summary, which says no tokens, is a compaction all the same.
    const session = await openSession(page)
    const old = block(session, 'w-6')
    await expect(old).toHaveAttribute('data-kind', 'compaction')
    expect(await colour(old)).toBe(violet)
    await old.getByRole('button', { name: 'Compacted (click to expand)' }).click()
    await expect(old.locator('.history-text')).toHaveText(
      'This session is being continued from a previous conversation.',
    )
  })

  test('draws the rows an older build copied as plain blocks of their role', async ({ page }) => {
    await open(page)
    const viewer = await openSession(page)
    // No kind: the human's words a prompt, the agent's words its text, a tool's output a
    // grey block headed as the drawer heads one.
    await expect(block(viewer, 'w-3')).toHaveAttribute('data-kind', 'prompt')
    await expect(block(viewer, 'w-3')).toHaveText('Write the lexer')
    await expect(block(viewer, 'w-4')).toHaveAttribute('data-kind', 'text')
    await expect(block(viewer, 'w-4')).toHaveText('On it, now.')
    const output = block(viewer, 'w-5')
    await expect(output).toHaveAttribute('data-kind', 'tool')
    await expect(output).toHaveAttribute('data-status', 'unknown')
    await expect(output.locator('.history-call')).toHaveText('Tool output')
    await expect(output.locator('.history-output')).toHaveText('ok 12 passed')
    await expect(output.locator('.history-took')).toHaveCount(0)
  })

  test('shows every text as text, never as markup', async ({ page }) => {
    const data = model()
    data.histories.chief.items = [
      item(1, 0, 'x-1', 'user', `${HOSTILE}${DELIVERY}${HOSTILE}`, {
        kind: 'prompt',
        delivery: { line: HOSTILE, at: HOSTILE.length },
      }),
      item(1, 1, 'x-2', 'custom', HOSTILE, { kind: 'thinking' }),
      call(1, 2, 'x-3', `$ ${HOSTILE}`, `${HOSTILE}\n`.repeat(9), {
        status: 'failed',
        diff: `-${HOSTILE}\n+${HOSTILE}`,
      }),
      call(1, 3, 'x-4', `read ${HOSTILE}:1-2`, HOSTILE, { status: 'ok' }),
      item(1, 4, 'x-5', 'assistant', `# ${HOSTILE}\n\n- \`${HOSTILE}\`\n\n${HOSTILE}`, {
        kind: 'text',
      }),
      item(1, 5, 'x-6', 'user', HOSTILE, { compaction: true, kind: 'compaction', tokens: 1 }),
      item(1, 6, 'x-7', 'custom', HOSTILE, { kind: 'notice' }),
      item(1, 7, 'x-8', 'assistant', HOSTILE),
    ]
    await open(page, data)
    const viewer = await openChief(page)
    // Every item shows the markup as its words, whatever it is drawn as; unfolded and opened,
    // every word of it is there.
    const items = viewer.locator('.history-item')
    await expect(items).toHaveCount(8)
    await viewer.getByRole('button', { name: /earlier lines, click to expand/ }).click()
    await viewer.getByRole('button', { name: /^Compacted/ }).click()
    for (let at = 0; at < 8; at += 1) await expect(items.nth(at)).toContainText(HOSTILE)
    const said = (await viewer.locator('.history-body').textContent()).split(HOSTILE).length - 1
    // 2 in the prompt, 1 thought, 12 in the failed call (header, change, 9 lines), 2 in the
    // read, 3 in the markdown, the summary, the notice, the plain answer.
    expect(said).toBe(23)
    await expect(viewer.locator('img, script')).toHaveCount(0)
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  })

  test('loads earlier history from a quiet first row, above what is shown, never repeating or skipping an item', async ({
    page,
  }) => {
    const data = model()
    data.pageSize = 4
    // A window short enough that the first page overflows it: what is in view can be kept.
    await page.setViewportSize({ width: 1000, height: 320 })
    await open(page, data)
    const viewer = await openChief(page)
    expect(await heads(viewer)).toEqual(headsOf('a-1', 'c-1', 'u-2', 'a-2'))
    const row = earlier(viewer)
    await expect(row).toHaveText('… earlier history, click to load')
    const first = viewer.locator('ol.history-items > :first-child .history-earlier')
    await expect(first).toHaveCount(1)
    // Meanwhile the chief says more: a page already read is not moved by it.
    await page.evaluate(() => {
      const { items } = window.__model.histories.chief
      items.push({ ...items.at(-1), row: 203, id: 'a-6', text: 'And more.' })
    })
    // The row is the stream's first, and loading it keeps what was in view where it was.
    const body = viewer.locator('.history-body')
    const top = () => block(viewer, 'a-1').evaluate((it) => it.getBoundingClientRect().top)
    // The row is scrolled to before it is clicked, as a click scrolls to what it clicks.
    await row.scrollIntoViewIfNeeded()
    const before = await top()
    await row.click()
    await expect(block(viewer, 't-1')).toHaveCount(1)
    expect(await heads(viewer)).toEqual(
      headsOf('t-1', 't-2', 't-3', 't-4', 'a-1', 'c-1', 'u-2', 'a-2'),
    )
    expect(Math.abs((await top()) - before)).toBeLessThan(2)
    await expect(first).toHaveCount(1)
    await row.focus()
    await page.keyboard.press('Enter')
    expect(await heads(viewer)).toEqual(
      headsOf('u-1', 'th-1', 't-1', 't-2', 't-3', 't-4', 'a-1', 'c-1', 'u-2', 'a-2'),
    )
    // Nothing older: the row goes.
    await expect(earlier(viewer)).toHaveCount(0)
    expect(
      (await asked(page)).map(({ before, after }) => ({
        before,
        after,
      })),
    ).toEqual([
      { before: undefined, after: undefined },
      { before: { conversation: 1, row: 106 }, after: undefined },
      { before: { conversation: 1, row: 102 }, after: undefined },
    ])
    // Refresh reads the most recent again: what the chief said meanwhile is there.
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    expect(await heads(viewer)).toEqual(headsOf('c-1', 'u-2', 'a-2', 'a-6'))
    await expect(body.locator('.history-earlier')).toHaveCount(1)
  })

  test("finds words in any case, in a call's header and its output, says when nothing holds them, and where a search stopped short", async ({
    page,
  }) => {
    await open(page)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Search history' })
    await find.fill('NPM TEST')
    await find.press('Enter')
    // Found in the calls' headers, the output alone holding none of it.
    expect(await heads(viewer)).toEqual(headsOf('t-2', 't-4'))
    await expect(block(viewer, 't-4').locator('.history-call')).toHaveText('$ npm test')
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      tools: true,
      find: 'NPM TEST',
    })
    await find.fill('typeerror')
    await find.press('Enter')
    expect(await heads(viewer)).toEqual(headsOf('t-2'))
    await find.fill('zzz')
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText('Nothing in this history holds "zzz".')
    await expect(earlier(viewer)).toHaveCount(0)
    // A search through a long history stops at what one read may go through, and goes on.
    await page.evaluate(() => {
      window.__model.partial = true
    })
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText('Nothing found yet that holds "zzz".')
    await expect(earlier(viewer)).toHaveText(
      '… the search stopped here to stay quick, click to search further back',
    )
    await earlier(viewer).click()
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      tools: true,
      find: 'zzz',
      before: { conversation: 1, row: 0 },
    })
  })

  test('says where a read stopped that found nothing to show, and goes on from there', async ({
    page,
  }) => {
    const data = model()
    data.readStops = true
    await open(page, data)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-note')).toHaveText('Nothing to show yet.')
    await expect(earlier(viewer)).toHaveText(
      '… the read stopped here to stay quick, click to read further back',
    )
    await earlier(viewer).click()
    expect((await asked(page)).at(-1)).toEqual({
      project: 1,
      handle: 'chief',
      tools: true,
      before: { conversation: 3, row: 0 },
    })
    await expect(viewer.locator('.history-note')).toHaveCount(0)
    expect((await heads(viewer)).length).toBe(10)
  })

  test('shows a long item by a part, and the whole of it on a click row, read a part at a time', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openSession(page)
    const long = lastItem(viewer)
    await expect(long.locator('.history-text, p').first()).toHaveText('The first part')
    const more = long.getByRole('button', { name: '… (684 KB in all, click to load the rest)' })
    await more.click()
    await expect(lastItem(viewer)).toHaveText('The first part and the rest, to its end.')
    await expect(lastItem(viewer).locator('.history-more')).toHaveCount(0)
    const read = (await asked(page, 'history.part')).map(({ from }) => from)
    expect(read).toEqual([0, 14, 29])
    expect((await asked(page, 'history.part'))[0]).toEqual({
      project: 1,
      handle: 'zeus-amber-pine',
      conversation: 7,
      row: 706,
      from: 0,
    })
    // A search shows the part where it found its words, and says from where.
    await page.evaluate(() => {
      window.__model.histories['zeus-amber-pine'].items[6].cut = {
        from: 5_000,
        bytes: 700_000,
      }
    })
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(lastItem(viewer).locator('.history-more')).toHaveText(
      '… (684 KB in all, shown from character 5,001, click to load it whole)',
    )
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
      'w-7': [
        { revision: 1, parts: first },
        { revision: 2, parts: grown },
      ],
    }
    await open(page, data)
    const viewer = await openSession(page)
    const more = () => lastItem(viewer).locator('.history-more')
    await more().click()
    await expect(lastItem(viewer)).toHaveText('The FIRST part grown, and now ends.')
    // Never a text of two copies: the second copy is read again from its start.
    expect((await asked(page, 'history.part')).map(({ from }) => from)).toEqual([0, 14, 0, 14, 22])
    // An item copied again at every part, as one still being written fast may be:
    // three reads, then the viewer says so and may be asked again.
    await page.evaluate((first) => {
      window.__model.copies['w-7'] = Array.from({ length: 20 }, (_, at) => ({
        revision: 10 + at,
        parts: first,
      }))
    }, first)
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await more().click()
    await expect(lastItem(viewer).locator('.history-refusal')).toHaveText(
      'It changed each time it was read: show it again to read it as it is now.',
    )
    await expect(more()).toBeEnabled()
    expect((await asked(page, 'history.part')).length).toBe(5 + 6)
  })

  test('shows a long delivery whole in its blue block once read, and a copy that holds it no longer as it is', async ({
    page,
  }) => {
    const data = model()
    const header = '[ConsensFlow m-12 · T-1 · result from @zeus]\n'
    data.histories['zeus-amber-pine'].items.push(
      item(7, 7, 'w-8', 'user', `Typed ${header}Parser done`, {
        kind: 'prompt',
        cut: { from: 0, bytes: 700_000, revision: 1 },
        delivery: { line: "· m-12: @zeus's result on T-1 (cf task get T-1)", at: 6 },
      }),
    )
    data.parts['w-8'] = [`Typed ${header}Parser done`, ', all of it.']
    await open(page, data)
    const viewer = await openSession(page)
    await lastItem(viewer).locator('.history-more').click()
    await expect(lastItem(viewer).locator('.history-text')).toHaveText([
      'Typed',
      `${header}Parser done, all of it.`,
    ])
    await expect(lastItem(viewer)).toHaveAttribute('data-kind', 'prompt')
    // Copied again since the page was read, the human's words in its place: drawn as it is,
    // not split where the delivery the page showed began.
    await page.evaluate(() => {
      window.__model.copies = {
        'w-8': [{ revision: 2, parts: ['Typed instead', ' by the human.'] }],
      }
    })
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await lastItem(viewer).locator('.history-more').click()
    await expect(lastItem(viewer).locator('.history-text')).toHaveText([
      'Typed instead by the human.',
    ])
  })

  test('holds a search to 1,000 characters, and says so of a longer one', async ({ page }) => {
    await open(page)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Search history' })
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
      `Nothing in this history holds "${'🙂'.repeat(500)}".`,
    )
    expect(await searches()).toBe(1)
  })

  test("says the daemon's refusal in its words", async ({ page }) => {
    const data = model()
    data.historyRefusal = '"chief" is not in project 1'
    await open(page, data)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-refusal')).toHaveText('"chief" is not in project 1')
  })

  for (const [scheme, expected] of [
    [
      'dark',
      {
        dialog: 'rgb(30, 33, 36)',
        words: 'rgb(230, 232, 234)',
        prompt: 'rgb(31, 74, 99)',
        ok: 'rgb(42, 74, 53)',
        failed: 'rgb(110, 40, 40)',
        compaction: 'rgb(62, 46, 92)',
      },
    ],
    [
      'light',
      {
        dialog: 'rgb(246, 245, 241)',
        words: 'rgb(34, 36, 38)',
        prompt: 'rgb(214, 230, 240)',
        ok: 'rgb(220, 235, 217)',
        failed: 'rgb(242, 212, 212)',
        compaction: 'rgb(230, 220, 242)',
      },
    ],
  ]) {
    test(`draws in the mock's ${scheme} colours, in a monospace of the Mac's or Windows'`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme })
      await open(page)
      const viewer = await openChief(page)
      expect({
        dialog: await colour(viewer),
        words: await colour(viewer, 'color'),
        prompt: await colour(block(viewer, 'u-1')),
        ok: await colour(block(viewer, 't-1')),
        failed: await colour(block(viewer, 't-2')),
        compaction: await colour(block(viewer, 'c-1')),
      }).toEqual(expected)
      // Each engine says the faces its own way: WebKit without their quotes.
      const font = (await colour(viewer.locator('.history-body'), 'font-family'))
        .split(',')
        .map((face) => face.trim().replaceAll('"', ''))
      for (const face of ['SF Mono', 'Menlo', 'Cascadia Mono', 'Consolas', 'monospace']) {
        expect(font).toContain(face)
      }
      // The Mac's faces before Windows', and a monospace last.
      expect(font.indexOf('Menlo')).toBeLessThan(font.indexOf('Consolas'))
      expect(font.at(-1)).toBe('monospace')
    })
  }
})

test.describe('the history viewer, after its review (T-1210)', () => {
  test("keeps each harness's exit line of a failed call in sight, Muse Code's JSON line too, its colour from the status alone", async ({
    page,
  }) => {
    const nine = Array.from({ length: 9 }, (_, at) => `line ${at + 1}`)
    // Each as its harness writes it, above the last 5 lines.
    const forms = [
      ['$ claude', ['Exit code 1', ...nine].join('\n'), 'Exit code 1'],
      ['$ devin', ['Exit code: 1', ...nine].join('\n'), 'Exit code: 1'],
      ['$ pi', ['Command exited with code 1', ...nine].join('\n'), 'Command exited with code 1'],
      [
        '$ codex',
        ['Chunk ID: 1', 'Wall time: 0.0500 seconds', 'Process exited with code 1', ...nine].join(
          '\n',
        ),
        'Process exited with code 1',
      ],
      ['$ ls no-such-folder', MUSE_FAILED, '"exit_code": 1,'],
    ]
    const data = model()
    data.histories.chief.items = [
      ...forms.map(([header, text], at) =>
        call(1, at, `f-${at}`, header, text, { status: 'failed' }),
      ),
      // The same words where the call went well, or said nothing of how it stands.
      call(1, 5, 'f-ok', '$ muse ok', MUSE_FAILED, { status: 'ok' }),
      call(1, 6, 'f-unknown', '$ muse unknown', MUSE_FAILED, { status: undefined }),
    ]
    await open(page, data)
    const viewer = await openChief(page)
    const callOf = (header) =>
      viewer.locator('.history-item').filter({
        has: page.locator('.history-call', { hasText: new RegExp(`^\\${header}$`) }),
      })
    for (const [header, , exit] of forms) {
      const failed = callOf(header)
      await expect(
        failed.getByRole('button', { name: /earlier lines, click to expand/ }),
      ).toBeVisible()
      await expect(failed.locator('.history-exit')).toHaveText(exit)
      await expect(failed.locator('.history-exit')).toBeVisible()
    }
    // Muse's: the 7 lines above the last 5, its code pinned below them.
    await expect(callOf('$ ls no-such-folder').locator('.history-fold')).toHaveText(
      '… (7 earlier lines, click to expand)',
    )
    const red = await colour(callOf('$ ls no-such-folder'))
    const green = await colour(callOf('$ muse ok'))
    const grey = await colour(callOf('$ muse unknown'))
    expect(new Set([red, green, grey]).size).toBe(3)
    for (const header of ['$ muse ok', '$ muse unknown']) {
      await expect(callOf(header).locator('.history-exit')).toHaveCount(0)
    }
  })

  test('shows what a search matched: a line above the fold, a successful edit’s output, a summary', async ({
    page,
  }) => {
    const data = model()
    data.histories.chief.items.push(
      call(
        2,
        2,
        's-1',
        '$ grep',
        ['NEEDLE in the first line', ...Array.from({ length: 9 }, (_, at) => `line ${at}`)].join(
          '\n',
        ),
        { status: 'ok' },
      ),
      call(2, 3, 's-2', 'edit src/a.ts', 'needle-warning: line endings changed', {
        name: 'Edit',
        status: 'ok',
        diff: '-a\n+b',
      }),
    )
    await open(page, data)
    const viewer = await openChief(page)
    // Drawn as always without a search: folded, and the edit's own words not drawn.
    await expect(viewer.getByText('NEEDLE in the first line')).toBeHidden()
    await expect(viewer.getByText('needle-warning: line endings changed')).toHaveCount(0)
    const find = viewer.getByRole('searchbox', { name: 'Search history' })
    await find.fill('needle')
    await find.press('Enter')
    await expect(viewer.locator('.history-item')).toHaveCount(2)
    const above = viewer.locator('.history-output', { hasText: 'NEEDLE in the first line' })
    await expect(above).toBeVisible()
    expect(await above.evaluate((it) => it.textContent.split('\n')[0])).toBe(
      'NEEDLE in the first line',
    )
    await expect(viewer.getByText('needle-warning: line endings changed')).toBeVisible()
    // A compaction's summary that holds the words is open.
    await find.fill('IS FIXED')
    await find.press('Enter')
    await expect(
      viewer.getByText('Summary: the empty filter is fixed, 12 tests pass.'),
    ).toBeVisible()
    await expect(viewer.getByRole('button', { name: /^Compacted from/ })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  test('a search that stopped short and goes on to the end says what the last read found', async ({
    page,
  }) => {
    const data = model()
    data.partialOnce = true
    await open(page, data)
    const viewer = await openChief(page)
    const find = viewer.getByRole('searchbox', { name: 'Search history' })
    // Stopped short, then nothing more: the notice is the end's.
    await find.fill('zzz')
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText('Nothing found yet that holds "zzz".')
    await earlier(viewer).click()
    await expect(viewer.locator('.history-note')).toHaveText('Nothing in this history holds "zzz".')
    await expect(earlier(viewer)).toHaveCount(0)
    // Stopped short, then found: the items, and no notice.
    await find.fill('npm test')
    await find.press('Enter')
    await expect(viewer.locator('.history-note')).toHaveText(
      'Nothing found yet that holds "npm test".',
    )
    await earlier(viewer).click()
    await expect(viewer.locator('.history-note')).toHaveCount(0)
    expect(await heads(viewer)).toEqual(headsOf('t-2', 't-4'))
  })

  test('reaches every click row and header control by Tab, and acts on Enter and Space', async ({
    page,
  }) => {
    const data = model()
    data.pageSize = 9
    await open(page, data)
    const viewer = await openChief(page)
    await expect(viewer.locator('.history-item')).toHaveCount(9)
    const focused = () =>
      page.evaluate(() => {
        const at = document.activeElement
        return at?.getAttribute('aria-label') || at?.textContent?.trim() || at?.tagName
      })
    /** Presses `key` until what has the keyboard says `name`, and says what it went through. */
    const press = async (key, name) => {
      const through = []
      for (let at = 0; at < 30; at += 1) {
        await page.keyboard.press(key)
        through.push(await focused())
        if (through.at(-1) === name) return through
      }
      throw new Error(`${key} never reached ${name}: ${through.join(' → ')}`)
    }
    // The stream has the keyboard as the viewer opens; Tab goes down its rows in order.
    await expect(viewer.locator('.history-body')).toBeFocused()
    const down = await press('Tab', 'Compacted from 3,903 tokens (click to expand)')
    expect(down).toEqual([
      '… earlier history, click to load',
      '… (33 earlier lines, click to expand)',
      'Compacted from 3,903 tokens (click to expand)',
    ])
    // Space opens the summary; Enter on the fold above unfolds the output.
    await page.keyboard.press(' ')
    await expect(
      viewer.getByText('Summary: the empty filter is fixed, 12 tests pass.'),
    ).toBeVisible()
    await page.keyboard.press('Shift+Tab')
    expect(await focused()).toBe('… (33 earlier lines, click to expand)')
    await page.keyboard.press('Enter')
    await expect(viewer.getByRole('button', { name: /earlier lines/ })).toHaveCount(0)
    // Back up past the stream, the header's controls: Close, Refresh, the search.
    await viewer.locator('.history-body').click({ position: { x: 5, y: 5 } })
    const up = await press('Shift+Tab', 'Search history')
    expect(up.slice(-3)).toEqual(['Close', 'Refresh', 'Search history'])
    // Enter on the earlier row loads it.
    await press('Tab', '… earlier history, click to load')
    await page.keyboard.press('Enter')
    await expect(viewer.locator('.history-item')).toHaveCount(10)
  })

  for (const scheme of ['dark', 'light']) {
    test(`draws every text of the viewer at 4.5:1 or more on its own background, ${scheme}`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme })
      const data = model()
      // Everything but the first prompt, so the earlier row is drawn too.
      data.pageSize = 14
      data.histories.chief.items.push(
        call(2, 2, 'k-1', '$ cargo test', '', { status: 'running' }),
        item(2, 3, 'k-2', 'tool', 'ok 12 passed'),
        item(2, 4, 'k-3', 'custom', 'A hook said this.', { kind: 'notice' }),
        item(2, 5, 'k-4', 'assistant', 'Still going', { kind: 'text', complete: false }),
        call(
          2,
          6,
          'k-5',
          '$ make',
          ['Exit code 2', ...Array.from({ length: 9 }, (_, at) => `line ${at}`)].join('\n'),
          { status: 'failed', tookMs: 10 },
        ),
      )
      await open(page, data)
      const viewer = await openChief(page)
      await viewer.getByRole('button', { name: /^Compacted from/ }).click()
      // What each text is drawn in, against what it stands on: the first background of its own
      // or of what holds it that is not transparent.
      const failures = await viewer.evaluate((dialog) => {
        const linear = (value) => {
          const v = value / 255
          return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
        }
        const luminance = (colour) => {
          const [r, g, b] = colour.match(/[\d.]+/g).map(Number)
          return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        const ground = (element) => {
          for (let at = element; at; at = at.parentElement) {
            const colour = getComputedStyle(at).backgroundColor
            const parts = colour.match(/[\d.]+/g)
            if (parts && (parts.length < 4 || Number(parts[3]) > 0)) return colour
          }
          return 'rgb(255, 255, 255)'
        }
        const TEXTS = [
          ['.history-title'],
          ['.history-icon'],
          ['.history-search input'],
          ['.history-search input', '::placeholder'],
          ['.history-earlier'],
          ['.history-item[data-kind="prompt"] .history-text'],
          ['.history-thinking'],
          ['.history-markdown p'],
          ['.history-markdown .md-heading'],
          ['.history-markdown li code'],
          ['.history-markdown ul > li', '::before'],
          ['.history-markdown pre'],
          ['.history-call .history-verb'],
          ['.history-call .history-path'],
          ['.history-call .history-range'],
          ['.history-item[data-kind="compaction"] .history-call'],
          ['.history-output'],
          ['.history-exit'],
          ['.history-fold'],
          ['.history-expand'],
          ['.history-took'],
          ['.history-quiet'],
          ['.history-diff-row[data-sign="place"]'],
          ['.history-diff-row[data-sign="removed"]'],
          ['.history-diff-row[data-sign="added"]'],
          ['.history-item[data-kind="notice"] .history-text'],
          ['.history-item[data-kind="compaction"] .history-text'],
        ]
        const said = []
        for (const [selector, pseudo] of TEXTS) {
          const found = [...dialog.querySelectorAll(selector)]
          if (found.length === 0) said.push(`${selector}: none drawn`)
          for (const element of found) {
            const text = getComputedStyle(element, pseudo ?? null).color
            const back = ground(element)
            const [light, dark] = [luminance(text), luminance(back)].sort((a, b) => b - a)
            const ratio = (light + 0.05) / (dark + 0.05)
            if (ratio < 4.5) {
              said.push(`${selector}${pseudo ?? ''}: ${ratio.toFixed(2)} (${text} on ${back})`)
            }
          }
        }
        return [...new Set(said)]
      })
      expect(failures).toEqual([])
    })
  }
})
