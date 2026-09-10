import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test } from '@playwright/test'
import { startUiServer } from '../../src/ui.js'
import { tempEnv } from '../../tests/helpers.mjs'

test('real administration page checks versions, shows Pi failure and retries without global configuration', async ({
  page,
}) => {
  const t = tempEnv()
  mkdirSync(t.env.HOME, { recursive: true })
  mkdirSync(t.env.PATH, { recursive: true })
  mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
  writeFileSync(join(t.env.PATH, 'pi'), '#!/bin/sh\necho 1.2.3\n')
  chmodSync(join(t.env.PATH, 'pi'), 0o755)
  writeFileSync(join(t.env.CONSENSFLOW_HOME, 'extensions'), 'installation blocked')
  let checks = 0
  const server = await startUiServer(t.env, {
    harnessLatest: async () => {
      checks++
      return '1.2.4'
    },
  })
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    const pi = page.locator('.host').filter({ has: page.locator('strong', { hasText: /^pi$/ }) })
    await expect(pi).toContainText('Version: 1.2.3')
    await expect(pi).toContainText('New release: 1.2.4')
    await expect(pi).toContainText('Pi extension missing / installation failed')
    await expect(pi.getByText(/Pi extension missing/)).toHaveCSS('color', 'rgb(244, 119, 105)')
    await pi.getByRole('button', { name: 'Retry extension installation' }).click()
    await expect.poll(() => checks).toBe(2)
    await expect(
      page.getByText('Role skills included in ConsensFlow', { exact: false }),
    ).toBeVisible()
    await expect(page.getByRole('button', { name: 'Update skills', exact: true })).toHaveCount(0)
    await expect(page.locator('.host')).toHaveCount(5)
    expect(errors).toEqual([])
  } finally {
    await server.close()
    t.cleanup()
  }
})
