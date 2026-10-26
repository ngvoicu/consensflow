import { createServer } from 'node:http'

import { expect, test } from '@playwright/test'

import { refusedByPolicy, serveUi } from './serve-ui.mjs'
import { answerFor, specsOf, withFile } from './specs-fixtures.mjs'

/**
 * The specs viewer (core/specs-viewer.js), against a stand-in for the app: the
 * daemon's answers come from an in-page model and every call is recorded, as
 * the Rust app forwards the page's requests. A project's specs are the fixture
 * folders of `fixtures/specs/`, answered as the daemon answers for them
 * (`specs-fixtures.mjs`). The page is served under the app's own
 * Content-Security-Policy: a test that makes it refuse something says so, and
 * every other is held to it refusing nothing. What the system's browser does
 * with an HTML spec is the lead's to check by hand: here, that nothing of one
 * runs in the app, and what the page asks the daemon to open.
 */
let ui
/** A server on this machine that a page asks nothing of: what it receives is what left the page. */
let trap
/** What the app's policy refused in the test running, and whether the test means it to refuse. */
let refused
let refusing

test.setTimeout(30_000)

test.beforeAll(async () => {
  ui = await serveUi()
  trap = await trapServer()
})

test.afterAll(() => Promise.all([ui?.close(), trap?.close()]))

test.beforeEach(({ page }) => {
  refused = refusedByPolicy(page)
  refusing = false
  trap.received.length = 0
})

/**
 * A server on 127.0.0.1, beside the page's own but on a port of its own, so the app's policy
 * holds it to be elsewhere (its `'self'` is the page's origin, port and all): every request it
 * receives is noted, as it arrives, whether or not it is ever answered.
 */
async function trapServer() {
  const received = []
  const server = createServer((request, response) => {
    received.push(request.url)
    response.writeHead(204)
    response.end()
  })
  await new Promise((listening) => server.listen(0, '127.0.0.1', listening))
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    received,
    close: () => new Promise((closed) => server.close(closed)),
  }
}

test.afterEach(() => {
  if (!refusing) expect(refused, "the app's policy refuses nothing the page does").toEqual([])
})

const human = (id, projectId) => ({
  id,
  projectId,
  handle: 'human',
  role: 'human',
  roles: ['human'],
  agent: null,
  harness: null,
  designer: false,
  tier: null,
  outUntil: null,
  memberId: null,
  member: null,
  session: null,
})

/** A project, its board, and its specs folder as the daemon reads it (none: no folder). */
function project(id, name, state, specs, extra = {}) {
  const shown = { id, name, directory: `/work/${name}`, state, ...extra }
  return {
    shown,
    board: {
      project: shown,
      open: [],
      lanes: [
        { participant: human(id * 10, id), tasks: [], activity: { state: 'closed' }, pane: null },
      ],
    },
    specs,
  }
}

/** Every project of the tests: one per fixture, and one with no specs folder. */
function model() {
  const projects = [
    project(1, 'harbour', 'open', specsOf('core')),
    project(2, 'foundry', 'suspended', null),
    project(3, 'checkout', 'open', specsOf('core-html', 'docs/plans'), {
      specs: { on: true, folder: 'docs/plans', variant: 'core-html' },
    }),
    project(4, 'limiter', 'open', specsOf('tdd'), { specs: { on: false } }),
    project(5, 'ledger', 'open', specsOf('tdd-html')),
    project(6, 'hostile', 'open', specsOf('hostile')),
  ]
  return {
    projects: projects.map((made) => made.shown),
    boards: Object.fromEntries(projects.map((made) => [made.shown.id, made.board])),
    specs: Object.fromEntries(projects.map((made) => [made.shown.id, made.specs])),
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
      'board.get': ({ project }) => answer({ board: data.boards[project] }),
      'inbox.get': () => answer({ messages: [], total: 0, shown: 0 }),
      // What the daemon reads of a project's specs folder: the folder the project names, else
      // `.specs`, found empty where it is not there, or refused in the daemon's words.
      'specs.tree': ({ project }) => {
        const specs = data.specs[project]
        if (data.treeRefusal) return { ok: false, error: data.treeRefusal }
        const folder = data.boards[project].project.specs?.folder ?? '.specs'
        if (!specs) return answer({ specs: { folder, found: false, entries: [] } })
        return answer({
          specs: { folder: specs.folder, found: specs.found, entries: specs.entries },
        })
      },
      // A file a test has slow (`slowFiles`, its path and how long) comes after that long.
      'specs.file': ({ project, path }) => {
        const file = data.specs[project]?.files[path]
        const said =
          file === undefined
            ? { ok: false, error: `no file ${path} in the specs folder` }
            : answer({ file })
        const slow = data.slowFiles?.[path]
        return slow ? new Promise((later) => setTimeout(() => later(said), slow)) : said
      },
      // Whether the folder holds a file anywhere: what shows the board's button. Said once from
      // the daemon's memory where a test says when it reads again (`presentAgainIn`), and held
      // until the test lets it go where a test has the folder slow (`presentHeld`).
      'specs.present': ({ project }) => {
        if (data.presentRefusal) return { ok: false, error: data.presentRefusal }
        const holds = (entries) =>
          entries.some((entry) => (entry.entries ? holds(entry.entries) : true))
        const said = { present: holds(data.specs[project]?.entries ?? []) }
        if (data.presentAgainIn) {
          said.readAgainIn = data.presentAgainIn
          data.presentAgainIn = null
        }
        if (!data.presentHeld) return answer(said)
        return new Promise((answered) => {
          window.__held = [...(window.__held ?? []), () => answered(answer(said))]
        })
      },
      // An HTML file of the folder opened in the browser: recorded, as the daemon would start
      // the system's opener; refused in the daemon's words where a test has it refuse, and for
      // any file that is no HTML file of the folder.
      'specs.open': ({ project, path }) => {
        if (data.openRefusal) return { ok: false, error: data.openRefusal }
        if (data.specs[project]?.files[path] === undefined) {
          return { ok: false, error: `no file ${path} in the specs folder` }
        }
        if (!/\.html?$/i.test(path)) {
          const words = `${path} is no HTML file: only an .html or .htm file opens in the browser`
          return { ok: false, error: words }
        }
        window.__opened = [...(window.__opened ?? []), path]
        return answer({ opened: path })
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
const calls = (page, operation) =>
  page.evaluate(
    (operation) =>
      window.__calls
        .filter(([command, args]) => command === 'daemon_request' && args.operation === operation)
        .map(([, args]) => args.body),
    operation,
  )

/** `text` as a pattern that matches it and nothing else. */
const literally = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/**
 * Every request whose address `pattern` matches, counted as the browser starts it: one a policy
 * refused before it left (Chromium fails it with `csp`) is not counted, and every other is, once
 * it has finished or failed, or while it is still on its way, for it left all the same.
 * `left()` says them, each with what came of it, once each has settled or a moment has passed.
 */
function reaching(page, pattern) {
  const started = new Map()
  page.on('request', (request) => {
    if (pattern.test(request.url())) started.set(request, 'on its way')
  })
  page.on('requestfinished', (request) => {
    if (started.has(request)) started.set(request, 'finished')
  })
  page.on('requestfailed', (request) => {
    if (!started.has(request)) return
    const failure = request.failure()?.errorText
    started.set(request, failure === 'csp' ? 'refused' : `failed: ${failure}`)
  })
  return {
    async left() {
      for (let at = 0; at < 40 && [...started.values()].includes('on its way'); at += 1) {
        await new Promise((later) => setTimeout(later, 50))
      }
      return [...started]
        .filter(([, state]) => state !== 'refused')
        .map(([request, state]) => `${request.url()} (${state})`)
    },
  }
}

/** The daemon says something changed: the page reads the board again. */
async function changed(page) {
  await page.evaluate(() => window.__listeners.get('state-changed')({ payload: {} }))
}

/** Chooses project `name` in the list. */
const choose = (page, name) =>
  page.locator('#projects .project-select', { hasText: name }).first().click()

/** Opens the viewer from the board's button, on the project the board shows. */
async function openViewer(page) {
  await page.locator('#specs-button').click()
  const viewer = page.getByRole('dialog', { name: /^Specs of/ })
  await expect(viewer).toBeVisible()
  return viewer
}

/** The viewer's file pane. */
const shown = (viewer) => viewer.getByRole('region', { name: 'The file chosen' })
/** A file of the tree, by its path. */
const fileIn = (viewer, path) => viewer.locator(`.tree-file[data-path="${path}"]`)

/** The labels of a tree level, in order: a file's own, a folder's summary. */
const level = (scope) =>
  scope.evaluate((list) =>
    [...list.children].map((item) => {
      const folder = item.querySelector(':scope > details')
      return folder
        ? `${folder.querySelector('summary').textContent}${folder.open ? '/' : '/…'}`
        : item.textContent
    }),
  )

test.describe('the Specs button', () => {
  test('is hidden with no specs folder, hidden while the folder is empty, and shows at the next read once the first spec is written', async ({
    page,
  }) => {
    const data = model()
    data.specs[1] = null
    await open(page, data)
    const specsButton = page.locator('#specs-button')
    await expect(page.locator('#project-title')).toHaveText('harbour')
    await expect(specsButton).toBeHidden()
    // The folder made, nothing in it yet (a folder only): still no button.
    await page.evaluate(() => {
      window.__model.specs[1] = {
        folder: '.specs',
        found: true,
        entries: [{ name: 'user-auth', path: 'user-auth', entries: [] }],
        files: {},
      }
    })
    await changed(page)
    await expect.poll(async () => (await calls(page, 'specs.present')).length).toBeGreaterThan(1)
    await expect(specsButton).toBeHidden()
    // The chief writes its first spec: the board's next read shows the button, no restart.
    await page.evaluate(
      ({ path, text }) => {
        const specs = window.__model.specs[1]
        specs.entries[0].entries.push({ name: 'SPEC.md', path, size: text.length })
        specs.files[path] = { path, size: text.length, text }
      },
      { path: 'user-auth/SPEC.md', text: '# User Auth\n' },
    )
    await changed(page)
    await expect(specsButton).toBeVisible()
    // Beside Project settings, for the project the board shows: one with no folder hides it.
    await expect(page.locator('.command-bar button')).toHaveText([
      'Inbox',
      'Specs',
      'Project settings',
      'New project',
      '',
    ])
    await choose(page, 'foundry')
    await expect(page.locator('#project-title')).toHaveText('foundry')
    await expect(specsButton).toBeHidden()
    expect(await calls(page, 'specs.present')).toContainEqual({ project: 2 })
    // The board asks whether the folder holds a file, never for the whole tree.
    expect(await calls(page, 'specs.tree')).toEqual([])
  })

  test('shows for a closed project and for one with specs off, and a folder the daemon refuses shows none', async ({
    page,
  }) => {
    const data = model()
    data.boards[1].project.state = 'suspended'
    data.projects[0].state = 'suspended'
    await open(page, data)
    await expect(page.locator('#specs-button')).toBeVisible()
    await choose(page, 'limiter')
    await expect(page.locator('#project-title')).toHaveText('limiter')
    await expect(page.locator('#specs-button')).toBeVisible()
    await page.evaluate(() => {
      window.__model.presentRefusal = 'no project 4'
    })
    await changed(page)
    await expect(page.locator('#specs-button')).toBeHidden()
    await expect(page.locator('#status')).toHaveText('')
  })

  test('never holds up the board: a folder slow to answer shows the button when it does', async ({
    page,
  }) => {
    const data = model()
    data.presentHeld = true
    await open(page, data)
    // The board, its inbox and its windows are drawn while the folder has not answered.
    await expect(page.locator('#project-title')).toHaveText('harbour')
    await expect(page.locator('#inbox-button')).toHaveText('Inbox')
    await expect(page.locator('#specs-button')).toBeHidden()
    expect(await calls(page, 'specs.present')).toContainEqual({ project: 1 })
    // Another project chosen meanwhile: the first one's answer, coming last, is not shown for it.
    await choose(page, 'foundry')
    await expect(page.locator('#project-title')).toHaveText('foundry')
    await page.evaluate(() => {
      window.__model.presentHeld = false
      for (const go of window.__held.splice(0).reverse()) go()
    })
    await page.waitForTimeout(200)
    await expect(page.locator('#specs-button')).toBeHidden()
    await choose(page, 'harbour')
    await expect(page.locator('#specs-button')).toBeVisible()
  })

  test('asks once more when the daemon answered from memory, so a spec written just before shows', async ({
    page,
  }) => {
    const data = model()
    data.specs[1] = null
    // The daemon remembered "no file" a moment ago: it reads the folder again in 400 ms.
    data.presentAgainIn = 400
    await open(page, data)
    const specsButton = page.locator('#specs-button')
    await expect(page.locator('#project-title')).toHaveText('harbour')
    await expect(specsButton).toBeHidden()
    // The chief wrote its first spec just before that answer, and nothing changes after it.
    await page.evaluate(
      ({ path, text }) => {
        window.__model.specs[1] = {
          folder: '.specs',
          found: true,
          entries: [
            {
              name: 'user-auth',
              path: 'user-auth',
              entries: [{ name: 'SPEC.md', path, size: text.length }],
            },
          ],
          files: { [path]: { path, size: text.length, text } },
        }
      },
      { path: 'user-auth/SPEC.md', text: '# User Auth\n' },
    )
    await expect(specsButton).toBeVisible({ timeout: 3_000 })
    // Asked once more, then no more: a fresh answer says nothing of reading again.
    await page.waitForTimeout(700)
    expect(await calls(page, 'specs.present')).toEqual([{ project: 1 }, { project: 1 }])
  })
})

test.describe('Specs… in the project menu', () => {
  test('opens the viewer on its project, shown first, and gives the keyboard back to it', async ({
    page,
  }) => {
    await open(page)
    const card = page.locator('#projects li.project[data-project="3"]')
    await card.click({ button: 'right' })
    const menu = page.getByRole('menu', { name: 'checkout' })
    await expect(menu.getByRole('menuitem')).toHaveText([
      'Rename…',
      'Description…',
      'Project settings…',
      'Specs…',
    ])
    await menu.getByRole('menuitem', { name: 'Specs…' }).click()
    const viewer = page.getByRole('dialog', { name: 'Specs of checkout' })
    await expect(viewer).toBeVisible()
    await expect(page.locator('#project-title')).toHaveText('checkout')
    await expect(shown(viewer).locator('.registry-row[data-spec="checkout-flow"]')).toBeVisible()
    await viewer.getByRole('button', { name: 'Close' }).click()
    await expect(viewer).toBeHidden()
    await expect(card.locator('.project-select')).toBeFocused()
  })

  test('opens on a project with no specs folder, which the viewer says', async ({ page }) => {
    await open(page)
    await page.locator('#projects li.project[data-project="2"]').click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Specs…' }).click()
    const viewer = page.getByRole('dialog', { name: 'Specs of foundry' })
    await expect(shown(viewer)).toHaveText(
      "No specs folder yet: .specs is not in the project's folder. The chief makes it when it writes its first spec.",
    )
    await expect(viewer.locator('.tree-file')).toHaveCount(0)
  })
})

test.describe('the specs tree', () => {
  test('lists the registry, the specs by the registry, the rest by name and assets last, folded', async ({
    page,
  }) => {
    const data = model()
    // A stray dotfile at each level: whatever answered, the viewer leaves it out.
    withFile(data.specs[1], '.DS_Store', Buffer.from([0, 1]))
    withFile(data.specs[1], 'user-auth/.DS_Store', Buffer.from([0, 1]))
    await open(page, data)
    const viewer = await openViewer(page)
    const tree = viewer.getByRole('navigation', { name: 'The specs folder' })
    await expect(viewer.locator('.specs-folder')).toHaveText('.specs')
    expect(await level(tree.locator('> ul'))).toEqual([
      'registry.md',
      'User Auth System/',
      'API Refactoring/',
      'Fix File Upload Bug/',
      'Legacy Cleanup/',
      // Not in the registry: by its id.
      'unlisted-spike/',
      'notes.txt',
    ])
    // Inside a spec: the spec, the research and the interviews by number, the rest by name.
    const auth = tree.locator('details[data-path="user-auth"] > ul')
    expect(await level(auth)).toEqual([
      'SPEC.md',
      'research-01.md',
      'research-02.md',
      'research-9.md',
      'research-10.md',
      'interview-01.md',
      'interview-02.md',
      'artifacts/…',
      'flow.png',
      'logo.svg',
      'page-audit.md',
      'SPEC.md.sha256',
      'strategy-api-v1.md',
    ])
    // A subfolder opens on a click, and stays as it was left when the tree is read again.
    await tree.locator('details[data-path="user-auth/artifacts"] > summary').click()
    expect(await level(tree.locator('details[data-path="user-auth/artifacts"] > ul'))).toEqual([
      'capture.bin',
      'lockouts.json',
      'notes.md',
    ])
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(tree.locator('details[data-path="user-auth/artifacts"]')).toHaveAttribute(
      'open',
      '',
    )
    // An HTML variant's folder: assets last, folded.
    await viewer.getByRole('button', { name: 'Close' }).click()
    await choose(page, 'checkout')
    const html = await openViewer(page)
    expect(await level(html.locator('.specs-tree > ul'))).toEqual([
      'registry.md',
      'Checkout Flow/',
      'assets/…',
    ])
    expect(await level(html.locator('details[data-path="checkout-flow"] > ul'))).toEqual([
      'SPEC.html',
      'research-01.md',
      'interview-01.md',
      'mockup-cart.html',
    ])
  })

  test('opens on the first spec where there is no registry', async ({ page }) => {
    const data = model()
    const specs = data.specs[1]
    specs.entries = specs.entries.filter((entry) => entry.name !== 'registry.md')
    await open(page, data)
    const viewer = await openViewer(page)
    // By name, with no registry to order them: api-refactor first.
    await expect(fileIn(viewer, 'api-refactor/SPEC.md')).toHaveAttribute('aria-current', 'true')
    await expect(shown(viewer).locator('h1')).toHaveText('API Refactoring')
  })
})

test.describe('the registry', () => {
  test('shows each spec as a row with its status chip, priority, progress bar and date; a row opens its spec', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openViewer(page)
    await expect(fileIn(viewer, 'registry.md')).toHaveAttribute('aria-current', 'true')
    const view = shown(viewer)
    await expect(view.locator('.registry-heading')).toHaveText('Spec Registry')
    const rows = view.getByRole('list', { name: 'Specs' }).locator('.registry-row')
    await expect(rows.locator('.registry-title strong')).toHaveText([
      'User Auth System',
      'API Refactoring',
      'Fix File Upload Bug',
      'Legacy Cleanup',
    ])
    await expect(rows.locator('.status-chip')).toHaveText([
      'active',
      'paused',
      'completed',
      'archived',
    ])
    expect(
      await rows
        .locator('.status-chip')
        .evaluateAll((chips) => chips.map((chip) => chip.dataset.status)),
    ).toEqual(['active', 'paused', 'completed', 'archived'])
    // Each status its own colour.
    const colours = await rows
      .locator('.status-chip')
      .evaluateAll((chips) => chips.map((chip) => getComputedStyle(chip).color))
    expect(new Set(colours).size).toBe(4)
    await expect(rows.locator('.registry-priority')).toHaveText(['high', 'medium', 'low', 'low'])
    await expect(rows.locator('.progress-words')).toHaveText(['5/12', '2/8', '5/5', '3/3'])
    await expect(rows.locator('.registry-updated')).toHaveText([
      '2026-10-14',
      '2026-10-09',
      '2026-09-30',
      '2026-08-21',
    ])
    const share = (at) =>
      rows
        .nth(at)
        .locator('.progress-fill')
        .evaluate(
          (fill) =>
            fill.getBoundingClientRect().width / fill.parentElement.getBoundingClientRect().width,
        )
    expect(await share(0)).toBeCloseTo(5 / 12, 1)
    expect(await share(2)).toBeCloseTo(1, 2)
    await expect(rows.first().getByRole('progressbar')).toHaveAttribute('aria-valuenow', '5')
    await rows.nth(1).click()
    await expect(fileIn(viewer, 'api-refactor/SPEC.md')).toHaveAttribute('aria-current', 'true')
    await expect(view.locator('h1')).toHaveText('API Refactoring')
  })

  test('that is not Specmint table shows as markdown', async ({ page }) => {
    const data = model()
    data.specs[1].files['registry.md'] = answerFor(
      'registry.md',
      Buffer.from('# Our specs\n\n| Spec | State |\n|---|---|\n| user-auth | going |\n'),
    )
    await open(page, data)
    const viewer = await openViewer(page)
    await expect(shown(viewer).locator('.registry-row')).toHaveCount(0)
    await expect(shown(viewer).locator('h1')).toHaveText('Our specs')
    await expect(shown(viewer).locator('table th')).toHaveText(['Spec', 'State'])
  })

  test('of thousands of specs draws its first 500 rows and says how many it lists', async ({
    page,
  }) => {
    const data = model()
    // Within the 512 KB the daemon shows of a file.
    const rows = Array.from({ length: 15_000 }, (_, at) => `|s${at}|S|active|low|1/2|2026|`)
    const text = [
      '# Spec Registry',
      '',
      '| ID | Title | Status | Priority | Progress | Updated |',
      '|---|---|---|---|---|---|',
      ...rows,
    ].join('\n')
    data.specs[1].files['registry.md'] = answerFor('registry.md', Buffer.from(text))
    await open(page, data)
    const viewer = await openViewer(page)
    await expect(shown(viewer).locator('button.registry-row')).toHaveCount(500)
    await expect(shown(viewer).locator('.registry-more')).toHaveText(
      "The registry lists 15000 specs: the first 500 are shown here. Every spec's folder is in the tree.",
    )
  })

  test('built to make a parser slow is read in a moment, as the registry or as markdown', async ({
    page,
  }) => {
    await open(page)
    // Read in a moment, and read at all: a reader that throws (its stack past what a page
    // holds) fails the assertion, as one that takes too long does.
    await expect(
      page.evaluate(async () => {
        const { readRegistry } = await import(new URL('core/specs-folder.js', location.href).href)
        const { render } = await import(new URL('core/markdown.js', location.href).href)
        const head = '| ID | Title | Status | Priority | Progress | Updated |'
        // A cell of escaped pipes, within the 512 KB the daemon shows: once read again for each.
        const pipes = '\\|'.repeat(250_000)
        const texts = [
          `${head}\n|---|---|---|---|---|---|\n| a | ${pipes} | active | high | 1/2 | 2026 |`,
          `| A | B |\n|---|---|\n| ${pipes} | x |`,
          `${head}\n${' '.repeat(131_072)}X`,
          `${head}\n|${' -'.repeat(65_536)} X`,
          // Lists and quotes nested past what a page's stack holds, read as words past a depth.
          `${'- '.repeat(65_536)}X`,
          `${'> '.repeat(65_536)}X`,
          `${'*'.repeat(131_072)}X`,
          `${'['.repeat(65_536)}](${'('.repeat(65_536)}`,
        ]
        const started = performance.now()
        for (const text of texts) {
          readRegistry(text)
          render(text)
        }
        return performance.now() - started
      }),
    ).resolves.toBeLessThan(2_000)
  })

  test('keeps what its cells say as text, and a row with no folder opens nothing', async ({
    page,
  }) => {
    await open(page)
    await choose(page, 'hostile')
    await expect(page.locator('#project-title')).toHaveText('hostile')
    const viewer = await openViewer(page)
    const rows = shown(viewer).locator('button.registry-row')
    await expect(rows.locator('.registry-title strong').first()).toHaveText(
      `<img src=x onerror="window.__pwned='registry-title'"> Evil Markdown`,
    )
    await expect(rows.nth(1).locator('.status-chip')).toHaveText('<b>paused</b>')
    await expect(rows.nth(1).locator('.status-chip')).toHaveAttribute('data-status', 'other')
    // A progress that is no count is its words, with no bar.
    await expect(rows.nth(1).locator('.progress-words')).toHaveText('9/x')
    await expect(rows.nth(1).getByRole('progressbar')).toHaveCount(0)
    await expect(shown(viewer).locator('.registry-heading')).toHaveText(
      "Spec Registry <script>window.__pwned = 'registry-heading'</script>",
    )
    await expect(viewer.locator('.specs-file script, .specs-file img, .specs-file a')).toHaveCount(
      0,
    )
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  })
})

test.describe('a Markdown spec', () => {
  test('shows its frontmatter, headings, task boxes, nested lists, tables and code', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/SPEC.md').click()
    const view = shown(viewer)
    await expect(view.locator('.specs-caption')).toHaveText('user-auth/SPEC.md · 3.4 KB')
    await expect(view.locator('.spec-fields dt')).toHaveText([
      'id',
      'title',
      'status',
      'created',
      'updated',
      'priority',
      'tags',
    ])
    await expect(view.locator('.spec-fields div').nth(2)).toHaveText('statusactive')
    await expect(view.locator('h1')).toHaveText('User Auth System')
    await expect(view.locator('h2').first()).toHaveText('Overview')
    await expect(view.locator('h3').first()).toHaveText('System Diagram')
    const acceptance = view.locator('ul').nth(0)
    await expect(acceptance.locator('> li.md-task')).toHaveCount(4)
    expect(
      await acceptance
        .locator('> li > input')
        .evaluateAll((boxes) => boxes.map((box) => [box.checked, box.disabled])),
    ).toEqual([
      [true, true],
      [true, true],
      [false, true],
      [false, true],
    ])
    await expect(acceptance.locator('> li').nth(0)).toHaveText(
      'POST /auth/login returns 200 and a token pair for a valid user',
    )
    // A task nests its own tasks, and an ordered list under them.
    const lockout = view.locator('li.md-task', { hasText: '[AUTH-06]' }).first()
    await expect(lockout.locator('> ul > li.md-task')).toHaveCount(2)
    await expect(lockout.locator('ol > li')).toHaveText([
      "store the lock's end",
      'check it on each attempt',
    ])
    await expect(view.locator('table').first().locator('th')).toHaveText([
      'Need',
      'Library',
      'Version',
      'Alternatives Considered',
      'Rationale',
    ])
    await expect(view.locator('pre code').first()).toContainText('Client → API Gateway')
    await expect(view.locator('hr')).toHaveCount(1)
    await expect(view.locator('blockquote')).toContainText('Lockout counting works')
  })

  test('opens a link to another file of the folder in the viewer; any other link opens nothing', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/SPEC.md').click()
    const view = shown(viewer)
    // Out of the folder, or anywhere else: words, where they lead on hover.
    await expect(view.locator('span[title="https://example.com/auth"]')).toHaveText(
      'a page outside',
    )
    await expect(view.locator('span[title="../../package.json"]')).toHaveText(
      'a file outside the folder',
    )
    await expect(view.locator('a')).toHaveCount(0)
    await view.getByRole('button', { name: 'research-01' }).click()
    await expect(fileIn(viewer, 'user-auth/research-01.md')).toHaveAttribute('aria-current', 'true')
    await expect(view.locator('h1')).toHaveText('Research 01: Password hashing')
    await view.getByRole('button', { name: 'the spec' }).click()
    await view.getByRole('button', { name: 'API refactor' }).click()
    await expect(view.locator('h1')).toHaveText('API Refactoring')
    await view.getByRole('button', { name: 'user-auth' }).click()
    // A link with a fragment opens its file; one into a folded folder unfolds it.
    await view.getByRole('button', { name: 'page audit' }).click()
    await expect(view.locator('h1')).toHaveText('Page audit: the login page')
    await fileIn(viewer, 'user-auth/SPEC.md').click()
    await view.getByRole('button', { name: 'the notes' }).click()
    await expect(fileIn(viewer, 'user-auth/artifacts/notes.md')).toBeVisible()
    await expect(fileIn(viewer, 'user-auth/artifacts/notes.md')).toHaveAttribute(
      'aria-current',
      'true',
    )
    await view.getByRole('button', { name: 'the spec' }).click()
    await expect(fileIn(viewer, 'user-auth/SPEC.md')).toHaveAttribute('aria-current', 'true')
    expect(page.url()).toBe(`${ui.origin}/index.html`)
  })

  test('draws its Mermaid diagram, and a broken one shows its source and Mermaid words', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/SPEC.md').click()
    const diagrams = shown(viewer).locator('figure.spec-diagram')
    await expect(diagrams).toHaveCount(2)
    await expect(diagrams.first()).toHaveAttribute('data-state', 'drawn', { timeout: 15_000 })
    await expect(diagrams.first().locator('svg')).toBeVisible()
    await expect(diagrams.first().locator('svg')).toContainText('API Gateway')
    await expect(diagrams.first().locator('svg')).toContainText('401 Response')
    await expect(diagrams.nth(1)).toHaveAttribute('data-state', 'failed')
    await expect(diagrams.nth(1).locator('.spec-diagram-error')).toContainText(
      'This diagram could not be drawn.',
    )
    await expect(diagrams.nth(1).locator('.spec-diagram-error')).toContainText(
      /Parse error|Expecting|error/i,
    )
    await expect(diagrams.nth(1).locator('pre code')).toHaveText(
      'graph TD\n    A[Client] -->|no closing --> B{{Gateway',
    )
    // Nothing Mermaid made to measure its drawings is left in the page.
    await expect(page.locator('body > [id^="dspec-diagram"], body > svg')).toHaveCount(0)
  })

  test('of many diagrams draws its first 50, shows the rest as their source, and stops drawing once left', async ({
    page,
  }) => {
    const data = model()
    // Each a chain of twenty steps, so Mermaid takes a moment over each.
    const chain = (at) =>
      Array.from(
        { length: 20 },
        (_, step) => `  S${at}x${step}[Step ${step}] --> S${at}x${step + 1}`,
      )
    const fences = Array.from(
      { length: 60 },
      (_, at) =>
        `\`\`\`mermaid\ngraph TD\n  A${at}[Step ${at}] --> B${at}[Next ${at}]\n${chain(at).join('\n')}\n\`\`\``,
    )
    const text = `# Many diagrams\n\n${fences.join('\n\n')}\n`
    withFile(data.specs[1], 'user-auth/diagrams.md', Buffer.from(text))
    await open(page, data)
    // Each drawing Mermaid lays out in the page, as it starts it.
    await page.evaluate(() => {
      window.__drawings = new Set()
      new MutationObserver((records) => {
        for (const record of records) {
          for (const node of record.addedNodes) {
            if (node.id?.startsWith('dspec-diagram-')) window.__drawings.add(node.id)
          }
        }
      }).observe(document.body, { childList: true, subtree: true })
    })
    const drawings = () => page.evaluate(() => window.__drawings.size)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/diagrams.md').click()
    const figures = shown(viewer).locator('figure.spec-diagram')
    await expect(figures).toHaveCount(60)
    await expect(shown(viewer).locator('figure[data-state="drawn"]')).toHaveCount(50, {
      timeout: 20_000,
    })
    await expect(shown(viewer).locator('figure[data-state="undrawn"]')).toHaveCount(10)
    await expect(figures.nth(50).locator('.spec-diagram-note')).toHaveText(
      'Not drawn: a document draws its first 50 diagrams.',
    )
    await expect(figures.nth(59).locator('pre code')).toHaveText(
      `graph TD\n  A59[Step 59] --> B59[Next 59]\n${chain(59).join('\n')}`,
    )
    expect(await drawings()).toBe(50)
    // Left the moment it shows, its diagrams waiting their turn: none of them is drawn after.
    for (const leave of ['another file', 'the viewer shut']) {
      const before = await drawings()
      await page.evaluate((leave) => {
        const dialog = document.querySelector('#specs-viewer-dialog')
        const view = dialog.querySelector('.specs-file')
        const file = (path) => dialog.querySelector(`.tree-file[data-path="${path}"]`)
        new MutationObserver((_, watch) => {
          if (view.querySelectorAll('figure.spec-diagram').length < 60) return
          watch.disconnect()
          if (leave === 'another file') file('user-auth/research-01.md').click()
          else dialog.close()
        }).observe(view, { childList: true, subtree: true })
        file('user-auth/diagrams.md').click()
      }, leave)
      await expect(shown(viewer).locator('figure.spec-diagram')).toHaveCount(0)
      const atLeaving = await drawings()
      await page.waitForTimeout(1_500)
      // The one being drawn as it was left may finish; none waiting its turn starts.
      expect((await drawings()) - before, leave).toBeLessThanOrEqual(2)
      expect(atLeaving - before).toBeLessThanOrEqual(2)
    }
  })

  test('chosen and left before it came is never drawn', async ({ page }) => {
    const data = model()
    withFile(
      data.specs[1],
      'user-auth/late.md',
      Buffer.from('# Late\n\n```mermaid\ngraph TD\n  A --> B\n```\n'),
    )
    data.slowFiles = { 'user-auth/late.md': 600 }
    await open(page, data)
    await page.evaluate(() => {
      window.__drawings = 0
      new MutationObserver((records) => {
        for (const record of records) {
          for (const node of record.addedNodes) {
            if (node.id?.startsWith('dspec-diagram-')) window.__drawings += 1
          }
        }
      }).observe(document.body, { childList: true, subtree: true })
    })
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/late.md').click()
    await fileIn(viewer, 'user-auth/research-01.md').click()
    await expect(shown(viewer).locator('.specs-path')).toHaveText('user-auth/research-01.md')
    await page.waitForTimeout(2_000)
    await expect(shown(viewer).locator('.specs-path')).toHaveText('user-auth/research-01.md')
    expect(await page.evaluate(() => window.__drawings)).toBe(0)
  })

  test('of the TDD variant draws its sequence diagram and its test log', async ({ page }) => {
    await open(page)
    await choose(page, 'limiter')
    await expect(page.locator('#project-title')).toHaveText('limiter')
    const viewer = await openViewer(page)
    await shown(viewer).locator('.registry-row[data-spec="rate-limiter"]').click()
    const view = shown(viewer)
    await expect(view.locator('h1')).toHaveText('Rate Limiter')
    await expect(view.locator('figure.spec-diagram')).toHaveAttribute('data-state', 'drawn', {
      timeout: 15_000,
    })
    await expect(view.locator('figure.spec-diagram svg')).toContainText('Limiter')
    await expect(view.locator('li.md-task', { hasText: '[TEST-RL-03] Write' })).toContainText(
      '<- current',
    )
    await expect(view.locator('table').last().locator('th')).toHaveText([
      'Task',
      'RED',
      'GREEN',
      'REFACTOR',
    ])
    expect(await level(viewer.locator('details[data-path="rate-limiter"] > ul'))).toEqual([
      'SPEC.md',
      'research-01.md',
      'interview-01.md',
    ])
  })
})

test.describe('other files', () => {
  test('show text plain in monospace, images as images, and the rest as words with their size', async ({
    page,
  }) => {
    const data = model()
    withFile(data.specs[1], 'user-auth/huge.log', Buffer.alloc(3_400_000, 'x'))
    await open(page, data)
    const viewer = await openViewer(page)
    const view = shown(viewer)
    for (const [path, text] of [
      [
        'user-auth/SPEC.md.sha256',
        '3f0a6c3e5b8f2d1e9c4b7a6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f  SPEC.md\n',
      ],
      ['notes.txt', 'Loose notes at the top of the specs folder, outside any spec.\n'],
    ]) {
      await fileIn(viewer, path).click()
      await expect(view.locator('pre.specs-plain code')).toHaveText(text)
      expect(
        await view.locator('pre.specs-plain').evaluate((pre) => getComputedStyle(pre).fontFamily),
      ).toMatch(/mono|Menlo|Consolas/i)
    }
    await viewer.locator('details[data-path="user-auth/artifacts"] > summary').click()
    await fileIn(viewer, 'user-auth/artifacts/lockouts.json').click()
    await expect(view.locator('pre.specs-plain code')).toContainText('"table": "lockouts"')
    // An image as an image; an SVG's script never runs, for an image runs none.
    await fileIn(viewer, 'user-auth/flow.png').click()
    const image = view.locator('img.specs-image')
    await expect(image).toHaveAttribute('src', /^data:image\/png;base64,iVBOR/)
    await expect.poll(() => image.evaluate((img) => img.naturalWidth)).toBe(24)
    await fileIn(viewer, 'user-auth/logo.svg').click()
    await expect(image).toHaveAttribute('src', /^data:image\/svg\+xml;base64,/)
    await expect.poll(() => image.evaluate((img) => img.naturalWidth)).toBe(48)
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
    await expect(view.locator('svg, script')).toHaveCount(0)
    // Neither text nor an image, or too large: says so, with its size.
    await fileIn(viewer, 'user-auth/artifacts/capture.bin').click()
    await expect(view.locator('.specs-withheld')).toHaveText(
      'user-auth/artifacts/capture.bin is 12 bytes that is neither text nor an image',
    )
    await fileIn(viewer, 'user-auth/huge.log').click()
    await expect(view.locator('.specs-withheld')).toHaveText(
      'user-auth/huge.log is 3.2 MB, more than the 512.0 KB the viewer shows',
    )
    await expect(view.locator('.specs-caption')).toHaveText('user-auth/huge.log · 3.2 MB')
  })

  test('a file the daemon refuses says why in the viewer', async ({ page }) => {
    const data = model()
    delete data.specs[1].files['notes.txt']
    await open(page, data)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'notes.txt').click()
    await expect(shown(viewer).locator('.specs-refusal')).toHaveText(
      'no file notes.txt in the specs folder',
    )
  })
})

/** The card an HTML file shows as, by its file name. */
const card = (viewer, name) => shown(viewer).getByRole('region', { name })

test.describe('an HTML spec', () => {
  test('shows as a card of its title, name and size, and opens in the browser from it', async ({
    page,
  }) => {
    await open(page)
    await choose(page, 'checkout')
    await expect(page.locator('#project-title')).toHaveText('checkout')
    const viewer = await openViewer(page)
    // The folder the project names, not `.specs`; and the viewer says where HTML opens.
    await expect(viewer.locator('.specs-folder')).toHaveText('docs/plans')
    await expect(viewer.locator('.specs-hint')).toHaveText(
      'HTML specs and mockups open in your browser, outside ConsensFlow.',
    )
    await fileIn(viewer, 'checkout-flow/SPEC.html').click()
    const spec = card(viewer, 'SPEC.html')
    await expect(spec.locator('h1')).toHaveText('Checkout Flow — SPEC')
    await expect(spec.locator('.spec-html-meta')).toHaveText('SPEC.html · HTML · 6.8 KB')
    await expect(spec.locator('.spec-html-help')).toHaveText(
      'It opens in your browser, outside ConsensFlow: a copy of it, with the assets it names.',
    )
    // Nothing of it runs here: no frame, and nothing of what it holds is in the page.
    await expect(viewer.locator('iframe, script, link, style')).toHaveCount(0)
    await expect(spec.locator('.scorecard, .spec-header')).toHaveCount(0)
    await spec.getByRole('button', { name: 'Open in your browser' }).click()
    await expect(spec.getByRole('status')).toHaveText(
      'Opened checkout-flow/SPEC.html in your browser.',
    )
    expect(await calls(page, 'specs.open')).toEqual([
      { project: 3, path: 'checkout-flow/SPEC.html' },
    ])
    // Its assets are the browser's to load from the folder: the viewer reads none of them.
    expect(
      (await calls(page, 'specs.file')).filter((asked) => asked.path.startsWith('assets/')),
    ).toEqual([])
  })

  test('of the TDD variant, and a mockup, show as cards too', async ({ page }) => {
    await open(page)
    await choose(page, 'ledger')
    await expect(page.locator('#project-title')).toHaveText('ledger')
    const viewer = await openViewer(page)
    await shown(viewer).locator('.registry-row[data-spec="ledger-sync"]').click()
    await expect(card(viewer, 'SPEC.html').locator('h1')).toHaveText('Ledger Sync — SPEC')
    await viewer.getByRole('button', { name: 'Close', exact: true }).click()
    await choose(page, 'checkout')
    await expect(page.locator('#project-title')).toHaveText('checkout')
    const checkout = await openViewer(page)
    await fileIn(checkout, 'checkout-flow/mockup-cart.html').click()
    const mockup = card(checkout, 'mockup-cart.html')
    await expect(mockup.locator('h1')).toHaveText('Cart — mockup')
    await mockup.getByRole('button', { name: 'Open in your browser' }).click()
    await expect(mockup.getByRole('status')).toHaveText(
      'Opened checkout-flow/mockup-cart.html in your browser.',
    )
  })

  test('that the daemon will not open says why on its card', async ({ page }) => {
    const data = model()
    data.openRefusal =
      'checkout-flow/SPEC.html has 2 names (hard links): the viewer reads only a file of one name, which no path outside the folder can share'
    await open(page, data)
    await choose(page, 'checkout')
    await expect(page.locator('#project-title')).toHaveText('checkout')
    const viewer = await openViewer(page)
    await fileIn(viewer, 'checkout-flow/SPEC.html').click()
    const spec = card(viewer, 'SPEC.html')
    await spec.getByRole('button', { name: 'Open in your browser' }).click()
    await expect(spec.getByRole('status')).toHaveText(data.openRefusal)
    await expect(spec.getByRole('status')).toHaveAttribute('data-tone', 'error')
    await expect(page.evaluate(() => window.__opened)).resolves.toBeUndefined()
  })

  test('with no title is named by its file', async ({ page }) => {
    const data = model()
    withFile(data.specs[3], 'checkout-flow/mockup-bare.html', Buffer.from('<p>bare</p>\n'))
    await open(page, data)
    await choose(page, 'checkout')
    await expect(page.locator('#project-title')).toHaveText('checkout')
    const viewer = await openViewer(page)
    await viewer.locator('details[data-path="checkout-flow"] > summary').isVisible()
    await fileIn(viewer, 'checkout-flow/mockup-bare.html').click()
    await expect(card(viewer, 'mockup-bare.html').locator('h1')).toHaveText('mockup-bare.html')
  })
})

test.describe('a hostile spec', () => {
  test('in Markdown stays words: no script, no handler, no address followed, nothing fetched', async ({
    page,
  }) => {
    const dialogs = []
    page.on('dialog', (dialog) => {
      dialogs.push(dialog.message())
      void dialog.dismiss()
    })
    const evil = reaching(page, /evil\.example/)
    await open(page)
    await choose(page, 'hostile')
    await expect(page.locator('#project-title')).toHaveText('hostile')
    const viewer = await openViewer(page)
    await shown(viewer).locator('.registry-row[data-spec="evil"]').click()
    const view = shown(viewer)
    await expect(view.locator('h1')).toHaveText("Evil <script>window.__pwned = 'heading'</script>")
    await expect(view.locator('p').filter({ hasText: '<script>' }).first()).toContainText(
      "<script>window.__pwned = 'script'</script>",
    )
    await expect(view.locator('.spec-fields dd').nth(1)).toHaveText(
      `<img src=x onerror="window.__pwned='frontmatter'">`,
    )
    for (const href of [
      "javascript:window.__pwned='link'",
      'data:text/html,<script>alert(1)</script>',
      '../../../etc/passwd',
      '/etc/passwd',
      '//evil.example/x',
      'https://evil.example/pixel.png',
      "javascript:window.__pwned='cell-link'",
    ]) {
      await expect(view.locator(`span[title="${href}"]`)).toHaveCount(1)
    }
    await expect(view.locator('.md-link')).toHaveCount(0)
    const diagrams = view.locator('figure.spec-diagram')
    await expect(diagrams).toHaveCount(3)
    await expect(diagrams.nth(0)).toHaveAttribute('data-state', /drawn|failed/, { timeout: 15_000 })
    await expect(diagrams.nth(1)).toHaveAttribute('data-state', /drawn|failed/, { timeout: 15_000 })
    // A diagram that asks for its own theme, colours and CSS is drawn in the page's all the same.
    await expect(diagrams.nth(2)).toHaveAttribute('data-state', 'drawn', { timeout: 15_000 })
    const raised = await page.evaluate(() => {
      const probe = document.createElement('span')
      probe.style.color = 'var(--panel-raised)'
      document.body.append(probe)
      const said = getComputedStyle(probe).color
      probe.remove()
      return said
    })
    const fills = await diagrams
      .nth(2)
      .locator('g.node rect, g.node polygon')
      .evaluateAll((shapes) => shapes.map((shape) => getComputedStyle(shape).fill))
    expect(fills.length).toBeGreaterThan(1)
    expect(new Set(fills)).toEqual(new Set([raised]))
    // Nothing that runs, loads or takes input, and no handler, anywhere in what was drawn.
    await expect(
      view.locator(
        'script, img, iframe, object, embed, a, image, input:not([type="checkbox"]), video, audio',
      ),
    ).toHaveCount(0)
    expect(
      await view.evaluate((root) =>
        [...root.querySelectorAll('*')].flatMap((node) =>
          [...node.attributes]
            .filter(
              ({ name, value }) =>
                name.startsWith('on') ||
                (/href|src/.test(name) &&
                  !value.startsWith('#') &&
                  !value.startsWith('data:image')),
            )
            .map(({ name }) => `${node.localName} ${name}`),
        ),
      ),
    ).toEqual([])
    // Click everything a diagram drew: nothing runs.
    for (const node of await diagrams.locator('svg g.node').all()) await node.click({ force: true })
    await page.waitForTimeout(300)
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
    expect(dialogs).toEqual([])
    expect(await evil.left()).toEqual([])
    expect(page.url()).toBe(`${ui.origin}/index.html`)
  })

  test('diagram makes no request leave the page while Mermaid lays it out, whatever it asks', async ({
    page,
  }) => {
    refusing = true
    // Every address a diagram names is the trap's: what reaches it left the page.
    const host = trap.origin.replace('http://', '')
    const at = (name) => `${trap.origin}/${name}`
    const evil = reaching(page, new RegExp(literally(host)))
    await open(page)
    // What the label sanitizer, the secured settings and the node-image rewrite stop before the
    // page ever asks: the policy is never even reached by these.
    const stoppedFirst = [
      `graph TD\n  A["<img src=${at('label.png')}>"] --> B`,
      `graph TD\n  A["<span style='background:url(${at('span.png')})'>x</span>"] --> B`,
      `%%{init: {"htmlLabels": true, "flowchart": {"htmlLabels": true}}}%%\ngraph TD\n  A["<img src=${at('html.png')}>"] --> B`,
      `%%{init: {"dompurifyConfig": {"FORBID_TAGS": [], "FORBID_ATTR": []}}}%%\ngraph TD\n  A["<img src=${at('none.png')}>"] --> B`,
      `---\nconfig:\n  dompurifyConfig:\n    FORBID_TAGS: []\n---\ngraph TD\n  A["<img src=${at('front.png')}>"] --> B`,
      `%%{init: {"themeCSS": ".label { background: url(${at('theme.png')}) }"}}%%\ngraph TD\n  A --> B`,
      `classDiagram\n  class A["<img src=${at('class.png')}>"]`,
      `sequenceDiagram\n  Note over A: <img src=${at('note.png')}>`,
      `flowchart TD\n  A@{ img: "${at('node.png')}", label: "x", pos: "t", h: 60, constraint: "on" }`,
      `flowchart TD\n  A@{ img: '${at('node2.png')}', label: "x" }`,
      `flowchart TD\n  A@{ "img": "${at('quoted.png')}", label: "x", h: 60 }`,
      // Entities Mermaid decodes, and one the page would: each stays text, and names nothing.
      `graph TD\n  A["#60;img src=${at('entity-label.png')}#62;"] --> B`,
      `stateDiagram-v2\n  [*] --> A\n  classDef leak background-image:url(http#58;//${host}/entity.png)\n  class A leak`,
      `stateDiagram-v2\n  [*] --> A\n  classDef leak background-image:url(http&#58;//${host}/html-entity.png)\n  class A leak`,
    ]
    // What only the page's policy stops: CSS a diagram writes into the live page (`classDef`,
    // plain or escaped), and an image whose key the rewrite does not see (a brace in a string).
    const policyOnly = [
      `stateDiagram-v2\n  [*] --> A\n  classDef leak background-image:url(${at('state.png')})\n  class A leak`,
      `stateDiagram-v2\n  [*] --> A\n  classDef leak background-image:u\\72l(${at('escaped.png')})\n  class A leak`,
      `block-beta\n  a\n  classDef leak fill:url(${at('block.svg')}#x)\n  class a leak`,
      `flowchart TD\n  A@{ label: "}", img: "${at('brace.png')}", h: 60 }`,
    ]
    const asks = [...stoppedFirst, ...policyOnly]
    await page.evaluate(async (asks) => {
      // The page's own module, by the address the page has it at.
      const { diagram } = await import(new URL('core/diagrams.js', location.href).href)
      const figures = asks.map((code) => {
        const figure = diagram(code)
        document.body.append(figure)
        return figure
      })
      for (
        let at = 0;
        at < 300 && figures.some((one) => one.dataset.state === 'drawing');
        at += 1
      ) {
        await new Promise((later) => setTimeout(later, 50))
      }
      return figures.map((figure) => figure.dataset.state)
    }, asks)
    await page.waitForTimeout(300)
    // Nothing left the page, by any way: none started but those the policy refused, and the
    // trap received nothing.
    expect(await evil.left()).toEqual([])
    expect(trap.received).toEqual([])
    // The policy refused what only it stops, and was never reached by the rest.
    for (const name of ['state.png', 'escaped.png', 'block.svg', 'brace.png']) {
      expect(
        refused.some((said) => said.includes(at(name))),
        `the policy refused ${name}`,
      ).toBe(true)
    }
    expect(
      refused.filter((said) =>
        ['label', 'span', 'html', 'none', 'front', 'theme', 'class', 'note', 'node', 'quoted']
          .concat(['entity'])
          .some((name) => said.includes(`${trap.origin}/${name}`)),
      ),
    ).toEqual([])
  })

  test('drawing is checked before it joins the page: nothing in it runs, loads or leads away', async ({
    page,
  }) => {
    await open(page)
    // What Mermaid at a level less strict, or a Mermaid with a bug, could hand the page.
    const drawn = await page.evaluate(async () => {
      const { checked } = await import(new URL('core/diagrams.js', location.href).href)
      const svg =
        checked(`<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" id="d" onload="window.__pwned='svg'">
        <style>@import url(https://evil.example/a.css); @font-face { src: url(https://evil.example/f.woff) } #d .x { fill: url(#grad); background: url( 'https://evil.example/b.png' ); stroke: u\\72l(https://evil.example/e.png) } #d .y { background-image: image-set("https://evil.example/s.png" 1x) }</style>
        <defs><linearGradient id="grad"><stop offset="0" stop-color="#63c7b2"/></linearGradient></defs>
        <script>window.__pwned = 'script'</script>
        <a href="javascript:window.__pwned='a'"><text class="x">a link</text></a>
        <a xlink:href="https://evil.example/away"><text>another</text></a>
        <image href="https://evil.example/i.png" width="9" height="9"/>
        <use href="#grad"/><use href="https://evil.example/sprite.svg#x"/>
        <rect class="x" style="fill: url(https://evil.example/c.png)" onclick="window.__pwned='click'" width="9" height="9"/>
        <animate attributeName="href" to="javascript:window.__pwned='animate'"/>
        <set attributeName="onmouseover" to="window.__pwned='set'"/>
        <foreignObject width="90" height="20"><div xmlns="http://www.w3.org/1999/xhtml"><img src="https://evil.example/f.png" onerror="window.__pwned='fo'"/><span onmouseover="window.__pwned='span'">label</span><iframe src="https://evil.example/frame"></iframe></div></foreignObject>
      </svg>`)
      return svg.outerHTML
    })
    expect(drawn).not.toMatch(
      /evil\.example|javascript:|__pwned|<script|<image|<img|<iframe|<animate|<set|<a |\son\w+=|@import|@font-face|image-set|u\\72l/i,
    )
    // What a drawing needs of itself stays: its gradient, its places, its labels.
    expect(drawn).toContain('url(#grad)')
    expect(drawn).toContain('<use href="#grad">')
    expect(drawn).toContain('a link')
    expect(drawn).toContain('<span>label</span>')
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  })

  test('in HTML runs nothing in the app: it shows as a card, and opens only in the browser', async ({
    page,
  }) => {
    const evil = reaching(page, /evil\.example|:4318|^tauri:|^asset:|^ipc:/)
    const popups = []
    page.on('popup', (popup) => popups.push(popup.url()))
    await open(page)
    await choose(page, 'hostile')
    await expect(page.locator('#project-title')).toHaveText('hostile')
    const viewer = await openViewer(page)
    await expect(shown(viewer).locator('.registry-heading')).toBeVisible()
    // The spec that tries everything, and the mockups that move their own page: each a card.
    for (const [path, title] of [
      ['evil-html/SPEC.html', 'Evil HTML — SPEC'],
      ['evil-html/mockup-leave.html', 'A mockup that leaves'],
      ['evil-html/mockup-app.html', 'A mockup that opens the app in its frame'],
    ]) {
      await fileIn(viewer, path).click()
      const name = path.slice(path.lastIndexOf('/') + 1)
      await expect(card(viewer, name).locator('h1')).toHaveText(title)
    }
    // Long enough for its refresh (a second) and its mockups' moves to have fired, had they run.
    await page.waitForTimeout(1_500)
    await expect(viewer).toBeVisible()
    // No frame but the Agents and Harnesses screens' own, empty, and no script it brought.
    await expect(page.locator('iframe:not(dialog.screen iframe), script:not([src])')).toHaveCount(0)
    expect(
      page
        .frames()
        .filter((frame) => frame !== page.mainFrame())
        .map((frame) => frame.url()),
    ).toEqual(['about:blank', 'about:blank'])
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
    // It opens in the browser, asked of the daemon by its path, and only there.
    await fileIn(viewer, 'evil-html/SPEC.html').click()
    await card(viewer, 'SPEC.html').getByRole('button', { name: 'Open in your browser' }).click()
    await expect(card(viewer, 'SPEC.html').getByRole('status')).toHaveText(
      'Opened evil-html/SPEC.html in your browser.',
    )
    expect(await calls(page, 'specs.open')).toEqual([{ project: 6, path: 'evil-html/SPEC.html' }])
    // Its assets were never read for it: nothing of the folder is put in the app for an HTML file.
    expect(
      (await calls(page, 'specs.file')).filter((asked) => asked.path.startsWith('assets/')),
    ).toEqual([])
    expect(page.url()).toBe(`${ui.origin}/index.html`)
    expect(popups).toEqual([])
    expect(await evil.left()).toEqual([])
  })

  test('in HTML has its title read as text, whatever markup it holds', async ({ page }) => {
    const data = model()
    withFile(
      data.specs[6],
      'evil-html/mockup-title.html',
      Buffer.from(
        `<title>&lt;b&gt;bold&lt;/b&gt; <img src=x onerror="window.__pwned='title'"></title><p>x</p>`,
      ),
    )
    await open(page, data)
    await choose(page, 'hostile')
    await expect(page.locator('#project-title')).toHaveText('hostile')
    const viewer = await openViewer(page)
    await fileIn(viewer, 'evil-html/mockup-title.html').click()
    const title = card(viewer, 'mockup-title.html').locator('h1')
    await expect(title).toHaveText(`<b>bold</b> <img src=x onerror="window.__pwned='title'">`)
    await expect(title.locator('*')).toHaveCount(0)
    expect(await page.evaluate(() => window.__pwned)).toBeUndefined()
  })
})

test.describe("the app's policy", () => {
  test('lets the page load nothing from anywhere else: an image, a script, a stylesheet, a frame, a request', async ({
    page,
  }) => {
    refusing = true
    // Every address but the frame's is the trap's; a frame on this machine is the daemon's
    // screens' (`frame-src`), so the frame asks for one elsewhere.
    const evil = reaching(page, new RegExp(`evil\\.example|${literally(trap.origin)}`))
    await open(page)
    await page.evaluate(async (trap) => {
      const image = new Image()
      image.src = `${trap}/test.png`
      document.body.append(image)
      const sheet = document.createElement('link')
      sheet.rel = 'stylesheet'
      sheet.href = `${trap}/test.css`
      document.head.append(sheet)
      const frame = document.createElement('iframe')
      frame.src = 'https://evil.example/frame.html'
      document.body.append(frame)
      await import(`${trap}/test.js`).catch(() => {})
      await fetch(`${trap}/collect`).catch(() => {})
    }, trap.origin)
    await page.waitForTimeout(300)
    expect(await evil.left()).toEqual([])
    expect(trap.received).toEqual([])
    // Each refused by the directive that holds it: a frame is said by its origin alone.
    for (const [directive, address] of [
      ['img-src', `${trap.origin}/test.png`],
      ['style-src', `${trap.origin}/test.css`],
      ['frame-src', 'https://evil.example/'],
      ['script-src', `${trap.origin}/test.js`],
      ['connect-src', `${trap.origin}/collect`],
    ]) {
      expect(
        refused.some((said) => said.includes(directive) && said.includes(address)),
        `the policy refused ${address} by ${directive}`,
      ).toBe(true)
    }
  })
})

test.describe('Refresh', () => {
  test('reads the folder again after a read that failed', async ({ page }) => {
    const data = model()
    data.treeRefusal = 'the specs folder .specs cannot be read: Input/output error'
    await open(page, data)
    const viewer = await openViewer(page)
    await expect(shown(viewer).locator('.specs-refusal')).toHaveText(data.treeRefusal)
    await page.evaluate(() => {
      delete window.__model.treeRefusal
    })
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(shown(viewer).locator('.registry-heading')).toHaveText('Spec Registry')
    expect(await calls(page, 'specs.tree')).toEqual([{ project: 1 }, { project: 1 }])
  })

  test('reads the folder again and the file shown; a file chosen again is read again', async ({
    page,
  }) => {
    await open(page)
    const viewer = await openViewer(page)
    await fileIn(viewer, 'user-auth/research-10.md').click()
    const words = shown(viewer).locator('.spec-markdown p')
    await expect(words).toHaveText(
      "A thousand sign-ins a second need four API instances at argon2's cost.",
    )
    await page.evaluate(() => {
      const specs = window.__model.specs[1]
      specs.files['user-auth/research-10.md'].text =
        '# Research 10: Load\n\nSix instances, measured.\n'
      specs.entries.push({ name: 'zeta.md', path: 'zeta.md', size: 8 })
      specs.files['zeta.md'] = { path: 'zeta.md', size: 8, text: '# Zeta\n' }
    })
    const read = async () =>
      (await calls(page, 'specs.file')).filter((body) => body.path === 'user-auth/research-10.md')
        .length
    expect(await read()).toBe(1)
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect(words).toHaveText('Six instances, measured.')
    await expect(fileIn(viewer, 'zeta.md')).toBeVisible()
    await expect(fileIn(viewer, 'user-auth/research-10.md')).toHaveAttribute('aria-current', 'true')
    expect(await read()).toBe(2)
    await page.evaluate(() => {
      window.__model.specs[1].files['user-auth/research-10.md'].text =
        '# Research 10: Load\n\nEight.\n'
    })
    await fileIn(viewer, 'user-auth/research-10.md').click()
    await expect(words).toHaveText('Eight.')
    expect(await read()).toBe(3)
  })
})

for (const colorScheme of ['dark', 'light']) {
  test(`the viewer is drawn in the page's colours, its diagrams too, in ${colorScheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme })
    await page.route('https://cdn.jsdelivr.net/**', (route) => route.abort())
    await open(page)
    const viewer = await openViewer(page)
    const colour = (variable) =>
      page.evaluate((variable) => {
        const probe = document.createElement('span')
        probe.style.color = `var(${variable})`
        document.body.append(probe)
        const said = getComputedStyle(probe).color
        probe.remove()
        return said
      }, variable)
    const style = (locator, property) =>
      locator.evaluate((node, property) => getComputedStyle(node)[property], property)
    expect(await style(viewer, 'backgroundColor')).toBe(await colour('--panel'))
    expect(await style(viewer.locator('.specs-tree'), 'backgroundColor')).toBe(
      await colour('--deep'),
    )
    expect(await style(fileIn(viewer, 'registry.md'), 'color')).toBe(await colour('--seafoam'))
    expect(await style(viewer.locator('.registry-title strong').first(), 'color')).toBe(
      await colour('--foam'),
    )
    await fileIn(viewer, 'user-auth/SPEC.md').click()
    const drawing = shown(viewer).locator('figure.spec-diagram').first()
    await expect(drawing).toHaveAttribute('data-state', 'drawn', { timeout: 15_000 })
    expect(await style(drawing.locator('g.node rect, g.node polygon').first(), 'fill')).toBe(
      await colour('--panel-raised'),
    )
    expect(await style(drawing.locator('.nodeLabel').first(), 'color')).toBe(await colour('--foam'))
    // An HTML spec's card is the page's too: Specmint's own look is the browser's to show.
    await viewer.getByRole('button', { name: 'Close', exact: true }).click()
    await choose(page, 'checkout')
    await expect(page.locator('#project-title')).toHaveText('checkout')
    const html = await openViewer(page)
    await fileIn(html, 'checkout-flow/SPEC.html').click()
    const spec = card(html, 'SPEC.html')
    await expect(spec).toBeVisible()
    expect(await style(spec, 'backgroundColor')).toBe(await colour('--panel-raised'))
    expect(await style(spec.locator('h1'), 'color')).toBe(await colour('--foam'))
  })
}
