import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { dirname, extname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'
import { Tabs } from '../../src/tabs.js'
import { startUiServer } from '../../src/ui.js'
import { tempEnv } from '../../tests/helpers.mjs'

const UI_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'ui')
const ROSTER_ORIGIN = 'http://localhost:43123'
const GEOMETRY_KEY = 'consensflow.window.geometry.v1'

let server
let origin

test.setTimeout(12_000)

test.beforeAll(async () => {
  server = createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname
      const relative = pathname === '/' ? 'index.html' : decodeURIComponent(pathname.slice(1))
      const file = resolve(UI_ROOT, relative)
      if (file !== UI_ROOT && !file.startsWith(`${UI_ROOT}${sep}`)) {
        response.writeHead(403).end('forbidden')
        return
      }
      const type =
        {
          '.css': 'text/css; charset=utf-8',
          '.html': 'text/html; charset=utf-8',
          '.js': 'text/javascript; charset=utf-8',
        }[extname(file)] ?? 'application/octet-stream'
      response.writeHead(200, { 'content-type': type, 'cache-control': 'no-store' })
      response.end(await readFile(file))
    } catch {
      response.writeHead(404).end('not found')
    }
  })
  await new Promise((resolveListening) => server.listen(0, '127.0.0.1', resolveListening))
  const address = server.address()
  origin = `http://127.0.0.1:${address.port}`
})

test.afterAll(async () => {
  if (server === undefined) return
  await new Promise((resolveClosed, reject) => {
    server.close((error) => (error === undefined ? resolveClosed() : reject(error)))
  })
})

function pane(id, kind, extra = {}) {
  return { id, generation: 1, kind, order: 0, alive: true, ...extra }
}

function inMemoryTabStore() {
  let envelope = { nextPane: 1, issued: {}, tabs: [] }
  return {
    async readTabs() {
      return envelope.tabs
    },
    async mutate(_directory, _name, operation) {
      return await operation({
        async readTabsEnvelope() {
          return structuredClone(envelope)
        },
        async writeTabsEnvelope(next) {
          envelope = next
        },
      })
    },
  }
}

function cannedState() {
  return {
    ok: true,
    available: true,
    roster: { url: `${ROSTER_ORIGIN}/`, token: 'ui-token' },
    agents: [
      { name: 'nyx', harness: 'codex' },
      { name: 'ares', harness: 'pi' },
    ],
    tabs: [
      {
        id: 't-four',
        name: 'harbour',
        directory: '/work/harbour',
        closed: false,
        policy: 'auto',
        lead: { name: 'harbour-lead', harness: 'claude', generation: 1 },
        panes: [
          pane('p4-lead', 'lead', {
            name: 'harbour-lead',
            harness: 'claude',
            effectivePolicy: { mode: 'auto', source: 'tab' },
          }),
          pane('p4-w1', 'worker', {
            order: 1,
            conversation: 'nyx-coral-lane',
            agent: 'nyx',
            effectivePolicy: { mode: 'manual', source: 'pane' },
          }),
          pane('p4-w2', 'worker', {
            order: 2,
            conversation: 'ares-amber-moss',
            agent: 'ares',
            effectivePolicy: { mode: 'auto', source: 'lead preference' },
          }),
          pane('p4-shell', 'shell', { order: 3 }),
        ],
      },
      {
        id: 't-five',
        name: 'foundry',
        directory: '/work/foundry',
        closed: false,
        policy: 'manual',
        lead: { name: 'foundry-lead', harness: 'codex', generation: 1 },
        panes: [
          pane('p5-lead', 'lead', {
            name: 'foundry-lead',
            harness: 'codex',
            effectivePolicy: { mode: 'manual', source: 'tab' },
          }),
          pane('p5-w1', 'worker', {
            order: 1,
            conversation: 'nyx-silver-pine',
            agent: 'nyx',
            effectivePolicy: { mode: 'manual', source: 'tab' },
          }),
          pane('p5-w2', 'worker', {
            order: 2,
            conversation: 'ares-blue-fjord',
            agent: 'ares',
            effectivePolicy: { mode: 'manual', source: 'tab' },
          }),
          pane('p5-w3', 'worker', {
            order: 3,
            conversation: 'nyx-stone-fern',
            agent: 'nyx',
            effectivePolicy: { mode: 'manual', source: 'tab' },
          }),
          pane('p5-w4', 'worker', {
            order: 4,
            conversation: 'ares-gold-wake',
            agent: 'ares',
            effectivePolicy: { mode: 'manual', source: 'tab' },
          }),
        ],
      },
      {
        id: 't-closed',
        name: 'archive',
        directory: '/work/archive',
        closed: true,
        policy: 'auto',
        lead: { name: 'archive-lead', harness: 'pi', generation: 2 },
        panes: [
          pane('pc-lead', 'lead', { name: 'archive-lead', harness: 'pi', alive: false }),
          pane('pc-w1', 'worker', {
            order: 1,
            alive: false,
            conversation: 'ares-old-map',
            agent: 'ares',
            effectivePolicy: { mode: 'inherit', source: 'default' },
          }),
        ],
      },
    ],
    answers: {
      'nyx-coral-lane': [
        { id: 'answer-fresh', preview: 'The parser is ready.', delivered: false, uncertain: false },
        { id: 'answer-sent', preview: 'Already sent.', delivered: true, uncertain: false },
        {
          id: 'answer-uncertain',
          preview: 'Receipt is unclear.',
          delivered: false,
          uncertain: true,
        },
      ],
    },
    results: [
      {
        id: 'd-1',
        tab: 't-four',
        conversation: 'nyx-coral-lane',
        agent: 'nyx',
        answerId: 'answer-one',
        state: 'waiting',
        preview: 'worker reply ready',
        parts: 1,
        receivedParts: 0,
        createdAt: 1,
      },
    ],
  }
}

function gridState(count) {
  const state = cannedState()
  const panes = Array.from({ length: count }, (_, index) =>
    pane(`p-grid-${index}`, index === 0 ? 'lead' : 'worker', {
      name: index === 0 ? 'grid-lead' : `agent-${index}`,
      order: index,
      conversation: index === 0 ? undefined : `conversation-${index}`,
      agent: index === 0 ? 'claude' : `agent-${index}`,
      effectivePolicy: { mode: 'auto', source: 'tab' },
    }),
  )
  state.tabs = [
    {
      ...state.tabs[0],
      id: 't-grid',
      name: `grid-${count}`,
      directory: '/work/grid',
      lead: { name: 'grid-lead', harness: 'claude', generation: 1 },
      panes,
    },
  ]
  state.results = []
  return state
}

async function installTauriShim(
  page,
  { state = cannedState(), geometry = null, emulatorGate = false, listStateGate = false } = {},
) {
  await page.addInitScript(
    ({ initialState, initialGeometry, geometryKey, gateEmulator, gateListState }) => {
      const copy = (value) => JSON.parse(JSON.stringify(value))
      window.__calls = []
      window.__state = copy(initialState)
      window.__commandResults = {}
      window.__outputChannel = null
      window.__disposedEmulators = []
      const eventListeners = new Map()
      const parserWrites = []
      let nextInputTicket = 1
      let parserGateOpen = !gateEmulator
      let tabDeleteGate = null
      let releaseTabDelete
      let releaseFirstListState
      let listStateCalls = 0
      const firstListStateGate = gateListState
        ? new Promise((resolveGate) => {
            releaseFirstListState = resolveGate
          })
        : null

      if (initialGeometry !== null) {
        localStorage.setItem(geometryKey, JSON.stringify(initialGeometry))
      } else {
        localStorage.removeItem(geometryKey)
      }

      class Channel {
        constructor() {
          this.onmessage = null
        }
      }

      class PhysicalSize {
        constructor(width, height) {
          this.width = width
          this.height = height
          this.type = 'Physical'
        }
      }

      class PhysicalPosition {
        constructor(x, y) {
          this.x = x
          this.y = y
          this.type = 'Physical'
        }
      }

      const moved = []
      const resized = []
      const windowState = {
        position: { x: 80, y: 90 },
        size: { width: 1120, height: 760 },
        maximized: false,
      }
      const recordWindow = (command, args = {}) => {
        window.__calls.push({ command, args })
      }
      const appWindow = {
        async maximize() {
          windowState.maximized = true
          recordWindow('window.maximize')
        },
        async unmaximize() {
          windowState.maximized = false
          recordWindow('window.unmaximize')
        },
        async setSize(size) {
          windowState.size = { width: size.width, height: size.height }
          recordWindow('window.setSize', { ...windowState.size, type: size.type })
        },
        async setPosition(position) {
          windowState.position = { x: position.x, y: position.y }
          recordWindow('window.setPosition', { ...windowState.position, type: position.type })
        },
        async innerSize() {
          return { ...windowState.size }
        },
        async outerPosition() {
          return { ...windowState.position }
        },
        async isMaximized() {
          return windowState.maximized
        },
        async onMoved(handler) {
          moved.push(handler)
          return () => {}
        },
        async onResized(handler) {
          resized.push(handler)
          return () => {}
        },
      }

      const findTab = (id) => window.__state.tabs.find((tab) => tab.id === id)
      const invoke = async (command, args = {}) => {
        const logged = { ...args }
        if (logged.onOutput !== undefined) logged.onOutput = '[channel]'
        window.__calls.push({ command, args: copy(logged) })
        if (command === 'pane_resize' && window.__resizeGate) await window.__resizeGate
        if (Object.hasOwn(window.__commandResults, command)) {
          return copy(window.__commandResults[command])
        }
        if (command === 'subscribe_output') {
          if (window.__outputChannel !== null) {
            throw new Error('replacing the Rust Channel ends the existing callback')
          }
          window.__outputChannel = args.onOutput
          return { ok: true }
        }
        if (command === 'list_state') {
          if (args.onOutput !== undefined) {
            throw new Error('state refresh must not replace the output subscription')
          }
          const snapshot = copy(window.__state)
          listStateCalls += 1
          if (listStateCalls === 1 && firstListStateGate !== null) await firstListStateGate
          if (window.__pendingListState !== undefined) await window.__pendingListState
          return snapshot
        }
        if (command === 'answers_list') {
          return {
            ok: true,
            answers: copy(window.__state.answers[args.conversation] ?? []),
          }
        }
        if (command === 'result_body') {
          const text = window.__state.resultBodies?.[args.result] ?? ''
          const end = Math.min((args.offset ?? 0) + 16000, text.length)
          return {
            ok: true,
            text: text.slice(args.offset ?? 0, end),
            next: end < text.length ? end : null,
          }
        }
        if (command === 'tab_resume') {
          const tab = findTab(args.tab)
          if (tab !== undefined) tab.closed = false
          return { ok: true }
        }
        if (command === 'rename_session') {
          const tab = findTab(args.tab)
          if (tab !== undefined) tab.name = args.name
          return { ok: true, tab: args.tab, name: args.name }
        }
        if (command === 'tab_delete') {
          if (tabDeleteGate !== null) await tabDeleteGate
          const tab = findTab(args.tab)
          if (tab === undefined) return { ok: false, error: 'tab not found' }
          if (args.generation !== tab.lead?.generation) {
            return { ok: false, error: 'stale tab generation' }
          }
          window.__state.tabs = window.__state.tabs.filter((candidate) => candidate.id !== args.tab)
          return { ok: true, tab: args.tab }
        }
        if (command === 'close_pane' || command === 'delete_pane') {
          const tab = window.__state.tabs.find((candidate) =>
            candidate.panes.some(
              (pane) => pane.id === args.id && pane.generation === args.generation,
            ),
          )
          if (tab === undefined) return { ok: false, error: 'stale pane generation' }
          const pane = tab.panes.find(
            (candidate) => candidate.id === args.id && candidate.generation === args.generation,
          )
          if (pane.kind === 'lead') {
            tab.closed = true
            pane.alive = false
          } else {
            tab.panes = tab.panes.filter((candidate) => candidate.id !== args.id)
          }
          return {
            ok: true,
            outcome: 'closed',
            tab: tab.id,
            pane: { id: args.id, generation: args.generation },
            kind: pane.kind,
          }
        }
        if (command === 'open_shell') {
          const tab = findTab(args.tab)
          const next = {
            id: `shell-${tab.panes.length}`,
            generation: 1,
            kind: 'shell',
            order: tab.panes.length,
            alive: true,
          }
          tab.panes.push(next)
          return { ok: true, pane: copy(next) }
        }
        if (command === 'open_consult' && args.task !== null) {
          const tab = findTab(args.tab)
          const index = tab.panes.filter((candidate) => candidate.kind === 'worker').length + 1
          const next = {
            id: `worker-${index + 10}`,
            generation: 1,
            kind: 'worker',
            order: tab.panes.length,
            conversation: `${args.agent}-new-answer`,
            agent: args.agent,
            effectivePolicy: { mode: 'auto', source: 'tab' },
            alive: true,
          }
          tab.panes.push(next)
          return { ok: true, pane: copy(next) }
        }
        if (command === 'pane_input_enqueue' || command === 'pane_reply_enqueue') {
          const ticket = `test-input-${nextInputTicket}`
          nextInputTicket += 1
          return { ok: true, ticket }
        }
        return { ok: true }
      }

      const consumeParserWrite = ({ host, bytes, resolve }) => {
        host.dataset.consumed = `${host.dataset.consumed ?? ''}${new TextDecoder().decode(
          new Uint8Array(bytes),
        )}`
        resolve()
      }
      const testApi = gateEmulator
        ? {
            createEmulator(host) {
              host.dataset.consumed = ''
              const listeners = { data: new Set(), reply: new Set(), resize: new Set() }
              const subscribe = (set, callback) => {
                set.add(callback)
                return { dispose: () => set.delete(callback) }
              }
              return {
                write(bytes) {
                  return new Promise((resolveWrite) => {
                    const write = { host, bytes: [...bytes], resolve: resolveWrite }
                    if (parserGateOpen) consumeParserWrite(write)
                    else parserWrites.push(write)
                  })
                },
                onData(callback) {
                  return subscribe(listeners.data, callback)
                },
                onReply(callback) {
                  return subscribe(listeners.reply, callback)
                },
                onResize(callback) {
                  return subscribe(listeners.resize, callback)
                },
                resize() {},
                fit() {},
                dispose() {
                  const card = host.closest('[data-pane-id]')
                  window.__disposedEmulators.push({
                    id: card?.dataset.paneId,
                    generation: Number(card?.dataset.generation),
                  })
                },
              }
            },
          }
        : undefined

      window.__TAURI__ = {
        core: { Channel, invoke },
        event: {
          async listen(name, handler) {
            if (!/^[\p{Alphabetic}\p{Number}/:_-]*$/u.test(name)) {
              throw new Error(`Tauri refuses the event name ${name}`)
            }
            const handlers = eventListeners.get(name) ?? []
            handlers.push(handler)
            eventListeners.set(name, handlers)
            return () => {
              const current = eventListeners.get(name) ?? []
              const at = current.indexOf(handler)
              if (at >= 0) current.splice(at, 1)
            }
          },
        },
        dialog: {
          async open(options) {
            window.__calls.push({ command: 'dialog.open', args: copy(options) })
            return '/picked/workspace'
          },
        },
        dpi: { PhysicalPosition, PhysicalSize },
        ...(testApi === undefined ? {} : { test: testApi }),
        window: { getCurrentWindow: () => appWindow },
      }
      window.__setCommandResult = (command, result) => {
        window.__commandResults[command] = copy(result)
      }
      window.__gateTabDelete = () => {
        tabDeleteGate = new Promise((resolveGate) => {
          releaseTabDelete = resolveGate
        })
      }
      window.__releaseTabDelete = () => {
        releaseTabDelete?.()
        releaseTabDelete = undefined
        tabDeleteGate = null
      }
      window.__emitPaneFlood = (id, generation, count) => {
        if (typeof window.__outputChannel?.onmessage !== 'function') {
          throw new Error('the page did not register its output channel')
        }
        const encoder = new TextEncoder()
        for (let seq = 1; seq <= count; seq += 1) {
          window.__outputChannel.onmessage({
            id,
            generation,
            seq,
            bytes: [...encoder.encode(`hidden ${seq}\r\n`)],
          })
        }
      }
      window.__emitPaneOutput = (id, generation, seq, bytes) => {
        if (typeof window.__outputChannel?.onmessage !== 'function') {
          throw new Error('the page did not register its output channel')
        }
        window.__outputChannel.onmessage({ id, generation, seq, bytes })
      }
      window.__emitTauriEvent = async (name, payload = {}) => {
        if (!/^[\p{Alphabetic}\p{Number}/:_-]*$/u.test(name)) {
          throw new Error(`Tauri refuses the event name ${name}`)
        }
        for (const handler of eventListeners.get(name) ?? []) {
          await handler({ event: name, payload: copy(payload) })
        }
      }
      window.__openExternalWorker = async () => {
        const tab = findTab('t-four')
        tab.panes.push({
          id: 'p4-w3',
          generation: 1,
          kind: 'worker',
          order: 4,
          conversation: 'clio-external-worker',
          agent: 'clio',
          effectivePolicy: { mode: 'auto', source: 'tab' },
          alive: true,
        })
        window.__state.results.push({
          id: 'd-2',
          conversation: 'clio-external-worker',
          agent: 'clio',
          preview: 'worker reply ready',
          parts: 1,
          receivedParts: 0,
          createdAt: 2,
          tab: 't-four',
          pane: 'p4-lead',
          generation: 1,
          state: 'waiting',
          reason: 'worker reply ready',
        })
        window.__emitPaneOutput('p4-w3', 1, 1, [
          ...new TextEncoder().encode('external worker output\r\n'),
        ])
        await window.__emitTauriEvent('state-changed', { reason: 'pane.open' })
      }
      window.__releaseOneParserWrite = () => {
        const write = parserWrites.shift()
        if (write !== undefined) consumeParserWrite(write)
      }
      window.__releaseParserGate = () => {
        parserGateOpen = true
        while (parserWrites.length > 0) consumeParserWrite(parserWrites.shift())
      }
      window.__releaseListState = () => releaseFirstListState?.()
      window.__moveMockWindow = async (position, size, maximized = false) => {
        windowState.position = { ...position }
        windowState.size = { ...size }
        windowState.maximized = maximized
        for (const handler of moved) await handler({ payload: { ...position } })
        for (const handler of resized) await handler({ payload: { ...size } })
      }
    },
    {
      initialState: state,
      initialGeometry: geometry,
      geometryKey: GEOMETRY_KEY,
      gateEmulator: emulatorGate,
      gateListState: listStateGate,
    },
  )
}

async function boot(page, options = {}) {
  await page.route(`${ROSTER_ORIGIN}/**`, (route) =>
    route.fulfill({
      status: 200,
      contentType: 'text/html',
      body: '<!doctype html><title>Roster probe</title><main>Roster connected</main>',
    }),
  )
  await installTauriShim(page, options)
  await page.goto(origin)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true', { timeout: 2_000 })
}

function commandCalls(page, command) {
  return page.evaluate((name) => window.__calls.filter((call) => call.command === name), command)
}

test('maximizes the first launch and persists changed geometry', async ({ page }) => {
  await boot(page)
  await expect.poll(async () => (await commandCalls(page, 'window.maximize')).length).toBe(1)

  await page.evaluate(() =>
    window.__moveMockWindow({ x: 140, y: 160 }, { width: 1280, height: 820 }),
  )
  await expect
    .poll(() => page.evaluate((key) => JSON.parse(localStorage.getItem(key)), GEOMETRY_KEY))
    .toEqual({ x: 140, y: 160, width: 1280, height: 820, maximized: false })
})

test('restores saved geometry on later launches', async ({ page }) => {
  await boot(page, {
    geometry: { x: 24, y: 36, width: 1040, height: 720, maximized: false },
  })
  await expect.poll(async () => (await commandCalls(page, 'window.setSize')).length).toBe(1)
  expect(await commandCalls(page, 'window.setSize')).toContainEqual({
    command: 'window.setSize',
    args: { width: 1040, height: 720, type: 'Physical' },
  })
  expect(await commandCalls(page, 'window.setPosition')).toContainEqual({
    command: 'window.setPosition',
    args: { x: 24, y: 36, type: 'Physical' },
  })
  expect(await commandCalls(page, 'window.maximize')).toHaveLength(0)
})

test('opens Agents full-window and preserves the roster and pane services when closed', async ({
  page,
}) => {
  await boot(page)
  const dialog = page.getByTestId('roster-panel')
  const iframe = page.getByTestId('roster-frame')
  const opener = page.getByRole('button', { name: 'Your agents', exact: true })
  await expect(dialog).not.toBeVisible()
  await expect(iframe).toHaveAttribute('src', `${ROSTER_ORIGIN}/?token=ui-token`)
  const before = await page.getByTestId('pane-stage').boundingBox()
  await opener.click()
  await expect(page.getByRole('dialog', { name: 'Your agents', exact: true })).toBeVisible()
  const bounds = await dialog.boundingBox()
  const viewport = page.viewportSize()
  expect(bounds.x).toBe(0)
  expect(bounds.y).toBe(0)
  expect(bounds.width).toBe(viewport.width)
  expect(bounds.height).toBe(viewport.height)
  await expect(
    page.frameLocator('[data-testid="roster-frame"]').getByText('Roster connected'),
  ).toBeVisible()
  await page.evaluate(() => window.__emitPaneFlood('p5-w1', 1, 8))
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p5-w1').length,
    )
    .toBe(8)
  await page.getByRole('button', { name: 'Close Your agents' }).click()
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  await expect(iframe).toHaveCount(1)
  expect(await page.getByTestId('pane-stage').boundingBox()).toEqual(before)
  await opener.click()
  await page.getByRole('button', { name: 'Close Your agents' }).focus()
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  expect(await commandCalls(page, 'close_pane')).toHaveLength(0)
})

test('opens Agent library beside Your agents and retains both frames with refresh on reopening', async ({
  page,
}) => {
  await boot(page)
  const own = page.getByRole('button', { name: 'Your agents', exact: true })
  const library = page.getByRole('button', { name: 'Agent library', exact: true })
  await expect(page.locator('#roster-toggle + #library-toggle')).toHaveText('Agent library')
  const frame = page.getByTestId('library-frame')
  await expect(frame).not.toHaveAttribute('src', /.+/)
  await library.click()
  await expect(page.getByRole('dialog', { name: 'Agent library', exact: true })).toBeVisible()
  await expect(frame).toHaveAttribute('src', `${ROSTER_ORIGIN}/library?token=ui-token`)
  expect(await page.locator('#library-dialog').boundingBox()).toEqual({
    x: 0,
    y: 0,
    ...page.viewportSize(),
  })
  const probe = page.frameLocator('[data-testid="library-frame"]')
  await probe.locator('main').evaluate((node) => {
    node.textContent = 'retained library'
    window.refreshes = 0
    window.addEventListener('message', (event) => {
      if (event.data === 'consensflow:refresh-agents') window.refreshes++
    })
  })
  await page.getByRole('button', { name: 'Close Agent library' }).click()
  await expect(library).toBeFocused()
  await own.click()
  await expect(page.getByRole('dialog', { name: 'Your agents', exact: true })).toBeVisible()
  await expect(page.locator('#library-dialog')).not.toBeVisible()
  await page.getByRole('button', { name: 'Close Your agents' }).click()
  await library.click()
  await expect(probe.locator('main')).toHaveText('retained library')
  await expect.poll(() => probe.locator('main').evaluate(() => window.refreshes)).toBe(1)
  await page.getByRole('button', { name: 'Close Agent library' }).focus()
  await page.keyboard.press('Escape')
  await expect(library).toBeFocused()
  expect(await commandCalls(page, 'close_pane')).toHaveLength(0)
})

test('real agent pages add and remove across native shell screens at the minimum window width', async ({
  page,
}) => {
  const box = tempEnv()
  const service = await startUiServer(box.env)
  try {
    await page.setViewportSize({ width: 560, height: 900 })
    const state = cannedState()
    state.roster = { url: service.url, token: service.token }
    await boot(page, { state })
    for (const id of ['roster-toggle', 'library-toggle', 'harnesses-toggle']) {
      const bounds = await page.locator(`#${id}`).boundingBox()
      expect(bounds.x).toBeGreaterThanOrEqual(0)
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(560)
    }
    await page.getByRole('button', { name: 'Your agents', exact: true }).click()
    const own = page.frameLocator('[data-testid="roster-frame"]')
    await expect(own.locator('#roster')).toContainText('No agents yet')
    await own.getByRole('searchbox').fill('Astra')
    await page.getByRole('button', { name: 'Close Your agents' }).click()
    await page.getByRole('button', { name: 'Agent library', exact: true }).click()
    const library = page.frameLocator('[data-testid="library-frame"]')
    await library.getByRole('searchbox').fill('maia')
    await library.getByRole('button', { name: 'Add', exact: true }).click()
    await expect(library.getByRole('button', { name: 'Already added' })).toBeDisabled()
    await page.getByRole('button', { name: 'Close Agent library' }).click()
    await page.getByRole('button', { name: 'Your agents', exact: true }).click()
    await expect(own.locator('.callsign')).toHaveText('maia')
    await expect(own.getByRole('searchbox')).toHaveValue('Astra')
    await own.getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(own.locator('.callsign')).toHaveCount(0)
    await page.getByRole('button', { name: 'Close Your agents' }).click()
    await page.getByRole('button', { name: 'Agent library', exact: true }).click()
    await expect(library.getByRole('button', { name: 'Add', exact: true })).toBeEnabled()
    await expect(library.getByRole('searchbox')).toHaveValue('maia')
  } finally {
    await page.close()
    await service.close()
    box.cleanup()
  }
})

test('opens Harnesses separately, loads it on demand and keeps Agents drafts and panes alive', async ({
  page,
}) => {
  await boot(page)
  const opener = page.getByRole('button', { name: 'Harnesses', exact: true })
  const dialog = page.getByRole('dialog', { name: 'Harnesses', exact: true })
  const frame = page.getByTestId('harnesses-frame')
  await expect(frame).not.toHaveAttribute('src', /.+/)
  await page.getByRole('button', { name: 'Your agents', exact: true }).click()
  const roster = page.frameLocator('[data-testid="roster-frame"]')
  await roster.locator('main').evaluate((node) => {
    const input = document.createElement('input')
    input.id = 'draft'
    input.value = 'unfinished agent'
    node.append(input)
  })
  await page.getByRole('button', { name: 'Close Your agents' }).click()
  const before = await page.getByTestId('pane-stage').boundingBox()
  await opener.click()
  await expect(dialog).toBeVisible()
  await expect(frame).toHaveAttribute('src', `${ROSTER_ORIGIN}/harnesses?token=ui-token`)
  const bounds = await dialog.boundingBox()
  expect(bounds).toEqual({ x: 0, y: 0, ...page.viewportSize() })
  await page.evaluate(() => window.__emitPaneFlood('p5-w1', 1, 8))
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p5-w1').length,
    )
    .toBe(8)
  await page.getByRole('button', { name: 'Close Harnesses' }).click()
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  expect(await page.getByTestId('pane-stage').boundingBox()).toEqual(before)
  await opener.click()
  await page.getByRole('button', { name: 'Close Harnesses' }).focus()
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  await page.getByRole('button', { name: 'Your agents', exact: true }).click()
  await expect(roster.locator('#draft')).toHaveValue('unfinished agent')
  expect(await commandCalls(page, 'close_pane')).toHaveLength(0)
})

test('explains reply delivery beside its explicit Automatic or Manual label', async ({ page }) => {
  await boot(page)
  await expect(page.getByTestId('tab-policy')).toHaveText('Reply delivery: Automatic')
  await page.getByRole('button', { name: 'About reply delivery' }).click()
  const help = page.getByTestId('delivery-help')
  await expect(help).toBeVisible()
  await expect(help).toContainText('complete worker replies')
  await expect(help).toContainText('Manual')
  await expect(help).toContainText('Opening it here does not mark it received')
  await expect(help).toContainText('all automatic replies')
  await page.keyboard.press('Escape')
  await expect(help).not.toBeVisible()
  await page.getByTestId('session-t-five-button').click()
  await expect(page.getByTestId('tab-policy')).toHaveText('Reply delivery: Manual')
})

for (const count of [1, 4]) {
  test(`keeps the same top edge when selecting an agent from a ${count}-pane session`, async ({
    page,
  }) => {
    const state = cannedState()
    state.tabs[0].panes = state.tabs[0].panes.slice(0, count)
    await boot(page, { state })
    const lead = page.getByTestId('pane-p4-lead')
    const gridTop = (await lead.boundingBox()).y
    await page.getByTestId('pane-node-p4-lead').click()
    await expect(page.getByTestId('pane-stage')).toHaveAttribute('data-mode', 'focused')
    expect((await lead.boundingBox()).y).toBe(gridTop)
    if (count > 1) {
      const nav = await page.getByRole('navigation', { name: 'Pane navigation' }).boundingBox()
      const title = await lead.locator('.pane-titlebar').boundingBox()
      expect(nav.y).toBeGreaterThanOrEqual(title.y)
      expect(nav.y + nav.height).toBeLessThanOrEqual(title.y + title.height)
    }
    await page.getByTestId('session-t-four-button').click()
    expect((await lead.boundingBox()).y).toBe(gridTop)
  })
}

test('collapses the sidebar left while leaving the pane area mounted', async ({ page }) => {
  await boot(page)
  const before = await page.getByTestId('pane-stage').boundingBox()
  await page.getByRole('button', { name: 'Collapse sessions' }).click()
  await expect(page.getByTestId('workspace')).toHaveAttribute('data-sidebar-collapsed', 'true')
  await expect(page.getByRole('button', { name: 'Expand sessions' })).toBeVisible()
  await expect(page.getByTestId('pane-stage')).toBeVisible()
  await expect
    .poll(async () => (await page.getByTestId('pane-stage').boundingBox()).width)
    .toBeGreaterThan(before.width)
})

test('renders the exact four- and five-pane templates counted with the lead', async ({ page }) => {
  await boot(page)
  const stage = page.getByTestId('pane-stage')
  await expect(stage).toHaveAttribute('data-layout', '"lead w1 w3" "lead w2 ."')
  const four = await stage.locator('.pane-card').evaluateAll((cards) =>
    Object.fromEntries(
      cards.map((card) => {
        const bounds = card.getBoundingClientRect()
        return [
          card.dataset.paneId,
          { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height },
        ]
      }),
    ),
  )
  expect(four['p4-lead'].y).toBe(four['p4-w1'].y)
  expect(four['p4-lead'].x).toBeLessThan(four['p4-w1'].x)
  expect(four['p4-w2'].y).toBeGreaterThan(four['p4-lead'].y)
  expect(four['p4-w2'].x).toBe(four['p4-w1'].x)
  expect(four['p4-shell'].x).toBeGreaterThan(four['p4-w1'].x)

  await page.getByTestId('session-t-five-button').click()
  await expect(stage).toHaveAttribute('data-layout', '"lead w1 w3" "lead w2 w4"')
  const five = await stage.locator('.pane-card').evaluateAll((cards) =>
    Object.fromEntries(
      cards.map((card) => {
        const bounds = card.getBoundingClientRect()
        return [
          card.dataset.paneId,
          { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height },
        ]
      }),
    ),
  )
  expect(five['p5-lead'].height).toBeGreaterThan(five['p5-w1'].height * 1.9)
  expect(five['p5-w1'].y).toBe(five['p5-w3'].y)
  expect(five['p5-w2'].y).toBeGreaterThan(five['p5-w1'].y)
  expect(five['p5-w1'].x).toBeLessThan(five['p5-w3'].x)
  expect(five['p5-w2'].x).toBe(five['p5-w1'].x)
})

test('renders session → lead → numbered workers and shells as a nested tree', async ({ page }) => {
  await boot(page)
  const session = page.getByTestId('session-t-four')
  const lead = session.locator(
    ':scope > [role="group"] > [data-kind="lead-group"] > [role="group"] > [data-kind="lead"]',
  )
  await expect(session).toContainText('harbour')
  await expect(lead).toContainText('harbour-lead')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('w1 nyx-coral-lane')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('w2 ares-amber-moss')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('shell')
})

test('renders a lead without workers as a leaf and keeps worker children grouped', async ({
  page,
}) => {
  const state = cannedState()
  state.tabs.push({
    id: 't-leaf',
    name: 'leaf',
    directory: '/work/leaf',
    closed: false,
    policy: 'auto',
    lead: { name: 'leaf-lead', harness: 'pi', generation: 4 },
    panes: [pane('p-leaf-lead', 'lead', { name: 'leaf-lead', harness: 'pi', generation: 4 })],
  })
  await boot(page, { state })

  const leaf = page.getByTestId('session-t-leaf')
  const leafLead = leaf.locator(
    ':scope > [role="group"] > [data-kind="lead-group"] > [role="group"] > [data-kind="lead"]',
  )
  await expect(leafLead).not.toHaveAttribute('aria-expanded')
  await expect(leafLead.locator(':scope > [role="group"]')).toHaveCount(0)

  const workerLead = page
    .getByTestId('session-t-four')
    .locator(
      ':scope > [role="group"] > [data-kind="lead-group"] > [role="group"] > [data-kind="lead"]',
    )
  await expect(workerLead).toHaveAttribute('aria-expanded', 'true')
  await expect(workerLead.locator(':scope > [role="group"]')).toContainText('w1 nyx-coral-lane')
})

test('session, lead, and worker nodes select the grid or one pane', async ({ page }) => {
  await boot(page)
  const stage = page.getByTestId('pane-stage')
  await page.getByTestId('session-t-four-button').click()
  await expect(stage.locator('.pane-card')).toHaveCount(4)
  await expect(stage).toHaveAttribute('data-mode', 'grid')

  await page.getByTestId('pane-node-p4-lead').click()
  await expect(stage.locator('.pane-card')).toHaveCount(1)
  await expect(stage.locator('.pane-title')).toContainText('harbour-lead')

  await page.getByTestId('pane-node-p4-w1').click()
  await expect(stage.locator('.pane-card')).toHaveCount(1)
  await expect(stage.locator('.pane-title')).toContainText('nyx-coral-lane')
})

test('keeps closed sessions greyed and resumes sessions or workers from the tree', async ({
  page,
}) => {
  await boot(page)
  await expect(page.getByTestId('session-t-closed')).toHaveAttribute('data-closed', 'true')
  await page.getByTestId('resume-t-closed').click()
  await expect.poll(async () => (await commandCalls(page, 'tab_resume')).length).toBe(1)

  await page.getByTestId('pane-node-pc-w1').click()
  const opens = await commandCalls(page, 'open_consult')
  expect(opens.at(-1)).toEqual({
    command: 'open_consult',
    args: {
      tab: 't-closed',
      agent: 'ares',
      conversation: 'ares-old-map',
      task: null,
    },
  })
})

test('keeps focus on the lead when a worker opens', async ({ page }) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  await page.getByRole('button', { name: 'New pane' }).click()
  await page.getByRole('menuitem', { name: 'Agent' }).click()
  await page
    .getByRole('dialog', { name: 'Open a worker pane' })
    .getByLabel('Agent')
    .selectOption('nyx')
  await page.getByLabel('Task').fill('Inspect the parser')
  await page.getByRole('button', { name: 'Open agent pane' }).click()

  await expect.poll(async () => (await commandCalls(page, 'open_consult')).length).toBe(1)
  await expect(page.getByTestId('pane-stage').locator('.pane-card')).toHaveCount(1)
  await expect(page.getByTestId('pane-stage').locator('.pane-title')).toContainText('harbour-lead')
})

test('explains worker reply delivery and its source in plain language', async ({ page }) => {
  await boot(page)
  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveText(
    'nyx-coral-lane · @nyx · Replies: Manual',
  )
  await expect(page.getByTestId('pane-p4-w2').locator('.pane-title')).toHaveText(
    'ares-amber-moss · @ares · Replies: Automatic',
  )
  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveAttribute(
    'title',
    /Reply delivery: Manual\. Set for this worker\./,
  )
  await expect(page.getByTestId('pane-p4-w2').locator('.pane-title')).toHaveAttribute(
    'title',
    /Reply delivery: Automatic\. Set for this session\./,
  )
  await expect(page.getByTestId('pane-p4-shell').locator('.pane-title')).toHaveText('shell')
})

test('uses the shared effective-policy rule for the tab manual veto and inheritance', async ({
  page,
}) => {
  const state = cannedState()
  const tab = state.tabs.find((candidate) => candidate.id === 't-four')
  const worker = tab.panes.find((candidate) => candidate.id === 'p4-w1')
  tab.policy = 'manual'
  worker.policy = 'auto'
  worker.effectivePolicy = { mode: 'auto', source: 'pane' }
  worker.notifyPreference = 'auto'
  await boot(page, { state })

  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveText(
    'nyx-coral-lane · @nyx · Replies: Manual',
  )
  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveAttribute(
    'title',
    /Set for this session\./,
  )

  await page.evaluate(async () => {
    const currentTab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    const currentWorker = currentTab.panes.find((candidate) => candidate.id === 'p4-w1')
    delete currentTab.policy
    currentWorker.policy = 'inherit'
    currentWorker.notifyPreference = 'manual'
    currentWorker.effectivePolicy = { mode: 'auto', source: 'default' }
    await window.__emitTauriEvent('state-changed', { reason: 'policy.set' })
  })
  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveText(
    'nyx-coral-lane · @nyx · Replies: Manual',
  )
  await expect(page.getByTestId('pane-p4-w1').locator('.pane-title')).toHaveAttribute(
    'title',
    /Requested by the lead\./,
  )
})

test('sets Auto, Manual, or Inherit on a worker and exposes tab policy in the header', async ({
  page,
}) => {
  await boot(page)
  for (const [label, mode] of [
    ['Automatic', 'auto'],
    ['Manual', 'manual'],
    ['Inherit session setting', 'inherit'],
  ]) {
    await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
    await expect(page.getByRole('menuitem', { name: label })).toBeVisible()
    await page.getByRole('menuitem', { name: label }).click()
    expect((await commandCalls(page, 'set_policy')).at(-1)).toEqual({
      command: 'set_policy',
      args: { scope: 'pane', id: 'p4-w1', mode },
    })
  }

  await page.getByTestId('tab-policy').click()
  await expect(page.getByRole('menu', { name: 'Session reply delivery' })).toContainText('Auto')
  await expect(page.getByRole('menu', { name: 'Session reply delivery' })).toContainText('Manual')
  await page.getByRole('menuitem', { name: 'Manual' }).click()
  expect((await commandCalls(page, 'set_policy')).at(-1)).toEqual({
    command: 'set_policy',
    args: { scope: 'tab', id: 't-four', mode: 'manual' },
  })
})

test('IMPL-PANE-142: shows startup failure without disabling its live terminal', async ({
  page,
}) => {
  const state = cannedState()
  const worker = state.tabs[0].panes.find((candidate) => candidate.id === 'p4-w2')
  worker.progress = {
    state: 'failed',
    pane: 'p4-w2',
    generation: 1,
    message: 'OpenCode task submission failed',
  }
  await boot(page, { state })

  const card = page.getByTestId('pane-p4-w2')
  await expect(card).toBeVisible()
  await expect(card).toContainText('Startup failed')
  await expect(card).toContainText('OpenCode task submission failed')
  await expect(card.locator('.xterm-helper-textarea')).toBeEnabled()

  await card.locator('.xterm-helper-textarea').focus()
  await page.keyboard.type('live after startup failure')
  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length)
    .toBeGreaterThan(0)
})

test('uses consistent session labels in the directory and harness flow', async ({ page }) => {
  await boot(page, { state: { ...cannedState(), tabs: [] } })
  await expect(
    page.getByText('Open a session to start a lead pane.', { exact: true }),
  ).toBeVisible()
  await expect(page.getByRole('button', { name: 'New session', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'New conversation', exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: 'New session', exact: true }).click()
  await expect.poll(async () => (await commandCalls(page, 'dialog.open')).length).toBe(1)
  await expect(page.getByRole('heading', { name: 'New session', exact: true })).toBeVisible()
  await page.getByLabel('Lead harness').selectOption('codex')
  await page.getByRole('button', { name: 'Start session', exact: true }).click()

  expect((await commandCalls(page, 'open_lead')).at(-1)).toEqual({
    command: 'open_lead',
    args: { dir: '/picked/workspace', harness: 'codex' },
  })
})

test('selects the new session in the same directory and focuses its own terminal', async ({
  page,
}) => {
  const state = cannedState()
  state.tabs[0].directory = '/picked/workspace'
  await boot(page, { state })
  await page.getByTestId('pane-node-p4-lead').click()
  for (const harness of ['pi', 'codex']) {
    await page.getByRole('button', { name: 'New session', exact: true }).click()
    await page.getByLabel('Lead harness').selectOption(harness)
    await page.evaluate((harness) => {
      const tab = {
        id: `fresh-${harness}`,
        name: `fresh-${harness}`,
        directory: '/picked/workspace',
        closed: false,
        lead: { harness, generation: 1 },
        panes: [{ id: `lead-${harness}`, generation: 1, kind: 'lead', order: 0, alive: true }],
      }
      window.__state.tabs.push(tab)
      window.__commandResults.open_lead = {
        ok: true,
        outcome: 'opened',
        tab: tab.id,
        pane: tab.panes[0],
      }
    }, harness)
    await page.getByRole('button', { name: 'Start session', exact: true }).click()
    await expect(page.locator('#current-session')).toHaveText(`fresh-${harness}`)
    await expect(page.getByTestId('pane-stage').locator('.pane-card')).toHaveCount(1)
    await expect(
      page.getByTestId(`pane-lead-${harness}`).locator('.xterm-helper-textarea'),
    ).toBeFocused()
  }
  await expect(page.getByTestId('pane-p4-lead')).toHaveAttribute('data-parked', 'true')
})

for (const outcome of ['opened', 'unknown']) {
  test(`selects a new session only after confirmed creation: ${outcome}`, async ({ page }) => {
    await boot(page)
    await page.getByRole('button', { name: 'New session', exact: true }).click()
    await page.getByLabel('Lead harness').selectOption('pi')
    await page.evaluate((outcome) => {
      if (outcome === 'opened') {
        window.__pendingListState = new Promise((release) => {
          window.__finishPendingListState = release
        })
        void window.__emitTauriEvent('state-changed')
      }
      const tab = {
        id: 'fresh-racing',
        name: 'fresh-racing',
        directory: '/picked/workspace',
        closed: false,
        lead: { harness: 'pi', generation: 1 },
        panes: [{ id: 'racing-lead', generation: 1, kind: 'lead', order: 0, alive: true }],
      }
      window.__state.tabs.push(tab)
      window.__commandResults.open_lead = { ok: true, outcome, tab: tab.id, pane: tab.panes[0] }
    }, outcome)
    await page.getByRole('button', { name: 'Start session', exact: true }).click()
    await expect.poll(async () => (await commandCalls(page, 'open_lead')).length).toBe(1)
    await page.evaluate(() => window.__finishPendingListState?.())
    await expect(page.getByTestId('session-fresh-racing-button')).toBeVisible()
    await expect(page.locator('#current-session')).toHaveText(
      outcome === 'opened' ? 'fresh-racing' : 'harbour',
    )
    const input = page.getByTestId('pane-racing-lead').locator('.xterm-helper-textarea')
    if (outcome === 'opened') await expect(input).toBeFocused()
    else await expect(input).not.toBeFocused()
  })
}

test('renames open and closed sessions through the accessible dialog and refreshes the header', async ({
  page,
}) => {
  await boot(page)

  const open = page.getByTestId('session-t-four')
  await open.getByRole('button', { name: 'Rename session' }).click()
  const dialog = page.getByRole('dialog', { name: 'Rename session' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByLabel('Session name')).toHaveValue('harbour')
  await dialog.getByLabel('Session name').fill('  Main Work  ')
  await dialog.getByRole('button', { name: 'Save name' }).click()
  await expect(page.getByTestId('session-t-four-button')).toHaveText('Main Work')
  await expect(page.locator('#current-session')).toHaveText('Main Work')
  expect((await commandCalls(page, 'rename_session')).at(-1)).toEqual({
    command: 'rename_session',
    args: { tab: 't-four', name: 'Main Work' },
  })

  const closed = page.getByTestId('session-t-closed')
  await closed.getByRole('button', { name: 'Rename session' }).click()
  const closedDialog = page.getByRole('dialog', { name: 'Rename session' })
  await expect(closedDialog).toBeVisible()
  await closedDialog.getByLabel('Session name').fill('Archive')
  await closedDialog.getByRole('button', { name: 'Save name' }).click()
  await expect(page.getByTestId('session-t-closed-button')).toHaveText('Archive')

  await open.getByRole('button', { name: 'Rename session' }).click()
  await expect(page.getByRole('dialog', { name: 'Rename session' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog', { name: 'Rename session' })).toBeHidden()
  expect((await commandCalls(page, 'rename_session')).at(-1)).toEqual({
    command: 'rename_session',
    args: { tab: 't-closed', name: 'Archive' },
  })
})

test('confirms active and closed session deletion and selects the next live session', async ({
  page,
}) => {
  await boot(page)

  const active = page.getByTestId('session-t-four')
  await active.getByRole('button', { name: 'Delete session' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete session' })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText('harbour')
  await expect(dialog).toContainText(/stops all its panes and removes it from the app/i)
  await expect(dialog).toContainText(/project files and native histories remain/i)
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).toBeHidden()
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(0)

  await active.getByRole('button', { name: 'Delete session' }).click()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(0)

  await active.getByRole('button', { name: 'Delete session' }).click()
  await dialog.getByRole('button', { name: 'Delete session', exact: true }).click()
  await expect.poll(async () => (await commandCalls(page, 'tab_delete')).length).toBe(1)
  expect((await commandCalls(page, 'tab_delete')).at(-1)).toEqual({
    command: 'tab_delete',
    args: { tab: 't-four', generation: 1 },
  })
  await expect(page.getByTestId('session-t-four')).toHaveCount(0)
  await expect(page.locator('#current-session')).toHaveText('foundry')
  await expect(page.getByTestId('session-t-five-button')).toHaveAttribute('aria-current', 'true')

  const closed = page.getByTestId('session-t-closed')
  await closed.getByRole('button', { name: 'Delete session' }).click()
  const closedDialog = page.getByRole('dialog', { name: 'Delete session' })
  await closedDialog.getByRole('button', { name: 'Delete session', exact: true }).click()
  await expect.poll(async () => (await commandCalls(page, 'tab_delete')).length).toBe(2)
  expect((await commandCalls(page, 'tab_delete')).at(-1)).toEqual({
    command: 'tab_delete',
    args: { tab: 't-closed', generation: 2 },
  })
  await expect(page.getByTestId('session-t-closed')).toHaveCount(0)
  await expect(page.locator('#current-session')).toHaveText('foundry')
  await expect(page.getByTestId('session-t-five')).toBeVisible()
})

test('keeps session deletion single-flight and reports a stale failure', async ({ page }) => {
  await boot(page)
  await page.evaluate(() => window.__gateTabDelete())

  const active = page.getByTestId('session-t-four')
  await active.getByRole('button', { name: 'Delete session' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete session' })
  const confirm = dialog.locator('button[type="submit"]')
  await confirm.click()
  await expect(confirm).toBeDisabled()
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(1)
  await page.evaluate(() =>
    document.querySelector('#delete-session-dialog button[type="submit"]').click(),
  )
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(1)
  await page.evaluate(() => window.__releaseTabDelete())
  await expect(dialog).toBeHidden()
  await expect(page.getByTestId('session-t-four')).toHaveCount(0)

  await page.evaluate(() =>
    window.__setCommandResult('tab_delete', { ok: false, error: 'stale tab generation' }),
  )
  const next = page.getByTestId('session-t-five')
  await next.getByRole('button', { name: 'Delete session' }).click()
  const failureDialog = page.getByRole('dialog', { name: 'Delete session' })
  const failureConfirm = failureDialog.getByRole('button', {
    name: 'Delete session',
    exact: true,
  })
  await failureConfirm.click()
  await expect(page.getByRole('status')).toContainText('stale tab generation')
  await expect(failureDialog).toBeVisible()
  await expect(failureConfirm).toBeEnabled()
  await expect(page.getByTestId('session-t-five')).toBeVisible()
})

test('locks the confirmed delete target while delayed IPC prevents cancel or retargeting', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() => window.__gateTabDelete())

  const active = page.getByTestId('session-t-four')
  const next = page.getByTestId('session-t-five')
  await active.getByRole('button', { name: 'Delete session' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete session' })
  const cancel = dialog.getByRole('button', { name: 'Cancel' })
  const confirm = dialog.locator('button[type="submit"]')
  await confirm.click()
  await expect(confirm).toBeDisabled()
  await expect(cancel).toBeDisabled()
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(1)

  await page.keyboard.press('Escape')
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText('harbour')

  await next.getByRole('button', { name: 'Delete session' }).click({ force: true })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText('harbour')
  await expect(dialog).not.toContainText('foundry')
  expect(await commandCalls(page, 'tab_delete')).toHaveLength(1)

  await page.evaluate(() => window.__releaseTabDelete())
  await expect(dialog).toBeHidden()
  expect((await commandCalls(page, 'tab_delete')).at(-1)).toEqual({
    command: 'tab_delete',
    args: { tab: 't-four', generation: 1 },
  })
  await expect(page.getByTestId('session-t-five')).toBeVisible()
})

test('offers only canonical lead harness ids accepted by the tab store', async ({ page }) => {
  await boot(page)
  const harnesses = await page
    .locator('#lead-harness option')
    .evaluateAll((options) => options.map((option) => option.value))
  expect(harnesses).toEqual(['claude-code', 'codex', 'pi', 'opencode'])
  for (const harness of harnesses) {
    const tabs = new Tabs(inMemoryTabStore())
    await expect(tabs.create('/tmp/picker-contract', harness)).resolves.toMatchObject({
      generation: 1,
    })
  }
})

test('opens Shell or Agent from the human New pane menu', async ({ page }) => {
  await boot(page)
  await page.getByRole('button', { name: 'New pane' }).click()
  await page.getByRole('menuitem', { name: 'Shell' }).click()
  expect((await commandCalls(page, 'open_shell')).at(-1)).toEqual({
    command: 'open_shell',
    args: { tab: 't-four' },
  })

  await page.getByRole('button', { name: 'New pane' }).click()
  await page.getByRole('menuitem', { name: 'Agent' }).click()
  await page
    .getByRole('dialog', { name: 'Open a worker pane' })
    .getByLabel('Agent')
    .selectOption('ares')
  await page.getByLabel('Task').fill('Trace delivery readiness')
  await page.getByRole('button', { name: 'Open agent pane' }).click()
  expect((await commandCalls(page, 'open_consult')).at(-1)).toEqual({
    command: 'open_consult',
    args: { tab: 't-four', agent: 'ares', task: 'Trace delivery readiness' },
  })
})

test('TEST-PANE-135: offers a clear close action on every pane, including failed workers', async ({
  page,
}) => {
  const state = cannedState()
  const failed = state.tabs[0].panes.find((pane) => pane.id === 'p4-w1')
  failed.alive = false
  failed.failure = { message: 'worker harness failed', exitCode: 23 }
  await boot(page, { state })

  await expect(
    page.getByTestId('pane-p4-lead').getByRole('button', { name: 'Suspend lead' }),
  ).toBeVisible()
  for (const id of ['p4-w1', 'p4-w2', 'p4-shell']) {
    await expect(
      page.getByTestId(`pane-${id}`).getByRole('button', { name: 'Close pane' }),
    ).toBeVisible()
  }
})

test('TEST-PANE-135: closes worker and shell panes by their generation without touching siblings or history', async ({
  page,
}) => {
  await boot(page)

  await page.getByTestId('pane-p4-w1').getByRole('button', { name: 'Close pane' }).click()
  await expect(page.getByTestId('pane-p4-w1')).toHaveCount(0)
  expect((await commandCalls(page, 'close_pane')).at(-1)).toEqual({
    command: 'close_pane',
    args: { id: 'p4-w1', generation: 1 },
  })
  for (const id of ['p4-lead', 'p4-w2', 'p4-shell']) {
    await expect(page.getByTestId(`pane-${id}`)).toBeVisible()
  }
  await expect(page.getByTestId('pane-node-p4-w2')).toBeVisible()
  expect(await page.evaluate(() => window.__state.answers['nyx-coral-lane'])).toHaveLength(3)

  await page.getByTestId('pane-p4-shell').getByRole('button', { name: 'Close pane' }).click()
  await expect(page.getByTestId('pane-p4-shell')).toHaveCount(0)
  expect((await commandCalls(page, 'close_pane')).at(-1)).toEqual({
    command: 'close_pane',
    args: { id: 'p4-shell', generation: 1 },
  })
  for (const id of ['p4-lead', 'p4-w2']) {
    await expect(page.getByTestId(`pane-${id}`)).toBeVisible()
  }
})

test('TEST-PANE-135: closing the lead suspends only its session and exposes Resume', async ({
  page,
}) => {
  await boot(page)

  await page.getByTestId('pane-p4-lead').getByRole('button', { name: 'Suspend lead' }).click()
  await expect(page.getByTestId('pane-p4-lead')).toHaveCount(0)
  await expect(page.getByTestId('pane-stage')).toContainText(
    'Resume this session to reopen its lead.',
  )
  await expect(page.getByTestId('resume-t-four')).toBeVisible()
  expect((await commandCalls(page, 'close_pane')).at(-1)).toEqual({
    command: 'close_pane',
    args: { id: 'p4-lead', generation: 1 },
  })
  expect(
    await page.evaluate(() => {
      const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
      return {
        closed: tab.closed,
        workerAlive: tab.panes.find((pane) => pane.id === 'p4-w1')?.alive,
      }
    }),
  ).toEqual({ closed: true, workerAlive: true })
})

test('TEST-PANE-136: uses the clicked generation and keeps a stale close error visible', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() => {
    window.__state.tabs[0].panes.find((pane) => pane.id === 'p4-w2').generation = 2
    window.__setCommandResult('close_pane', { ok: false, error: 'stale pane generation' })
  })

  await page.getByTestId('pane-p4-w2').getByRole('button', { name: 'Close pane' }).click()
  await expect(page.getByRole('status')).toContainText('stale pane generation')
  await expect(page.getByTestId('pane-p4-w2')).toBeVisible()
  expect((await commandCalls(page, 'close_pane')).at(-1)).toEqual({
    command: 'close_pane',
    args: { id: 'p4-w2', generation: 1 },
  })
})

test('TEST-PANE-136: closes a failed worker through the same pane action', async ({ page }) => {
  const state = cannedState()
  const failed = state.tabs[0].panes.find((pane) => pane.id === 'p4-w1')
  failed.alive = false
  failed.failure = { message: 'worker harness failed', exitCode: 23 }
  await boot(page, { state })

  await expect(page.getByTestId('pane-p4-w1')).toHaveAttribute('data-state', 'failed')
  await page.getByTestId('pane-p4-w1').getByRole('button', { name: 'Close pane' }).click()
  await expect(page.getByTestId('pane-p4-w1')).toHaveCount(0)
  expect((await commandCalls(page, 'close_pane')).at(-1)).toEqual({
    command: 'close_pane',
    args: { id: 'p4-w1', generation: 1 },
  })
})

test('keeps actual xterm terminals full height with hidden and visible failure banners', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1100 })
  const state = cannedState()
  state.results = []
  await boot(page, { state })

  const stage = page.getByTestId('pane-stage')
  const lead = page.getByTestId('pane-p4-lead')
  const metrics = () =>
    lead.evaluate((card) => {
      const bounds = (selector) => card.querySelector(selector).getBoundingClientRect()
      const host = bounds('.terminal-host')
      const hostElement = card.querySelector('.terminal-host')
      const hostStyles = getComputedStyle(hostElement)
      const xterm = card.querySelector('.xterm')
      const screen = bounds('.xterm-screen')
      const rows = card.querySelector('.xterm-rows')
      const firstRow = rows?.firstElementChild
      const failure = card.querySelector('.pane-failure')
      return {
        cardBottom: card.getBoundingClientRect().bottom,
        titleBottom: bounds('.pane-titlebar').bottom,
        failureBottom: failure.getBoundingClientRect().bottom,
        failureDisplay: getComputedStyle(failure).display,
        hostTop: host.top,
        hostBottom: host.bottom,
        availableHeight:
          hostElement.clientHeight -
          Number.parseFloat(hostStyles.paddingTop) -
          Number.parseFloat(hostStyles.paddingBottom),
        xtermBottom: xterm.getBoundingClientRect().bottom,
        screenHeight: screen.height,
        screenBottom: screen.bottom,
        rowCount: rows?.children.length ?? 0,
        rowHeight: firstRow === null ? 0 : firstRow.getBoundingClientRect().height,
      }
    })
  const latestResizeRows = () =>
    commandCalls(page, 'pane_resize').then(
      (calls) => calls.filter((call) => call.args.id === 'p4-lead').at(-1)?.args.rows ?? 0,
    )
  const assertTerminalFillsCard = async (expectedFailureDisplay) => {
    // Native resize stops after exit; wait for the emulator's own next-frame fit.
    await expect
      .poll(async () => {
        const current = await metrics()
        return Math.abs(current.screenHeight - current.availableHeight) <= current.rowHeight
      })
      .toBe(true)
    const current = await metrics()
    const resizeRows = await latestResizeRows()
    expect(current.failureDisplay).toBe(expectedFailureDisplay)
    expect(
      Math.abs(current.hostBottom - current.cardBottom),
      JSON.stringify(current),
    ).toBeLessThanOrEqual(1)
    expect(current.hostTop).toBeGreaterThanOrEqual(current.titleBottom)
    if (expectedFailureDisplay !== 'none') {
      expect(current.hostTop).toBeGreaterThanOrEqual(current.failureBottom)
    }
    expect(current.xtermBottom).toBeLessThanOrEqual(current.hostBottom)
    expect(
      Math.abs(current.screenHeight - current.rowCount * current.rowHeight),
      JSON.stringify(current),
    ).toBeLessThanOrEqual(1)
    expect(
      Math.abs(current.screenHeight - current.availableHeight),
      JSON.stringify(current),
    ).toBeLessThanOrEqual(current.rowHeight)
    if (expectedFailureDisplay === 'none')
      expect(
        Math.abs(resizeRows - current.rowCount),
        JSON.stringify({ current, resizeRows }),
      ).toBeLessThanOrEqual(1)
    expect(resizeRows).toBeGreaterThan(0)
    expect(current.rowHeight).toBeGreaterThan(0)
  }

  await expect(lead).toBeVisible()
  await expect.poll(latestResizeRows).toBeGreaterThan(0)
  await assertTerminalFillsCard('none')
  const gridResizeRows = await latestResizeRows()

  await page.getByTestId('pane-node-p4-lead').click()
  await expect(stage).toHaveAttribute('data-mode', 'focused')
  await expect.poll(latestResizeRows).toBeGreaterThanOrEqual(gridResizeRows)
  await assertTerminalFillsCard('none')
  const focusedResizeRows = await latestResizeRows()

  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    const pane = tab.panes.find((candidate) => candidate.id === 'p4-lead')
    pane.alive = false
    pane.failure = { message: 'lead harness failed', exitCode: 17 }
    await window.__emitTauriEvent('state-changed', { reason: 'lead.failed' })
  })
  await expect(lead).toHaveAttribute('data-state', 'failed')
  await expect(lead).toContainText('lead harness failed')
  await expect.poll(latestResizeRows).toBeGreaterThanOrEqual(focusedResizeRows)
  await assertTerminalFillsCard('flex')
})

for (const count of [2, 3, 5, 7, 10, 20]) {
  test(`session geometry keeps ${count} panes in fixed-height horizontal columns`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 700 })
    await boot(page, { state: gridState(count) })
    const stage = page.getByTestId('pane-stage')
    await expect(stage.locator('.pane-card')).toHaveCount(count)
    const geometry = await stage.evaluate((element) => {
      const lead = element.querySelector('.pane-card').getBoundingClientRect()
      const workers = element.querySelector('.worker-stage')
      const worker = workers.querySelector('.pane-card').getBoundingClientRect()
      const bounds = workers.getBoundingClientRect()
      const visible = [...workers.children].filter((card) => {
        const rect = card.getBoundingClientRect()
        return rect.left < bounds.right - 1 && rect.right > bounds.left + 1
      }).length
      return {
        ratio: lead.width / worker.width,
        visible,
        height: element.clientHeight,
        scrollHeight: element.scrollHeight,
        overflow: workers.scrollWidth > workers.clientWidth + 1,
      }
    })
    expect(geometry.ratio).toBeCloseTo(2, 2)
    expect(geometry.visible).toBeLessThanOrEqual(4)
    expect(geometry.scrollHeight).toBeLessThanOrEqual(geometry.height + 1)
    expect(geometry.overflow).toBe(count > 5)
    await page.setViewportSize({ width: 650, height: 470 })
    await expect(stage.locator('.pane-card')).toHaveCount(count)
    expect(await stage.evaluate((e) => e.scrollHeight <= e.clientHeight + 1)).toBe(true)
  })
}

test('keeps a compact pending-result count within each pane and the page width', async ({
  page,
}) => {
  const state = cannedState()
  state.results = Array.from({ length: 20 }, (_, index) => ({
    id: `d-${index + 1}`,
    conversation: 'nyx-coral-lane',
    agent: 'nyx',
    parts: 1,
    receivedParts: 0,
    tab: 't-four',
    pane: 'p4-w1',
    generation: 1,
    state: 'waiting',
    reason: `result ${index} with a very long reason`,
  }))
  await page.setViewportSize({ width: 760, height: 600 })
  await boot(page, { state })
  const summary = page.getByTestId('result-summary-p4-w1')
  await expect(summary).toHaveText('20 unconfirmed results')
  const bounds = await summary.boundingBox()
  const pane = await page.getByTestId('pane-p4-w1').boundingBox()
  const pageWidth = await page.evaluate(() => document.documentElement.scrollWidth)
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(pane.x + pane.width)
  expect(pageWidth).toBeLessThanOrEqual(760)
})

test('keeps a hidden tab emulator alive so a canned flood drains and acks after writes', async ({
  page,
}) => {
  await boot(page, { emulatorGate: true })
  await page.getByTestId('session-t-four-button').click()
  await page.evaluate(() => window.__emitPaneFlood('p5-w1', 1, 160))
  await page.waitForTimeout(50)
  expect(
    (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p5-w1'),
  ).toHaveLength(0)
  await expect(page.getByTestId('pane-p5-w1').locator('.terminal-host')).toHaveAttribute(
    'data-consumed',
    '',
  )

  await page.evaluate(() => window.__releaseOneParserWrite())
  await expect
    .poll(async () => {
      const calls = await commandCalls(page, 'pane_ack')
      return calls.filter((call) => call.args.id === 'p5-w1').length
    })
    .toBe(1)
  await expect(page.getByTestId('pane-p5-w1').locator('.terminal-host')).toHaveAttribute(
    'data-consumed',
    /hidden 1/,
  )

  await page.evaluate(() => window.__releaseParserGate())
  await expect
    .poll(async () => {
      const calls = await commandCalls(page, 'pane_ack')
      return calls.filter((call) => call.args.id === 'p5-w1').length
    })
    .toBe(160)
  const acks = (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p5-w1')
  expect(acks.at(-1).args).toEqual({ id: 'p5-w1', generation: 1, seq: 160 })
  await expect(page.getByTestId('pane-p5-w1').locator('.terminal-host')).toHaveAttribute(
    'data-consumed',
    /hidden 160/,
  )
  await expect(page.getByTestId('pane-p5-w1')).toHaveAttribute('data-parked', 'true')
})

test('routes terminal replies separately while keyboard, paste, and IME remain human input', async ({
  page,
}) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()

  await page.evaluate(() => {
    window.__emitPaneOutput('p4-lead', 1, 1, [0x1b, 0x5b, 0x35, 0x6e])
  })
  await expect.poll(async () => (await commandCalls(page, 'pane_reply_enqueue')).length).toBe(1)
  expect(await commandCalls(page, 'pane_input_enqueue')).toHaveLength(0)

  const textarea = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await textarea.focus()
  await page.keyboard.type('K')
  await textarea.evaluate((element) => {
    const transfer = new DataTransfer()
    transfer.setData('text/plain', 'PASTE')
    element.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, clipboardData: transfer }))
    element.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }))
    element.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data: '漢' }))
    element.value = '漢'
    element.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '漢' }))
    element.dispatchEvent(
      new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: '漢' }),
    )
  })

  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length)
    .toBeGreaterThan(2)
  const humanBytes = (await commandCalls(page, 'pane_input_enqueue')).flatMap(
    (call) => call.args.bytes,
  )
  expect(new TextDecoder().decode(new Uint8Array(humanBytes))).toContain('K')
  expect(new TextDecoder().decode(new Uint8Array(humanBytes))).toContain('PASTE')
  expect(new TextDecoder().decode(new Uint8Array(humanBytes))).toContain('漢')
  const admitted = await page.evaluate(() =>
    window.__calls.filter(
      (call) => call.command === 'pane_input_enqueue' || call.command === 'pane_reply_enqueue',
    ),
  )
  expect(admitted.map((call) => call.args.sequence)).toEqual(admitted.map((_, index) => index + 1))
  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_wait')).length)
    .toBe(admitted.length)
})

test('keeps a human keystroke human while an xterm parser write is queued', async ({ page }) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()

  await page.evaluate(
    () =>
      new Promise((resolve) => {
        window.__emitPaneOutput('p4-lead', 1, 1, [...new TextEncoder().encode('queued output')])
        queueMicrotask(() => {
          const textarea = document.querySelector(
            '[data-testid="pane-p4-lead"] .xterm-helper-textarea',
          )
          const keydown = new KeyboardEvent('keydown', {
            bubbles: true,
            cancelable: true,
            code: 'KeyQ',
            key: 'q',
          })
          Object.defineProperty(keydown, 'keyCode', { value: 81 })
          textarea.dispatchEvent(keydown)
          resolve()
        })
      }),
  )

  await expect.poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length).toBe(1)
  const humanBytes = (await commandCalls(page, 'pane_input_enqueue')).flatMap(
    (call) => call.args.bytes,
  )
  expect(new TextDecoder().decode(new Uint8Array(humanBytes))).toBe('q')
  expect(await commandCalls(page, 'pane_reply_enqueue')).toHaveLength(0)
})

test('keeps the injectable Emulator contract to write, onData, resize, and dispose', async ({
  page,
}) => {
  await boot(page)
  const result = await page.evaluate(async () => {
    const { EmulatorRegistry } = await import('/term.js')
    const emulator = {
      async write() {},
      onData() {
        return { dispose() {} }
      },
      resize() {},
      dispose() {},
    }
    const registry = new EmulatorRegistry({
      createEmulator: () => emulator,
      onData() {},
      onReply() {},
      onResize() {},
    })
    try {
      registry.ensure({ id: 'contract-pane', generation: 1 }, document.createElement('div'))
      registry.fit('contract-pane', 1)
      registry.dispose()
      return { ok: true }
    } catch (error) {
      return { ok: false, error: String(error) }
    }
  })
  expect(result).toEqual({ ok: true })
})

test('reconciles Node state changes and consumes output for a worker the page has not seen', async ({
  page,
}) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  await page.evaluate(() => window.__openExternalWorker())

  await expect(page.getByTestId('pane-node-p4-w3')).toContainText('w3 clio-external-worker')
  await expect(page.getByTestId('pane-stage').locator('.pane-title')).toContainText('harbour-lead')
  await expect(page.getByTestId('result-summary-p4-lead')).toHaveText('2 unconfirmed results')
  await page.getByTestId('result-summary-p4-lead').click()
  await expect(page.getByRole('dialog', { name: 'Lead results' })).toContainText(
    'worker reply ready',
  )
  await expect
    .poll(async () => {
      const calls = await commandCalls(page, 'pane_ack')
      return calls.filter((call) => call.args.id === 'p4-w3').length
    })
    .toBe(1)
  await expect(page.getByTestId('pane-p4-w3').locator('.xterm-rows')).toContainText(
    'external worker output',
  )
  await page.evaluate(async () => {
    await window.__emitTauriEvent('state-changed', { reason: 'second refresh' })
    window.__emitPaneOutput('p4-w3', 1, 2, [...new TextEncoder().encode('still connected\r\n')])
  })
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-w3').length,
    )
    .toBe(2)
  await page.keyboard.press('Escape')
  await page.getByTestId('pane-node-p4-w3').click()
  await expect(page.getByTestId('pane-p4-w3').locator('.xterm-rows')).toContainText(
    'still connected',
  )
  expect(await commandCalls(page, 'subscribe_output')).toHaveLength(1)
  expect((await commandCalls(page, 'list_state')).length).toBeGreaterThanOrEqual(3)
})

test('sends the latest terminal size only after the native pane becomes live', async ({ page }) => {
  const state = gridState(1)
  const pane = state.tabs[0].panes[0]
  pane.alive = false
  pane.starting = true
  await boot(page, { state })
  const rows = page.getByTestId(`pane-${pane.id}`).locator('.xterm-rows > div')
  await expect.poll(() => rows.count()).toBeGreaterThan(24)
  expect(await commandCalls(page, 'pane_resize')).toEqual([])
  await page.setViewportSize({ width: 1500, height: 900 })
  await expect.poll(() => rows.count()).toBeGreaterThan(40)
  expect(await commandCalls(page, 'pane_resize')).toEqual([])
  await page.evaluate(async () => {
    window.__state.tabs[0].panes[0].alive = true
    delete window.__state.tabs[0].panes[0].starting
    await window.__emitTauriEvent('state-changed', { reason: 'native launch resolved' })
  })
  await expect
    .poll(async () => (await commandCalls(page, 'pane_resize')).at(-1)?.args.rows)
    .toBe(await rows.count())
  const count = (await commandCalls(page, 'pane_resize')).length
  await page.evaluate(async () => {
    await window.__emitTauriEvent('state-changed', { reason: 'unchanged state' })
    await window.__emitTauriEvent('state-changed', { reason: 'unchanged state again' })
  })
  expect((await commandCalls(page, 'pane_resize')).length).toBe(count)
  await expect(page.locator('#status')).toBeHidden()
})

test('does not show an internal pane-not-open error when a resize races pane lifetime', async ({
  page,
}) => {
  const state = gridState(1)
  const pane = state.tabs[0].panes[0]
  await boot(page, { state })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBeGreaterThan(0)
  await page.evaluate((pane) => {
    window.__calls = []
    window.__commandResults.pane_resize = {
      ok: false,
      error: `pane ${pane.id} generation ${pane.generation} is not open`,
    }
  }, pane)
  await page.setViewportSize({ width: 1500, height: 900 })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBeGreaterThan(0)
  await expect(page.locator('#status')).toBeHidden()
  await page.evaluate(async () => {
    delete window.__commandResults.pane_resize
    window.__calls = []
    await window.__emitTauriEvent('state-changed', { reason: 'native pane ready' })
  })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBe(1)
  await page.evaluate(() => {
    window.__commandResults.pane_resize = { ok: false, error: 'PTY I/O error: resize failed' }
  })
  await page.setViewportSize({ width: 1600, height: 950 })
  await expect(page.locator('#status')).toHaveText('PTY I/O error: resize failed')
})

test('serializes native terminal sizes and applies the latest geometry after an in-flight resize', async ({
  page,
}) => {
  await boot(page, { state: gridState(1) })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBeGreaterThan(0)
  await page.evaluate(() => {
    window.__calls = []
    window.__resizeGate = new Promise((resolve) => {
      window.__releaseResize = resolve
    })
  })
  await page.setViewportSize({ width: 1500, height: 850 })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBe(1)
  await page.setViewportSize({ width: 1600, height: 950 })
  await expect.poll(() => page.locator('.xterm-rows > div').count()).toBeGreaterThan(50)
  expect((await commandCalls(page, 'pane_resize')).length).toBe(1)
  await page.evaluate(() => {
    delete window.__resizeGate
    window.__releaseResize()
  })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBe(2)
  const latest = (await commandCalls(page, 'pane_resize')).at(-1).args
  expect(latest.rows).toBe(await page.locator('.xterm-rows > div').count())
})

test('discards queued terminal sizes when their pane closes during resize', async ({ page }) => {
  await boot(page, { state: gridState(1) })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBeGreaterThan(0)
  await page.evaluate(() => {
    window.__calls = []
    window.__resizeGate = new Promise((resolve) => {
      window.__releaseResize = resolve
    })
  })
  await page.setViewportSize({ width: 1500, height: 850 })
  await expect.poll(async () => (await commandCalls(page, 'pane_resize')).length).toBe(1)
  await page.setViewportSize({ width: 1600, height: 950 })
  await expect.poll(() => page.locator('.xterm-rows > div').count()).toBeGreaterThan(50)
  await page.evaluate(async () => {
    window.__state.tabs[0].closed = true
    await window.__emitTauriEvent('state-changed', { reason: 'pane exited' })
    delete window.__resizeGate
    window.__releaseResize()
  })
  await expect(page.locator('.xterm')).toHaveCount(0)
  expect((await commandCalls(page, 'pane_resize')).length).toBe(1)
  await expect(page.locator('#status')).toBeHidden()
})

test('preserves startup output while the lead has not been marked alive yet', async ({ page }) => {
  const state = gridState(1)
  state.tabs[0].panes[0].alive = false
  state.tabs[0].panes[0].starting = true
  const id = state.tabs[0].panes[0].id
  await boot(page, { state })
  await page.evaluate(async (id) => {
    window.__emitPaneOutput(id, 1, 1, [...new TextEncoder().encode('startup banner\r\n')])
    await window.__emitTauriEvent('state-changed', { reason: 'launch still pending' })
  }, id)
  await expect.poll(async () => (await commandCalls(page, 'pane_ack')).length).toBe(1)
  await page.evaluate(async () => {
    window.__state.tabs[0].panes[0].alive = true
    delete window.__state.tabs[0].panes[0].starting
    await window.__emitTauriEvent('state-changed', { reason: 'launch resolved' })
  })
  await expect(page.getByTestId(`pane-${id}`).locator('.xterm-rows')).toContainText(
    'startup banner',
  )
})

test('keeps an unseen pane emulator across reconciliation until state adopts it', async ({
  page,
}) => {
  await boot(page, { emulatorGate: true })
  await page.evaluate(async () => {
    window.__emitPaneOutput('p4-w3', 1, 1, [...new TextEncoder().encode('before state\r\n')])
    await window.__emitTauriEvent('state-changed', { reason: 'unrelated' })
  })
  await page.waitForTimeout(50)
  await page.evaluate(() => window.__releaseOneParserWrite())
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-w3').length,
    )
    .toBe(1)

  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    tab.panes.push({
      id: 'p4-w3',
      generation: 1,
      kind: 'worker',
      order: 4,
      conversation: 'clio-external-worker',
      agent: 'clio',
      alive: true,
    })
    await window.__emitTauriEvent('state-changed', { reason: 'pane.open' })
  })
  await expect(page.getByTestId('pane-node-p4-w3')).toBeVisible()
  await page.evaluate(() => {
    window.__emitPaneOutput('p4-w3', 1, 2, [...new TextEncoder().encode('after state\r\n')])
    window.__releaseParserGate()
  })
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-w3').length,
    )
    .toBe(2)
  await expect(page.getByTestId('pane-p4-w3')).toHaveCount(1)
  await expect(page.getByTestId('pane-p4-w3').locator('.terminal-host')).toHaveAttribute(
    'data-consumed',
    /before state[\s\S]*after state/,
  )
})

test('retains failed worker diagnostics without terminal actions while live workers stay interactive', async ({
  page,
}) => {
  await boot(page)
  const failed = page.getByTestId('pane-p4-w1')

  await page.evaluate(() => {
    window.__emitPaneOutput('p4-w1', 1, 1, [
      ...new TextEncoder().encode('scrollback before failure\r\n'),
    ])
  })
  await expect(failed.locator('.xterm-rows')).toContainText('scrollback before failure')
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-w1').length,
    )
    .toBe(1)

  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    const pane = tab.panes.find((candidate) => candidate.id === 'p4-w1')
    pane.alive = false
    pane.failure = { message: 'worker harness failed', exitCode: 23 }
    await window.__emitTauriEvent('state-changed', { reason: 'worker.failed' })
  })

  await expect(failed).toHaveAttribute('data-state', 'failed')
  await expect(failed).toContainText('worker harness failed')
  await expect(failed).toContainText('23')
  await expect(failed).toContainText('Ended')
  await expect(failed.locator('.xterm-rows')).toContainText('scrollback before failure')
  const failedNode = page.getByTestId('pane-node-p4-w1').locator('../..')
  await expect(failedNode).toHaveAttribute('data-state', 'failed')
  await expect(failedNode).toBeVisible()
  await page.evaluate(async () => {
    window.__state.tabs[0].panes.find((pane) => pane.id === 'p4-w1').failure.exitCode = null
    await window.__emitTauriEvent('state-changed', { reason: 'worker.signal' })
  })
  await expect(failed).toContainText('Exit code: unknown')
  await expect(failed.locator('.xterm-rows')).toContainText('scrollback before failure')
  expect(await commandCalls(page, 'subscribe_output')).toHaveLength(1)

  const beforeFailedActions = await page.evaluate(
    () =>
      window.__calls.filter(
        (call) =>
          call.command === 'open_consult' ||
          call.command === 'pane_input_enqueue' ||
          call.command === 'pane_reply_enqueue',
      ).length,
  )
  await page.getByTestId('pane-node-p4-w1').click()
  await expect(failed).toHaveAttribute('data-selected', 'true')
  await expect(page.getByTestId('pane-stage')).toContainText('worker harness failed')

  await failed.locator('.pane-titlebar').click({ button: 'right' })
  await expect(page.getByRole('menu', { name: 'Worker actions' })).toHaveCount(0)
  await failed.locator('.xterm-helper-textarea').focus()
  await page.keyboard.type('do not send')
  await page.evaluate(() => {
    window.__emitPaneOutput('p4-w1', 1, 2, [
      ...new TextEncoder().encode('output after failure\r\n\u001b[5n'),
    ])
  })
  await expect(failed.locator('.xterm-rows')).toContainText('output after failure')
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-w1').length,
    )
    .toBe(2)
  await expect
    .poll(
      async () =>
        await page.evaluate(
          () =>
            window.__calls.filter(
              (call) =>
                call.command === 'open_consult' ||
                call.command === 'pane_input_enqueue' ||
                call.command === 'pane_reply_enqueue',
            ).length,
        ),
    )
    .toBe(beforeFailedActions)

  await page.getByTestId('pane-node-p4-w2').click()
  const liveInput = page.getByTestId('pane-p4-w2').locator('.xterm-helper-textarea')
  await liveInput.focus()
  await page.keyboard.type('live worker input')
  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length)
    .toBeGreaterThan(0)
  await page.getByTestId('pane-p4-w2').locator('.pane-titlebar').click({ button: 'right' })
  await expect(page.getByRole('menu', { name: 'Worker actions' })).toBeVisible()
})

test('removes an obsolete failed row when its conversation is admitted again and hides it in closed sessions', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    const pane = tab.panes.find((candidate) => candidate.id === 'p4-w1')
    pane.alive = false
    pane.failure = { message: 'worker harness failed', exitCode: 23 }
    await window.__emitTauriEvent('state-changed', { reason: 'worker.failed' })
  })
  await expect(page.getByTestId('pane-p4-w1')).toBeVisible()

  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    tab.panes = tab.panes.filter((pane) => pane.id !== 'p4-w1')
    tab.panes.push({
      id: 'p4-w1-reopened',
      generation: 1,
      kind: 'worker',
      order: 1,
      conversation: 'nyx-coral-lane',
      agent: 'nyx',
      effectivePolicy: { mode: 'manual', source: 'pane' },
      alive: true,
    })
    await window.__emitTauriEvent('state-changed', { reason: 'worker.reopened' })
  })
  await expect(page.getByTestId('pane-p4-w1')).toHaveCount(0)
  await expect(page.getByTestId('pane-p4-w1-reopened')).toBeVisible()
  await expect(page.getByTestId('pane-node-p4-w1-reopened')).toBeVisible()

  await page.getByTestId('session-t-closed-button').click()
  await expect(page.getByTestId('session-t-closed')).toBeVisible()
  await expect(page.getByTestId('pane-stage')).toContainText(
    'Resume this session to reopen its lead.',
  )
  await expect(page.getByTestId('pane-stage').locator('.pane-card')).toHaveCount(0)
})

test('drains and retires an unseen pane that exited before the first snapshot', async ({
  page,
}) => {
  const state = cannedState()
  state.tabs[0].panes.push(
    pane('p4-exit', 'worker', {
      order: 4,
      conversation: 'nyx-brief-life',
      agent: 'nyx',
      alive: false,
    }),
  )
  await page.route(`${ROSTER_ORIGIN}/**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<main>Roster connected</main>' }),
  )
  await installTauriShim(page, { state, emulatorGate: true, listStateGate: true })
  await page.goto(origin)
  await expect.poll(async () => (await commandCalls(page, 'list_state')).length).toBe(1)

  await page.evaluate(() =>
    window.__emitPaneOutput('p4-exit', 1, 1, [...new TextEncoder().encode('last words\r\n')]),
  )
  const exitedCard = page.getByTestId('pane-p4-exit')
  await expect(exitedCard).toHaveCount(1)
  await expect(exitedCard.locator('.terminal-host')).toHaveAttribute('data-consumed', '')

  await page.evaluate(() => window.__releaseListState())
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  await expect(exitedCard).toHaveCount(1)
  expect(await page.evaluate(() => window.__disposedEmulators)).toEqual([])
  expect(
    (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-exit'),
  ).toHaveLength(0)

  await page.evaluate(() => window.__releaseOneParserWrite())
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_ack')).filter((call) => call.args.id === 'p4-exit').length,
    )
    .toBe(1)
  await expect(exitedCard).toHaveCount(0)
  expect(await page.evaluate(() => window.__disposedEmulators)).toEqual([
    { id: 'p4-exit', generation: 1 },
  ])
  expect(await page.evaluate(() => window.__TAURI__.test.inspectPane('p4-exit:1'))).toEqual({
    emulator: false,
    outputChain: false,
    provisional: false,
    retired: true,
    retiring: false,
  })
})

test('retires a provisional generation after an authoritative replacement', async ({ page }) => {
  const state = cannedState()
  state.tabs[0].panes.push(
    pane('p4-replace', 'worker', {
      generation: 2,
      order: 4,
      conversation: 'nyx-replacement',
      agent: 'nyx',
    }),
  )
  await page.route(`${ROSTER_ORIGIN}/**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<main>Roster connected</main>' }),
  )
  await installTauriShim(page, { state, emulatorGate: true, listStateGate: true })
  await page.goto(origin)
  await expect.poll(async () => (await commandCalls(page, 'list_state')).length).toBe(1)

  await page.evaluate(() =>
    window.__emitPaneOutput('p4-replace', 1, 1, [...new TextEncoder().encode('old generation')]),
  )
  await page.evaluate(() => window.__releaseListState())
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  const oldCard = page.locator('[data-testid="pane-p4-replace"][data-generation="1"]')
  const currentCard = page.locator('[data-testid="pane-p4-replace"][data-generation="2"]')
  await expect(oldCard).toHaveCount(1)
  await expect(currentCard).toHaveCount(1)

  await page.evaluate(() => window.__releaseOneParserWrite())
  await expect(oldCard).toHaveCount(0)
  await expect(currentCard).toHaveCount(1)
  expect(await page.evaluate(() => window.__TAURI__.test.inspectPane('p4-replace:1'))).toEqual({
    emulator: false,
    outputChain: false,
    provisional: false,
    retired: true,
    retiring: false,
  })
})

test('replays a state change that arrives while a state request is in flight', async ({ page }) => {
  await page.route(`${ROSTER_ORIGIN}/**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<main>Roster connected</main>' }),
  )
  await installTauriShim(page, { listStateGate: true })
  await page.goto(origin)
  await expect.poll(async () => (await commandCalls(page, 'list_state')).length).toBe(1)

  await page.evaluate(async () => {
    const tab = window.__state.tabs.find((candidate) => candidate.id === 't-four')
    tab.panes.push({
      id: 'p4-w3',
      generation: 1,
      kind: 'worker',
      order: 4,
      conversation: 'clio-racing-worker',
      agent: 'clio',
      alive: true,
    })
    await window.__emitTauriEvent('state-changed', { reason: 'pane.open' })
    window.__releaseListState()
  })

  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  await expect(page.getByTestId('pane-node-p4-w3')).toContainText('w3 clio-racing-worker')
  expect(await commandCalls(page, 'list_state')).toHaveLength(2)
})

test('preserves the active terminal and keyboard routing across reconciliation and resize', async ({
  page,
}) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  const leadInput = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await leadInput.focus()
  await page.evaluate(() => {
    window.__calls = window.__calls.filter((call) => call.command !== 'pane_input_enqueue')
  })

  await page.evaluate(() => window.__openExternalWorker())
  await expect(leadInput).toBeFocused()
  await page.keyboard.type('after-open')
  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length)
    .toBeGreaterThan(0)
  let calls = await commandCalls(page, 'pane_input_enqueue')
  let lastInput = calls.at(-1)
  expect(lastInput.args.id).toBe('p4-lead')
  expect(
    new TextDecoder().decode(new Uint8Array(calls.flatMap((call) => call.args.bytes))),
  ).toContain('after-open')

  await page.setViewportSize({ width: 1060, height: 760 })
  await expect(leadInput).toBeFocused()
  const beforeResizeInput = calls.length
  await page.keyboard.type('after-resize')
  await expect
    .poll(async () => {
      const resizedCalls = (await commandCalls(page, 'pane_input_enqueue')).slice(beforeResizeInput)
      return new TextDecoder().decode(
        new Uint8Array(resizedCalls.flatMap((call) => call.args.bytes)),
      )
    })
    .toContain('after-resize')
  calls = await commandCalls(page, 'pane_input_enqueue')
  lastInput = calls.at(-1)
  expect(lastInput.args.id).toBe('p4-lead')
})

test('keeps every request on the page origin except the roster iframe', async ({ page }) => {
  const requests = []
  page.on('request', (request) => requests.push(request.url()))
  await boot(page)

  expect(requests.length).toBeGreaterThan(1)
  for (const url of requests) {
    expect(url.startsWith(origin) || url.startsWith(ROSTER_ORIGIN), url).toBe(true)
  }
  expect(requests.some((url) => url.startsWith(ROSTER_ORIGIN))).toBe(true)
})

test('renders unknown bridge operations as not available yet and keeps running', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() =>
    window.__setCommandResult('set_policy', {
      ok: false,
      error: 'not-available-yet',
      operation: 'notify.set',
    }),
  )
  await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
  await page.getByRole('menuitem', { name: 'Auto' }).click()

  await expect(page.getByRole('status')).toContainText('not available yet')
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  await expect(page.getByTestId('pane-stage')).toBeVisible()
})

test('shows a bounded pane-input refusal without parking the page', async ({ page }) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  await page.evaluate(() =>
    window.__setCommandResult('pane_input_enqueue', {
      ok: false,
      error: 'pane-input-queue-full',
    }),
  )

  const textarea = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await textarea.focus()
  await page.keyboard.type('x')

  await expect(page.getByRole('status')).toContainText('pane-input-queue-full')
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  expect((await commandCalls(page, 'pane_input_enqueue')).at(-1).args.id).toBe('p4-lead')
})

test('keeps the next input sequence after an oversized paste is refused', async ({ page }) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  await page.evaluate(() =>
    window.__setCommandResult('pane_input_enqueue', {
      ok: false,
      error: 'pane input exceeds 65536 bytes',
    }),
  )

  const textarea = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await textarea.focus()
  await textarea.evaluate((element) => {
    const transfer = new DataTransfer()
    transfer.setData('text/plain', 'P'.repeat(65_537))
    element.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, clipboardData: transfer }))
  })

  await expect(page.getByRole('status')).toContainText('pane input exceeds 65536 bytes')
  await expect.poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length).toBe(1)
  let calls = await commandCalls(page, 'pane_input_enqueue')
  expect(calls[0].args.sequence).toBe(1)
  expect(calls[0].args.bytes).toHaveLength(65_537)

  await page.evaluate(() =>
    window.__setCommandResult('pane_input_enqueue', {
      ok: true,
      ticket: 'after-oversized-paste',
    }),
  )
  await page.keyboard.type('K')

  await expect.poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length).toBe(2)
  calls = await commandCalls(page, 'pane_input_enqueue')
  expect(calls[1].args.sequence).toBe(2)
  expect(new TextDecoder().decode(new Uint8Array(calls[1].args.bytes))).toBe('K')
  await expect.poll(async () => (await commandCalls(page, 'pane_input_wait')).length).toBe(1)
  expect((await commandCalls(page, 'pane_input_wait'))[0].args.ticket).toBe('after-oversized-paste')
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
})

test('shows input sequence gap and regression refusals without parking the page', async ({
  page,
}) => {
  await boot(page)
  await page.getByTestId('pane-node-p4-lead').click()
  const textarea = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await textarea.focus()

  await page.evaluate(() =>
    window.__setCommandResult('pane_input_enqueue', {
      ok: false,
      error: 'pane-input-sequence-gap',
    }),
  )
  await page.keyboard.type('g')
  await expect(page.getByRole('status')).toContainText('pane-input-sequence-gap')

  await page.evaluate(() =>
    window.__setCommandResult('pane_input_enqueue', {
      ok: false,
      error: 'pane-input-sequence-regression',
    }),
  )
  await page.keyboard.type('r')
  await expect(page.getByRole('status')).toContainText('pane-input-sequence-regression')
  await expect(page.locator('#app')).toHaveAttribute('data-ready', 'true')
  const calls = await commandCalls(page, 'pane_input_enqueue')
  expect(calls.slice(-2).map((call) => call.args.sequence)).toEqual([1, 2])
})

for (const harness of ['claude-code', 'codex', 'pi', 'opencode']) {
  test(`no reply confirmation controls for ${harness} leads or workers (TEST-PANE-111)`, async ({
    page,
  }) => {
    const state = cannedState()
    state.tabs[0].lead.harness = harness
    await boot(page, { state })
    await expect(
      page.getByRole('button', { name: /Resume replies|Allow lead messages/ }),
    ).toHaveCount(0)
    await expect(page.locator('#resume-replies-dialog')).toHaveCount(0)
    expect(await commandCalls(page, 'pane_input_snapshot')).toHaveLength(0)
    expect(await commandCalls(page, 'pane_resume_replies')).toHaveLength(0)
    await expect(page.getByTestId('pane-p4-lead').locator('.terminal-host')).toBeVisible()
    await expect(page.getByTestId('pane-p4-w1').locator('.terminal-host')).toBeVisible()
  })
}

test('renders native harness colors without an app-specific ANSI palette', async ({ page }) => {
  await boot(page)
  const result = await page.evaluate(async () => {
    const { XtermEmulator } = await import('/term.js')
    const { Terminal } = await import('/vendor/xterm.js')
    const replies = []
    const defaults = []
    const hosts = [document.createElement('div'), document.createElement('div')]
    for (const host of hosts) {
      host.style.cssText = 'width:600px;height:300px'
      document.body.append(host)
    }
    const appTerminal = new XtermEmulator(hosts[0], { onReply: (data) => replies.push(data) })
    const nativeTerminal = new Terminal()
    nativeTerminal.open(hosts[1])
    nativeTerminal.onData((data) => defaults.push(data))
    // Query the actual parser's default foreground/background and all ANSI
    // slots, then check that RGB text keeps the colors emitted by the TUI.
    const query =
      '\x1b]10;?\x1b\\\x1b]11;?\x1b\\' +
      Array.from({ length: 16 }, (_, i) => `\x1b]4;${i};?\x1b\\`).join('')
    await appTerminal.write(
      new TextEncoder().encode(`${query}\x1b[38;2;12;34;56m\x1b[48;2;78;90;123mX`),
    )
    await new Promise((resolve) => nativeTerminal.write(query, resolve))
    const cell = appTerminal.terminal.buffer.active.getLine(0).getCell(0)
    const colors = { foreground: cell.getFgColor(), background: cell.getBgColor() }
    appTerminal.dispose()
    nativeTerminal.dispose()
    for (const host of hosts) host.remove()
    return { replies, defaults, colors }
  })
  expect(result.replies).toHaveLength(18)
  expect(result.replies).toEqual(result.defaults)
  expect(result.colors).toEqual({ foreground: 0x0c2238, background: 0x4e5a7b })
})

test('session keeps a double-width lead fixed while later workers scroll horizontally', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await boot(page, { state: gridState(10) })
  const stage = page.getByTestId('pane-stage')
  const measure = () =>
    stage.evaluate((element) => {
      const cards = [...element.querySelectorAll('.pane-card')]
      const lead = cards[0].getBoundingClientRect()
      const worker = cards[1].getBoundingClientRect()
      const workers = element.querySelector('.worker-stage')
      return {
        leadX: lead.x,
        leadWidth: lead.width,
        workerWidth: worker.width,
        scrollHeight: element.scrollHeight,
        height: element.clientHeight,
        overflow: workers ? workers.scrollWidth > workers.clientWidth : false,
      }
    })
  const before = await measure()
  expect(Math.abs(before.leadWidth - 2 * before.workerWidth)).toBeLessThanOrEqual(2)
  expect(before.scrollHeight).toBeLessThanOrEqual(before.height + 1)
  expect(before.overflow).toBe(true)
  await stage.locator('.worker-stage').evaluate((element) => {
    element.scrollLeft = element.scrollWidth
  })
  expect((await measure()).leadX).toBe(before.leadX)
  const last = stage.locator('[data-testid="pane-p-grid-9"]')
  await expect(last).toBeInViewport()
})

test('PM grid appears above the lead and preserves its terminal across group switches', async ({
  page,
}) => {
  const state = cannedState()
  state.tabs[0].roleName = 'clear-forest'
  state.tabs.push({
    id: 't-pm',
    role: 'pm',
    parentTabId: state.tabs[0].id,
    roleName: 'quiet-river',
    directory: state.tabs[0].directory,
    lead: { harness: 'pi', generation: 1 },
    panes: [pane('p-pm', 'lead')],
  })
  await boot(page, { state })
  const stage = page.getByTestId('pane-stage')
  const pm = page.getByTestId('pm-t-pm')
  expect((await pm.boundingBox()).y).toBeLessThan(
    (await page.getByTestId('pane-node-p4-lead').boundingBox()).y,
  )
  await expect(stage.locator('[data-pane-id="p-pm"]')).toHaveCount(0)
  await pm.click()
  await expect(stage.getByTestId('pane-p-pm')).toBeVisible()
  await expect(page.getByTestId('view-pm')).toHaveAttribute('aria-pressed', 'true')
  await expect(stage.locator('.pane-card')).toHaveCount(1)
  await expect(page.locator('#new-pane')).toBeVisible()
  await page.evaluate(() =>
    window.__emitPaneOutput('p-pm', 1, 1, [...new TextEncoder().encode('PM-MAIN-OUTPUT')]),
  )
  await expect(stage.locator('.xterm-rows')).toContainText('PM-MAIN-OUTPUT')
  await stage.locator('.xterm-helper-textarea').focus()
  await page.keyboard.type('hello')
  await expect
    .poll(
      async () =>
        (await commandCalls(page, 'pane_input_enqueue')).filter((c) => c.args.id === 'p-pm').length,
    )
    .toBeGreaterThan(0)
  const box = await stage.boundingBox()
  const card = await stage.getByTestId('pane-p-pm').boundingBox()
  expect(Math.abs(box.height - 16 - card.height)).toBeLessThanOrEqual(2)
  await page.getByTestId('view-lead').click()
  await expect(stage.locator('[data-pane-id="p-pm"]')).toHaveCount(0)
  await pm.click()
  await expect(stage.locator('.xterm-rows')).toContainText('PM-MAIN-OUTPUT')
  expect(await commandCalls(page, 'open_pm')).toHaveLength(0)
})

test('focused worker fills the stage after grid selection and viewport resize', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 1000 })
  await boot(page, { state: gridState(8) })
  await page.getByTestId('pane-node-p-grid-2').click()
  const stage = page.getByTestId('pane-stage')
  await expect(stage).toHaveAttribute('data-mode', 'focused')
  for (const height of [1000, 720, 1100]) {
    await page.setViewportSize({ width: 1440, height })
    await expect
      .poll(() =>
        stage.evaluate((element) => {
          const box = element.getBoundingClientRect()
          const card = element.querySelector('.pane-card').getBoundingClientRect()
          return Math.abs(box.bottom - 8 - card.bottom)
        }),
      )
      .toBeLessThanOrEqual(2)
  }
})

test('Delete pane requires confirmation and retains the captured pane identity', async ({
  page,
}) => {
  await boot(page)
  await page
    .getByTestId('pane-node-p4-w1')
    .locator('..')
    .getByRole('button', { name: /Delete pane/ })
    .click()
  const dialog = page.getByRole('dialog', { name: 'Delete pane', exact: true })
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  expect(await commandCalls(page, 'delete_pane')).toHaveLength(0)
  await page
    .getByTestId('pane-node-p4-w1')
    .locator('..')
    .getByRole('button', { name: /Delete pane/ })
    .click()
  await dialog.getByRole('button', { name: 'Delete pane', exact: true }).click()
  expect(await commandCalls(page, 'delete_pane')).toEqual([
    { command: 'delete_pane', args: { id: 'p4-w1', generation: 1 } },
  ])
  await expect(page.getByTestId('pane-p4-w1')).toHaveCount(0)
  await expect(page.getByTestId('pane-p4-w2')).toBeVisible()
})

test('a closed worker can be deleted from the sidebar without reopening it', async ({ page }) => {
  const state = cannedState()
  state.tabs[0].closed = true
  for (const pane of state.tabs[0].panes) pane.alive = false
  await boot(page, { state })
  await page
    .getByTestId('pane-node-p4-w1')
    .locator('..')
    .getByRole('button', { name: /Delete pane/ })
    .click()
  await page
    .getByRole('dialog', { name: 'Delete pane', exact: true })
    .getByRole('button', { name: 'Delete pane', exact: true })
    .click()
  expect(await commandCalls(page, 'open_consult')).toHaveLength(0)
  expect(await commandCalls(page, 'tab_resume')).toHaveLength(0)
  await expect(page.getByTestId('pane-node-p4-w1')).toHaveCount(0)
})

test('PM and Lead grids isolate the same preset and preserve hidden output and focused navigation', async ({
  page,
}) => {
  const state = cannedState()
  state.tabs.push({
    id: 't-pm',
    role: 'pm',
    parentTabId: state.tabs[0].id,
    roleName: 'quiet-river',
    directory: state.tabs[0].directory,
    lead: { harness: 'pi', generation: 1 },
    panes: [
      pane('p-pm', 'lead'),
      pane('p-advisor', 'worker', { conversation: 'zeus-advice', agent: 'zeus' }),
    ],
  })
  await boot(page, { state })
  const stage = page.getByTestId('pane-stage')
  await page.getByTestId('view-pm').click()
  await expect(stage.locator('.pane-card')).toHaveCount(2)
  await expect(page.getByTestId('pane-node-p-advisor')).toContainText('a1')
  await page.getByTestId('pane-node-p-advisor').click()
  await expect(stage.locator('.pane-card')).toHaveCount(1)
  await page.getByTestId('view-lead').click()
  await expect(stage.getByTestId('pane-p-advisor')).toHaveCount(0)
  await page.evaluate(() =>
    window.__emitPaneOutput('p-advisor', 1, 1, [
      ...new TextEncoder().encode('HIDDEN-ADVISOR-OUTPUT'),
    ]),
  )
  await page.getByTestId('view-pm').click()
  await expect(stage.locator('.pane-card')).toHaveCount(2)
  await expect(stage.getByTestId('pane-p-advisor').locator('.xterm-rows')).toContainText(
    'HIDDEN-ADVISOR-OUTPUT',
  )
  expect(await commandCalls(page, 'open_pm')).toHaveLength(0)
  expect(await commandCalls(page, 'tab_resume')).toHaveLength(0)
})

test('result inbox counts every reply on its worker and owner and opens complete safe history without receipt', async ({
  page,
}) => {
  const state = cannedState()
  state.results = Array.from({ length: 25 }, (_, index) => ({
    id: `d-${index + 1}`,
    tab: 't-four',
    conversation: 'nyx-coral-lane',
    agent: 'nyx',
    answerId: `answer-${index + 1}`,
    state: index === 1 ? 'received' : index === 2 ? 'uncertain' : 'waiting',
    createdAt: index + 1,
    preview: `Reply ${index + 1}`,
    parts: 1,
    receivedParts: 0,
  }))
  const body = '<script>window.inboxExecuted=true</script>\n😀 complete text '.repeat(1500)
  state.resultBodies = { 'd-1': body }
  await boot(page, { state })
  await expect(page.getByTestId('result-summary-p4-w1')).toHaveText('24 unconfirmed results')
  await expect(page.getByTestId('result-summary-p4-lead')).toHaveText('24 unconfirmed results')
  await expect(page.getByTestId('tree-results-p4-w1')).toHaveText('24')
  await page.getByTestId('tree-results-p4-w1').click()
  const history = page.getByRole('dialog', { name: 'Lead results' })
  await expect(history.locator('.result-row')).toHaveCount(25)
  await expect(history).toContainText('Received by Lead')
  await expect(history).toContainText('Receipt unconfirmed')
  await history.getByRole('button', { name: 'Read result d-1', exact: true }).click()
  await expect(history.locator('.result-body:visible')).toHaveText(body)
  expect(await page.evaluate(() => window.inboxExecuted)).toBeUndefined()
  expect(await commandCalls(page, 'result_collect')).toHaveLength(0)
  expect(await commandCalls(page, 'result_body')).toHaveLength(Math.ceil(body.length / 16000))
  await expect(page.getByTestId('result-summary-p4-w1')).toHaveText('24 unconfirmed results')
})

test('result inbox remains independent between PM advisors and Lead workers', async ({ page }) => {
  const state = cannedState()
  state.tabs.push({
    ...state.tabs[0],
    id: 't-pm',
    role: 'pm',
    parentTabId: 't-four',
    lead: { harness: 'pi', generation: 1 },
    panes: [
      pane('pm-lead', 'lead'),
      pane('pm-advisor', 'worker', { conversation: 'advice', agent: 'nyx', order: 1 }),
    ],
  })
  state.results = [
    {
      id: 'd-1',
      tab: 't-four',
      conversation: 'nyx-coral-lane',
      agent: 'nyx',
      state: 'waiting',
      preview: 'lead result',
    },
    {
      id: 'd-2',
      tab: 't-pm',
      conversation: 'advice',
      agent: 'nyx',
      state: 'uncertain',
      preview: 'PM findings',
    },
  ]
  await boot(page, { state })
  await page.getByTestId('view-pm').click()
  await expect(page.getByTestId('result-summary-pm-advisor')).toHaveText('1 unconfirmed result')
  await page.getByRole('button', { name: 'Results', exact: true }).click()
  const history = page.getByRole('dialog', { name: 'PM results' })
  await expect(history.locator('.result-row')).toHaveCount(1)
  await expect(history).toContainText('PM findings')
  await expect(history).not.toContainText('lead result')
})
