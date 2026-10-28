import { expect, test } from '@playwright/test'

import { refusedByPolicy, serveUi } from './serve-ui.mjs'
import { specsOf } from './specs-fixtures.mjs'

/**
 * The page's modals, against a stand-in for the app: a modal's `::backdrop` dims everything under
 * it, so a modal that paints nothing (a box with no size, no words, or no opacity) over another
 * veils it with nothing on top: what is under it looks disabled, and reads fine. The page keeps
 * none: every dialog it opens, in whatever order, paints, and whatever is open is covered only by
 * a dialog that opened later. `veils` is the check; the page's routes to its dialogs (the
 * Specs viewer's button, the project menu, Settings, the menu's Check for updates, Project
 * settings) each run through it, and so does every `<dialog>` of the page opened by name.
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

/** Two projects, the first with a specs folder, as the daemon answers for them. */
function model() {
  const shown = (id, name) => ({ id, name, directory: `/work/${name}`, state: 'open' })
  const projects = [shown(1, 'harbour'), shown(2, 'foundry')]
  return {
    projects,
    boards: Object.fromEntries(
      projects.map((project) => [
        project.id,
        {
          project,
          open: [],
          lanes: [
            {
              participant: human(project.id * 10, project.id),
              tasks: [],
              activity: { state: 'closed' },
              pane: null,
            },
          ],
        },
      ]),
    ),
    specs: { 1: specsOf('core') },
  }
}

async function open(page, data = model()) {
  // The order the page opens its modals in, which is the order of the top layer: counted as
  // `showModal` is called, whatever the order of the dialogs in the document.
  await page.addInitScript(() => {
    let count = 0
    const showModal = HTMLDialogElement.prototype.showModal
    HTMLDialogElement.prototype.showModal = function () {
      count += 1
      this.dataset.modalOrder = String(count)
      return showModal.call(this)
    }
  })
  await page.addInitScript((data) => {
    window.__listeners = new Map()
    window.__calls = []
    const answer = (value) => JSON.parse(JSON.stringify({ ok: true, ...value }))
    const operations = {
      'projects.list': () => answer({ projects: data.projects }),
      'agents.list': () => answer({ agents: [], missing: [] }),
      'board.get': ({ project }) => answer({ board: data.boards[project] }),
      'inbox.get': () => answer({ messages: [], total: 0, shown: 0 }),
      'staff.last': () => answer({ staff: [] }),
      'specs.get': () =>
        answer({
          app: { on: false },
          applied: { folder: '.specs', variant: 'core' },
          specmint: { version: '3.0.0', variants: [] },
        }),
      'specs.present': ({ project }) => answer({ present: data.specs[project] !== undefined }),
      'specs.tree': ({ project }) => {
        const specs = data.specs[project]
        if (!specs) return answer({ specs: { folder: '.specs', found: false, entries: [] } })
        return answer({
          specs: { folder: specs.folder, found: specs.found, entries: specs.entries },
        })
      },
      'specs.file': ({ project, path }) => answer({ file: data.specs[project].files[path] }),
      'wakeup.get': () =>
        answer({
          wakeUp: {
            on: false,
            everyMs: 3_600_000,
            every: 1,
            unit: 'hours',
            text: 'Check in.',
            defaultText: true,
          },
        }),
      'computer_use.get': () =>
        answer({
          system: 'macos',
          grantsOf: null,
          permissions: { accessibility: false, screenRecording: false },
          harnesses: {
            'claude-code': { on: false, folder: '/work/harbour' },
            codex: { setUp: false },
          },
        }),
    }
    const invoke = async (command, args = {}) => {
      if (command === 'daemon_request') {
        window.__calls.push([command, args.body, args.operation])
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
  await expect(page.locator('#specs-button')).toBeVisible()
}

/**
 * What is wrong with the modals open now, each as a sentence; none where all is well. Run in the
 * page. Every open modal paints (a box of some size, shown, not transparent, with words in it),
 * and is covered by no one but a modal opened after it, which paints too: the one on top is the
 * one hit at its own centre.
 */
function veils() {
  const modals = [...document.querySelectorAll('dialog[open]')].sort(
    (a, b) => Number(a.dataset.modalOrder ?? 0) - Number(b.dataset.modalOrder ?? 0),
  )
  const name = (dialog) => dialog.id || dialog.className || 'dialog'
  const paints = (dialog) => {
    const box = dialog.getBoundingClientRect()
    const style = getComputedStyle(dialog)
    return (
      box.width >= 40 &&
      box.height >= 40 &&
      style.display !== 'none' &&
      style.visibility === 'visible' &&
      Number(style.opacity) > 0 &&
      dialog.textContent.trim() !== ''
    )
  }
  const said = []
  modals.forEach((dialog, place) => {
    if (!dialog.matches(':modal')) said.push(`${name(dialog)} is open and is no modal`)
    if (!paints(dialog)) said.push(`${name(dialog)} is open and paints nothing`)
    // Its backdrop dims whatever is under it: all of it, with nothing on top.
    for (const below of modals.slice(0, place)) {
      if (!paints(dialog))
        said.push(`${name(below)} is under ${name(dialog)}, which paints nothing`)
    }
    const box = dialog.getBoundingClientRect()
    const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)
    const cover = hit?.closest('dialog') ?? null
    if (paints(dialog) && cover !== dialog && !modals.slice(place + 1).includes(cover)) {
      said.push(
        `${name(dialog)} is covered by ${cover ? name(cover) : (hit?.tagName ?? 'nothing')}`,
      )
    }
  })
  return said
}

const seen = (page) => page.evaluate(veils)

/** The ids of the dialogs open, the first opened first. */
const stack = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('dialog[open]')]
      .sort((a, b) => Number(a.dataset.modalOrder) - Number(b.dataset.modalOrder))
      .map((dialog) => dialog.id),
  )

/** A control pressed as the app's own menu or events press it: with the page behind a modal inert. */
const press = (page, selector) =>
  page.evaluate((selector) => document.querySelector(selector).click(), selector)

/** Shuts every dialog that is open. */
const shutAll = (page) =>
  page.evaluate(() => {
    for (const dialog of document.querySelectorAll('dialog[open]')) dialog.close()
  })

const VIEWER = 'specs-viewer-dialog'

test.describe('the page keeps no veil', () => {
  test('sees a modal that paints nothing over another, and one that is only covered', async ({
    page,
  }) => {
    await open(page)
    await page.locator('#specs-button').click()
    await expect(page.locator(`#${VIEWER}`)).toBeVisible()
    expect(await seen(page)).toEqual([])
    // An open dialog with nothing in it: its backdrop dims the viewer, and nothing shows on top.
    await page.evaluate(() => {
      const empty = document.createElement('dialog')
      empty.id = 'empty-dialog'
      document.body.append(empty)
      empty.showModal()
    })
    expect(await seen(page)).toEqual([
      'empty-dialog is open and paints nothing',
      `${VIEWER} is under empty-dialog, which paints nothing`,
    ])
    await shutAll(page)
    await page.evaluate(() => document.querySelector('#empty-dialog').remove())
    // A dialog whose box is there but whose words and opacity are not.
    await page.locator('#specs-button').click()
    await page.evaluate(() => {
      const faint = document.createElement('dialog')
      faint.id = 'faint-dialog'
      faint.style.opacity = '0'
      faint.textContent = 'Hello'
      document.body.append(faint)
      faint.showModal()
    })
    expect(await seen(page)).toContain('faint-dialog is open and paints nothing')
    // A dialog that is covered by one opened after it is no veil.
    await shutAll(page)
    await page.evaluate(() => document.querySelector('#faint-dialog').remove())
    await page.locator('#specs-button').click()
    await press(page, '#settings-button')
    expect(await stack(page)).toEqual([VIEWER, 'settings-dialog'])
    expect(await seen(page)).toEqual([])
  })

  test('paints every dialog of the page when it opens, alone and over the Specs viewer', async ({
    page,
  }) => {
    await open(page)
    const ids = await page.evaluate(() => [...document.querySelectorAll('dialog')].map((d) => d.id))
    // The viewer, Settings, the screens, the stack, Project settings, History and the rest:
    // a dialog added to the page is in this list by being in the page.
    expect(ids).toContain(VIEWER)
    expect(ids.length).toBeGreaterThan(10)
    for (const id of ids) {
      await page.evaluate((id) => document.getElementById(id).showModal(), id)
      expect(await seen(page), `${id} opened alone`).toEqual([])
      expect(await stack(page)).toEqual([id])
      await shutAll(page)
    }
    for (const id of ids.filter((id) => id !== VIEWER)) {
      await page.evaluate(() => document.getElementById('specs-viewer-dialog').showModal())
      await page.evaluate((id) => document.getElementById(id).showModal(), id)
      expect(await seen(page), `${id} over the viewer`).toEqual([])
      expect(await stack(page)).toEqual([VIEWER, id])
      await shutAll(page)
      // And the viewer opened over it, as the project menu's Specs… does over Project settings.
      await page.evaluate((id) => document.getElementById(id).showModal(), id)
      await page.evaluate(() => document.getElementById('specs-viewer-dialog').showModal())
      expect(await seen(page), `the viewer over ${id}`).toEqual([])
      expect(await stack(page)).toEqual([id, VIEWER])
      await shutAll(page)
    }
  })

  test('paints at each step of the routes that open a modal over the viewer, and back', async ({
    page,
  }) => {
    await open(page)
    await page.locator('#specs-button').click()
    await expect(page.locator(`#${VIEWER}`)).toBeVisible()
    expect(await seen(page)).toEqual([])
    const over = async (what, route, expected) => {
      await page.evaluate(route)
      await expect.poll(() => stack(page), what).toEqual(expected)
      expect(await seen(page), what).toEqual([])
    }
    // The page behind the viewer is inert; the app's own menu and events reach it all the same.
    await over('Settings', () => document.querySelector('#settings-button').click(), [
      VIEWER,
      'settings-dialog',
    ])
    await page.evaluate(() => document.getElementById('settings-dialog').close())
    await over('Check for updates', () => window.__listeners.get('check-updates')({}), [
      VIEWER,
      'updates-dialog',
    ])
    await page.evaluate(() => document.getElementById('updates-dialog').close())
    await over(
      'Project settings',
      () => document.querySelector('#project-settings-button').click(),
      [VIEWER, 'project-settings-dialog'],
    )
    await page.evaluate(() => document.getElementById('project-settings-dialog').close())
    // The viewer asked for again while it is open, from its button and from the menu.
    await press(page, '#specs-button')
    expect(await stack(page)).toEqual([VIEWER])
    expect(await seen(page)).toEqual([])
    await shutAll(page)
    // The project menu's Specs… for another project than the board shows: the page shows
    // that project first, and opens the viewer on it.
    await page
      .locator('#projects .project-select', { hasText: 'harbour' })
      .click({ button: 'right' })
    await page.getByRole('menuitem', { name: 'Specs…' }).click()
    await expect(page.locator(`#${VIEWER}`)).toBeVisible()
    expect(await seen(page)).toEqual([])
    await page.getByRole('button', { name: 'Close', exact: true }).click()
    await expect(page.locator(`#${VIEWER}`)).toBeHidden()
    expect(await seen(page)).toEqual([])
    expect(await stack(page)).toEqual([])
  })

  // `close` is an event the browser fires a task after `close()`: a viewer opened again in the
  // task that shut it, as the project menu's Specs… does once it has shown the project, is open
  // when it fires, and used to be emptied by it: a modal open with nothing in it.
  test('keeps a viewer opened in the task that shut it as full as one opened later', async ({
    page,
  }) => {
    await open(page)
    await page.locator('#specs-button').click()
    const viewer = page.locator(`#${VIEWER}`)
    const registry = viewer.locator('.tree-file[data-path="registry.md"]')
    await expect(registry).toBeVisible()
    await page.evaluate(() => {
      document.querySelector('#specs-viewer-dialog').close()
      document.querySelector('#specs-button').click()
    })
    await expect(viewer).toBeVisible()
    await expect(registry).toBeVisible()
    await expect(viewer.locator('.registry-heading')).toHaveText('Spec Registry')
    expect(await seen(page)).toEqual([])
    // Refresh still reads: the viewer still knows its project.
    const reads = () =>
      page.evaluate(
        () => window.__calls.filter(([, , operation]) => operation === 'specs.tree').length,
      )
    const before = await reads()
    await viewer.getByRole('button', { name: 'Refresh' }).click()
    await expect.poll(reads).toBe(before + 1)
    await expect(viewer.locator('.registry-heading')).toHaveText('Spec Registry')
  })
})
