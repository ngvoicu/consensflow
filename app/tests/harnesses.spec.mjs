import { chmodSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test } from '@playwright/test'
import { CATALOG } from '../../src/catalog.js'
import { agentsUi } from '../../src/core/agents-server.js'
import { Credentials, startApi } from '../../src/core/api.js'
import { openLedger } from '../../src/ledger/index.js'
import { addAgent, listAgents } from '../../src/roster.js'
import { tempEnv } from '../../tests/helpers.mjs'

/**
 * The agents screens the way the daemon serves them: Agents, one list of the
 * catalog's entries and the saved agents (a saved catalog entry takes its
 * entry's place), and Harnesses, behind the API, opened with the UI token.
 */
async function agentsServer(env, options = {}) {
  mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
  const ledger = openLedger(join(env.CONSENSFLOW_HOME, 'consensflow.db'))
  const token = 'ui-token'
  const server = await startApi({
    ledger,
    credentials: new Credentials(),
    ui: agentsUi(env, { token, ...options }),
  })
  return {
    url: server.url,
    token,
    async close() {
      await server.close()
      ledger.close()
    },
  }
}

/** A catalog entry not saved: its row offers Add. */
const offer = (page, name) =>
  page
    .locator('#agents .offer')
    .filter({ has: page.locator('.offer__name', { hasText: new RegExp(`^${name}$`) }) })
/** A saved agent: its card, in its catalog entry's place when it came from one. */
const member = (page, name) =>
  page
    .locator('#agents .member')
    .filter({ has: page.locator('.callsign', { hasText: new RegExp(`^${name}$`) }) })

for (const [harness, label] of [
  ['pi', 'Pi'],
  ['opencode', 'OpenCode'],
]) {
  test(`real administration page checks ${label} setup and retries without global configuration`, async ({
    page,
  }) => {
    const t = tempEnv()
    mkdirSync(t.env.HOME, { recursive: true })
    mkdirSync(t.env.PATH, { recursive: true })
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.PATH, harness), '#!/bin/sh\necho 1.2.3\n')
    chmodSync(join(t.env.PATH, harness), 0o755)
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'extensions'), 'installation blocked')
    let checks = 0
    const server = await agentsServer(t.env, {
      harnessLatest: async () => {
        checks++
        return '1.2.4'
      },
    })
    const errors = []
    page.on('pageerror', (error) => errors.push(error.message))
    try {
      await page.goto(`${server.url}/harnesses?token=${server.token}`)
      const pi = page
        .locator('.host')
        .filter({ has: page.locator('strong', { hasText: new RegExp(`^${harness}$`) }) })
      await expect(pi).toContainText('Version 1.2.3, 1.2.4 is out')
      await expect(pi).toContainText('Update it the way you installed it.')
      await expect(pi).toContainText(`${label} setup failed`)
      await expect(pi.getByText(`${label} setup failed`)).toHaveCSS('color', 'rgb(244, 119, 105)')
      await expect(page.locator('body')).not.toContainText(
        /Integration:|result receipt|live connection|connection status/,
      )
      rmSync(join(t.env.CONSENSFLOW_HOME, 'extensions'))
      await pi.getByRole('button', { name: `Retry ${label} setup` }).click()
      await expect.poll(() => checks).toBe(2)
      await expect(pi.getByRole('button', { name: `Retry ${label} setup` })).toHaveCount(0)
      await expect(page.locator('body')).not.toContainText(
        /Integration:|result receipt|live connection|delivery verified/,
      )
      await expect(page.getByRole('link')).toHaveCount(0)
      await expect(
        page.getByText('Role skills included in ConsensFlow', { exact: false }),
      ).toHaveCount(0)
      await expect(page.getByRole('button', { name: 'Update skills', exact: true })).toHaveCount(0)
      await expect(page.locator('.host')).toHaveCount(6)
      await expect(page.locator('.host').filter({ hasText: 'claude' })).toContainText(
        'Not installed',
      )
      for (const label of ['Turn off', 'Reset everything']) {
        await expect(page.getByRole('button', { name: label, exact: true })).toHaveCount(0)
      }
      await page.getByRole('button', { name: 'Check all harnesses' }).click()
      await expect.poll(() => checks).toBe(3)
      expect(errors).toEqual([])
    } finally {
      await server.close()
      t.cleanup()
    }
  })
}

test('Agents adds and removes a saved agent without loading harness diagnostics or system facts', async ({
  page,
}) => {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  const requests = []
  page.on('request', (request) => requests.push(new URL(request.url()).pathname))
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    await expect(page.locator('#lede')).toContainText('add one from the catalog')
    const form = page.locator('#add')
    await form.locator('[name="name"]').fill('custom')
    await form.locator('[name="harness"]').selectOption('claude')
    await form.locator('[name="model"]').fill('example-model')
    // The same choices a catalog agent gets: an effort and a work tier.
    await expect(form.getByLabel('Work tier').locator('option')).toHaveText([
      'Automatic (model and reasoning)',
      'Critical work',
      'Complex work',
      'Standard work',
      'Light work',
    ])
    await form.getByLabel('Work tier').selectOption('complex')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(member(page, 'custom')).toBeVisible()
    // Its own model card carries the tier, said once for the model and reasoning.
    await expect(
      page
        .locator('.model-group')
        .filter({ has: page.locator('.callsign', { hasText: /^custom$/ }) })
        .locator('.model-summary .tier-pill'),
    ).toHaveText('T2 · Complex work')
    expect(listAgents(t.env).find((a) => a.name === 'custom').workTier).toBe('complex')
    await expect(form.getByLabel('Work tier')).toHaveValue('auto')
    await expect(page.locator('#lede')).toContainText('1 saved')
    await member(page, 'custom').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'custom')).toHaveCount(0)
    await expect(page.locator('#lede')).toContainText('add one from the catalog')
    for (const selector of ['#system', '#off', '#reset', '.host']) {
      await expect(page.locator(selector)).toHaveCount(0)
    }
    await expect(page.getByText('Talking to an agent', { exact: true })).toHaveCount(0)
    await expect(page.locator('.cmds')).toHaveCount(0)
    expect(requests).not.toContain('/api/system')
    expect(requests).not.toContain('/api/harnesses/check')
  } finally {
    await server.close()
    t.cleanup()
  }
})

test('Agents shows a saved agent as the catalog has it now, with nothing to press', async ({
  page,
}) => {
  const t = tempEnv()
  addAgent(
    { name: 'diana', harness: 'codex', model: 'gpt-5.5', effort: 'xhigh', preset: 'diana' },
    t.env,
  )
  const server = await agentsServer(t.env)
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    await expect(member(page, 'diana')).toBeVisible()
    await expect(page.locator('#agents').getByRole('button', { name: /^Update/ })).toHaveCount(0)
    expect(listAgents(t.env).find((p) => p.name === 'diana').model).toBe('gpt-5.6-luna')
    await page.locator('#agents').getByRole('button', { name: 'Edit', exact: true }).click()
    const model = page.locator('#agents input[name="model"]')
    await expect(model).toHaveValue('gpt-5.6-luna')
    await model.fill('')
    await page.locator('#agents').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByRole('status')).toContainText('model')
    await expect(model).toBeVisible()
  } finally {
    await server.close()
    t.cleanup()
  }
})

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
    await expect(page.locator('.host')).toHaveCount(6)
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
    lead: true,
    checkedAt: Date.now(),
    version: { state: 'checked', value: version },
    distribution: 'Homebrew',
    update,
  })
  const others = ['claude', 'opencode', 'pi', 'kimi', 'devin'].map((id) => ({
    id,
    path: null,
    installed: false,
    lead: id !== 'kimi',
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
    await expect(page.locator('.host').nth(5)).not.toContainText('next prompt')
  } finally {
    await server.close()
    t.cleanup()
  }
})

/**
 * The Agents screen in two tabs, `page` and `second`, the way the app has it
 * open beside the board while another window changes the saved agents. The
 * `saved` helper narrows a tab to the saved agents alone (Show: Saved only).
 */
async function catalogPage(page, agents = [], benchmarks, group = 'none') {
  const t = tempEnv()
  if (benchmarks) {
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(
      join(t.env.CONSENSFLOW_HOME, 'artificial-analysis-cache.json'),
      JSON.stringify(benchmarks),
    )
  }
  for (const agent of agents) addAgent(agent, t.env)
  const server = await agentsServer(t.env)
  const second = await page.context().newPage()
  await second.setViewportSize(page.viewportSize())
  await second.emulateMedia({
    colorScheme: await page.evaluate(() =>
      matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
    ),
  })
  await second.goto(`${server.url}/?token=${server.token}`)
  await page.goto(`${server.url}/?token=${server.token}`)
  await expect(page.locator('#agents .offer, #agents .member').first()).toBeVisible()
  // Full-row tests select None explicitly; null exercises the actual page default.
  if (group !== null)
    for (const screen of [page, second]) await screen.getByLabel('Group by').selectOption(group)
  return {
    t,
    server,
    second,
    saved: (screen) => screen.getByLabel('Show', { exact: true }).selectOption('saved'),
    close: async () => {
      await second.close()
      await server.close()
      t.cleanup()
    },
  }
}

function benchmarkCache(tier = 'pro') {
  return {
    schemaVersion: 1,
    status: 'ready',
    fetchedAt: new Date().toISOString(),
    tier,
    indexVersion: 4.3,
    models: {
      'gpt-6-astra': {
        name: 'GPT-6 Astra (max)',
        scores: {
          intelligence: 51.11,
          coding: 0,
          agentic: 40,
          ...(tier === 'pro' ? { hallucinations: 80, accuracy: 62, terminal: 60 } : {}),
        },
      },
      'gpt-6-astra-low': {
        name: 'GPT-6 Astra (low)',
        scores: {
          intelligence: 40,
          coding: 66,
          ...(tier === 'pro' ? { hallucinations: 0, accuracy: 45, terminal: 50 } : {}),
        },
      },
      'claude-fable-5-1': {
        name: 'Claude Fable 5.1 (Adaptive Reasoning, Max Effort, Default Fallback)',
        scores: {
          intelligence: 51.12,
          coding: 77,
          agentic: 55,
          ...(tier === 'pro' ? { hallucinations: 25, accuracy: 68, terminal: 55 } : {}),
        },
      },
    },
  }
}

async function refreshAgents(page) {
  await Promise.all([
    page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === '/api/agents' && response.request().method() === 'GET',
    ),
    page.evaluate(() => window.postMessage('consensflow:refresh-agents', location.origin)),
  ])
}

test('Kimi K3 effort follows the catalog until edited, and edits are validated', async ({
  page,
}) => {
  const fixture = await catalogPage(
    page,
    [
      { name: 'saved-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', preset: 'ilmarinen' },
      { name: 'low-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'low' },
      { name: 'high-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'high' },
    ],
    undefined,
    null,
  )
  const { second } = fixture
  try {
    await page.getByRole('searchbox').fill('Kimi')
    // The saved agent sits in its catalog entry's place, whatever its own effort.
    await expect(member(page, 'saved-kimi').locator('..').getByRole('heading')).toHaveText(
      'Kimi K3 · Max · 5',
    )
    await expect(offer(page, 'ilmarinen')).toHaveCount(0)
    await expect(page.locator('#agents')).not.toContainText(/K2\.7|seppo|ahti/)
    await fixture.saved(second)
    await second.getByRole('searchbox').fill('Kimi')
    await expect(second.locator('#agents').getByRole('heading')).toHaveText([
      'Kimi K3 · Max · 1',
      'Kimi K3 · High · 1',
      'Kimi K3 · Low · 1',
    ])
    // Saved without an effort, it took the catalog's the moment the screen read it.
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBe('max')
    await expect(second.locator('#agents').getByRole('button', { name: /^Update/ })).toHaveCount(0)
    await member(second, 'saved-kimi').getByRole('button', { name: 'Edit', exact: true }).click()
    await member(second, 'saved-kimi').locator('input[name=effort]').fill('high')
    await member(second, 'saved-kimi').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(member(second, 'saved-kimi').locator('form')).toHaveCount(0)
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBe('high')
    await member(second, 'saved-kimi').getByRole('button', { name: 'Edit', exact: true }).click()
    await member(second, 'saved-kimi').locator('input[name=effort]').fill('medium')
    await member(second, 'saved-kimi').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(second.getByRole('status')).toContainText('low, high or max')
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBe('high')
    await member(second, 'saved-kimi').getByRole('button', { name: 'Cancel', exact: true }).click()
    await member(second, 'saved-kimi').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(second, 'saved-kimi')).toHaveCount(0)
    await page.reload()
    await page.getByRole('searchbox').fill('ilmarinen')
    await offer(page, 'ilmarinen').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(member(page, 'ilmarinen')).toBeVisible()
    await expect(offer(page, 'ilmarinen')).toHaveCount(0)
    expect(listAgents(fixture.t.env).find((a) => a.name === 'ilmarinen').effort).toBe('max')
    await second.locator('#add [name=harness]').selectOption('kimi')
    await expect(second.locator('#effort-options option')).toHaveText(['low', 'high', 'max'])
  } finally {
    await fixture.close()
  }
})

test('AA score pills show exact settings, zero, attribution, explanations and unavailable metrics in both tabs', async ({
  page,
}) => {
  const fixture = await catalogPage(
    page,
    [
      { name: 'alpha', harness: 'codex', model: 'gpt-6-astra', effort: 'max' },
      { name: 'missing', harness: 'codex', model: 'gpt-6-astra', effort: 'ultra' },
    ],
    benchmarkCache('free'),
  )
  try {
    for (const screen of [page, fixture.second]) {
      await expect(
        screen.getByRole('link', { name: 'Artificial Analysis', exact: true }).first(),
      ).toBeVisible()
      const row = screen === page ? offer(page, 'astraeus') : member(screen, 'alpha')
      await expect(row.locator('.benchmark-pills').first()).toContainText('Intelligence 51.1')
      await expect(row.locator('.benchmark-pills').first()).toContainText('Coding 0.0')
      await row.locator('.benchmark-details summary').click()
      await expect(row).toContainText('GPT-6 Astra (max)')
      await expect(row).toContainText('v4.3')
      await expect(row.getByRole('link', { name: 'AA model result' })).toHaveAttribute(
        'href',
        'https://artificialanalysis.ai/models/gpt-6-astra',
      )
      await screen.locator('.benchmark-guide summary').click()
      await expect(screen.locator('.benchmark-guide')).toContainText('Free access')
      await expect(screen.locator('.benchmark-guide')).toContainText('Correct answers are excluded')
      await expect(screen.getByLabel('Sort by', { exact: true }).locator('option')).toHaveText([
        'Model and reasoning',
        'Intelligence · highest first',
        'Coding · highest first',
        'Agentic · highest first',
      ])
    }
    await expect(member(fixture.second, 'missing')).toContainText(
      'No AA score for this model and reasoning setting',
    )
    await expect(member(fixture.second, 'missing').locator('.benchmark-pill')).toHaveCount(0)
  } finally {
    await fixture.close()
  }
})

test('AA sorting uses unrounded values, lower hallucinations first, missing last and independent grouping/filter state', async ({
  page,
}) => {
  const fixture = await catalogPage(
    page,
    [
      { name: 'astra', harness: 'codex', model: 'gpt-6-astra', effort: 'max' },
      { name: 'low', harness: 'codex', model: 'gpt-6-astra', effort: 'low' },
      { name: 'fable', harness: 'claude', model: 'claude-fable-5-1', effort: 'max' },
      { name: 'missing', harness: 'codex', model: 'gpt-6-astra', effort: 'ultra' },
    ],
    benchmarkCache(),
  )
  const own = fixture.second
  try {
    await fixture.saved(own)
    await own.getByLabel('Sort by', { exact: true }).selectOption('hallucinations')
    await expect(own.locator('.callsign')).toHaveText(['low', 'fable', 'astra', 'missing'])
    await expect(page.getByLabel('Sort by', { exact: true })).toHaveValue('default')
    await expect(member(own, 'low').locator('.benchmark-pills').first()).toContainText(
      'Hallucinations 0.0%',
    )
    await own.getByLabel('Group by', { exact: true }).selectOption('model-reasoning')
    await expect(own.locator('#agents h3')).toHaveText([
      'GPT-6 Astra · Low · 1',
      'Claude Fable 5.1 · Max · 1',
      'GPT-6 Astra · Max · 1',
      'GPT-6 Astra · Ultra · 1',
    ])
    await own.getByLabel('Sort by', { exact: true }).selectOption('intelligence')
    await expect(own.locator('.callsign')).toHaveText(['fable', 'astra', 'low', 'missing'])
    await own.getByLabel('Group by', { exact: true }).selectOption('harness')
    await expect(own.locator('#agents h3')).toHaveText(['Claude Code · 1', 'Codex · 3'])
    await expect(own.locator('.callsign')).toHaveText(['fable', 'astra', 'low', 'missing'])
    await page.getByLabel('Sort by', { exact: true }).selectOption('coding')
    await expect(page.locator('.offer__name').first()).toHaveText('calliope')
    await own.getByRole('searchbox').fill('astra')
    await refreshAgents(own)
    await expect(own.getByRole('searchbox')).toHaveValue('astra')
    await expect(own.getByLabel('Sort by', { exact: true })).toHaveValue('intelligence')
    await expect(own.getByLabel('Show', { exact: true })).toHaveValue('saved')
    await own.getByRole('button', { name: 'Clear filters' }).click()
    await expect(own.getByLabel('Sort by', { exact: true })).toHaveValue('default')
    await expect(own.getByLabel('Show', { exact: true })).toHaveValue('all')
    await expect(page.getByLabel('Sort by', { exact: true })).toHaveValue('coding')
  } finally {
    await fixture.close()
  }
})

for (const colorScheme of ['light', 'dark']) {
  test(`AA benchmark pills and details wrap at 390px in ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.emulateMedia({ colorScheme })
    const fixture = await catalogPage(
      page,
      [{ name: 'alpha', harness: 'codex', model: 'gpt-6-astra', effort: 'max' }],
      benchmarkCache(),
    )
    try {
      for (const screen of [page, fixture.second]) {
        await screen.getByLabel('Sort by', { exact: true }).selectOption('hallucinations')
        await screen.locator('.benchmark-details summary').first().click()
        await screen.locator('.benchmark-guide summary').click()
        expect(
          await screen.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
        ).toBe(true)
        const pills = screen.locator('.benchmark-pill:visible')
        expect(await pills.count()).toBeGreaterThan(0)
        for (const pill of await pills.all()) {
          const bounds = await pill.boundingBox()
          expect(bounds.x).toBeGreaterThanOrEqual(0)
          expect(bounds.x + bounds.width).toBeLessThanOrEqual(390)
        }
      }
    } finally {
      await fixture.close()
    }
  })
}

test('all browsing modes keep descending effort order, saved agents among the catalog entries of their model', async ({
  page,
}) => {
  const efforts = [
    'ultra',
    'max',
    'xhigh',
    'high',
    'medium',
    'low',
    'minimal',
    'off',
    undefined,
    'unusual',
  ]
  const names = [
    'z-ultra',
    'y-max',
    'x-xhigh',
    'w-high',
    'v-medium',
    'u-low',
    't-minimal',
    's-off',
    'r-default',
    'a-unusual',
  ]
  const fixture = await catalogPage(
    page,
    efforts.map((effort, index) => ({
      name: names[index],
      harness: 'codex',
      model: 'gpt-6-astra',
      ...(effort ? { effort } : {}),
    })),
  )
  try {
    await page.getByRole('searchbox').fill('Astra')
    await fixture.saved(fixture.second)
    for (const group of ['none', 'harness', 'model-reasoning']) {
      await fixture.second.getByLabel('Group by').selectOption(group)
      await expect(fixture.second.locator('.callsign')).toHaveText(names)
      await page.getByLabel('Group by').selectOption(group)
      const actual = await page
        .locator(group === 'model-reasoning' ? '.agent-group h3' : '.offer__model')
        .allTextContents()
      const expected =
        group === 'model-reasoning'
          ? [
              'Ultra',
              'Max',
              'Xhigh',
              'High',
              'Medium',
              'Low',
              'Minimal',
              'Off',
              'Default',
              'unusual',
            ]
          : group === 'harness'
            ? [
                'Max',
                'Xhigh',
                'High',
                'Medium',
                'Low',
                'Max',
                'Xhigh',
                'High',
                'Medium',
                'Low',
                'Max',
                'Xhigh',
                'High',
                'Medium',
                'Low',
              ]
            : [
                'Max',
                'Max',
                'Max',
                'Xhigh',
                'Xhigh',
                'Xhigh',
                'High',
                'High',
                'High',
                'Medium',
                'Medium',
                'Medium',
                'Low',
                'Low',
                'Low',
              ]
      expect(
        actual.map((label) =>
          group === 'model-reasoning' ? label.split(' · ')[1] : label.split(' · ').at(-1),
        ),
      ).toEqual(expected)
    }
    await expect(fixture.second.getByRole('heading', { level: 3 })).toHaveText([
      'GPT-6 Astra · Ultra · 1',
      'GPT-6 Astra · Max · 1',
      'GPT-6 Astra · Xhigh · 1',
      'GPT-6 Astra · High · 1',
      'GPT-6 Astra · Medium · 1',
      'GPT-6 Astra · Low · 1',
      'GPT-6 Astra · Minimal · 1',
      'GPT-6 Astra · Off · 1',
      'GPT-6 Astra · Default · 1',
      'GPT-6 Astra · unusual · 1',
    ])
  } finally {
    await fixture.close()
  }
})

test('model capability order takes precedence over agent names and reasoning effort', async ({
  page,
}) => {
  const ordered = [
    ['claude-fable-5.1', 'low'],
    ['claude-opus-5', 'max'],
    ['claude-sonnet-5', 'high'],
    ['claude-haiku-5', 'max'],
    ['gpt-6-astra', 'low'],
    ['gpt-5.6-sol', 'ultra'],
    ['gpt-5.6-terra', 'high'],
    ['gpt-5.6-luna', 'high'],
    ['gemini-3.8-flash', 'max'],
    ['deepseek-v4-pro-0813', 'high'],
    ['deepseek-v4-flash-0731', 'max'],
    ['glm-5.3', 'high'],
    ['glm-5.3-flash', 'max'],
    ['kimi-k3', 'high'],
    ['qwen3.8-max', 'high'],
    ['qwen3.8-27b', 'max'],
    ['codex-image', undefined],
    ['custom-fable-model', 'ultra'],
  ]
  const names = ordered.map(
    (_, index) => `agent-${String(ordered.length - index).padStart(2, '0')}`,
  )
  const fixture = await catalogPage(
    page,
    ordered.map(([model, effort], index) => ({
      name: names[index],
      harness: model === 'codex-image' ? 'image' : 'codex',
      model,
      ...(effort ? { effort } : {}),
    })),
  )
  try {
    await fixture.saved(fixture.second)
    for (const group of ['none', 'model-reasoning']) {
      await fixture.second.getByLabel('Group by').selectOption(group)
      await expect(fixture.second.locator('.callsign')).toHaveText(names)
    }
    await fixture.second.getByLabel('Group by').selectOption('harness')
    await expect(fixture.second.locator('.callsign')).toHaveText([
      ...names.slice(0, -2),
      names.at(-1),
      names.at(-2),
    ])
    for (const group of ['none', 'model-reasoning', 'harness']) {
      await page.getByRole('searchbox').fill('GPT-')
      await page.getByLabel('Group by').selectOption(group)
      const sections = group === 'harness' ? page.locator('.agent-group') : page.locator('#agents')
      for (const section of await sections.all()) {
        const labels = await section
          .locator(group === 'model-reasoning' ? '.agent-group h3' : '.offer__model')
          .allTextContents()
        expect([...new Set(labels.map((label) => label.split(' · ')[0]))]).toEqual([
          'GPT-6 Astra',
          'GPT-5.6 Sol',
          'GPT-5.6 Terra',
          'GPT-5.6 Luna',
        ])
      }
    }
  } finally {
    await fixture.close()
  }
})

test('a saved agent shows its tier and route in its entry’s place; no card names roles', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'maia', harness: 'codex', model: 'gpt-6-astra', effort: 'medium', preset: 'maia' },
    { name: 'custom', harness: 'codex', model: 'my-model', effort: 'high', preset: 'electra' },
    { name: 'draw', harness: 'image', model: 'codex-image' },
  ])
  try {
    // A saved catalog entry takes the row: no Add, and no second description of the model.
    await expect(offer(page, 'maia')).toHaveCount(0)
    await expect(offer(page, 'electra')).toHaveCount(0)
    const maia = member(page, 'maia')
    await expect(maia.locator('.tier-pill')).toHaveText('T3 · Standard work')
    await expect(maia.locator('.agent-route')).toHaveText('Codex login')
    await expect(maia.getByRole('list', { name: 'Roles' })).toHaveCount(0)
    await expect(maia.getByRole('list', { name: 'Tags' })).toHaveCount(0)
    await expect(member(page, 'custom').locator('.agent-route')).toHaveText('Codex login')
    await expect(member(page, 'draw').locator('.tier-pill')).toHaveText('T4 · Light work')
    // No card says what roles a model suits: any saved agent takes any role.
    await expect(page.locator('.category-pill')).toHaveCount(0)
    await expect(page.getByRole('list', { name: 'Roles' })).toHaveCount(0)
    await expect(offer(page, 'skirnir').locator('.agent-route')).toHaveText('OpenRouter · API')
    // The tier follows the agent's own model and reasoning.
    await maia.getByRole('button', { name: 'Edit', exact: true }).click()
    await maia.locator('[name=effort]').fill('low')
    await maia.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(maia.locator('.tier-pill')).toHaveText('T4 · Light work')
  } finally {
    await fixture.close()
  }
})

test('a second tab refreshes saved changes while preserving its filters and open edits', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'electra', harness: 'codex', model: 'gpt-6-astra', effort: 'low', preset: 'electra' },
    { name: 'last-max', harness: 'codex', model: 'gpt-6-astra', effort: 'max' },
    { name: 'first-xhigh', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
  ])
  const { second } = fixture
  try {
    await expect(page.locator('#add')).toHaveCount(1)
    await expect(page.getByRole('searchbox')).toHaveCount(1)
    await expect(second.getByRole('searchbox')).toHaveCount(1)
    await fixture.saved(second)
    await second.getByRole('searchbox').fill('Astra')
    await second.getByLabel('Group by').selectOption('model-reasoning')
    await expect(second.getByRole('heading', { level: 3 })).toHaveText([
      'GPT-6 Astra · Max · 1',
      'GPT-6 Astra · Xhigh · 1',
      'GPT-6 Astra · Low · 1',
    ])
    const edited = member(second, 'electra')
    await edited.getByRole('button', { name: 'Edit', exact: true }).click()
    await edited.locator('[name=effort]').fill('medium')
    await second.locator('#add [name=name]').fill('another-draft')
    await page.getByRole('searchbox').fill('maia')
    await offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(member(page, 'maia')).toBeVisible()
    await expect(offer(page, 'maia')).toHaveCount(0)
    await refreshAgents(second)
    await expect(member(second, 'maia')).toBeVisible()
    await expect(edited.locator('[name=effort]')).toHaveValue('medium')
    await expect(second.locator('#add [name=name]')).toHaveValue('another-draft')
    await expect(second.getByRole('searchbox')).toHaveValue('Astra')
    await expect(second.getByLabel('Group by')).toHaveValue('model-reasoning')
    await expect(second.getByLabel('Show', { exact: true })).toHaveValue('saved')
    await member(second, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(second, 'maia')).toHaveCount(0)
    await refreshAgents(page)
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(page.getByRole('searchbox')).toHaveValue('maia')
    await expect(edited.locator('[name=effort]')).toHaveValue('medium')
  } finally {
    await fixture.close()
  }
})

test('Show, search and grouping work per tab, saved agents and catalog entries alike', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'lead-one', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
    { name: 'peer-one', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'xhigh' },
    { name: 'quick-one', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'low' },
    { name: 'custom', harness: 'claude', model: '<custom-model>', effort: 'unusual' },
    { name: 'draw', harness: 'image', model: 'gpt-image-2' },
    { name: 'default-one', harness: 'kimi', model: 'moonshot-ai/kimi-k3' },
    { name: 'off-one', harness: 'codex', model: 'gpt-6-astra', effort: 'off' },
    { name: 'minimal-one', harness: 'codex', model: 'gpt-6-astra', effort: 'minimal' },
  ])
  const own = fixture.second
  const search = own.getByRole('searchbox', { name: 'Search agents' })
  const group = own.getByLabel('Group by')
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  try {
    for (const screen of [page, own]) {
      await expect(screen.getByRole('searchbox')).toBeVisible({ timeout: 1500 })
      await expect(screen.getByLabel('Group by')).toHaveValue('none')
      await expect(screen.getByLabel('Group by').locator('option')).toHaveText([
        'None',
        'Harness',
        'Model and reasoning',
        'Work tier',
      ])
      await expect(screen.getByRole('heading', { level: 3 })).toHaveCount(0)
      await expect(screen.locator('#agents-count')).toHaveText('110 of 110 shown')
    }
    await fixture.saved(own)
    await expect(own.locator('#agents-count')).toHaveText('8 of 8 shown')
    await search.fill('Astra')
    await expect(own.locator('#agents-count')).toHaveText('5 of 8 shown')
    await expect(page.locator('#agents-count')).toHaveText('110 of 110 shown')
    await expect(own.locator('.callsign')).toHaveCount(5)
    await group.selectOption('model-reasoning')
    await expect(own.getByRole('heading', { level: 3 })).toHaveCount(4)
    await expect(
      own.getByRole('heading', { name: 'GPT-6 Astra · Xhigh · 2', exact: true }),
    ).toBeVisible()
    await page.getByRole('searchbox').fill('Astra')
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await expect(page.locator('#agents-count')).toHaveText('20 of 110 shown')
    await expect(page.getByRole('heading', { level: 3 })).toHaveText([
      'GPT-6 Astra · Max · 3',
      'GPT-6 Astra · Xhigh · 5',
      'GPT-6 Astra · High · 3',
      'GPT-6 Astra · Medium · 3',
      'GPT-6 Astra · Low · 4',
      'GPT-6 Astra · Minimal · 1',
      'GPT-6 Astra · Off · 1',
    ])
    const cardOf = (name) =>
      page.locator('.agent-group').filter({ has: page.getByRole('heading', { name, exact: true }) })
    await expect(cardOf('GPT-6 Astra · Medium · 3').locator('.offer__name')).toHaveText([
      'maia',
      'merope',
      'skirnir',
    ])
    // A saved agent defined by hand sits with the catalog's entries of its model and reasoning.
    await expect(cardOf('GPT-6 Astra · Xhigh · 5').locator('.callsign')).toHaveText([
      'lead-one',
      'peer-one',
    ])
    await expect(own.locator('.callsign')).toHaveCount(5)
    await search.fill('OpenRouter')
    await expect(own.locator('#agents')).toContainText('No agents match')
    await expect(page.locator('#agents-count')).toHaveText('20 of 110 shown')
    await own.getByRole('button', { name: 'Clear filters' }).click()
    await expect(search).toHaveValue('')
    await expect(group).toHaveValue('model-reasoning')
    await expect(own.getByLabel('Show', { exact: true })).toHaveValue('all')
    await fixture.saved(own)
    await expect(own.getByRole('heading', { level: 3 })).toHaveCount(7)
    await expect(page.getByRole('searchbox')).toHaveValue('Astra')
    await expect(page.getByLabel('Group by')).toHaveValue('model-reasoning')
    await group.selectOption('harness')
    await expect(own.getByRole('heading', { level: 3 })).toHaveText([
      'Claude Code · 1',
      'Codex · 3',
      'Pi · 2',
      'Kimi · 1',
      'Images · 1',
    ])
    await page.getByLabel('Group by').selectOption('harness')
    await expect(page.getByRole('heading', { level: 3 })).toHaveText([
      'Codex · 8',
      'OpenCode · 5',
      'Pi · 7',
    ])
    await page.getByRole('button', { name: 'Clear filters' }).click()
    await expect(page.getByRole('searchbox')).toHaveValue('')
    await expect(page.getByLabel('Group by')).toHaveValue('model-reasoning')
    await expect(page.getByRole('heading', { level: 3 })).toHaveCount(42)
    await expect(group).toHaveValue('harness')
    await group.selectOption('model-reasoning')
    for (const name of [
      'GPT-6 Astra · Off · 1',
      'GPT-6 Astra · Minimal · 1',
      'GPT-6 Astra · Low · 1',
      'GPT-6 Astra · Xhigh · 2',
      'Kimi K3 · Kimi setting · 1',
      'Codex Images · Not applicable · 1',
      '<custom-model> · unusual · 1',
    ]) {
      await expect(own.getByRole('heading', { name, exact: true })).toBeVisible()
    }
    await expect(page.locator('#agents-count')).toHaveText('110 of 110 shown')
    await expect(offer(page, 'astraeus')).toBeVisible()
    // No description of a model anywhere: its tier and scores say it all.
    await expect(page.locator('#agents')).not.toContainText('Good for')
    await expect(own.locator('.callsign')).toHaveCount(8, 'every saved agent')
    await search.fill('<custom-model>')
    await expect(own.locator('#agents')).toContainText('<custom-model>')
    await expect(own.locator('custom-model')).toHaveCount(0)
    await search.fill('lead-one')
    const row = member(own, 'lead-one')
    await row.getByRole('button', { name: 'Edit', exact: true }).click()
    await row.locator('[name=model]').fill('custom-after-edit')
    await row.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(
      own.getByRole('heading', { name: 'custom-after-edit · Xhigh · 1', exact: true }),
    ).toBeVisible()
    await expect(group).toHaveValue('model-reasoning')
    await expect(search).toHaveValue('lead-one')
    await expect(offer(page, 'astraeus')).toBeVisible()
    await search.fill('no-such-saved-agent')
    await expect(own.locator('#agents')).toContainText('No agents match')
    await page.getByRole('searchbox').fill('no-such-agent')
    await expect(page.locator('#agents')).toContainText('No agents match these filters.')
    expect(errors).toEqual([])
  } finally {
    await fixture.close()
  }
})

test('shared model cards default to every model and reasoning across all harnesses and providers in both tabs', async ({
  page,
}) => {
  const entries = Object.entries(CATALOG).flatMap(([harness, rows]) =>
    rows.map((p) => ({ ...p, harness })),
  )
  const fixture = await catalogPage(page, entries, benchmarkCache(), null)
  try {
    const expected = new Map()
    for (const p of entries) {
      const key = [
        p.profile.modelKey,
        p.harness === 'image' ? 'not-applicable' : p.effort || 'default',
      ].join('|')
      if (!expected.has(key)) expected.set(key, [])
      expected.get(key).push(p.name)
    }
    expect([...expected.values()].filter((names) => names.length > 1)).toHaveLength(31)
    for (const screen of [page, fixture.second]) {
      await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
      const cards = screen.locator('.model-group')
      await expect(cards).toHaveCount(expected.size)
      const summaries = await cards.evaluateAll((nodes) =>
        nodes.map((node) => ({
          names: [...node.querySelectorAll('.offer__name, .callsign')]
            .map((n) => n.textContent)
            .sort(),
          repeated: node.querySelectorAll(
            '.offer .category-pills, .member .category-pills, .offer .benchmark-details, .member .benchmark-details',
          ).length,
          routes: node.querySelectorAll('.offer .agent-route, .member .agent-route').length,
        })),
      )
      expect(summaries.map((s) => s.names.join(',')).sort()).toEqual(
        [...expected.values()].map((names) => names.sort().join(',')).sort(),
      )
      // The model card says once what the model is for; every saved agent
      // under it is a member card with its route, and nothing repeated.
      await expect(screen.locator('#agents .offer')).toHaveCount(0)
      for (const card of summaries) {
        expect(card.repeated).toBe(0)
        expect(card.routes).toBe(card.names.length)
      }
      const luna = cards.filter({
        has: screen.getByRole('heading', { name: 'GPT-5.6 Luna · Xhigh · 5', exact: true }),
      })
      await expect(luna.locator('.callsign')).toHaveText([
        'bil',
        'diana',
        'hjuki',
        'phoebe',
        'selene',
      ])
      await expect(luna.locator('.agent-route')).toHaveText([
        'OpenRouter · API',
        'Codex login',
        'OpenCode Go',
        'Codex subscription',
        'OpenCode Go',
      ])
      const astra = cards.filter({
        has: screen.getByRole('heading', { name: 'GPT-6 Astra · Max · 3', exact: true }),
      })
      await expect(astra.locator('.benchmark-details')).toHaveCount(1)
      await expect(
        astra.locator('.model-summary .benchmark-pill[data-metric=intelligence]'),
      ).toHaveText('Intelligence 51.1')
      await screen.getByRole('searchbox').fill('DeepSeek V4 Pro')
      await expect(screen.locator('.model-summary h3')).toHaveText([
        'DeepSeek V4 Pro (0813) · High · 2',
        'DeepSeek V4 Pro · Max · 2',
      ])
    }
  } finally {
    await fixture.close()
  }
})

test('model-level AA scores are labeled and sortable while Muse provider choices share exact scores', async ({
  page,
}) => {
  const cache = benchmarkCache('free')
  cache.models['qwen3-8-max'] = {
    name: 'Qwen3.8 Max',
    scores: { intelligence: 60, coding: 72, agentic: 50 },
  }
  cache.models['muse-spark-1-3-xhigh'] = {
    name: 'Muse Spark 1.3 (xhigh)',
    scores: { intelligence: 45.2, coding: 76.5, agentic: 51.8 },
  }
  const entries = Object.entries(CATALOG)
    .flatMap(([harness, rows]) => rows.map((p) => ({ ...p, harness })))
    .filter((p) => ['eos', 'logi', 'urania', 'odrerir', 'gefjon', 'tyr'].includes(p.name))
  const fixture = await catalogPage(page, entries, cache, null)
  try {
    for (const screen of [page, fixture.second]) {
      await screen.getByLabel('Sort by').selectOption('intelligence')
      await expect(screen.locator('.model-summary h3').first()).toContainText('Qwen 3.8 Max')
      await expect(screen.locator('.model-group').first().locator('.benchmark-context')).toHaveText(
        'AA reasoning level not specified',
      )
      await expect(
        screen
          .locator('.model-group')
          .first()
          .locator('.benchmark-pill[data-metric=intelligence]')
          .first(),
      ).toHaveText('Intelligence 60.0')
      await screen.getByRole('searchbox').fill('Muse Spark')
      await expect(screen.locator('.model-summary h3')).toHaveText('Muse Spark 1.3 · Xhigh · 5')
      await expect(screen.locator('.benchmark-details')).toHaveCount(1)
      await expect(screen.locator('.benchmark-context')).toHaveCount(0)
      await expect(screen.locator('.agent-route')).toHaveText([
        'OpenRouter · API',
        'OpenCode Zen · Contributor · Free',
        'OpenRouter · API',
        'OpenCode Go · Contributor',
        'OpenCode Go · Contributor',
      ])
      await expect(screen.locator('.agent-route-note')).toHaveText(
        Array(3).fill('Prompts and replies may train Meta models.'),
      )
    }
    const saved = listAgents(fixture.t.env)
    expect(saved.find((p) => p.name === 'tyr').profile.benchmarks.reasoningMatch).toBe(
      'unspecified',
    )
    expect(saved.find((p) => p.name === 'gefjon').model).toBe(
      'opencode/muse-spark-1.3-contributor-free',
    )
    await member(page, 'gefjon').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'gefjon').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(member(page, 'logi')).toBeVisible()
    await expect(offer(page, 'logi')).toHaveCount(0)
  } finally {
    await fixture.close()
  }
})

test('shared model cards return after reload and Clear filters in both tabs', async ({ page }) => {
  const entries = Object.entries(CATALOG)
    .flatMap(([harness, rows]) => rows.map((p) => ({ ...p, harness })))
    .filter((p) => ['clio', 'orpheus', 'saga'].includes(p.name))
  const fixture = await catalogPage(page, entries, undefined, null)
  try {
    for (const screen of [page, fixture.second]) {
      const card = screen.locator('.model-group').filter({
        has: screen.getByRole('heading', { name: 'Claude Fable 5.1 · Xhigh · 3', exact: true }),
      })
      await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
      for (const alternate of ['none', 'harness']) {
        await screen.getByLabel('Group by').selectOption(alternate)
        await expect(screen.locator('.model-summary')).toHaveCount(0)
        await screen.getByRole('searchbox').fill('clio')
        await screen.getByRole('button', { name: 'Clear filters' }).click()
        await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
        await expect(screen.getByRole('searchbox')).toHaveValue('')
        await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
        await expect(card.locator('.offer, .member')).toHaveCount(3)
      }
      await screen.reload()
      await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
      await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    }
  } finally {
    await fixture.close()
  }
})

test('shared model cards keep Fable choices independent through add, remove, filtering and edits', async ({
  page,
}) => {
  const cache = benchmarkCache()
  cache.models['claude-fable-5-1-xhigh'] = {
    name: 'Claude Fable 5.1 (Adaptive Reasoning, Xhigh Effort, Default Fallback)',
    scores: { intelligence: 53.2, coding: 80.7, agentic: 57.1 },
  }
  const trio = Object.entries(CATALOG)
    .flatMap(([harness, rows]) => rows.map((p) => ({ ...p, harness })))
    .filter((p) => ['clio', 'orpheus', 'saga'].includes(p.name))
  const fixture = await catalogPage(
    page,
    [{ ...trio.find((p) => p.name === 'clio'), description: 'My Claude notes' }],
    cache,
  )
  const { second } = fixture
  try {
    await page.getByRole('searchbox').fill('Fable')
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await page.getByLabel('Sort by').selectOption('coding')
    const card = page
      .locator('.model-group')
      .filter({ has: page.getByRole('heading', { name: /^Claude Fable 5.1 · Xhigh ·/ }) })
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 3')
    await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    await expect(card.locator('.benchmark-details')).toHaveCount(1)
    await expect(card.locator('.model-summary .benchmark-pill')).toHaveText([
      'Intelligence 53.2',
      'Coding 80.7',
      'Agentic 57.1',
    ])
    // The saved clio sits first, as a member card; orpheus and saga still offer Add.
    await expect(card.locator('.member__head .tag, .offer__model')).toHaveText([
      'Claude Code',
      'Pi',
      'OpenCode',
    ])
    await expect(member(page, 'clio')).toBeVisible()
    await expect(offer(page, 'clio')).toHaveCount(0)
    await page.screenshot({ path: '/tmp/cf-model-card-fable.png' })
    await offer(page, 'orpheus').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(member(page, 'orpheus')).toBeVisible()
    await expect(
      offer(page, 'saga').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await refreshAgents(second)
    await fixture.saved(second)
    await second.getByLabel('Group by').selectOption('model-reasoning')
    const saved = second.locator('.model-group')
    await expect(saved.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 2')
    await expect(saved.locator('.benchmark-details')).toHaveCount(1)
    await expect(saved.locator('.member')).toHaveCount(2)
    await expect(member(second, 'clio').locator('.member__desc')).toHaveCount(0)
    for (const name of ['clio', 'orpheus'])
      await member(second, name).getByRole('button', { name: 'Edit', exact: true }).click()
    await expect(member(second, 'clio').locator('[name=description]')).toHaveCount(0)
    await expect(member(second, 'clio').locator('[name=tags]')).toHaveCount(0)
    await member(second, 'clio').locator('[name=effort]').fill('medium')
    const edited = member(second, 'orpheus')
    await edited.locator('[name=effort]').fill('low')
    await edited.getByRole('button', { name: 'Save', exact: true }).click()
    // An edited catalog agent is the human's own now: it leaves its entry's group for its own.
    await expect(saved.locator('h3')).toHaveText([
      'Claude Fable 5.1 · Xhigh · 1',
      'Claude Fable 5.1 · Low · 1',
    ])
    const orpheus = listAgents(fixture.t.env).find((p) => p.name === 'orpheus')
    expect([orpheus.effort, orpheus.preset]).toEqual(['low', undefined])
    await expect(member(second, 'clio').locator('[name=effort]')).toHaveValue('medium')
    await refreshAgents(page)
    await member(page, 'orpheus').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'orpheus').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(member(page, 'clio')).toBeVisible()
    expect(listAgents(fixture.t.env).map((p) => p.name)).toEqual(['clio'])
    await expect(page.getByRole('searchbox')).toHaveValue('Fable')
    await expect(page.getByLabel('Sort by')).toHaveValue('coding')
    await page.getByRole('searchbox').fill('OpenRouter')
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 2')
    await expect(card.locator('.offer__name')).toHaveText(['orpheus', 'saga'])
    await expect(card.locator('.benchmark-details')).toHaveCount(1)
  } finally {
    await fixture.close()
  }
})

test('shared model cards keep each saved agent’s own tier on its row when the tiers differ', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'codex-ultra', harness: 'codex', model: 'gpt-6-astra', effort: 'ultra' },
    { name: 'pi-ultra', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'ultra' },
  ])
  const { second } = fixture
  try {
    await fixture.saved(second)
    await second.getByLabel('Group by').selectOption('model-reasoning')
    const card = second.locator('.model-group')
    await expect(card.locator('h3')).toHaveText('GPT-6 Astra · Ultra · 2')
    // Ultra means critical on Codex and nothing special on Pi: the card shares no tier.
    await expect(card.locator('.model-summary .tier-pill')).toHaveCount(0)
    await expect(member(second, 'codex-ultra').locator('.tier-pill')).toHaveText(
      'T1 · Critical work',
    )
    await expect(member(second, 'pi-ultra').locator('.tier-pill')).toHaveText('T4 · Light work')
    await second.getByLabel('Work tier', { exact: true }).selectOption('critical')
    await expect(card.locator('h3')).toHaveText('GPT-6 Astra · Ultra · 1')
    await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    await expect(member(second, 'codex-ultra').locator('.tier-pill')).toHaveCount(0)
    await expect(member(second, 'pi-ultra')).toHaveCount(0)
  } finally {
    await fixture.close()
  }
})

test('a saved catalog entry takes its row’s place, and removal restores Add without losing filters', async ({
  page,
}) => {
  const fixture = await catalogPage(page)
  const { second } = fixture
  try {
    await page.getByRole('searchbox', { name: 'Search agents' }).fill('maia')
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await second.getByLabel('Group by').selectOption('harness')
    await offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }).click()
    await refreshAgents(second)
    await expect(member(second, 'maia')).toBeVisible()
    await expect(second.getByLabel('Group by')).toHaveValue('harness')
    await expect(member(page, 'maia')).toBeVisible({ timeout: 1500 })
    await expect(offer(page, 'maia')).toHaveCount(0)
    await expect(
      page
        .locator('.model-group')
        .filter({ has: page.getByRole('heading', { name: 'GPT-6 Astra · Medium · 1' }) })
        .locator('.callsign'),
    ).toHaveText('maia')
    await expect(page.locator('#agents-count')).toHaveText('1 of 102 shown')
    await member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await refreshAgents(second)
    await expect(member(second, 'maia')).toHaveCount(0)
    expect(listAgents(fixture.t.env)).toHaveLength(0)
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(page.getByLabel('Group by')).toHaveValue('model-reasoning')
  } finally {
    await fixture.close()
  }
})

test('a saved agent stands in for its entry by provenance or exact identity, while a name collision keeps the row', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'first-copy', harness: 'codex', model: 'custom-edited', preset: 'maia' },
    {
      name: 'second-copy',
      harness: 'codex',
      model: 'gpt-6-astra',
      effort: 'medium',
      preset: 'maia',
    },
    { name: 'electra', harness: 'claude', model: 'custom-model' },
    {
      name: 'skirnir',
      harness: 'opencode',
      model: 'openrouter/openai/gpt-6-astra',
      effort: 'medium',
    },
    {
      name: 'unrelated',
      harness: 'opencode',
      model: 'openrouter/openai/gpt-6-astra',
      effort: 'low',
    },
  ])
  const { second } = fixture
  try {
    // Two agents from one entry: both cards, no Add.
    await expect(offer(page, 'maia')).toHaveCount(0, { timeout: 1500 })
    await expect(member(page, 'first-copy')).toBeVisible()
    await expect(member(page, 'second-copy')).toBeVisible()
    // A different agent with the entry's name: the row stays, Add cannot.
    await expect(offer(page, 'electra').getByRole('button', { name: 'Name in use' })).toBeDisabled()
    await expect(member(page, 'electra')).toBeVisible()
    // The same harness, model and effort under the entry's name, saved before provenance was kept.
    await expect(offer(page, 'skirnir')).toHaveCount(0)
    await expect(member(page, 'skirnir')).toBeVisible()
    await expect(
      offer(page, 'dagr').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(offer(page, 'electra').getByRole('button', { name: /Remove/ })).toHaveCount(0)
    await member(page, 'first-copy').getByRole('button', { name: 'Remove', exact: true }).click()
    await refreshAgents(second)
    await expect(member(second, 'first-copy')).toHaveCount(0)
    await expect(offer(page, 'maia')).toHaveCount(0)
    await expect(member(page, 'second-copy')).toBeVisible()
    await member(page, 'second-copy').getByRole('button', { name: 'Remove', exact: true }).click()
    await member(page, 'skirnir').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'skirnir').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    expect(
      listAgents(fixture.t.env)
        .map((a) => a.name)
        .sort(),
    ).toEqual(['electra', 'unrelated'])
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
  } finally {
    await fixture.close()
  }
})

test('removal guards duplicate clicks and keeps the saved card after HTTP and network errors', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'maia', harness: 'codex', model: 'gpt-6-astra', effort: 'medium', preset: 'maia' },
  ])
  let release
  let deletes = 0
  let mode = 'wait'
  const waiting = new Promise((resolve) => {
    release = resolve
  })
  await page.route(`${fixture.server.url}/api/agents/maia`, async (route) => {
    if (route.request().method() !== 'DELETE') return route.continue()
    deletes++
    if (mode === 'wait') {
      await waiting
      return route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Could not remove agent' }),
      })
    }
    if (mode === 'network') return route.abort('failed')
    return route.continue()
  })
  try {
    await member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'maia').getByRole('button', { name: 'Removing…' })).toBeDisabled()
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await member(page, 'maia')
      .getByRole('button', { name: 'Removing…' })
      .evaluate((button) => button.click())
    expect(deletes).toBe(1)
    release()
    await expect(page.getByRole('status')).toHaveText('Could not remove agent')
    await expect(
      member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    expect(listAgents(fixture.t.env).map((a) => a.name)).toEqual(['maia'])
    mode = 'network'
    await member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(page.getByRole('status')).not.toBeEmpty()
    await expect(
      member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    await refreshAgents(fixture.second)
    await expect(member(fixture.second, 'maia')).toBeVisible()
    mode = 'success'
    await member(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    expect(listAgents(fixture.t.env)).toHaveLength(0)
    expect(deletes).toBe(3)
  } finally {
    release()
    await fixture.close()
  }
})

test('pending adds reject duplicate clicks and expose server errors and concurrent conflicts', async ({
  page,
}) => {
  const fixture = await catalogPage(page)
  let release
  let posts = 0
  let mode = 'wait'
  const waiting = new Promise((resolve) => {
    release = resolve
  })
  await page.route(`${fixture.server.url}/api/agents`, async (route) => {
    if (route.request().method() !== 'POST') return route.continue()
    posts++
    if (mode === 'wait') {
      await waiting
      return route.continue()
    }
    if (mode === 'error')
      return route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Could not save agent' }),
      })
    if (mode === 'conflict')
      addAgent({ name: 'electra', harness: 'codex', model: 'custom-concurrent' }, fixture.t.env)
    return route.continue()
  })
  try {
    const row = offer(page, 'maia')
    await row.getByRole('button', { name: 'Add', exact: true }).click()
    await expect(row.getByRole('button', { name: 'Adding…' })).toBeDisabled({ timeout: 1500 })
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await expect(offer(page, 'maia').getByRole('button', { name: 'Adding…' })).toBeDisabled()
    expect(posts).toBe(1)
    release()
    await expect(member(page, 'maia')).toBeVisible()
    await expect(offer(page, 'maia')).toHaveCount(0)
    mode = 'error'
    await offer(page, 'electra').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(page.getByRole('status')).toHaveText('Could not save agent')
    await expect(
      offer(page, 'electra').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    mode = 'conflict'
    await offer(page, 'electra').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(offer(page, 'electra').getByRole('button', { name: 'Name in use' })).toBeDisabled()
    await expect(page.getByRole('status')).toContainText('already exists')
    await refreshAgents(fixture.second)
    await expect(member(fixture.second, 'electra')).toContainText('custom-concurrent')
    expect(posts).toBe(3)
  } finally {
    release()
    await fixture.close()
  }
})

test('image agents keep the Codex route and offer only their tier for editing', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'old-image', harness: 'image', model: 'gpt-image-2', description: 'My image notes' },
  ])
  const { second } = fixture
  try {
    const old = member(second, 'old-image')
    await old.getByRole('button', { name: 'Edit', exact: true }).click()
    await expect(old.locator('[name=model]')).toHaveCount(0)
    await expect(old.locator('[name=effort]')).toHaveCount(0)
    await expect(old.locator('[name=tags]')).toHaveCount(0)
    await old.getByLabel('Work tier').selectOption('standard')
    await old.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(old.locator('.tier-pill')).toHaveText('T3 · Standard work')
    await expect(offer(page, 'pygmalion')).toContainText('Codex Images')
    await expect(offer(page, 'pygmalion')).not.toContainText(/gpt-image-2|Sunburst|Flare/)
    const form = second.locator('#add')
    await form.locator('[name=harness]').selectOption('image')
    await expect(form.locator('[name=model]')).toBeHidden()
    await expect(form.locator('[name=effort]')).toBeHidden()
    await form.locator('[name=name]').fill('my-image')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(member(second, 'my-image')).toContainText('Codex Images')
    await form.locator('[name=harness]').selectOption('codex')
    await expect(form.locator('[name=model]')).toBeVisible()
    await expect(form.locator('[name=effort]')).toBeVisible()
  } finally {
    await fixture.close()
  }
})

for (const colorScheme of ['light', 'dark']) {
  for (const width of [390, 760, 1280]) {
    test(`the agents layout remains usable at ${width}px in ${colorScheme}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 })
      await page.emulateMedia({ colorScheme })
      const fixture = await catalogPage(
        page,
        [
          {
            name: 'maia',
            harness: 'codex',
            model: 'gpt-6-astra',
            effort: 'medium',
            preset: 'maia',
          },
        ],
        benchmarkCache(),
      )
      try {
        for (const screen of [page, fixture.second]) {
          const section = screen.getByRole('region', { name: 'Agents', exact: true })
          await section.getByRole('searchbox', { name: 'Search agents' }).fill('Astra')
          const contrast = await section
            .getByRole('searchbox', { name: 'Search agents' })
            .evaluate((input) => {
              const style = getComputedStyle(input)
              const luminance = (color) =>
                color
                  .match(/[\d.]+/g)
                  .slice(0, 3)
                  .map(Number)
                  .map((value) => value / 255)
                  .map((value) =>
                    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
                  )
                  .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0)
              const values = [luminance(style.color), luminance(style.backgroundColor)].sort(
                (a, b) => b - a,
              )
              return (values[0] + 0.05) / (values[1] + 0.05)
            })
          expect(contrast).toBeGreaterThanOrEqual(4.5)
          await section.getByLabel('Group by').selectOption('model-reasoning')
          await expect(section.locator('.model-summary').first()).toBeVisible()
          for (const node of await section
            .locator('.model-summary, .offer__actions, .member__head button, .benchmark-pill')
            .all()) {
            const bounds = await node.boundingBox()
            expect(bounds.x).toBeGreaterThanOrEqual(0)
            expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
          }
        }
        await expect(member(page, 'maia')).toBeVisible()
        await expect(offer(page, 'maia')).toHaveCount(0)
        expect(
          await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
        ).toBe(true)
        await expect(
          page.getByRole('button', {
            name: /Launch lead|Turn off|Reset everything|Update instructions/,
          }),
        ).toHaveCount(0)
        await page.screenshot({ path: `/tmp/cf-model-card-${colorScheme}-${width}.png` })
        for (const screen of [page, fixture.second]) {
          await screen.getByRole('button', { name: 'Clear filters' }).click()
          const pills = screen.locator('.tier-pill')
          expect(await pills.count()).toBeGreaterThan(0)
          for (const pill of await pills.all()) {
            const checks = await pill.evaluate((node) => {
              const style = getComputedStyle(node)
              const rgb = (value) =>
                value
                  .match(/[\d.]+/g)
                  .slice(0, 3)
                  .map(Number)
              const luminance = (value) =>
                rgb(value)
                  .map((v) => v / 255)
                  .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
                  .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0)
              const values = [luminance(style.color), luminance(style.backgroundColor)].sort(
                (a, b) => b - a,
              )
              const rect = node.getBoundingClientRect()
              return {
                contrast: (values[0] + 0.05) / (values[1] + 0.05),
                rounded: parseFloat(style.borderRadius) >= rect.height / 2,
                withinViewport: rect.left >= 0 && rect.right <= innerWidth,
              }
            })
            expect(checks.contrast).toBeGreaterThanOrEqual(4.5)
            expect(checks.rounded).toBe(true)
            expect(checks.withinViewport).toBe(true)
          }
        }
        const head = member(page, 'maia').locator('.member__head')
        await expect(head.getByRole('button', { name: 'Remove', exact: true })).toBeEnabled()
        const bounds = await head.boundingBox()
        expect(bounds.x).toBeGreaterThanOrEqual(0)
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
        if (width === 760) await page.screenshot({ path: `/tmp/cf-agents-${colorScheme}.png` })
      } finally {
        await fixture.close()
      }
    })
  }
}

test('work tiers filter and group the list, and a saved override keeps its honest model card', async ({
  page,
  context,
}) => {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  const own = await context.newPage()
  try {
    for (const [name, model, effort] of [
      ['specialist', 'gpt-6-astra', 'max'],
      ['builder', 'gpt-6-astra', 'xhigh'],
      ['ordinary', 'gpt-5.6-sol', 'max'],
    ])
      addAgent({ name, model, effort, harness: 'codex' }, t.env)
    await page.goto(`${server.url}/?token=${server.token}`)
    await own.goto(`${server.url}/?token=${server.token}`)
    await own.getByLabel('Show', { exact: true }).selectOption('saved')
    for (const screen of [page, own]) {
      await screen.getByLabel('Work tier', { exact: true }).selectOption('critical')
      await expect(screen.locator('.tier-pill')).not.toHaveCount(0)
      await expect(screen.locator('.tier-note').first()).toContainText('No coding')
      await screen.getByRole('button', { name: 'Clear filters' }).click()
      await expect(screen.getByLabel('Work tier', { exact: true })).toHaveValue('all')
      await screen.getByLabel('Group by').selectOption('tier')
      const headings = await screen.locator('.agent-group > h3').allTextContents()
      expect(headings[0]).toContain('Critical work')
      expect(headings[1]).toContain('Complex work')
      expect(headings[2]).toContain('Standard work')
    }
    await own.getByLabel('Show', { exact: true }).selectOption('saved')
    await own.getByLabel('Group by').selectOption('none')
    const card = own.locator('[data-agent-name=specialist]')
    await card.getByRole('button', { name: 'Edit', exact: true }).click()
    await card.getByLabel('Work tier').selectOption('standard')
    await card.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(card.locator('.tier-pill')).toContainText('Standard work')
    await expect(card.locator('.tag-pill')).toHaveCount(0)
    expect(listAgents(t.env).find((a) => a.name === 'specialist').workTier).toBe('standard')
    await own.reload()
    await own.getByLabel('Group by').selectOption('none')
    await expect(card.locator('.tier-pill')).toContainText('Standard work')
    await card.getByRole('button', { name: 'Edit', exact: true }).click()
    await card.getByLabel('Work tier').selectOption('auto')
    await card.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(card.locator('.tier-pill')).toContainText('Critical work')
    expect(listAgents(t.env).find((a) => a.name === 'specialist').workTier).toBeUndefined()
    // Catalog defaults are independent of saved overrides.
    await page.getByLabel('Work tier', { exact: true }).selectOption('critical')
    await expect(page.locator('.offer__name', { hasText: /^astraeus$/ })).toBeVisible()
    await page.screenshot({ path: '/tmp/cf-tiers-agents.png', fullPage: true })
  } finally {
    await own.close()
    await server.close()
    t.cleanup()
  }
})
