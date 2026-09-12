import { chmodSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test } from '@playwright/test'
import { CATALOG } from '../../src/catalog.js'
import { addAgent, listAgents } from '../../src/roster.js'
import { startUiServer } from '../../src/ui.js'
import { tempEnv } from '../../tests/helpers.mjs'

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
    const server = await startUiServer(t.env, {
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
      await expect(pi).toContainText('Version: 1.2.3')
      await expect(pi).toContainText('New release: 1.2.4')
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
      await expect(page.locator('.host')).toHaveCount(5)
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

test('Agents edits its roster without loading harness diagnostics or system facts', async ({
  page,
}) => {
  const t = tempEnv()
  const server = await startUiServer(t.env)
  const requests = []
  page.on('request', (request) => requests.push(new URL(request.url()).pathname))
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    await expect(page.getByText('No agents yet.', { exact: false })).toBeVisible()
    const form = page.locator('#add')
    await form.locator('[name="name"]').fill('custom')
    await form.locator('[name="harness"]').selectOption('claude')
    await form.locator('[name="model"]').fill('example-model')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(page.locator('#roster')).toContainText('custom')
    await page.locator('#roster').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(page.getByText('No agents yet.', { exact: false })).toBeVisible()
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

test('Agents retains catalog update feedback after removing the installation panel', async ({
  page,
}) => {
  const t = tempEnv()
  addAgent(
    { name: 'diana', harness: 'codex', model: 'gpt-5.5', effort: 'xhigh', preset: 'diana' },
    t.env,
  )
  const server = await startUiServer(t.env)
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    await page.locator('#roster').getByRole('button', { name: 'Update', exact: true }).click()
    await expect(page.getByRole('status')).toContainText('diana: model → gpt-5.6-luna')
    await expect(
      page.locator('#roster').getByRole('button', { name: 'Update', exact: true }),
    ).toHaveCount(0)
    await page.locator('#roster').getByRole('button', { name: 'Edit', exact: true }).click()
    const model = page.locator('#roster input[name="model"]')
    await expect(model).toHaveValue('gpt-5.6-luna')
    await model.fill('')
    await page.locator('#roster').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByRole('status')).toContainText('model')
    await expect(model).toBeVisible()
  } finally {
    await server.close()
    t.cleanup()
  }
})

test('Harnesses reports a failed initial check and allows retry', async ({ page }) => {
  const t = tempEnv()
  const server = await startUiServer(t.env)
  let fail = true
  await page.route('**/api/harnesses/check', (route) =>
    fail ? route.fulfill({ status: 503, body: '{}' }) : route.continue(),
  )
  try {
    await page.goto(`${server.url}/harnesses?token=${server.token}`)
    await expect(page.getByRole('status')).toContainText('Harness check failed')
    fail = false
    await page.getByRole('button', { name: 'Check all harnesses' }).click()
    await expect(page.locator('.host')).toHaveCount(5)
    await expect(page.getByRole('status')).toBeEmpty()
  } finally {
    await server.close()
    t.cleanup()
  }
})

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
  const server = await startUiServer(t.env)
  const roster = await page.context().newPage()
  await roster.setViewportSize(page.viewportSize())
  await roster.emulateMedia({
    colorScheme: await page.evaluate(() =>
      matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
    ),
  })
  await roster.goto(`${server.url}/?token=${server.token}`)
  await page.goto(`${server.url}/library?token=${server.token}`)
  await expect(page.locator('#catalog .offer').first()).toBeVisible()
  // Full-row tests select None explicitly; null exercises the actual page default.
  if (group !== null)
    for (const screen of [page, roster]) await screen.getByLabel('Group by').selectOption(group)
  return {
    t,
    server,
    roster,
    close: async () => {
      await roster.close()
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

test('Kimi K3 effort survives grouping, add, edit and explicit library updates', async ({
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
  const roster = fixture.roster
  try {
    await page.getByRole('searchbox').fill('Kimi')
    await expect(offer(page, 'ilmarinen').locator('..').getByRole('heading')).toHaveText(
      'Kimi K3 · Max · 5',
    )
    await expect(page.locator('#catalog')).not.toContainText(/K2\.7|seppo|ahti/)
    await expect(roster.locator('#roster').getByRole('heading')).toHaveText([
      'Kimi K3 · High · 1',
      'Kimi K3 · Low · 1',
      'Kimi K3 · Kimi setting · 1',
    ])
    await expect(
      member(roster, 'saved-kimi').getByRole('button', { name: 'Update', exact: true }),
    ).toBeVisible()
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBeUndefined()
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Update', exact: true }).click()
    await expect(roster.locator('#roster').getByRole('heading')).toHaveText([
      'Kimi K3 · Max · 1',
      'Kimi K3 · High · 1',
      'Kimi K3 · Low · 1',
    ])
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Edit', exact: true }).click()
    await member(roster, 'saved-kimi').locator('input[name=effort]').fill('high')
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(roster.locator('#roster').getByRole('heading')).toHaveText([
      'Kimi K3 · High · 2',
      'Kimi K3 · Low · 1',
    ])
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBe('high')
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Edit', exact: true }).click()
    await member(roster, 'saved-kimi').locator('input[name=effort]').fill('medium')
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Save', exact: true }).click()
    await expect(roster.getByRole('status')).toContainText('low, high or max')
    expect(listAgents(fixture.t.env).find((a) => a.name === 'saved-kimi').effort).toBe('high')
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Cancel', exact: true }).click()
    await member(roster, 'saved-kimi').getByRole('button', { name: 'Remove', exact: true }).click()
    await page.reload()
    await page.getByRole('searchbox').fill('ilmarinen')
    await offer(page, 'ilmarinen').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(
      offer(page, 'ilmarinen').getByRole('button', { name: 'Already added' }),
    ).toBeDisabled()
    expect(listAgents(fixture.t.env).find((a) => a.name === 'ilmarinen').effort).toBe('max')
    await roster.locator('#add [name=harness]').selectOption('kimi')
    await expect(roster.locator('#effort-options option')).toHaveText(['low', 'high', 'max'])
  } finally {
    await fixture.close()
  }
})

test('AA score pills show exact settings, zero, attribution, explanations and unavailable metrics on both screens', async ({
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
    for (const screen of [page, fixture.roster]) {
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
    await expect(member(fixture.roster, 'missing')).toContainText(
      'No AA score for this model and reasoning setting',
    )
    await expect(member(fixture.roster, 'missing').locator('.benchmark-pill')).toHaveCount(0)
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
  const own = fixture.roster
  try {
    await own.getByLabel('Sort by', { exact: true }).selectOption('hallucinations')
    await expect(own.locator('.callsign')).toHaveText(['low', 'fable', 'astra', 'missing'])
    await expect(page.getByLabel('Sort by', { exact: true })).toHaveValue('default')
    await expect(member(own, 'low').locator('.benchmark-pills').first()).toContainText(
      'Hallucinations 0.0%',
    )
    await own.getByLabel('Group by', { exact: true }).selectOption('model-reasoning')
    await expect(own.locator('#roster h3')).toHaveText([
      'GPT-6 Astra · Low · 1',
      'Claude Fable 5.1 · Max · 1',
      'GPT-6 Astra · Max · 1',
      'GPT-6 Astra · Ultra · 1',
    ])
    await own.getByLabel('Sort by', { exact: true }).selectOption('intelligence')
    await expect(own.locator('.callsign')).toHaveText(['fable', 'astra', 'low', 'missing'])
    await own.getByLabel('Group by', { exact: true }).selectOption('harness')
    await expect(own.locator('#roster h3')).toHaveText(['Claude Code · 1', 'Codex · 3'])
    await expect(own.locator('.callsign')).toHaveText(['fable', 'astra', 'low', 'missing'])
    await page.getByLabel('Sort by', { exact: true }).selectOption('coding')
    await expect(page.locator('.offer__name').first()).toHaveText('calliope')
    await own.getByRole('searchbox').fill('astra')
    await refreshAgents(own)
    await expect(own.getByRole('searchbox')).toHaveValue('astra')
    await expect(own.getByLabel('Sort by', { exact: true })).toHaveValue('intelligence')
    await own.getByRole('button', { name: 'Clear filters' }).click()
    await expect(own.getByLabel('Sort by', { exact: true })).toHaveValue('default')
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
      for (const screen of [page, fixture.roster]) {
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

async function refreshAgents(page) {
  await Promise.all([
    page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === '/api/agents' && response.request().method() === 'GET',
    ),
    page.evaluate(() => window.postMessage('consensflow:refresh-agents', location.origin)),
  ])
}

test('all browsing modes keep descending effort order in both agent screens', async ({ page }) => {
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
    for (const group of ['none', 'harness', 'model-reasoning']) {
      await fixture.roster.getByLabel('Group by').selectOption(group)
      await expect(fixture.roster.locator('.callsign')).toHaveText(names)
      await page.getByLabel('Group by').selectOption(group)
      const actual = await page
        .locator(group === 'model-reasoning' ? '.agent-group h3' : '.offer__model')
        .allTextContents()
      const expected =
        group === 'model-reasoning'
          ? ['Max', 'Xhigh', 'Medium', 'Low']
          : group === 'harness'
            ? [
                'Max',
                'Xhigh',
                'Medium',
                'Low',
                'Max',
                'Xhigh',
                'Medium',
                'Low',
                'Max',
                'Xhigh',
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
    await expect(fixture.roster.getByRole('heading', { level: 3 })).toHaveText([
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
    (_, index) => 'agent-' + String(ordered.length - index).padStart(2, '0'),
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
    for (const group of ['none', 'model-reasoning']) {
      await fixture.roster.getByLabel('Group by').selectOption(group)
      await expect(fixture.roster.locator('.callsign')).toHaveText(names)
    }
    await fixture.roster.getByLabel('Group by').selectOption('harness')
    await expect(fixture.roster.locator('.callsign')).toHaveText([
      ...names.slice(0, -2),
      names.at(-1),
      names.at(-2),
    ])
    for (const group of ['none', 'model-reasoning', 'harness']) {
      await page.getByRole('searchbox').fill('GPT-')
      await page.getByLabel('Group by').selectOption(group)
      const sections = group === 'harness' ? page.locator('.agent-group') : page.locator('#catalog')
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

test('category pills reflect saved profiles and update after edits without changing provider labels', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'maia', harness: 'codex', model: 'gpt-6-astra', effort: 'medium', preset: 'maia' },
    { name: 'custom', harness: 'codex', model: 'my-model', effort: 'high', preset: 'electra' },
    { name: 'draw', harness: 'image', model: 'codex-image' },
  ])
  try {
    const categories = (row) => row.getByRole('list', { name: 'Categories' }).getByRole('listitem')
    for (const row of [offer(page, 'maia'), member(fixture.roster, 'maia')]) {
      await expect(categories(row)).toHaveText(['Coding', 'Reviewer / second opinion'])
      await expect(row.locator('.agent-route')).toHaveText('Codex login')
    }
    await expect(categories(offer(page, 'electra'))).toHaveText(['Coding'])
    await expect(categories(member(fixture.roster, 'custom'))).toHaveText(['Coding'])
    await expect(categories(offer(page, 'pygmalion'))).toHaveText(['Images'])
    await expect(categories(member(fixture.roster, 'draw'))).toHaveText(['Images'])
    await expect(offer(page, 'skirnir').locator('.agent-route')).toHaveText('OpenRouter · API')
    const maia = member(fixture.roster, 'maia')
    await maia.getByRole('button', { name: 'Edit', exact: true }).click()
    await maia.locator('[name=effort]').fill('low')
    await maia.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(categories(maia)).toHaveText(['Coding'])
    await fixture.roster.getByLabel('Category', { exact: true }).selectOption('lead')
    await expect(fixture.roster.locator('.callsign')).toHaveCount(0)
    await expect(categories(offer(page, 'maia'))).toHaveText([
      'Coding',
      'Reviewer / second opinion',
    ])
  } finally {
    await fixture.close()
  }
})

test('separate agent screens refresh saved changes while preserving filters and open edits', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'electra', harness: 'codex', model: 'gpt-6-astra', effort: 'low', preset: 'electra' },
    { name: 'last-max', harness: 'codex', model: 'gpt-6-astra', effort: 'max' },
    { name: 'first-xhigh', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
  ])
  try {
    await expect(page.locator('#roster-section, #add')).toHaveCount(0)
    await expect(fixture.roster.locator('#catalog-section')).toHaveCount(0)
    await expect(page.getByRole('searchbox')).toHaveCount(1)
    await expect(fixture.roster.getByRole('searchbox')).toHaveCount(1)
    const own = fixture.roster.getByRole('region', { name: 'Your agents', exact: true })
    await own.getByRole('searchbox').fill('Astra')
    await own.getByLabel('Group by').selectOption('model-reasoning')
    await expect(own.getByRole('heading', { level: 3 })).toHaveText([
      'GPT-6 Astra · Max · 1',
      'GPT-6 Astra · Xhigh · 1',
      'GPT-6 Astra · Low · 1',
    ])
    const edited = member(fixture.roster, 'electra')
    await edited.getByRole('button', { name: 'Edit', exact: true }).click()
    await edited.locator('[name=description]').fill('Unfinished personal draft')
    await fixture.roster.locator('#add [name=name]').fill('another-draft')
    await page.getByRole('searchbox').fill('maia')
    await offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(offer(page, 'maia').getByRole('button', { name: 'Already added' })).toBeDisabled()
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'maia')).toBeVisible()
    await expect(edited.locator('[name=description]')).toHaveValue('Unfinished personal draft')
    await expect(fixture.roster.locator('#add [name=name]')).toHaveValue('another-draft')
    await expect(own.getByRole('searchbox')).toHaveValue('Astra')
    await expect(own.getByLabel('Group by')).toHaveValue('model-reasoning')
    await member(fixture.roster, 'maia')
      .getByRole('button', { name: 'Remove', exact: true })
      .click()
    await expect(member(fixture.roster, 'maia')).toHaveCount(0)
    await refreshAgents(page)
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(page.getByRole('searchbox')).toHaveValue('maia')
    await expect(edited.locator('[name=description]')).toHaveValue('Unfinished personal draft')
  } finally {
    await fixture.close()
  }
})

test('each agent list filters independently and supports optional ungrouped and harness views', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    {
      name: 'lead-one',
      harness: 'codex',
      model: 'gpt-6-astra',
      effort: 'xhigh',
      preset: 'maia',
      description: 'Personal notes',
    },
    { name: 'peer-one', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'xhigh' },
    { name: 'quick-one', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'low' },
    { name: 'custom', harness: 'claude', model: '<custom-model>', effort: 'unusual' },
    { name: 'draw', harness: 'image', model: 'gpt-image-2' },
    { name: 'default-one', harness: 'kimi', model: 'moonshot-ai/kimi-k3' },
    { name: 'off-one', harness: 'codex', model: 'gpt-6-astra', effort: 'off' },
    { name: 'minimal-one', harness: 'codex', model: 'gpt-6-astra', effort: 'minimal' },
  ])
  const own = fixture.roster.getByRole('region', { name: 'Your agents', exact: true })
  const ready = page.getByRole('region', { name: 'Agent library', exact: true })
  const search = own.getByRole('searchbox', { name: 'Search agents' })
  const category = own.getByLabel('Category', { exact: true })
  const group = own.getByLabel('Group by')
  const errors = []
  page.on('pageerror', (e) => errors.push(e.message))
  try {
    for (const section of [own, ready]) {
      await expect(section.getByRole('searchbox')).toBeVisible({ timeout: 1500 })
      await expect(section.getByLabel('Group by')).toHaveValue('none')
      await expect(section.getByLabel('Group by').locator('option')).toHaveText([
        'None',
        'Harness',
        'Model and reasoning',
      ])
      await expect(section.getByRole('heading', { level: 3 })).toHaveCount(0)
    }
    await expect(fixture.roster.locator('#roster-count')).toHaveText('8 of 8 shown')
    await expect(page.locator('#catalog-count')).toHaveText('98 of 98 shown')
    await search.fill('Astra')
    await expect(fixture.roster.locator('#roster-count')).toHaveText('5 of 8 shown')
    await expect(page.locator('#catalog-count')).toHaveText('98 of 98 shown')
    await category.selectOption('lead')
    await expect(own.locator('.callsign')).toHaveText(['lead-one', 'peer-one'])
    await group.selectOption('model-reasoning')
    await expect(own.getByRole('heading', { level: 3 })).toHaveText(['GPT-6 Astra · Xhigh · 2'])
    await ready.getByRole('searchbox').fill('Astra')
    await ready.getByLabel('Group by').selectOption('model-reasoning')
    await expect(page.locator('#catalog-count')).toHaveText('12 of 98 shown')
    await expect(ready.getByRole('heading', { level: 3 })).toHaveText([
      'GPT-6 Astra · Max · 3',
      'GPT-6 Astra · Xhigh · 3',
      'GPT-6 Astra · Medium · 3',
      'GPT-6 Astra · Low · 3',
    ])
    const medium = ready
      .locator('.agent-group')
      .filter({ has: page.getByRole('heading', { name: 'GPT-6 Astra · Medium · 3', exact: true }) })
    await expect(medium.locator('.offer__name')).toHaveText(['maia', 'merope', 'skirnir'])
    await expect(own.locator('.callsign')).toHaveText(['lead-one', 'peer-one'])
    await category.selectOption('pm')
    await expect(own).toContainText('Personal notes')
    await search.fill('OpenRouter')
    await expect(own).toContainText('No agents match')
    await expect(page.locator('#catalog-count')).toHaveText('12 of 98 shown')
    await own.getByRole('button', { name: 'Clear filters' }).click()
    await expect(search).toHaveValue('')
    await expect(category).toHaveValue('all')
    await expect(group).toHaveValue('model-reasoning')
    await expect(own.getByRole('heading', { level: 3 })).toHaveCount(7)
    await expect(ready.getByRole('searchbox')).toHaveValue('Astra')
    await expect(ready.getByLabel('Group by')).toHaveValue('model-reasoning')
    await group.selectOption('harness')
    await expect(own.getByRole('heading', { level: 3 })).toHaveText([
      'Claude Code · 1',
      'Codex · 3',
      'Pi · 2',
      'Kimi · 1',
      'Images · 1',
    ])
    await ready.getByLabel('Group by').selectOption('harness')
    await expect(ready.getByRole('heading', { level: 3 })).toHaveText([
      'Codex · 4',
      'OpenCode · 4',
      'Pi · 4',
    ])
    await ready.getByRole('button', { name: 'Clear filters' }).click()
    await expect(ready.getByRole('searchbox')).toHaveValue('')
    await expect(ready.getByLabel('Group by')).toHaveValue('model-reasoning')
    await expect(ready.getByLabel('Category', { exact: true })).toHaveValue('all')
    await expect(ready.getByRole('heading', { level: 3 })).toHaveCount(36)
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
    await category.selectOption('images')
    await expect(own.locator('.callsign')).toHaveText(['draw'])
    await expect(page.locator('#catalog-count')).toHaveText('98 of 98 shown')
    await ready.getByLabel('Category', { exact: true }).selectOption('images')
    await expect(ready.locator('.offer__name')).toHaveText(['pygmalion'])
    await expect(own).toContainText('Codex Images')
    await expect(ready).toContainText('Good for: Generate illustrations')
    await category.selectOption('coding')
    await expect(own.locator('.callsign')).toHaveCount(7)
    await search.fill('<custom-model>')
    await expect(own).toContainText('<custom-model>')
    await expect(own.locator('custom-model')).toHaveCount(0)
    await search.fill('lead-one')
    await category.selectOption('all')
    const row = member(fixture.roster, 'lead-one')
    await row.getByRole('button', { name: 'Edit', exact: true }).click()
    await row.locator('[name=model]').fill('custom-after-edit')
    await row.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(row).toContainText('custom-after-edit')
    await expect(group).toHaveValue('model-reasoning')
    await expect(search).toHaveValue('lead-one')
    await expect(ready.getByLabel('Category', { exact: true })).toHaveValue('images')
    await expect(ready.locator('.offer__name')).toHaveText(['pygmalion'])
    await category.selectOption('lead')
    await expect(own).toContainText('No agents match')
    await ready.getByRole('searchbox').fill('no-such-agent')
    await expect(ready).toContainText('No ready-made agents match')
    expect(errors).toEqual([])
  } finally {
    await fixture.close()
  }
})

const offer = (page, name) =>
  page
    .locator('#catalog .offer')
    .filter({ has: page.locator('.offer__name', { hasText: new RegExp('^' + name + '$') }) })
const member = (page, name) =>
  page
    .locator('#roster .member')
    .filter({ has: page.locator('.callsign', { hasText: new RegExp('^' + name + '$') }) })

test('shared model cards default to every model and reasoning across all harnesses and providers on both screens', async ({
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
    expect([...expected.values()].filter((names) => names.length > 1)).toHaveLength(30)
    for (const screen of [page, fixture.roster]) {
      await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
      const cards = screen.locator('.model-group')
      await expect(cards).toHaveCount(expected.size)
      const summaries = await cards.evaluateAll((nodes) =>
        nodes.map((node) => ({
          names: [...node.querySelectorAll('.offer__name, .callsign')]
            .map((n) => n.textContent)
            .sort(),
          profiles: node.querySelectorAll('.model-summary .agent-focus').length,
          categories: node.querySelectorAll('.model-summary .category-pills').length,
          repeated: node.querySelectorAll(
            '.offer .agent-focus, .member .agent-focus, .offer .category-pills, .member .category-pills, .offer .benchmark-details, .member .benchmark-details',
          ).length,
          routes: node.querySelectorAll('.offer .agent-route, .member .agent-route').length,
        })),
      )
      expect(summaries.map((s) => s.names.join(',')).sort()).toEqual(
        [...expected.values()].map((names) => names.sort().join(',')).sort(),
      )
      for (const card of summaries) {
        expect(card.profiles).toBe(1)
        expect(card.categories).toBe(1)
        expect(card.repeated).toBe(0)
        expect(card.routes).toBe(card.names.length)
      }
      const luna = cards.filter({
        has: screen.getByRole('heading', { name: 'GPT-5.6 Luna · Xhigh · 5', exact: true }),
      })
      await expect(luna.locator('.offer__name, .callsign')).toHaveText([
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
    for (const screen of [page, fixture.roster]) {
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
    await offer(page, 'gefjon').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'gefjon').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(offer(page, 'logi').getByRole('button', { name: 'Already added' })).toBeDisabled()
  } finally {
    await fixture.close()
  }
})

test('shared model cards return after reload and Clear filters on both screens', async ({
  page,
}) => {
  const entries = Object.entries(CATALOG)
    .flatMap(([harness, rows]) => rows.map((p) => ({ ...p, harness })))
    .filter((p) => ['clio', 'orpheus', 'saga'].includes(p.name))
  const fixture = await catalogPage(page, entries, undefined, null)
  try {
    for (const screen of [page, fixture.roster]) {
      const card = screen.locator('.model-group').filter({
        has: screen.getByRole('heading', { name: 'Claude Fable 5.1 · Xhigh · 3', exact: true }),
      })
      await expect(card.locator('.agent-focus')).toHaveCount(1)
      for (const alternate of ['none', 'harness']) {
        await screen.getByLabel('Group by').selectOption(alternate)
        await expect(screen.locator('.model-summary')).toHaveCount(0)
        await screen.getByRole('searchbox').fill('clio')
        await screen.getByRole('button', { name: 'Clear filters' }).click()
        await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
        await expect(screen.getByRole('searchbox')).toHaveValue('')
        await expect(card.locator('.agent-focus')).toHaveCount(1)
        await expect(card.locator('.offer, .member')).toHaveCount(3)
      }
      await screen.reload()
      await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
      await expect(card.locator('.agent-focus')).toHaveCount(1)
    }
  } finally {
    await fixture.close()
  }
})

test('shared model cards keep Fable choices independent through add, remove, filtering and edited regrouping', async ({
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
  try {
    await page.getByRole('searchbox').fill('Fable')
    await page.getByLabel('Category', { exact: true }).selectOption('lead')
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await page.getByLabel('Sort by').selectOption('coding')
    const card = page
      .locator('.model-group')
      .filter({ has: page.getByRole('heading', { name: /^Claude Fable 5.1 · Xhigh ·/ }) })
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 3')
    await expect(card.locator('.category-pills')).toHaveCount(1)
    await expect(card.locator('.agent-focus')).toHaveCount(1)
    await expect(card.locator('.benchmark-details')).toHaveCount(1)
    await expect(card.locator('.model-summary .benchmark-pill')).toHaveText([
      'Intelligence 53.2',
      'Coding 80.7',
      'Agentic 57.1',
    ])
    await expect(card.locator('.offer__model')).toHaveText(['Claude Code', 'Pi', 'OpenCode'])
    await page.screenshot({ path: '/tmp/cf-model-card-fable.png' })
    await expect(offer(page, 'clio').getByRole('button', { name: 'Already added' })).toBeDisabled()
    await offer(page, 'orpheus').getByRole('button', { name: 'Add', exact: true }).click()
    await expect(
      offer(page, 'orpheus').getByRole('button', { name: 'Already added' }),
    ).toBeDisabled()
    await expect(
      offer(page, 'saga').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await refreshAgents(fixture.roster)
    await fixture.roster.getByLabel('Group by').selectOption('model-reasoning')
    const saved = fixture.roster.locator('.model-group')
    await expect(saved.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 2')
    await expect(saved.locator('.benchmark-details')).toHaveCount(1)
    await expect(saved.locator('.cmd')).toHaveCount(2)
    await expect(member(fixture.roster, 'clio').locator('.member__desc')).toHaveText(
      'My Claude notes',
    )
    for (const name of ['clio', 'orpheus'])
      await member(fixture.roster, name).getByRole('button', { name: 'Edit', exact: true }).click()
    await member(fixture.roster, 'clio')
      .locator('[name=description]')
      .fill('Keep this unfinished draft')
    const edited = member(fixture.roster, 'orpheus')
    await edited.locator('[name=effort]').fill('low')
    await edited.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(saved.locator('h3')).toHaveText([
      'Claude Fable 5.1 · Xhigh · 1',
      'Claude Fable 5.1 · Low · 1',
    ])
    await expect(member(fixture.roster, 'clio').locator('[name=description]')).toHaveValue(
      'Keep this unfinished draft',
    )
    await refreshAgents(page)
    await offer(page, 'orpheus').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(
      offer(page, 'orpheus').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(offer(page, 'clio').getByRole('button', { name: 'Already added' })).toBeDisabled()
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

test('shared model cards retain differing role recommendations on their own harness rows', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'codex-ultra', harness: 'codex', model: 'gpt-6-astra', effort: 'ultra' },
    { name: 'pi-ultra', harness: 'pi', model: 'openai-codex/gpt-6-astra', effort: 'ultra' },
  ])
  try {
    await fixture.roster.getByLabel('Group by').selectOption('model-reasoning')
    const card = fixture.roster.locator('.model-group')
    await expect(card.locator('h3')).toHaveText('GPT-6 Astra · Ultra · 2')
    await expect(card.locator('.model-summary .category-pills')).toHaveCount(0)
    await expect(member(fixture.roster, 'codex-ultra').locator('.category-pill')).toHaveText([
      'Coding',
      'Recommended lead',
      'Recommended PM',
      'Reviewer / second opinion',
    ])
    await expect(member(fixture.roster, 'pi-ultra').locator('.category-pill')).toHaveText([
      'Coding',
    ])
    await fixture.roster.getByLabel('Category', { exact: true }).selectOption('lead')
    await expect(card.locator('h3')).toHaveText('GPT-6 Astra · Ultra · 1')
    await expect(card.locator('.model-summary .category-pill')).toHaveCount(4)
    await expect(member(fixture.roster, 'pi-ultra')).toHaveCount(0)
  } finally {
    await fixture.close()
  }
})

test('catalog entries remain visible after Add and removal restores Add without losing filters', async ({
  page,
}) => {
  const fixture = await catalogPage(page)
  try {
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByRole('searchbox', { name: 'Search agents' })
      .fill('maia')
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByLabel('Category', { exact: true })
      .selectOption('reviewer')
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByLabel('Group by')
      .selectOption('model-reasoning')
    await fixture.roster
      .getByRole('region', { name: 'Your agents', exact: true })
      .getByLabel('Group by')
      .selectOption('harness')
    await offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }).click()
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'maia')).toBeVisible()
    await expect(
      fixture.roster
        .getByRole('region', { name: 'Your agents', exact: true })
        .getByLabel('Group by'),
    ).toHaveValue('harness')
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Already added', exact: true }),
    ).toBeDisabled({ timeout: 1500 })
    await expect(page.locator('#catalog-count')).toHaveText('1 of 98 shown')
    await offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'maia')).toHaveCount(0)
    expect(listAgents(fixture.t.env)).toHaveLength(0)
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(
      page.getByRole('region', { name: 'Agent library', exact: true }).getByLabel('Group by'),
    ).toHaveValue('model-reasoning')
    await expect(
      page
        .getByRole('region', { name: 'Agent library', exact: true })
        .getByLabel('Category', { exact: true }),
    ).toHaveValue('reviewer')
  } finally {
    await fixture.close()
  }
})

test('added state follows provenance and exact legacy identity, while name collisions stay distinct', async ({
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
  try {
    await expect(offer(page, 'maia').getByRole('button', { name: 'Already added' })).toBeDisabled({
      timeout: 1500,
    })
    await expect(offer(page, 'electra').getByRole('button', { name: 'Name in use' })).toBeDisabled()
    await expect(
      offer(page, 'skirnir').getByRole('button', { name: 'Already added' }),
    ).toBeDisabled()
    await expect(
      offer(page, 'dagr').getByRole('button', { name: 'Add', exact: true }),
    ).toBeEnabled()
    await expect(offer(page, 'electra').getByRole('button', { name: /Remove/ })).toHaveCount(0)
    await offer(page, 'maia')
      .getByRole('button', { name: 'Remove first-copy', exact: true })
      .click()
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'first-copy')).toHaveCount(0)
    await expect(offer(page, 'maia').getByRole('button', { name: 'Already added' })).toBeDisabled()
    await expect(member(fixture.roster, 'second-copy')).toBeVisible()
    await offer(page, 'maia')
      .getByRole('button', { name: 'Remove second-copy', exact: true })
      .click()
    await offer(page, 'skirnir').getByRole('button', { name: 'Remove', exact: true }).click()
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

test('removal guards duplicate clicks and keeps the saved row after HTTP and network errors', async ({
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
  await page.route(fixture.server.url + '/api/agents/maia', async (route) => {
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
    await offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(offer(page, 'maia').getByRole('button', { name: 'Removing…' })).toBeDisabled()
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByLabel('Group by')
      .selectOption('model-reasoning')
    await offer(page, 'maia')
      .getByRole('button', { name: 'Removing…' })
      .evaluate((button) => button.click())
    expect(deletes).toBe(1)
    release()
    await expect(page.getByRole('status')).toHaveText('Could not remove agent')
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    expect(listAgents(fixture.t.env).map((a) => a.name)).toEqual(['maia'])
    mode = 'network'
    await offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(page.getByRole('status')).not.toBeEmpty()
    await expect(
      offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'maia')).toBeVisible()
    mode = 'success'
    await offer(page, 'maia').getByRole('button', { name: 'Remove', exact: true }).click()
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
  await page.route(fixture.server.url + '/api/agents', async (route) => {
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
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByLabel('Group by')
      .selectOption('model-reasoning')
    await expect(offer(page, 'maia').getByRole('button', { name: 'Adding…' })).toBeDisabled()
    expect(posts).toBe(1)
    release()
    await expect(offer(page, 'maia').getByRole('button', { name: 'Already added' })).toBeDisabled()
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
    await refreshAgents(fixture.roster)
    await expect(member(fixture.roster, 'electra')).toContainText('custom-concurrent')
    expect(posts).toBe(3)
  } finally {
    release()
    await fixture.close()
  }
})

test('image agents keep the Codex route and offer only meaningful editing fields', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'old-image', harness: 'image', model: 'gpt-image-2', description: 'My image notes' },
  ])
  try {
    await page
      .getByRole('region', { name: 'Agent library', exact: true })
      .getByLabel('Category', { exact: true })
      .selectOption('images')
    const old = member(fixture.roster, 'old-image')
    await old.getByRole('button', { name: 'Edit', exact: true }).click()
    await expect(old.locator('[name=model]')).toHaveCount(0)
    await expect(old.locator('[name=effort]')).toHaveCount(0)
    await old.locator('[name=description]').fill('Updated image notes')
    await old.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(old).toContainText('Updated image notes')
    await expect(offer(page, 'pygmalion')).toContainText('Codex Images')
    await expect(offer(page, 'pygmalion')).not.toContainText(/gpt-image-2|Sunburst|Flare/)
    const form = fixture.roster.locator('#add')
    await form.locator('[name=harness]').selectOption('image')
    await expect(form.locator('[name=model]')).toBeHidden()
    await expect(form.locator('[name=effort]')).toBeHidden()
    await form.locator('[name=name]').fill('my-image')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(member(fixture.roster, 'my-image')).toContainText('Codex Images')
    await form.locator('[name=harness]').selectOption('codex')
    await expect(form.locator('[name=model]')).toBeVisible()
    await expect(form.locator('[name=effort]')).toBeVisible()
  } finally {
    await fixture.close()
  }
})

for (const colorScheme of ['light', 'dark']) {
  for (const width of [390, 760, 1280]) {
    test(`catalog layout remains usable at ${width}px in ${colorScheme}`, async ({ page }) => {
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
        for (const name of ['Your agents', 'Agent library']) {
          const section = (name === 'Your agents' ? fixture.roster : page).getByRole('region', {
            name,
            exact: true,
          })
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
          await expect(section.getByLabel('Category', { exact: true })).toBeVisible()
        }
        await expect(
          offer(page, 'maia').getByRole('button', { name: 'Already added' }),
        ).toBeDisabled()
        await expect(
          page
            .getByRole('region', { name: 'Agent library', exact: true })
            .getByLabel('Category', { exact: true }),
        ).toBeVisible()
        expect(
          await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
        ).toBe(true)
        await expect(
          page.getByRole('button', {
            name: /Launch lead|Launch PM|Turn off|Reset everything|Update instructions/,
          }),
        ).toHaveCount(0)
        await page.screenshot({ path: `/tmp/cf-model-card-${colorScheme}-${width}.png` })
        for (const screen of [page, fixture.roster]) {
          await screen.getByRole('button', { name: 'Clear filters' }).click()
          const pills = screen.locator('.category-pill')
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
        const actions = offer(page, 'maia').locator('.offer__actions')
        await expect(actions.getByRole('button', { name: 'Remove', exact: true })).toBeEnabled()
        const bounds = await actions.boundingBox()
        expect(bounds.x).toBeGreaterThanOrEqual(0)
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
        if (width === 760) {
          await page.screenshot({ path: `/tmp/cf-catalog-${colorScheme}.png` })
          await fixture.roster.screenshot({ path: `/tmp/cf-your-agents-${colorScheme}.png` })
        }
      } finally {
        await fixture.close()
      }
    })
  }
}

test('reviewer category and stricter lead tags agree in saved agents and new Sol library entries', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'sol-medium', harness: 'codex', model: 'gpt-5.6-sol', effort: 'medium' },
    { name: 'opus-high', harness: 'claude', model: 'claude-opus-5', effort: 'high' },
    { name: 'astra-xhigh', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
  ])
  try {
    for (const screen of [page, fixture.roster]) {
      await expect(
        screen.getByLabel('Category', { exact: true }).locator('option[value=reviewer]'),
      ).toHaveText('Reviewer / second opinion')
      await screen.getByLabel('Category', { exact: true }).selectOption('reviewer')
    }
    await expect(fixture.roster.locator('.callsign')).toHaveText([
      'opus-high',
      'astra-xhigh',
      'sol-medium',
    ])
    await page.getByRole('searchbox').fill('Sol')
    for (const name of ['phaethon', 'asterope', 'alsvidr'])
      await expect(offer(page, name)).toBeVisible()
    for (const name of ['hemera', 'leto', 'arvakr']) await expect(offer(page, name)).toHaveCount(0)
    for (const screen of [page, fixture.roster])
      await screen.getByLabel('Category', { exact: true }).selectOption('lead')
    await expect(fixture.roster.locator('.callsign')).toHaveText(['astra-xhigh'])
    for (const name of ['phaethon', 'asterope', 'alsvidr'])
      await expect(offer(page, name)).toHaveCount(0)
    for (const screen of [page, fixture.roster])
      await screen.getByLabel('Category', { exact: true }).selectOption('pm')
    await expect(fixture.roster.locator('.callsign')).toHaveText(['astra-xhigh'])
  } finally {
    await fixture.close()
  }
})
