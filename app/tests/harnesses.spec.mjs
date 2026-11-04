import { mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test } from '@playwright/test'
import { tempEnv } from '../../tests/helpers.mjs'
import { agentsServer } from './agents-server.mjs'

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
      await expect(pi.getByText(`${label} setup failed`)).toHaveCSS('color', 'rgb(244, 119, 105)')
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
