import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * What a daemon's agents screens must do, asked of it over its API and of the
 * roster file it writes, and nothing else: the catalog it serves, an agent
 * saved by hand with the profile it is shown with, the screens behind the UI
 * token, and the deletion of the agent saved. It imports no module of the
 * product, so it holds whichever daemon answers: the packaged smoke
 * (tests/smoke.test.mjs) runs it against the daemon the built app chose, and
 * tests/agents-proof.test.mjs against each daemon from the checkout.
 *
 *   await proveAgents({ url, token, home })
 *
 * `url` is where the daemon listens, `token` its UI token, `home` the folder it
 * keeps its roster (agents.json) in. The home has no agent of the human's own
 * called `my-maia`; it may hold others. It rejects with the assertion that
 * failed.
 */

/** What the catalog holds, as the packaged build has it (every agent is in the roster as the catalog has it). */
const CATALOG_SIZE = 119
const MINE = 'my-maia'

/** What the Agents screen has and no longer has: the controls the human browses and sorts by. */
export const PRESENT = [
  'aria-label="Agents"',
  'Model and reasoning',
  'My own agents',
  'model-summary',
  'model-group',
  'value="model-reasoning" selected',
  'Work tier',
  'tier-pill',
  'Important work only · No coding',
]
export const GONE = [
  'id="catalog-section"',
  'Agent library',
  'Your agents',
  'PM candidate',
  'name="tags"',
  'category-pill',
  'Chief of Staff candidate',
  'name="category"',
  'Name in use',
  'offer__actions',
  'Saved only',
  'Sort by',
  'benchmark',
  'Artificial Analysis',
  'AA ',
]

export async function proveAgents({ url, token, home }) {
  const origin = new URL(url).origin
  const ask = (path, { method = 'GET', body, bearer = token } = {}) =>
    fetch(`${origin}${path}`, {
      method,
      headers: {
        ...(bearer === null ? {} : { authorization: `Bearer ${bearer}` }),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  const agents = async () => {
    const answer = await ask('/api/agents')
    assert.equal(answer.status, 200, `GET /api/agents: ${answer.status}`)
    return answer.json()
  }
  const stored = () =>
    JSON.parse(readFileSync(join(home, 'agents.json'), 'utf8')).agents.find(
      (row) => row.id === MINE,
    )

  // The screens and their API are the app's, behind its UI token: with none, or another, they say so.
  for (const path of ['/', '/harnesses', '/api/agents']) {
    for (const [what, bearer] of [
      ['no token', null],
      ['another token', `${token}x`],
    ]) {
      const refused = await ask(path, { bearer })
      assert.equal(refused.status, 401, `${path} with ${what}: ${refused.status}`)
      assert.deepEqual(await refused.json(), { error: 'unauthorized' }, `${path} with ${what}`)
    }
  }

  // The catalog: every agent in the roster, as the catalog has it.
  const first = await agents()
  const catalog = first.agents.filter((agent) => agent.custom !== true)
  assert.equal(catalog.length, CATALOG_SIZE, 'packaged preset count')
  assert.equal(first.agents.find((agent) => agent.name === 'pygmalion')?.model, 'codex-image')
  assert.equal(
    first.agents.find((agent) => agent.name === MINE),
    undefined,
    `${MINE} is not yet`,
  )

  // An agent saved by hand: the file keeps what was said, and the screen shows it with its profile.
  const added = await ask('/api/agents', {
    method: 'POST',
    body: { name: MINE, harness: 'codex', model: 'gpt-6-astra', effort: 'low' },
  })
  assert.equal(added.status, 201, `POST /api/agents: ${added.status} ${await added.text()}`)
  const row = stored()
  assert.deepEqual(
    [row.effort, row.model, Object.hasOwn(row, 'profile')],
    ['low', 'gpt-6-astra', false],
  )
  const listed = await agents()
  const mine = listed.agents.find((agent) => agent.name === MINE)
  assert.deepEqual([mine.effort, mine.custom, mine.profile.workTier], ['low', true, 'light'])
  assert.equal(listed.agents.length, first.agents.length + 1)

  // The screens, opened as the app opens them: the token in the address, or as the bearer.
  const page = await ask('/')
  assert.equal(page.status, 200)
  assert.match(page.headers.get('content-type') ?? '', /^text\/html/)
  const html = await page.text()
  for (const text of PRESENT) assert.ok(html.includes(text), text)
  for (const text of GONE) assert.ok(!html.includes(text), `gone: ${text}`)
  const framed = await ask(`/?token=${encodeURIComponent(token)}`, { bearer: null })
  assert.equal(framed.status, 200, `the Agents screen at its framed address: ${framed.status}`)
  assert.ok((await framed.text()).includes('aria-label="Agents"'))
  const harnesses = await ask(`/harnesses?token=${encodeURIComponent(token)}`, { bearer: null })
  assert.equal(
    harnesses.status,
    200,
    `the Harnesses screen at its framed address: ${harnesses.status}`,
  )
  assert.ok((await harnesses.text()).includes('<title>ConsensFlow Harnesses</title>'))

  // The deletion: a catalog agent is not the human's to delete, the agent saved is.
  assert.equal((await ask('/api/agents/maia', { method: 'DELETE' })).status, 400)
  assert.equal((await ask(`/api/agents/${MINE}`, { method: 'DELETE' })).status, 204)
  const after = await agents()
  assert.equal(after.agents.length, first.agents.length)
  assert.equal(
    after.agents.find((agent) => agent.name === MINE),
    undefined,
  )
  assert.equal(Object.hasOwn(after, 'catalog'), false)
  assert.equal(stored(), undefined, 'the file no longer holds it')
}
