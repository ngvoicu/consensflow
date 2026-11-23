import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test } from '@playwright/test'
import { tempEnv } from '../../tests/helpers.mjs'
import { agentsServer } from './agents-server.mjs'

test('tested versions and installed release tell one consistent story', async ({ page }) => {
  const html = readFileSync(
    new URL('../../crates/cf-daemon/src/screens/pages/harnesses.html', import.meta.url),
    'utf8',
  )
  const row = (id, installed, release, beyond) => ({
    id,
    installed,
    path: installed ? `/test/${id}` : null,
    checkedAt: 0,
    version: installed ? { state: 'checked', value: '1.0.0' } : { state: 'not-installed' },
    update: { state: installed ? 'current' : 'not-checked' },
    tested: { newest: '2.0.0', oldest: null, release, beyond },
  })
  await page.route('http://a3.test/**', (route) => {
    if (new URL(route.request().url()).pathname === '/api/harnesses/check') {
      return route.fulfill({
        json: {
          harnesses: [
            row('claude', true, '9999.0.2', true),
            row('pi', true, '0.0.1', false),
            row('codex', false, null, false),
          ],
        },
      })
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://a3.test/harnesses')
  const rows = page.locator('.host')
  await expect(rows).toHaveCount(3)
  const claude = rows.filter({ has: page.getByText('claude', { exact: true }) })
  const claudeText = await claude.innerText()
  expect.soft(claudeText).toContain('Version 9999.0.2, up to date')
  expect.soft(claudeText).toContain('Tested up to 2.0.0.')
  expect.soft(claudeText).toContain('This version is newer and has not been checked yet.')
  const pi = rows.filter({ has: page.getByText('pi', { exact: true }) })
  const piText = await pi.innerText()
  expect.soft(piText).toContain('Version 0.0.1, up to date')
  expect.soft(piText).toContain('Tested up to 2.0.0.')
  expect.soft(piText).not.toContain('This version is newer')
  const codex = rows.filter({ has: page.getByText('codex', { exact: true }) })
  const codexText = await codex.innerText()
  expect.soft(codexText).not.toContain('Tested up to')
  expect.soft(codexText).not.toContain('This version is newer')
})

test('an older check response never replaces what a later look already showed', async ({ page }) => {
  const html = readFileSync(
    new URL('../../crates/cf-daemon/src/screens/pages/harnesses.html', import.meta.url),
    'utf8',
  )
  const row = (id, reason, stamp) => ({
    id,
    stamp,
    installed: true,
    path: `/test/${id}`,
    checkedAt: 0,
    version: { state: 'checked', value: '1.0.0' },
    update: { state: 'current', value: '1.0.0', source: 'x', command: null },
    tested: { newest: '2.0.0', oldest: null, release: null, beyond: false },
    setup: { state: 'update-required', reason },
  })
  const held = []
  await page.route('http://order.test/**', (route) => {
    if (new URL(route.request().url()).pathname === '/api/harnesses/check') {
      held.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  const answer = (at, rows) => held[at].route.fulfill({ json: { harnesses: rows } })
  await page.goto('http://order.test/harnesses')
  await expect.poll(() => held.length).toBe(1)
  // Check all begins while the opening look is still out: it is the newer.
  await page.locator('#check-all').click()
  await expect.poll(() => held.length).toBe(2)
  expect(held[1].body.refresh).toBe(true)
  await answer(1, [row('codex', 'NEW codex', 2), row('devin', 'NEW devin', 2)])
  await expect(page.getByText('NEW codex')).toBeVisible()
  // The older response arrives last, and changes nothing.
  await answer(0, [row('codex', 'OLD codex', 1), row('devin', 'OLD devin', 1)])
  await page.waitForTimeout(150)
  await expect(page.getByText('NEW codex')).toBeVisible()
  await expect(page.getByText('NEW devin')).toBeVisible()
  await expect(page.getByText('OLD codex')).toHaveCount(0)
  await expect(page.getByText('OLD devin')).toHaveCount(0)
  // A row's own look, begun after an all-harness look that has not ended, wins for that
  // row and for it alone.
  await page.locator('#check-all').click()
  await expect.poll(() => held.length).toBe(3)
  await page.getByRole('button', { name: 'Check again' }).first().click()
  await expect.poll(() => held.length).toBe(4)
  expect(held[3].body.id).toBe('codex')
  await answer(3, [row('codex', 'ROW codex', 4)])
  await expect(page.getByText('ROW codex')).toBeVisible()
  await answer(2, [row('codex', 'ALL codex', 3), row('devin', 'ALL devin', 3)])
  await expect(page.getByText('ALL devin')).toBeVisible()
  await expect(page.getByText('ROW codex')).toBeVisible()
  await expect(page.getByText('ALL codex')).toHaveCount(0)
})

test('Harnesses loads the bundled Geist in standalone and framed views', async ({
  page,
  browserName,
}) => {
  const t = tempEnv()
  const server = await agentsServer(t.env, { installed: [] })
  const outside = []
  await page.route('**/*', (route) => {
    const url = new URL(route.request().url())
    if (url.origin !== server.url) {
      outside.push(url.href)
      return route.abort()
    }
    if (url.pathname === '/api/harnesses/check') {
      return route.fulfill({ json: { harnesses: [] } })
    }
    return route.continue()
  })

  const headingUsesGeist = async (heading) => {
    const result = await heading.evaluate(async (element) => {
      await document.fonts.ready
      const style = getComputedStyle(element)
      const faces = []
      // The five subsets in the board's Geist stylesheet, including the heading's Latin.
      for (const text of ['Harnesses', 'Ā', 'Ж', 'Ѡ', 'ắ']) {
        const loaded = await document.fonts.load(
          `${style.fontWeight} ${style.fontSize} Geist`,
          text,
        )
        faces.push({ text, loaded: loaded.map(({ family, status }) => ({ family, status })) })
      }
      return { family: style.fontFamily, faces }
    })
    expect(result.family.split(',')[0].replaceAll('"', '')).toBe('Geist')
    for (const { text, loaded } of result.faces) {
      expect(loaded, `loaded Geist for ${text}`).not.toEqual([])
      expect(loaded.every((face) => face.family === 'Geist' && face.status === 'loaded')).toBe(true)
    }
  }

  try {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme })
      await page.goto(`${server.url}/harnesses?token=${server.token}`)
      await headingUsesGeist(page.getByRole('heading', { name: 'Harnesses' }))
      await expect(page.locator('body')).toHaveCSS(
        'background-color',
        scheme === 'light' ? 'rgb(255, 255, 255)' : 'rgb(9, 9, 11)',
      )
      if (browserName === 'chromium') {
        // A declared family alone can still draw in a fallback: ask Chromium what drew the heading.
        const cdp = await page.context().newCDPSession(page)
        await cdp.send('DOM.enable')
        await cdp.send('CSS.enable')
        const { root } = await cdp.send('DOM.getDocument')
        const { nodeId } = await cdp.send('DOM.querySelector', {
          nodeId: root.nodeId,
          selector: 'h1',
        })
        const { fonts } = await cdp.send('CSS.getPlatformFontsForNode', { nodeId })
        expect(fonts).toContainEqual(
          expect.objectContaining({ familyName: 'Geist', isCustomFont: true }),
        )
        await cdp.detach()
      }
      await page.evaluate((url) => {
        const frame = document.createElement('iframe')
        frame.title = 'Harnesses'
        frame.sandbox = 'allow-forms allow-same-origin allow-scripts'
        frame.src = url
        document.body.append(frame)
      }, `${server.url}/harnesses?token=${server.token}`)
      await headingUsesGeist(
        page.frameLocator('iframe[title="Harnesses"]').getByRole('heading', { name: 'Harnesses' }),
      )
      await expect(page.frameLocator('iframe[title="Harnesses"]').locator('body')).toHaveCSS(
        'background-color',
        'rgb(9, 9, 11)',
      )
    }
    expect(outside).toEqual([])
  } finally {
    await server.close()
    t.cleanup()
  }
})

for (const [harness, label] of [['pi', 'Pi']]) {
  test(`real administration page checks ${label} setup and retries without global configuration`, async ({
    page,
  }) => {
    const t = tempEnv()
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'extensions'), 'installation blocked')
    // The harness is a stand-in at 1.2.3 that counts how often it is asked its version,
    // which a check does once. What the feeds say of a newer release is theirs, and the
    // screen's words for it are held where a check is answered (the route test below).
    const server = await agentsServer(t.env, { installed: [harness] })
    const checks = () => server.runs(harness)
    const errors = []
    page.on('pageerror', (error) => errors.push(error.message))
    try {
      await page.goto(`${server.url}/harnesses?token=${server.token}`)
      const pi = page
        .locator('.host')
        .filter({ has: page.locator('strong', { hasText: new RegExp(`^${harness}$`) }) })
      await expect(pi).toContainText('Version 1.2.3')
      await expect(pi).toContainText(`${label} setup failed`)
      // In the page's red for the light scheme the test browser has (#B91C1C).
      await expect(pi.getByText(`${label} setup failed`)).toHaveCSS('color', 'rgb(185, 28, 28)')
      await expect(page.locator('body')).not.toContainText(
        /Integration:|result receipt|live connection|connection status/,
      )
      rmSync(join(t.env.CONSENSFLOW_HOME, 'extensions'))
      await pi.getByRole('button', { name: `Retry ${label} setup` }).click()
      await expect.poll(checks).toBe(2)
      await expect(pi.getByRole('button', { name: `Retry ${label} setup` })).toHaveCount(0)
      await expect(page.locator('body')).not.toContainText(
        /Integration:|result receipt|live connection|delivery verified/,
      )
      await expect(page.getByRole('link')).toHaveCount(0)
      await expect(
        page.getByText('Role skills included in ConsensFlow', { exact: false }),
      ).toHaveCount(0)
      await expect(page.getByRole('button', { name: 'Update skills', exact: true })).toHaveCount(0)
      // Every harness ConsensFlow runs, Muse Code the fifth (T-946: the page listed it, and
      // the count said four).
      await expect(page.locator('.host')).toHaveCount(5)
      await expect(
        page.locator('.host').filter({ has: page.locator('strong', { hasText: /^muse$/ }) }),
      ).toHaveCount(1)
      await expect(page.locator('.host').filter({ hasText: 'claude' })).toContainText(
        'Not installed',
      )
      for (const label of ['Turn off', 'Reset everything']) {
        await expect(page.getByRole('button', { name: label, exact: true })).toHaveCount(0)
      }
      await page.getByRole('button', { name: 'Check all harnesses' }).click()
      await expect.poll(checks).toBe(3)
      expect(errors).toEqual([])
    } finally {
      await server.close()
      t.cleanup()
    }
  })
}

test('Harnesses reports a failed initial check and allows retry', async ({ page }) => {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  let fail = true
  await page.route('**/api/harnesses/check', (route) =>
    fail ? route.fulfill({ status: 503, body: '{}' }) : route.continue(),
  )
  try {
    await page.goto(`${server.url}/harnesses?token=${server.token}`)
    await expect(page.getByRole('status')).toContainText('Harness check failed')
    fail = false
    await page.getByRole('button', { name: 'Check all harnesses' }).click()
    // The retry runs the real check, which probes every harness on this
    // machine: on a busy CI runner that took longer than the 5 s default.
    await expect(page.locator('.host')).toHaveCount(5, { timeout: 30_000 })
    await expect(page.getByRole('status')).toBeEmpty()
  } finally {
    await server.close()
    t.cleanup()
  }
})

test('Harnesses says how each one was installed and updates it from a button', async ({ page }) => {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  const row = (version, update) => ({
    id: 'codex',
    path: '/opt/homebrew/Caskroom/codex/0.1/bin/codex',
    installed: true,
    checkedAt: Date.now(),
    version: { state: 'checked', value: version },
    distribution: 'Homebrew',
    update,
  })
  const others = ['claude', 'pi', 'devin'].map((id) => ({
    id,
    path: null,
    installed: false,
    checkedAt: Date.now(),
    version: { state: 'not-installed' },
    update: { state: 'not-checked' },
  }))
  const available = {
    state: 'available',
    value: '0.2.0',
    command: '/opt/homebrew/bin/brew upgrade --cask codex',
  }
  await page.route('**/api/harnesses/check', (route) =>
    route.fulfill({ json: { harnesses: [row('0.1.0', available), ...others] } }),
  )
  let asked = null
  await page.route('**/api/harnesses/update', async (route) => {
    asked = route.request().postDataJSON()
    await route.fulfill({
      json: {
        result: {
          id: 'codex',
          state: 'updated',
          before: '0.1.0',
          after: '0.2.0',
          command: available.command,
          output: '',
          harness: row('0.2.0', { state: 'current', value: '0.2.0', command: available.command }),
        },
      },
    })
  })
  try {
    await page.goto(`${server.url}/harnesses?token=${server.token}`)
    const codex = page.locator('.host').first()
    await expect(codex).toContainText('Version 0.1.0, 0.2.0 is out')
    await expect(codex).toContainText('Installed with Homebrew')
    await expect(codex).not.toContainText('Release from')
    await codex.getByRole('button', { name: 'Update to 0.2.0' }).click()
    await expect(codex).toContainText('Updated 0.1.0 → 0.2.0')
    await expect(codex).toContainText('Version 0.2.0, up to date')
    await expect(codex.getByRole('button', { name: /^Update to/ })).toHaveCount(0)
    expect(asked).toEqual({ id: 'codex' })
    await expect(page.locator('.host').nth(3)).not.toContainText('next prompt')
  } finally {
    await server.close()
    t.cleanup()
  }
})

test('Harnesses tells the human to update a harness the way it was installed when no command is known', async ({
  page,
}) => {
  const t = tempEnv()
  const server = await agentsServer(t.env, { installed: [] })
  // The check is answered here: what the real feeds say of a newer release is for no test to depend on.
  const pi = {
    id: 'pi',
    path: '/usr/local/bin/pi',
    installed: true,
    checkedAt: Date.now(),
    version: { state: 'checked', value: '1.2.3' },
    distribution: null,
    update: { state: 'available', value: '1.2.4', command: null },
  }
  const others = ['claude', 'codex', 'devin'].map((id) => ({
    id,
    path: null,
    installed: false,
    checkedAt: Date.now(),
    version: { state: 'not-installed' },
    update: { state: 'not-checked' },
  }))
  await page.route('**/api/harnesses/check', (route) =>
    route.fulfill({ json: { harnesses: [pi, ...others] } }),
  )
  try {
    await page.goto(`${server.url}/harnesses?token=${server.token}`)
    const row = page.locator('.host').first()
    await expect(row).toContainText('Version 1.2.3, 1.2.4 is out')
    await expect(row).toContainText('Installed in a way ConsensFlow does not recognize')
    await expect(row).toContainText('Update it the way you installed it.')
    await expect(row.getByRole('button', { name: /^Update to/ })).toHaveCount(0)
  } finally {
    await server.close()
    t.cleanup()
  }
})

// P2 A2 review T-2056, blockers 1 and 3: the page keeps the newest look, and an update's outcome
// is its own.
const harnessesHtml = () =>
  readFileSync(
    new URL('../../crates/cf-daemon/src/screens/pages/harnesses.html', import.meta.url),
    'utf8',
  )
const updateRow = (version, update, reason, stamp) => ({
  id: 'codex',
  stamp,
  installed: true,
  path: '/test/codex',
  checkedAt: 0,
  version: { state: 'checked', value: version },
  update,
  distribution: 'Homebrew',
  tested: { newest: '2.0.0', oldest: null, release: null, beyond: false },
  setup: reason ? { state: 'update-required', reason } : { state: 'ready' },
})
const available = { state: 'available', value: '0.171.0', command: 'brew upgrade codex' }
const current = { state: 'current', value: '0.171.0', command: 'brew upgrade codex' }
const updated = {
  id: 'codex',
  state: 'updated',
  before: '0.150.0',
  after: '0.171.0',
  command: 'brew upgrade codex',
  output: '',
  harness: updateRow('0.171.0', current, undefined, 3),
}
const failedUpdate = (output) => ({
  id: 'codex',
  state: 'failed',
  before: '0.150.0',
  after: '0.150.0',
  command: 'brew upgrade codex',
  output,
  reason: 'brew failed',
  harness: updateRow('0.150.0', available, 'Codex 0.150.0 has no native queue', 3),
})

// T-2056 blocker 1 and T-2072 finding 6: the page shows the row of the latest look, whichever
// answer arrives last. The daemon stamps each row when its look begins: the check begun during an
// update looked before the install ended, the update's own look after it, and a Refresh made
// after that is the latest of all.
for (const scope of ['row', 'all']) {
  for (const checkFirst of [true, false]) {
    for (const later of ['update', 'check']) {
      test(`a ${scope} check during an update shows the later look (${later}'s), ${checkFirst ? 'check' : 'update'} answered first`, async ({
        page,
      }) => {
        const html = harnessesHtml()
        const checks = []
        const updates = []
        await page.route('http://upd.test/**', (route) => {
          const path = new URL(route.request().url()).pathname
          if (path === '/api/harnesses/check') {
            checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
            return
          }
          if (path === '/api/harnesses/update') {
            updates.push(route)
            return
          }
          return route.fulfill({ contentType: 'text/html', body: html })
        })
        await page.goto('http://upd.test/harnesses')
        await expect.poll(() => checks.length).toBe(1)
        await checks[0].route.fulfill({
          json: {
            harnesses: [updateRow('0.150.0', available, 'Codex 0.150.0 has no native queue', 1)],
          },
        })
        await page.getByRole('button', { name: 'Update to 0.171.0' }).click()
        await expect.poll(() => updates.length).toBe(1)
        if (scope === 'all') await page.locator('#check-all').click()
        else await page.getByRole('button', { name: 'Check again' }).click()
        await expect.poll(() => checks.length).toBe(2)
        // The update's own look began at stamp 3 and read a transient refusal; the Refresh,
        // later at stamp 4, read the CLI passing. Or the Refresh began first, at stamp 2.
        const result =
          later === 'update'
            ? updated
            : {
                ...updated,
                harness: updateRow('0.171.0', current, 'Codex 0.171.0 has no native queue', 3),
              }
        const checkRow =
          later === 'update'
            ? updateRow('0.150.0', available, 'Codex 0.150.0 has no native queue', 2)
            : updateRow('0.171.0', current, undefined, 4)
        const answerCheck = () => checks[1].route.fulfill({ json: { harnesses: [checkRow] } })
        const finishUpdate = () => updates[0].fulfill({ json: { result } })
        if (checkFirst) {
          await answerCheck()
          await finishUpdate()
        } else {
          await finishUpdate()
          await answerCheck()
        }
        const codex = page.locator('.host').first()
        await expect(codex).toContainText('Updated 0.150.0 → 0.171.0')
        await page.waitForTimeout(150)
        await expect(codex).toContainText('Version 0.171.0, up to date')
        await expect(codex).not.toContainText('no native queue')
        await expect(codex.getByRole('button', { name: /^Update to/ })).toHaveCount(0)
      })
    }
  }
}

// T-2091: a page that opens or reloads during an install says so from the running list in each
// check's answer, offers no second install and shows Refresh. It polls nothing: the row it shows
// after the install is the one Refresh asks for.
test('a page opened while an update runs says so, offers no second update and polls nothing', async ({
  page,
}) => {
  const html = harnessesHtml()
  const checks = []
  const updates = []
  await page.route('http://reload.test/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/harnesses/check') {
      checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    if (path === '/api/harnesses/update') {
      updates.push(route)
      return
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://reload.test/harnesses')
  await expect.poll(() => checks.length).toBe(1)
  await checks[0].route.fulfill({
    json: {
      harnesses: [updateRow('0.150.0', available, 'Codex 0.150.0 has no native queue', 1)],
      updating: ['codex'],
    },
  })
  const codex = page.locator('.host').first()
  await expect(codex).toContainText('being updated')
  await expect(codex.getByRole('button', { name: /^Update to/ })).toBeDisabled()
  const refresh = codex.getByRole('button', { name: 'Refresh' })
  await expect(refresh).toBeVisible()
  // Nothing asks the daemon again on its own.
  await page.waitForTimeout(2600)
  expect(checks.length).toBe(1)
  expect(updates.length).toBe(0)
  // Refresh asks for this harness, and the row it gets is shown, the update over.
  await refresh.click()
  await expect.poll(() => checks.length).toBe(2)
  expect(checks[1].body).toEqual({ id: 'codex', refresh: true })
  await checks[1].route.fulfill({
    json: { harnesses: [updateRow('0.171.0', current, undefined, 3)], updating: [] },
  })
  await expect(codex).toContainText('Version 0.171.0, up to date')
  await expect(codex).not.toContainText('being updated')
  await expect(codex.getByRole('button', { name: 'Refresh' })).toHaveCount(0)
  expect(updates.length).toBe(0)
})

test('a check that says another harness is updating marks that one, and polls nothing', async ({
  page,
}) => {
  const html = harnessesHtml()
  const checks = []
  await page.route('http://other.test/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/harnesses/check') {
      checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://other.test/harnesses')
  await expect.poll(() => checks.length).toBe(1)
  const pi = {
    ...updateRow('0.8.0', available, undefined, 1),
    id: 'pi',
    path: '/test/pi',
    distribution: 'npm',
  }
  await checks[0].route.fulfill({
    json: { harnesses: [updateRow('0.150.0', current, undefined, 1), pi], updating: ['pi'] },
  })
  const rows = page.locator('.host')
  await expect(rows.nth(1)).toContainText('being updated')
  await expect(rows.nth(1).getByRole('button', { name: /^Update to/ })).toBeDisabled()
  await expect(rows.first()).not.toContainText('being updated')
  await page.waitForTimeout(2600)
  expect(checks.length).toBe(1)
})

// T-2089 blocker 3: the refusal as the daemon sends it: HTTP 400 and `{"error": words}`.
test('a second page that asks for an update the daemon refuses shows its words and keeps Update off', async ({
  page,
}) => {
  const html = harnessesHtml()
  const checks = []
  let updates = 0
  await page.route('http://tab.test/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/harnesses/check') {
      checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    if (path === '/api/harnesses/update') {
      updates += 1
      return route.fulfill({
        status: 400,
        json: { error: 'an update of Codex is already running' },
      })
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://tab.test/harnesses')
  await expect.poll(() => checks.length).toBe(1)
  await checks[0].route.fulfill({ json: { harnesses: [updateRow('0.150.0', available, undefined, 1)] } })
  await page.getByRole('button', { name: 'Update to 0.171.0' }).click()
  await expect(page.getByRole('status')).toContainText('an update of Codex is already running')
  await expect(page.getByRole('status')).not.toContainText('could not be started')
  await expect(page.getByRole('button', { name: /^Update to/ })).toBeDisabled()
  await expect(page.locator('.host').first().getByRole('button', { name: 'Refresh' })).toBeVisible()
  await page.waitForTimeout(2600)
  expect(updates).toBe(1)
  expect(checks.length).toBe(1)
})

test('a redraw during an update does not offer a second update, and an older outcome never replaces a later one', async ({
  page,
}) => {
  const html = harnessesHtml()
  const checks = []
  const updates = []
  await page.route('http://two.test/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/harnesses/check') {
      checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    if (path === '/api/harnesses/update') {
      updates.push(route)
      return
    }
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://two.test/harnesses')
  await expect.poll(() => checks.length).toBe(1)
  const piRow = {
    id: 'pi',
    installed: true,
    path: '/test/pi',
    checkedAt: 0,
    version: { state: 'checked', value: '1.0.0' },
    update: { state: 'current', value: '1.0.0', command: null },
    stamp: 1,
    distribution: 'npm',
    tested: { newest: null, oldest: null, release: null, beyond: false },
  }
  await checks[0].route.fulfill({ json: { harnesses: [updateRow('0.150.0', available, undefined, 1), piRow] } })
  await page.getByRole('button', { name: 'Update to 0.171.0' }).click()
  await expect.poll(() => updates.length).toBe(1)
  // Pi's check ends while Codex's installer runs: the redraw offers no second Codex update.
  await page.locator('.host').nth(1).getByRole('button', { name: 'Check again' }).click()
  await expect.poll(() => checks.length).toBe(2)
  await checks[1].route.fulfill({ json: { harnesses: [{ ...piRow, stamp: 2 }] } })
  await page.waitForTimeout(150)
  const button = page.locator('.host').first().getByRole('button', { name: /^Update to/ })
  await expect(button).toBeDisabled()
  expect(updates.length).toBe(1)
  await updates[0].fulfill({ json: { result: failedUpdate('OLD ERROR OUTPUT') } })
  await expect(page.locator('.host').first()).toContainText('Update failed: brew failed')
  // The row's later update succeeds and its outcome is what stays.
  await page.locator('.host').first().getByRole('button', { name: /^Update to/ }).click()
  await expect.poll(() => updates.length).toBe(2)
  await updates[1].fulfill({ json: { result: updated } })
  await expect(page.locator('.host').first()).toContainText('Updated 0.150.0 → 0.171.0')
  await expect(page.locator('.host').first()).not.toContainText('OLD ERROR OUTPUT')
})

test('an older check that succeeds does not clear the error of a later update that failed to start', async ({
  page,
}) => {
  const html = harnessesHtml()
  const checks = []
  await page.route('http://err.test/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/harnesses/check') {
      checks.push({ route, body: JSON.parse(route.request().postData() || '{}') })
      return
    }
    if (path === '/api/harnesses/update') return route.fulfill({ status: 500, body: '{}' })
    return route.fulfill({ contentType: 'text/html', body: html })
  })
  await page.goto('http://err.test/harnesses')
  await expect.poll(() => checks.length).toBe(1)
  await checks[0].route.fulfill({ json: { harnesses: [updateRow('0.150.0', available, undefined, 1)] } })
  await page.locator('#check-all').click()
  await expect.poll(() => checks.length).toBe(2)
  await page.getByRole('button', { name: 'Update to 0.171.0' }).click()
  await expect(page.getByRole('status')).toContainText('The update could not be started')
  await checks[1].route.fulfill({ json: { harnesses: [updateRow('0.150.0', available, undefined, 2)] } })
  await page.waitForTimeout(150)
  await expect(page.getByRole('status')).toContainText('The update could not be started')
})
