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
 * catalog's agents (with the human's edits on them) and the agents defined by
 * hand, and Harnesses, behind the API, opened with the UI token.
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

/** An agent's row: every catalog agent has one, and every agent defined by hand. */
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

test('Agents lists every catalog agent as one row with nothing to add, and takes an agent of your own', async ({
  page,
}) => {
  const t = tempEnv()
  const server = await agentsServer(t.env)
  const requests = []
  page.on('request', (request) => requests.push(new URL(request.url()).pathname))
  try {
    await page.goto(`${server.url}/?token=${server.token}`)
    await expect(page.locator('#agents-count')).toHaveText('102 of 102 shown')
    await expect(page.locator('#lede')).toHaveText(
      '102 agents, the catalog’s and your own; a project’s team is picked from them.',
    )
    await expect(page.locator('#agents .offer')).toHaveCount(0)
    await expect(page.locator('#agents').getByRole('button', { name: /^Add/ })).toHaveCount(0)
    const gefjon = member(page, 'gefjon')
    await expect(gefjon.getByRole('button')).toHaveCount(0)
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
    const custom = member(page, 'custom')
    await expect(custom).toBeVisible()
    await expect(custom).toHaveAttribute('data-custom', 'true')
    await expect(custom.locator('.tag--own')).toHaveText('your own')
    await expect(custom.getByRole('button', { name: 'Edit', exact: true })).toBeVisible()
    // Its own model card carries the tier, said once for the model and reasoning.
    await expect(
      page
        .locator('.model-group')
        .filter({ has: page.locator('.callsign', { hasText: /^custom$/ }) })
        .locator('.model-summary .tier-pill'),
    ).toHaveText('T2 · Complex work')
    expect(listAgents(t.env).find((a) => a.name === 'custom').workTier).toBe('complex')
    await expect(form.getByLabel('Work tier')).toHaveValue('auto')
    await expect(page.locator('#lede')).toContainText('1 is yours')
    await expect(page.locator('#agents-count')).toHaveText('103 of 103 shown')
    // A catalog name is not yours to define again.
    await form.locator('[name="name"]').fill('gefjon')
    await form.locator('[name="harness"]').selectOption('claude')
    await form.locator('[name="model"]').fill('example-model')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(page.locator('#error')).toContainText('catalog agent')
    await custom.getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'custom')).toHaveCount(0)
    await expect(page.locator('#lede')).not.toContainText('yours')
    for (const selector of ['#system', '#off', '#reset', '.host']) {
      await expect(page.locator(selector)).toHaveCount(0)
    }
    expect(requests).not.toContain('/api/system')
    expect(requests).not.toContain('/api/harnesses/check')
  } finally {
    await server.close()
    t.cleanup()
  }
})

test('Agents keeps catalog agents as the catalog has them, and Show: mine lists only your own', async ({
  page,
}) => {
  const fixture = await catalogPage(
    page,
    [
      {
        name: 'mine',
        harness: 'opencode',
        model: 'opencode/muse-spark-1.3-contributor-free',
        effort: 'low',
      },
    ],
    null,
  )
  const { second } = fixture
  try {
    const gefjon = member(page, 'gefjon')
    await expect(gefjon.getByRole('button')).toHaveCount(0)
    await expect(gefjon.locator('.tag--own')).toHaveCount(0)
    const mine = member(page, 'mine')
    await expect(mine.getByRole('button', { name: 'Edit', exact: true })).toBeVisible()
    await expect(mine.getByRole('button', { name: 'Remove', exact: true })).toBeVisible()
    await mine.getByRole('button', { name: 'Edit', exact: true }).click()
    await expect(mine.locator('[name=effort]')).toHaveAttribute(
      'placeholder',
      'effort (blank for none)',
    )
    await mine.locator('[name=effort]').fill('xhigh')
    await mine.getByRole('button', { name: 'Save', exact: true }).click()
    // It sits in the card of the model and reasoning it runs now, beside the catalog's gefjon.
    await expect(
      page
        .locator('.model-group')
        .filter({ has: page.locator('.callsign', { hasText: /^mine$/ }) })
        .locator('h3'),
    ).toHaveText('Muse Spark 1.3 · Xhigh · 6')
    expect(listAgents(fixture.t.env).find((p) => p.name === 'mine').effort).toBe('xhigh')
    await expect(page.locator('#lede')).toContainText('1 is yours')
    await fixture.saved(second)
    await expect(second.locator('.callsign')).toHaveText(['mine'])
    await mine.getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'mine')).toHaveCount(0)
    await refreshAgents(second)
    await expect(second.locator('#agents')).toContainText('No agents match these filters.')
  } finally {
    await fixture.close()
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
async function catalogPage(page, agents = [], group = 'none') {
  const t = tempEnv()
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
  await expect(page.locator('#agents .member').first()).toBeVisible()
  // Full-row tests select None explicitly; null exercises the actual page default.
  if (group !== null)
    for (const screen of [page, second]) await screen.getByLabel('Group by').selectOption(group)
  return {
    t,
    server,
    second,
    saved: (screen) => screen.getByLabel('Show', { exact: true }).selectOption('mine'),
    close: async () => {
      await second.close()
      await server.close()
      t.cleanup()
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
      { name: 'low-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'low' },
      { name: 'high-kimi', harness: 'kimi', model: 'moonshot-ai/kimi-k3', effort: 'high' },
    ],
    null,
  )
  const { second } = fixture
  try {
    await page.getByRole('searchbox').fill('Kimi')
    // The catalog's Kimi agent sits with the other K3 max entries.
    await expect(member(page, 'ilmarinen').locator('..').getByRole('heading')).toHaveText(
      'Kimi K3 · Max · 5',
    )
    await expect(page.locator('#agents')).not.toContainText(/K2\.7|seppo|ahti/)
    await fixture.saved(second)
    await second.getByRole('searchbox').fill('Kimi')
    await expect(second.locator('#agents').getByRole('heading')).toHaveText([
      'Kimi K3 · High · 1',
      'Kimi K3 · Low · 1',
    ])
    expect(listAgents(fixture.t.env).find((a) => a.name === 'ilmarinen').effort).toBe('max')
    await expect(member(page, 'ilmarinen').getByRole('button')).toHaveCount(0)
    const mine = member(second, 'low-kimi')
    await mine.getByRole('button', { name: 'Edit', exact: true }).click()
    await mine.locator('input[name=effort]').fill('high')
    await mine.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(mine.locator('form')).toHaveCount(0)
    expect(listAgents(fixture.t.env).find((a) => a.name === 'low-kimi').effort).toBe('high')
    await mine.getByRole('button', { name: 'Edit', exact: true }).click()
    await mine.locator('input[name=effort]').fill('medium')
    await mine.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(second.getByRole('status')).toContainText('low, high or max')
    expect(listAgents(fixture.t.env).find((a) => a.name === 'low-kimi').effort).toBe('high')
    await mine.locator('input[name=effort]').fill('')
    await mine.getByRole('button', { name: 'Save', exact: true }).click()
    expect(listAgents(fixture.t.env).find((a) => a.name === 'low-kimi').effort).toBeUndefined()
    await second.locator('#add [name=harness]').selectOption('kimi')
    await expect(second.locator('#effort-options option')).toHaveText(['low', 'high', 'max'])
  } finally {
    await fixture.close()
  }
})

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
      if (group !== 'model-reasoning') continue
      const actual = await page.locator('.agent-group h3').allTextContents()
      expect(actual.map((label) => label.split(' · ')[1])).toEqual([
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
      ])
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
    ['claude-opus-5.5', 'max'],
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
          .locator(
            group === 'model-reasoning' ? '.agent-group h3' : '.member__head > .tag:not(.tag--own)',
          )
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

test('every row shows its tier and route; an edit to your own agent moves its tier', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'custom', harness: 'codex', model: 'gpt-6-astra', effort: 'medium' },
    { name: 'draw', harness: 'image', model: 'codex-image' },
  ])
  try {
    const maia = member(page, 'maia')
    await expect(maia.locator('.tier-pill')).toHaveText('T3 · Standard work')
    await expect(maia.locator('.agent-route')).toHaveText('Codex login')
    await expect(page.locator('.category-pill')).toHaveCount(0)
    await expect(member(page, 'custom').locator('.agent-route')).toHaveText('Codex login')
    await expect(member(page, 'draw').locator('.tier-pill')).toHaveText('T4 · Light work')
    await expect(member(page, 'skirnir').locator('.agent-route')).toHaveText('OpenRouter · API')
    // The tier follows the agent's own model and reasoning, edits included.
    const custom = member(page, 'custom')
    await expect(custom.locator('.tier-pill')).toHaveText('T3 · Standard work')
    await custom.getByRole('button', { name: 'Edit', exact: true }).click()
    await custom.locator('[name=effort]').fill('low')
    await custom.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(custom.locator('.tier-pill')).toHaveText('T4 · Light work')
    await expect(maia.getByRole('button')).toHaveCount(0)
  } finally {
    await fixture.close()
  }
})

test('a second tab refreshes saved changes while preserving its filters and open edits', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'last-max', harness: 'codex', model: 'gpt-6-astra', effort: 'max' },
    { name: 'first-xhigh', harness: 'codex', model: 'gpt-6-astra', effort: 'xhigh' },
    { name: 'own-minimal', harness: 'codex', model: 'gpt-6-astra', effort: 'minimal' },
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
      'GPT-6 Astra · Minimal · 1',
    ])
    const edited = member(second, 'own-minimal')
    await edited.getByRole('button', { name: 'Edit', exact: true }).click()
    await edited.locator('[name=effort]').fill('medium')
    await second.locator('#add [name=name]').fill('another-draft')
    const form = page.locator('#add')
    await form.locator('[name=name]').fill('newbie')
    await form.locator('[name=harness]').selectOption('codex')
    await form.locator('[name=model]').fill('gpt-6-astra')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(member(page, 'newbie')).toBeVisible()
    await refreshAgents(second)
    await expect(member(second, 'newbie')).toBeVisible()
    await expect(edited.locator('[name=effort]')).toHaveValue('medium')
    await expect(second.locator('#add [name=name]')).toHaveValue('another-draft')
    await expect(second.getByRole('searchbox')).toHaveValue('Astra')
    await expect(second.getByLabel('Group by')).toHaveValue('model-reasoning')
    await expect(second.getByLabel('Show', { exact: true })).toHaveValue('mine')
    await member(second, 'newbie').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(second, 'newbie')).toHaveCount(0)
    await page.getByRole('searchbox').fill('newbie')
    await refreshAgents(page)
    await expect(member(page, 'newbie')).toHaveCount(0)
    await expect(page.getByRole('searchbox')).toHaveValue('newbie')
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
    await expect(cardOf('GPT-6 Astra · Medium · 3').locator('.callsign')).toHaveText([
      'maia',
      'merope',
      'skirnir',
    ])
    // An agent defined by hand sits with the catalog's agents of its model and reasoning.
    await expect(cardOf('GPT-6 Astra · Xhigh · 5').locator('.callsign')).toHaveText([
      'asteria',
      'delling',
      'hesperos',
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
    await expect(member(page, 'astraeus')).toBeVisible()
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
    await expect(member(page, 'astraeus')).toBeVisible()
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
  const fixture = await catalogPage(page, [], null)
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
          names: [...node.querySelectorAll('.callsign')].map((n) => n.textContent).sort(),
          routes: node.querySelectorAll('.member .agent-route').length,
        })),
      )
      expect(summaries.map((s) => s.names.join(',')).sort()).toEqual(
        [...expected.values()].map((names) => names.sort().join(',')).sort(),
      )
      // Every agent under a model card is a row with its route.
      for (const card of summaries) expect(card.routes).toBe(card.names.length)
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

test('the Muse Spark card lists each provider choice with its route and its training note', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [], null)
  try {
    for (const screen of [page, fixture.second]) {
      await screen.getByRole('searchbox').fill('Muse Spark')
      await expect(screen.locator('.model-summary h3')).toHaveText('Muse Spark 1.3 · Xhigh · 5')
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
    expect(listAgents(fixture.t.env).find((p) => p.name === 'gefjon').model).toBe(
      'opencode/muse-spark-1.3-contributor-free',
    )
    await expect(member(page, 'gefjon').getByRole('button', { name: /Remove|Reset/ })).toHaveCount(
      0,
    )
  } finally {
    await fixture.close()
  }
})

test('shared model cards return after reload and Clear filters in both tabs', async ({ page }) => {
  const fixture = await catalogPage(page, [], null)
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
        await expect(card.locator('.member')).toHaveCount(3)
      }
      await screen.reload()
      await expect(screen.getByLabel('Group by')).toHaveValue('model-reasoning')
      await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    }
  } finally {
    await fixture.close()
  }
})

test('a model card keeps its shared tier while an agent of your own joins and leaves it', async ({
  page,
}) => {
  const fixture = await catalogPage(page)
  const { second } = fixture
  try {
    await page.getByRole('searchbox').fill('Fable')
    await page.getByLabel('Group by').selectOption('model-reasoning')
    const card = page
      .locator('.model-group')
      .filter({ has: page.getByRole('heading', { name: /^Claude Fable 5.1 · Xhigh ·/ }) })
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 3')
    await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    // Three harnesses run this model at this effort; each row says which.
    await expect(card.locator('.member__head > .tag:not(.tag--own)')).toHaveText([
      'Claude Code',
      'Pi',
      'OpenCode',
    ])
    await expect(card.getByRole('button')).toHaveCount(0)
    await page.screenshot({ path: '/tmp/cf-model-card-fable.png' })
    const form = second.locator('#add')
    await form.locator('[name=name]').fill('my-fable')
    await form.locator('[name=harness]').selectOption('claude')
    await form.locator('[name=model]').fill('claude-fable-5-1')
    await form.locator('[name=effort]').fill('xhigh')
    await form.getByRole('button', { name: 'Add agent' }).click()
    await expect(member(second, 'my-fable')).toBeVisible()
    await refreshAgents(page)
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 4')
    await expect(card.locator('.model-summary .tier-pill')).toHaveCount(1)
    await expect(card.locator('.member .tier-pill')).toHaveCount(0)
    await expect(page.getByRole('searchbox')).toHaveValue('Fable')
    await member(page, 'my-fable').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(card.locator('h3')).toHaveText('Claude Fable 5.1 · Xhigh · 3')
    expect(listAgents(fixture.t.env).some((p) => p.name === 'my-fable')).toBe(false)
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

test('removal guards duplicate clicks and keeps the row after HTTP and network errors', async ({
  page,
}) => {
  const fixture = await catalogPage(page, [
    { name: 'mine', harness: 'codex', model: 'gpt-6-astra', effort: 'medium' },
  ])
  let release
  let deletes = 0
  let mode = 'wait'
  const waiting = new Promise((resolve) => {
    release = resolve
  })
  await page.route(`${fixture.server.url}/api/agents/mine`, async (route) => {
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
    await member(page, 'mine').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'mine').getByRole('button', { name: 'Removing…' })).toBeDisabled()
    await page.getByLabel('Group by').selectOption('model-reasoning')
    await member(page, 'mine')
      .getByRole('button', { name: 'Removing…' })
      .evaluate((button) => button.click())
    expect(deletes).toBe(1)
    release()
    await expect(page.getByRole('status')).toHaveText('Could not remove agent')
    await expect(
      member(page, 'mine').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    expect(listAgents(fixture.t.env).some((a) => a.name === 'mine')).toBe(true)
    mode = 'network'
    await member(page, 'mine').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(page.getByRole('status')).not.toBeEmpty()
    await expect(
      member(page, 'mine').getByRole('button', { name: 'Remove', exact: true }),
    ).toBeEnabled()
    await refreshAgents(fixture.second)
    await expect(member(fixture.second, 'mine')).toBeVisible()
    mode = 'success'
    await member(page, 'mine').getByRole('button', { name: 'Remove', exact: true }).click()
    await expect(member(page, 'mine')).toHaveCount(0)
    expect(listAgents(fixture.t.env).some((a) => a.name === 'mine')).toBe(false)
    expect(deletes).toBe(3)
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
    await expect(member(page, 'pygmalion')).toContainText('Codex Images')
    await expect(member(page, 'pygmalion')).not.toContainText(/gpt-image-2|Sunburst|Flare/)
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
      const fixture = await catalogPage(page, [
        { name: 'maia-2', harness: 'codex', model: 'gpt-6-astra', effort: 'medium' },
      ])
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
          for (const node of await section.locator('.model-summary, .member__head button').all()) {
            const bounds = await node.boundingBox()
            expect(bounds.x).toBeGreaterThanOrEqual(0)
            expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
          }
        }
        await expect(member(page, 'maia-2')).toBeVisible()
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
        const head = member(page, 'maia-2').locator('.member__head')
        await expect(head.getByRole('button', { name: 'Edit', exact: true })).toBeEnabled()
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
    await own.getByLabel('Show', { exact: true }).selectOption('mine')
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
    await own.getByLabel('Show', { exact: true }).selectOption('mine')
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
    // A catalog agent's tier is the catalog's, whatever your own agents say.
    await page.getByLabel('Work tier', { exact: true }).selectOption('critical')
    await expect(page.locator('.callsign', { hasText: /^astraeus$/ })).toBeVisible()
    await page.screenshot({ path: '/tmp/cf-tiers-agents.png', fullPage: true })
  } finally {
    await own.close()
    await server.close()
    t.cleanup()
  }
})
