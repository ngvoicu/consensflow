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
  const row = (id, reason) => ({
    id,
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
  await answer(1, [row('codex', 'NEW codex'), row('devin', 'NEW devin')])
  await expect(page.getByText('NEW codex')).toBeVisible()
  // The older response arrives last, and changes nothing.
  await answer(0, [row('codex', 'OLD codex'), row('devin', 'OLD devin')])
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
  await answer(3, [row('codex', 'ROW codex')])
  await expect(page.getByText('ROW codex')).toBeVisible()
  await answer(2, [row('codex', 'ALL codex'), row('devin', 'ALL devin')])
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
