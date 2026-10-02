import { expect, test } from '@playwright/test'

import { serveUi } from './serve-ui.mjs'

let ui

test.setTimeout(12_000)

test.beforeAll(async () => {
  ui = await serveUi()
})

test.afterAll(() => ui?.close())

function updateSnapshot(overrides = {}) {
  return {
    ok: true,
    currentVersion: '3.0.0-alpha.35',
    channel: 'alpha',
    phase: 'idle',
    available: null,
    downloadedBytes: 0,
    totalBytes: null,
    lastChecked: null,
    error: null,
    blockers: [],
    compatibility: [{ harness: 'codex', version: '0.153.4', status: 'verified', fixedIn: null }],
    ...overrides,
  }
}

/** What the new core answers the board: one project whose worker window is the blocker, if any. */
function coreState(blocker = null) {
  const participant = (id, handle, role) => ({
    id,
    projectId: 1,
    handle,
    role,
    agent: role === 'worker' ? handle : null,
    harness: role === 'human' ? null : 'codex',
    tier: role === 'worker' ? 'standard' : null,
    outUntil: null,
  })
  const lane = (who, pane) => ({ participant: who, tasks: [], activity: { state: 'idle' }, pane })
  const project = { id: 1, name: 'hidden-session', directory: '/work/hidden', state: 'open' }
  return {
    projects: [project],
    boards: {
      1: {
        project: { ...project, review: 'none' },
        open: [],
        lanes: [
          lane(participant(1, 'human', 'human'), null),
          lane(participant(2, 'chief', 'chief'), { id: 'hidden-chief', generation: 1 }),
          ...(blocker === null
            ? []
            : [lane(participant(3, 'background-worker', 'worker'), { ...blocker })]),
        ],
      },
    },
  }
}

async function installTauriShim(page, { snapshot = updateSnapshot(), state = coreState() } = {}) {
  await page.addInitScript(
    ({ initialSnapshot, initialState }) => {
      const copy = (value) => JSON.parse(JSON.stringify(value))
      const eventListeners = new Map()
      const timers = []
      const nativeSetTimeout = window.setTimeout.bind(window)

      window.__calls = []
      window.__updateSnapshot = copy(initialSnapshot)
      window.__commandResults = {}
      window.setTimeout = (callback, delay, ...args) => {
        if (delay === 10_000 || delay === 6 * 60 * 60 * 1_000) {
          const timer = { callback, delay, args, consumed: false }
          timers.push(timer)
          return timer
        }
        return nativeSetTimeout(callback, delay, ...args)
      }

      class Channel {
        constructor() {
          this.onmessage = null
        }
      }

      const invoke = async (command, args = {}) => {
        window.__calls.push({ command, args: copy(args) })
        if (Object.hasOwn(window.__commandResults, command)) {
          return copy(window.__commandResults[command])
        }
        if (command === 'core_request') {
          const { operation, body } = args
          if (operation === 'projects.list')
            return copy({ ok: true, projects: initialState.projects })
          if (operation === 'board.get') {
            return copy({ ok: true, board: initialState.boards[body.project] })
          }
          if (operation === 'inbox.get') return { ok: true, messages: [], total: 0, shown: 0 }
          if (operation === 'agents.list') return { ok: true, agents: [] }
          return { ok: true }
        }
        if (command === 'update_status' || command === 'update_check') {
          return copy(window.__updateSnapshot)
        }
        if (command === 'update_channel') {
          window.__updateSnapshot.channel = args.channel
          window.__updateSnapshot.available = null
          window.__updateSnapshot.downloadedBytes = 0
          window.__updateSnapshot.totalBytes = null
          window.__updateSnapshot.phase = 'idle'
          return copy(window.__updateSnapshot)
        }
        if (command === 'update_download') {
          window.__updateSnapshot.phase = 'downloading'
          return copy(window.__updateSnapshot)
        }
        if (command === 'update_install') {
          window.__updateSnapshot.phase = 'installing'
          return copy(window.__updateSnapshot)
        }
        return { ok: true }
      }

      window.__setCommandResult = (command, result) => {
        window.__commandResults[command] = copy(result)
      }
      window.__runUpdateTimer = async (delay) => {
        for (const timer of timers.filter((candidate) => candidate.delay === delay)) {
          if (timer.consumed) continue
          timer.consumed = true
          await timer.callback(...timer.args)
          await Promise.resolve()
          return
        }
      }
      window.__emitTauriEvent = async (name, payload = {}) => {
        for (const handler of eventListeners.get(name) ?? []) {
          await handler({ event: name, payload: copy(payload) })
        }
      }

      window.__TAURI__ = {
        core: { Channel, invoke },
        event: {
          async listen(name, handler) {
            const handlers = eventListeners.get(name) ?? []
            handlers.push(handler)
            eventListeners.set(name, handlers)
            return () => {}
          },
        },
      }
    },
    { initialSnapshot: snapshot, initialState: state },
  )
}

async function boot(page, options = {}) {
  await installTauriShim(page, options)
  await page.goto(ui.origin)
  await page.waitForLoadState('networkidle')
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', { timeout: 2_000 })
}

function commandCalls(page, command) {
  return page.evaluate((name) => window.__calls.filter((call) => call.command === name), command)
}

test('native menu opens and checks without a dedicated header button', async ({ page }) => {
  await boot(page)
  await expect(page.getByRole('button', { name: 'Updates', exact: true })).toHaveCount(0)
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  await expect(page.locator('#updates-dialog')).toBeVisible()
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(1)
  await expect(page.getByRole('button', { name: 'Close Updates' })).toBeFocused()
})

test('menu check keeps unavailable-feed failures visible and never enables download or install', async ({
  page,
}) => {
  await boot(page)
  const failed = updateSnapshot({
    channel: 'stable',
    phase: 'error',
    error: 'The Stable update feed is unavailable. Please try again later.',
  })
  await page.evaluate((next) => window.__setCommandResult('update_check', next), failed)
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog).toContainText(failed.error)
  await expect(dialog).not.toContainText('You are up to date')
  await expect(dialog.getByRole('button', { name: 'Download update' })).toBeDisabled()
  await expect(dialog.getByRole('button', { name: 'Install and restart' })).toBeDisabled()
  await expect(dialog.getByRole('button', { name: 'Check for updates' })).toBeEnabled()
  await dialog.getByRole('button', { name: 'Close Updates' }).click()
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  await expect(dialog).toBeVisible()
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(2)
})

test('quiet checks run at ten seconds and six hours without reopening a dismissed banner', async ({
  page,
}) => {
  const candidate = updateSnapshot({
    available: { version: '3.0.0-alpha.36', notes: 'Quiet release', date: '2026-09-09' },
  })
  await boot(page)
  await page.evaluate((next) => window.__setCommandResult('update_check', next), candidate)

  await page.evaluate(() => window.__runUpdateTimer(10_000))
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(1)
  await expect(page.locator('#update-banner')).toContainText('3.0.0-alpha.36')
  await expect(page.locator('#updates-dialog')).not.toBeVisible()
  await page.getByRole('button', { name: 'Dismiss update notice' }).click()

  await page.evaluate(() => window.__runUpdateTimer(6 * 60 * 60 * 1_000))
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(2)
  await expect(page.locator('#update-banner')).toBeHidden()
})

test('quiet failures stay invisible while a manual check shows offline and up-to-date states', async ({
  page,
}) => {
  await boot(page)
  await page.evaluate(() =>
    window.__setCommandResult('update_check', { ok: false, error: 'offline' }),
  )
  await page.evaluate(() => window.__runUpdateTimer(10_000))
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(1)
  await expect(page.locator('#updates-dialog')).not.toBeVisible()
  await expect(page.locator('#status')).toBeHidden()

  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  await expect(page.locator('#updates-dialog')).toBeVisible()
  await expect(page.locator('#updates-dialog')).toContainText('offline')

  await page.evaluate(() =>
    window.__setCommandResult('update_check', {
      ok: true,
      currentVersion: '3.0.0-alpha.35',
      channel: 'alpha',
      phase: 'idle',
      available: null,
      downloadedBytes: 0,
      totalBytes: null,
      lastChecked: '2026-09-09T10:00:00Z',
      error: null,
      blockers: [],
      compatibility: [],
    }),
  )
  await page.getByRole('button', { name: 'Check for updates' }).click()
  await expect(page.locator('#updates-dialog')).toContainText('You are up to date')
})

test('renders notes as text, downloads with progress, and Later never installs', async ({
  page,
}) => {
  const notes = '<img src="https://evil.example/payload"> <script>alert(1)</script>'
  const available = updateSnapshot({
    available: { version: '3.0.0-alpha.36', notes, date: '2026-09-09' },
  })
  await boot(page, { snapshot: available })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog).toBeVisible()
  await expect(dialog).toContainText('Available version: 3.0.0-alpha.36')
  await expect(dialog).toContainText(notes)
  await expect(dialog.locator('img, script, a')).toHaveCount(0)

  await dialog.getByRole('button', { name: 'Download update' }).click()
  await expect.poll(async () => (await commandCalls(page, 'update_download')).length).toBe(1)
  await expect(commandCalls(page, 'update_install')).resolves.toHaveLength(0)

  await page.evaluate((progress) => window.__emitTauriEvent('update-state-changed', progress), {
    ...available,
    phase: 'downloading',
    downloadedBytes: 1024,
    totalBytes: 4096,
  })
  await expect(dialog).toContainText('1,024 of 4,096 bytes')
  await dialog.getByRole('button', { name: 'Later' }).click()
  await expect(dialog).not.toBeVisible()
  await expect(commandCalls(page, 'update_install')).resolves.toHaveLength(0)
})

test('channel changes clear the candidate and disable while downloading or installing', async ({
  page,
}) => {
  const available = updateSnapshot({
    available: { version: '3.0.0-alpha.36', notes: 'Candidate', date: '2026-09-09' },
  })
  await boot(page, { snapshot: available })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const channel = page.locator('#updates-channel')
  await expect(channel).toHaveValue('alpha')

  await page.evaluate((snapshot) => window.__emitTauriEvent('update-state-changed', snapshot), {
    ...available,
    phase: 'downloading',
    downloadedBytes: 20,
    totalBytes: 100,
  })
  await expect(channel).toBeDisabled()

  await page.evaluate((snapshot) => window.__emitTauriEvent('update-state-changed', snapshot), {
    ...available,
    channel: 'stable',
    phase: 'idle',
    available: null,
  })
  await expect(channel).toBeEnabled()
  await channel.selectOption('alpha')
  await expect.poll(async () => (await commandCalls(page, 'update_channel')).length).toBe(1)
  await expect(commandCalls(page, 'update_channel')).resolves.toContainEqual({
    command: 'update_channel',
    args: { channel: 'alpha' },
  })
})

test('shows active hidden blockers, allows download, and never offers an unsafe install', async ({
  page,
}) => {
  const blockers = [{ id: 'hidden-pane', generation: 4 }]
  const available = updateSnapshot({
    phase: 'idle',
    available: { version: '3.0.0-alpha.36', notes: 'Ready', date: '2026-09-09' },
    blockers,
  })
  await boot(page, { snapshot: available, state: coreState(blockers[0]) })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog).toContainText('background-worker')
  await expect(dialog).toContainText('Before installing')
  await expect(dialog).toContainText(
    'All open panes must be closed or suspended first, even if idle',
  )
  await expect(dialog.getByRole('button', { name: 'Install and restart' })).toBeDisabled()
  await dialog.getByRole('button', { name: 'Download update' }).click()
  await expect.poll(async () => (await commandCalls(page, 'update_download')).length).toBe(1)
  await expect(commandCalls(page, 'update_install')).resolves.toHaveLength(0)
})

test('explains finishing and unconfirmed cleanup without inventing open panes', async ({
  page,
}) => {
  await boot(page, {
    snapshot: updateSnapshot({
      phase: 'ready',
      available: { version: '3.0.0-alpha.36', notes: 'Ready', date: '2026-09-09' },
      blockers: [
        { id: 'finishing-pane-cleanup-0', generation: 0 },
        { id: 'unconfirmed-pane-cleanup-0', generation: 0 },
      ],
    }),
  })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog).toContainText('Installation is waiting for closed-pane cleanup')
  await expect(dialog).toContainText('still finishing cleanup')
  await expect(dialog).toContainText('Quit and reopen ConsensFlow to install')
  await expect(dialog).not.toContainText('unconfirmed-pane-cleanup-0')
  await expect(dialog.getByRole('button', { name: 'Install and restart' })).toBeDisabled()
})

test('refreshes before install and reports a backend race or install failure', async ({ page }) => {
  const ready = updateSnapshot({
    phase: 'ready',
    available: { version: '3.0.0-alpha.36', notes: 'Ready', date: '2026-09-09' },
    downloadedBytes: 100,
    totalBytes: 100,
  })
  await boot(page, { snapshot: ready })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await page.evaluate(() => {
    window.__setCommandResult('update_install', { ok: false, error: 'pane opened before install' })
  })
  await page.getByRole('button', { name: 'Install and restart' }).click()
  await expect.poll(async () => (await commandCalls(page, 'update_install')).length).toBe(1)
  await expect(dialog).toContainText('pane opened before install')
  await expect(commandCalls(page, 'update_status')).resolves.toHaveLength(3)

  await page.evaluate(() =>
    window.__setCommandResult('update_install', { ok: false, error: 'signature mismatch' }),
  )
  await page.getByRole('button', { name: 'Install and restart' }).click()
  await expect.poll(async () => (await commandCalls(page, 'update_install')).length).toBe(2)
  await expect(dialog).toContainText('signature mismatch')
})

test('Later closes the dialog and dismisses the same-version banner', async ({ page }) => {
  const available = updateSnapshot({
    available: { version: '3.0.0-alpha.36', notes: 'Quiet release', date: '2026-09-09' },
  })
  await boot(page, { snapshot: available })
  await page.evaluate(() => window.__runUpdateTimer(10_000))
  await expect(page.locator('#update-banner')).toBeVisible()

  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: 'Later' }).click()
  await expect(dialog).not.toBeVisible()
  await expect(page.locator('#update-banner')).toBeHidden()
})

test('changing channel immediately checks the selected feed', async ({ page }) => {
  const alpha = updateSnapshot({
    available: { version: '3.0.0-alpha.36', notes: 'Alpha release', date: '2026-09-09' },
  })
  const stable = updateSnapshot({ channel: 'stable', available: null })
  const stableCandidate = updateSnapshot({
    channel: 'stable',
    available: { version: '3.0.0', notes: 'Stable release', date: '2026-09-09' },
  })
  await boot(page, { snapshot: alpha })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  const channel = page.locator('#updates-channel')
  await expect(channel).toHaveValue('alpha')
  await page.evaluate(
    ({ nextChannel, nextCheck }) => {
      window.__setCommandResult('update_channel', nextChannel)
      window.__setCommandResult('update_check', nextCheck)
    },
    { nextChannel: stable, nextCheck: stableCandidate },
  )

  await channel.selectOption('stable')
  await expect.poll(async () => (await commandCalls(page, 'update_channel')).length).toBe(1)
  await expect.poll(async () => (await commandCalls(page, 'update_check')).length).toBe(2)
  await expect(dialog).toContainText('3.0.0')
  await expect(dialog).toContainText('Stable release')
})

test('aborts install when the pre-install status refresh fails', async ({ page }) => {
  const ready = updateSnapshot({
    phase: 'ready',
    available: { version: '3.0.0-alpha.36', notes: 'Ready', date: '2026-09-09' },
    downloadedBytes: 100,
    totalBytes: 100,
  })
  await boot(page, { snapshot: ready })
  await page.evaluate(() => window.__emitTauriEvent('check-updates'))
  const dialog = page.locator('#updates-dialog')
  await expect(dialog.getByRole('button', { name: 'Install and restart' })).toBeEnabled()
  await page.evaluate(() =>
    window.__setCommandResult('update_status', { ok: false, error: 'status unavailable' }),
  )

  await dialog.getByRole('button', { name: 'Install and restart' }).click()
  await expect.poll(async () => (await commandCalls(page, 'update_status')).length).toBe(2)
  await expect(commandCalls(page, 'update_install')).resolves.toHaveLength(0)
  await expect(dialog).toContainText('status unavailable')
})
