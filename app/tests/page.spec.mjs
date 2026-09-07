import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { dirname, extname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'
import { Tabs } from '../../src/tabs.js'

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
    deliveries: [
      { id: 'd-draft', tab: 't-four', pane: 'p4-lead', state: 'pending', reason: 'draft open' },
      { id: 'd-busy', tab: 't-four', pane: 'p4-w1', state: 'pending', reason: 'lead busy' },
      { id: 'd-unbound', tab: 't-four', pane: 'p4-w2', state: 'pending', reason: 'unbound' },
    ],
    held: [{ id: 'held-1', tab: 't-four', answerId: 'answer-held' }],
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
  state.deliveries = []
  state.held = []
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
      const inputSequences = new Map()
      const inputEpochs = new Map()
      let inputSnapshotGate = null
      let releaseInputSnapshot
      let parserGateOpen = !gateEmulator
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
      const inputKey = (args) => `${args.id}:${args.generation}`
      const invoke = async (command, args = {}) => {
        const logged = { ...args }
        if (logged.onOutput !== undefined) logged.onOutput = '[channel]'
        window.__calls.push({ command, args: copy(logged) })
        if (command === 'pane_input_enqueue' || command === 'pane_reply_enqueue') {
          const key = inputKey(args)
          inputSequences.set(key, (inputSequences.get(key) ?? 0) + 1)
          inputEpochs.set(key, (inputEpochs.get(key) ?? 0) + (args.bytes?.length ?? 0))
        }
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
          return snapshot
        }
        if (command === 'answers_list') {
          return {
            ok: true,
            answers: copy(window.__state.answers[args.conversation] ?? []),
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
        if (command === 'pane_input_snapshot') {
          const key = inputKey(args)
          const snapshot = {
            ok: true,
            inputEpoch: inputEpochs.get(key) ?? 0,
            sequence: inputSequences.get(key) ?? 0,
            draftLatched: true,
          }
          if (inputSnapshotGate !== null) await inputSnapshotGate
          return snapshot
        }
        if (command === 'pane_resume_replies') return { ok: true }
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
      window.__gateInputSnapshot = () => {
        inputSnapshotGate = new Promise((resolveGate) => {
          releaseInputSnapshot = resolveGate
        })
      }
      window.__releaseInputSnapshot = () => {
        releaseInputSnapshot?.()
        releaseInputSnapshot = undefined
        inputSnapshotGate = null
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
        window.__state.deliveries.push({
          id: 'd-external',
          tab: 't-four',
          pane: 'p4-lead',
          state: 'pending',
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
  const opener = page.getByRole('button', { name: 'Agents', exact: true })
  await expect(dialog).not.toBeVisible()
  await expect(iframe).toHaveAttribute('src', `${ROSTER_ORIGIN}/?token=ui-token`)
  const before = await page.getByTestId('pane-stage').boundingBox()
  await opener.click()
  await expect(page.getByRole('dialog', { name: 'Agents', exact: true })).toBeVisible()
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
  await page.getByRole('button', { name: 'Close Agents' }).click()
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  await expect(iframe).toHaveCount(1)
  expect(await page.getByTestId('pane-stage').boundingBox()).toEqual(before)
  await opener.click()
  await page.getByRole('button', { name: 'Close Agents' }).focus()
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  expect(await commandCalls(page, 'close_pane')).toHaveLength(0)
})

test('explains reply delivery beside its explicit Automatic or Manual label', async ({ page }) => {
  await boot(page)
  await expect(page.getByTestId('tab-policy')).toHaveText('Reply delivery: Automatic')
  await page.getByRole('button', { name: 'About reply delivery' }).click()
  const help = page.getByTestId('delivery-help')
  await expect(help).toBeVisible()
  await expect(help).toContainText('complete worker replies')
  await expect(help).toContainText('typing')
  await expect(help).toContainText('Manual')
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
  await expect(stage).toHaveAttribute('data-layout', '"lead w1" "w2 w3"')
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
  expect(four['p4-w2'].x).toBe(four['p4-lead'].x)
  expect(four['p4-shell'].x).toBe(four['p4-w1'].x)

  await page.getByTestId('session-t-five-button').click()
  await expect(stage).toHaveAttribute('data-layout', '"lead w1 w2" "lead w3 w4"')
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
  expect(five['p5-w1'].y).toBe(five['p5-w2'].y)
  expect(five['p5-w3'].y).toBeGreaterThan(five['p5-w1'].y)
  expect(five['p5-w1'].x).toBeLessThan(five['p5-w2'].x)
  expect(five['p5-w3'].x).toBe(five['p5-w1'].x)
})

test('renders session → lead → numbered workers and shells as a nested tree', async ({ page }) => {
  await boot(page)
  const session = page.getByTestId('session-t-four')
  const lead = session.locator(':scope > [role="group"] > [data-kind="lead"]')
  await expect(session).toContainText('harbour')
  await expect(lead).toContainText('harbour-lead')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('w1 nyx-coral-lane')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('w2 ares-amber-moss')
  await expect(lead.locator(':scope > [role="group"]')).toContainText('shell')
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

test('lists transcript answers with delivered and uncertain marks and requires explicit resend', async ({
  page,
}) => {
  await boot(page)
  await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
  const menu = page.getByRole('menu', { name: 'Worker actions' })
  await expect(menu).toContainText('Send reply to lead…')
  await expect(menu).toContainText('The parser is ready.')
  await expect(menu).toContainText('Delivered')
  await expect(menu).toContainText('Uncertain')
  await expect(menu.getByRole('menuitem', { name: 'Resend answer-sent to lead' })).toBeVisible()
  await menu.getByRole('menuitem', { name: 'Send answer-fresh to lead' }).click()
  expect((await commandCalls(page, 'deliver_now')).at(-1)).toEqual({
    command: 'deliver_now',
    args: {
      tab: 't-four',
      conversation: 'nyx-coral-lane',
      answerId: 'answer-fresh',
      resend: false,
    },
  })

  await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
  const reopened = page.getByRole('menu', { name: 'Worker actions' })
  await reopened.getByRole('menuitem', { name: 'Resend answer-sent to lead' }).click()
  expect((await commandCalls(page, 'deliver_now')).at(-1)).toEqual({
    command: 'deliver_now',
    args: {
      tab: 't-four',
      conversation: 'nyx-coral-lane',
      answerId: 'answer-sent',
      resend: true,
    },
  })
})

test('shows exactly which reply parts are still unconfirmed', async ({ page }) => {
  const state = cannedState()
  state.answers['nyx-coral-lane'] = [
    {
      id: 'answer-partial',
      preview: 'A long answer with missing text.',
      ready: true,
      delivered: false,
      uncertain: true,
      partProgress: { delivery: 'd-8002', total: 3, uncovered: [1, 3] },
    },
  ]
  await boot(page, { state })
  await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
  const menu = page.getByRole('menu', { name: 'Worker actions' })
  await expect(menu).toContainText('1 of 3 parts confirmed. Not confirmed: 1, 3.')
  await expect(menu.getByRole('menuitem', { name: 'Resend answer-partial to lead' })).toBeVisible()
})

test('shows an unfinished answer as in progress without offering send or resend', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() => {
    window.__state.answers['nyx-coral-lane'] = [
      {
        id: 'answer-writing',
        preview: 'Still writing.',
        ready: false,
        delivered: false,
        uncertain: true,
      },
    ]
  })
  await page.getByTestId('pane-p4-w1').locator('.pane-titlebar').click({ button: 'right' })
  const menu = page.getByRole('menu', { name: 'Worker actions' })
  await expect(menu).toContainText('Still writing.')
  await expect(menu).toContainText('In progress')
  await expect(
    menu.getByRole('menuitem', { name: 'Waiting for answer-writing to complete' }),
  ).toBeDisabled()
  await expect(menu.getByRole('menuitem', { name: /^(Send|Resend) answer-writing/ })).toHaveCount(0)
  expect(await commandCalls(page, 'deliver_now')).toEqual([])
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

test('summarizes waiting results per pane and opens bounded details with safe actions', async ({
  page,
}) => {
  await boot(page)
  await expect(page.getByTestId('delivery-summary-p4-lead')).toHaveText('1 pending result')
  await expect(page.getByTestId('delivery-summary-p4-w1')).toHaveText('1 pending result')
  await expect(page.getByTestId('delivery-summary-p4-w2')).toHaveText('1 pending result')
  await expect(page.locator('.delivery-summary')).toHaveCount(3)

  await page.getByTestId('delivery-summary-p4-lead').click()
  const details = page.getByRole('menu', { name: 'Pending results' })
  await expect(details).toContainText('draft open')
  const button = details.getByRole('menuitem', { name: 'Deliver now' })
  await expect(button).toBeDisabled()
  await expect(button).toHaveAttribute('title', /draft open/i)
})

test('offers held answers to the resumed lead', async ({ page }) => {
  await boot(page)
  await page.getByRole('button', { name: 'Send held answers to this lead' }).click()
  expect((await commandCalls(page, 'held_send')).at(-1)).toEqual({
    command: 'held_send',
    args: { tab: 't-four' },
  })
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

test('offers Resume replies only on live lead and worker panes with cancel and Escape', async ({
  page,
}) => {
  await boot(page)

  await expect(page.locator('.resume-replies-button:visible')).toHaveCount(3)
  await expect(page.getByTestId('pane-p4-shell').locator('.resume-replies-button')).toHaveCount(0)

  const button = page.getByTestId('pane-p4-w1').getByRole('button', { name: 'Resume replies' })
  await button.click()
  const dialog = page.getByRole('dialog', { name: 'Resume replies' })
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText(/send or erase your terminal input/i)
  await expect(dialog).toContainText(/does not erase text or change reply policy/i)
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).toBeHidden()
  expect(await commandCalls(page, 'pane_resume_replies')).toHaveLength(0)

  await button.click()
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  expect(await commandCalls(page, 'pane_resume_replies')).toHaveLength(0)
  expect(await commandCalls(page, 'pane_input_snapshot')).toHaveLength(2)
})

test('captures one snapshot and resumes replies with its exact identity and sequence', async ({
  page,
}) => {
  await boot(page)

  const button = page.getByTestId('pane-p4-lead').getByRole('button', { name: 'Resume replies' })
  await button.click()
  const dialog = page.getByRole('dialog', { name: 'Resume replies' })
  await expect(dialog).toBeVisible()
  expect((await commandCalls(page, 'pane_input_snapshot')).at(-1)).toEqual({
    command: 'pane_input_snapshot',
    args: { id: 'p4-lead', generation: 1 },
  })

  await dialog.getByRole('button', { name: 'I confirm the input line is empty' }).click()
  await expect.poll(async () => (await commandCalls(page, 'pane_resume_replies')).length).toBe(1)
  expect((await commandCalls(page, 'pane_resume_replies')).at(-1)).toEqual({
    command: 'pane_resume_replies',
    args: { id: 'p4-lead', generation: 1, inputEpoch: 0, sequence: 0 },
  })
})

test('keeps replies held and asks to reopen after a stale generation failure', async ({ page }) => {
  await boot(page)
  await page.evaluate(() =>
    window.__setCommandResult('pane_resume_replies', {
      ok: false,
      error: 'stale pane generation',
    }),
  )

  await page.getByTestId('pane-p4-lead').getByRole('button', { name: 'Resume replies' }).click()
  const dialog = page.getByRole('dialog', { name: 'Resume replies' })
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: 'I confirm the input line is empty' }).click()
  await expect.poll(async () => (await commandCalls(page, 'pane_resume_replies')).length).toBe(1)
  await expect(page.getByRole('status')).toContainText('Reopen the confirmation dialog')
  await expect(dialog).toBeHidden()
})

test('rejects input typed after the snapshot without sending a resume command', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() => window.__gateInputSnapshot())

  const button = page.getByTestId('pane-p4-lead').getByRole('button', { name: 'Resume replies' })
  await button.click()
  const textarea = page.getByTestId('pane-p4-lead').locator('.xterm-helper-textarea')
  await textarea.focus()
  await page.keyboard.type('typed after snapshot')
  await expect
    .poll(async () => (await commandCalls(page, 'pane_input_enqueue')).length)
    .toBeGreaterThan(0)

  await page.evaluate(() => window.__releaseInputSnapshot())
  await expect(page.getByRole('status')).toContainText('Reopen Resume replies')
  expect(await page.getByRole('dialog', { name: 'Resume replies' })).toBeHidden()
  expect(await commandCalls(page, 'pane_resume_replies')).toHaveLength(0)
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

test('keeps every pane in a scrollable grid when height cannot fit all rows', async ({ page }) => {
  await page.setViewportSize({ width: 650, height: 470 })
  await boot(page)
  const stage = page.getByTestId('pane-stage')
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage.locator('.pane-card')).toHaveCount(4)
  await expect(page.getByRole('navigation', { name: 'Pane navigation' })).toBeHidden()
  const dimensions = await stage.evaluate((element) => ({
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }))
  expect(dimensions.scrollHeight).toBeGreaterThan(dimensions.clientHeight)
})

test('subtracts grid padding and gaps before enforcing the minimum card size', async ({ page }) => {
  await boot(page)
  const stage = page.getByTestId('pane-stage')
  await stage.evaluate((element) => {
    element.style.position = 'absolute'
    element.style.inset = '0 auto auto 0'
    element.style.width = '520px'
    element.style.height = '360px'
  })
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage).toHaveAttribute('data-layout', '"lead" "w1" "w2" "w3"')

  await stage.evaluate((element) => {
    element.style.width = '544px'
    element.style.height = '384px'
  })
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage).toHaveAttribute('data-layout', '"lead w1" "w2 w3"')
  const dimensions = await stage.locator('.pane-card').evaluateAll((cards) =>
    cards.map((card) => {
      const bounds = card.getBoundingClientRect()
      return { width: bounds.width, height: bounds.height }
    }),
  )
  expect(dimensions).toHaveLength(4)
  for (const dimension of dimensions) {
    expect(dimension.width).toBeGreaterThanOrEqual(260)
    expect(dimension.height).toBeGreaterThanOrEqual(180)
  }
})

test('keeps 10 panes in three columns and makes the bottom row reachable by scrolling', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 520 })
  await boot(page, { state: gridState(10) })
  const stage = page.getByTestId('pane-stage')
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage.locator('.pane-card')).toHaveCount(10)
  await expect(stage).toHaveAttribute('data-layout', '"lead w1 w2" "w3 w4 w5" "w6 w7 w8" "w9 . ."')
  const columns = await stage.evaluate((element) => getComputedStyle(element).gridTemplateColumns)
  expect(columns.trim().split(/\s+/)).toHaveLength(3)
  const lastRowReachable = await stage.evaluate((element) => {
    element.scrollTop = element.scrollHeight
    const stageBounds = element.getBoundingClientRect()
    const last = element.querySelector('[data-testid="pane-p-grid-9"]')
    const bounds = last.getBoundingClientRect()
    return bounds.top >= stageBounds.top && bounds.bottom <= stageBounds.bottom
  })
  expect(lastRowReachable).toBe(true)
})

test('keeps 20 panes in a three-column grid rather than switching to focus mode', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 520 })
  await boot(page, { state: gridState(20) })
  const stage = page.getByTestId('pane-stage')
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage.locator('.pane-card')).toHaveCount(20)
  await expect(stage.locator('[data-testid="pane-p-grid-19"]')).toHaveCount(1)
  await expect(stage).toHaveAttribute('data-layout', /"w18 w19 \."$/)
  const columns = await stage.evaluate((element) => getComputedStyle(element).gridTemplateColumns)
  expect(columns.trim().split(/\s+/)).toHaveLength(3)
})

for (const count of [7, 10, 20]) {
  test(`shows no more than six panes before scrolling a tall ${count}-pane grid`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 1200 })
    await boot(page, { state: gridState(count) })
    const stage = page.getByTestId('pane-stage')
    const metrics = await stage.evaluate((element) => {
      const styles = getComputedStyle(element)
      const padding = Number.parseFloat(styles.getPropertyValue('--pane-grid-padding'))
      const gap = Number.parseFloat(styles.getPropertyValue('--pane-grid-gap'))
      const stageBounds = element.getBoundingClientRect()
      const cards = [...element.querySelectorAll('.pane-card')]
      const visible = cards.filter((card) => {
        const bounds = card.getBoundingClientRect()
        return bounds.top < stageBounds.bottom && bounds.bottom > stageBounds.top
      }).length
      const first = cards[0].getBoundingClientRect()
      const third = cards[6].getBoundingClientRect()
      return {
        visible,
        firstHeight: first.height,
        minimumRowHeight: (element.clientHeight - 2 * padding - gap) / 2,
        thirdTop: third.top,
        stageBottom: stageBounds.bottom,
        scrollHeight: element.scrollHeight,
        clientHeight: element.clientHeight,
      }
    })
    expect(metrics.visible).toBeLessThanOrEqual(6)
    expect(metrics.firstHeight).toBeGreaterThanOrEqual(metrics.minimumRowHeight)
    expect(metrics.thirdTop).toBeGreaterThan(metrics.stageBottom)
    expect(metrics.scrollHeight).toBeGreaterThan(metrics.clientHeight)

    await stage.evaluate((element) => {
      const third = element.querySelector('[data-testid="pane-p-grid-6"]')
      element.scrollTop = third.offsetTop - element.clientHeight + third.offsetHeight
    })
    const thirdRowReached = await stage.evaluate((element) => {
      const stageBounds = element.getBoundingClientRect()
      const third = element.querySelector('[data-testid="pane-p-grid-6"]').getBoundingClientRect()
      return {
        reached: third.top >= stageBounds.top && third.bottom <= stageBounds.bottom + 1,
        thirdTop: third.top,
        thirdBottom: third.bottom,
        stageTop: stageBounds.top,
        stageBottom: stageBounds.bottom,
        scrollTop: element.scrollTop,
        scrollHeight: element.scrollHeight,
        clientHeight: element.clientHeight,
      }
    })
    expect(thirdRowReached.reached, JSON.stringify(thirdRowReached)).toBe(true)
  })
}

test('reflows a many-pane session to two and one columns at narrower widths', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 })
  await boot(page, { state: gridState(10) })
  const stage = page.getByTestId('pane-stage')
  const columnCount = () =>
    stage.evaluate(
      (element) => getComputedStyle(element).gridTemplateColumns.trim().split(/\s+/).length,
    )
  await expect.poll(columnCount).toBe(2)
  await page.setViewportSize({ width: 600, height: 700 })
  await expect.poll(columnCount).toBe(1)
  await expect(stage).toHaveAttribute('data-mode', 'grid')
  await expect(stage.locator('.pane-card')).toHaveCount(10)
})

test('keeps a compact pending-result count within each pane and the page width', async ({
  page,
}) => {
  const state = cannedState()
  state.deliveries = Array.from({ length: 20 }, (_, index) => ({
    id: `d-many-${index}`,
    tab: 't-four',
    pane: 'p4-w1',
    state: 'pending',
    reason: `result ${index} with a very long reason`,
  }))
  await page.setViewportSize({ width: 760, height: 600 })
  await boot(page, { state })
  const summary = page.getByTestId('delivery-summary-p4-w1')
  await expect(summary).toHaveText('20 pending results')
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
  await expect(page.getByTestId('delivery-summary-p4-lead')).toHaveText('2 pending results')
  await page.getByTestId('delivery-summary-p4-lead').click()
  await expect(page.getByRole('menu', { name: 'Pending results' })).toContainText(
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
  await page.getByTestId('pane-node-p4-w3').click()
  await expect(page.getByTestId('pane-p4-w3').locator('.xterm-rows')).toContainText(
    'still connected',
  )
  expect(await commandCalls(page, 'subscribe_output')).toHaveLength(1)
  expect((await commandCalls(page, 'list_state')).length).toBeGreaterThanOrEqual(3)
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
