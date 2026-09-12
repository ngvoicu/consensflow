import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { answers } from '../../hosts/lib/completion.js'

const session = 'fb561379-bcab-4045-92d2-d460bb19ed36'
const captured = (
  await fs.readFile(
    new URL('./fixtures/completion/claude-code/v268-clear.jsonl', import.meta.url),
    'utf8',
  )
)
  .trim()
  .split('\n')
  .map(JSON.parse)
async function examine(t, mutate = () => {}) {
  const config = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-claude-clear-fixture-'))
  t.after(() => fs.rm(config, { recursive: true, force: true }))
  await fs.mkdir(path.join(config, 'projects', 'fixture'), { recursive: true })
  const rows = structuredClone(captured)
  mutate(rows)
  await fs.writeFile(
    path.join(config, 'projects', 'fixture', `${session}.jsonl`),
    `${rows.map(JSON.stringify).join('\n')}\n`,
  )
  const completion = await answers('claude-code', session, { CLAUDE_CONFIG_DIR: config })
  return { completion }
}
test('native Claude 2.1.268 /clear is ready before any model turn', async (t) => {
  const { completion } = await examine(t)
  assert.equal(completion.items.filter((x) => x.role === 'assistant').length, 0)
  assert.equal(completion.settlement.state, 'settled')
  assert.equal(completion.settlement.boundary, 'system.local_command')
})
for (const [name, mutate] of [
  ['missing native boundary', (rows) => rows.pop()],
  ['wrong boundary parent', (rows) => (rows.at(-1).parentUuid = 'unrelated-user')],
  ['sidechain boundary', (rows) => (rows.at(-1).isSidechain = true)],
  ['foreign-session boundary', (rows) => (rows.at(-1).sessionId = 'foreign-session')],
  [
    'quoted command-shaped user text',
    (rows) => (rows.at(-2).message.content = `Please explain ${rows.at(-2).message.content}`),
  ],
  [
    'later queued turn',
    (rows) =>
      rows.push({
        type: 'queue-operation',
        operation: 'enqueue',
        content: 'pending real work',
        sessionId: session,
      }),
  ],
  [
    'later real user turn',
    (rows) =>
      rows.push({
        type: 'user',
        uuid: 'next-user',
        sessionId: session,
        message: { role: 'user', content: 'Do real work now' },
      }),
  ],
])
  test(`native /clear cannot settle ${name}`, async (t) => {
    const { completion } = await examine(t, mutate)
    assert.notEqual(completion.settlement.state, 'settled')
  })

test('native continuation retires readiness while retaining predecessor receipt items', async (t) => {
  const next = 'e6acbeb1-181f-4fdc-a963-6ff6d6d792ee'
  const { completion } = await examine(t, (rows) =>
    rows.push({
      type: 'continued-in',
      sessionId: session,
      continuedInSessionId: next,
      timestamp: '2026-09-12T13:57:18.549Z',
    }),
  )
  assert.equal(completion.continuedInSessionId, next)
  assert.equal(completion.replaced, false, 'predecessor evidence remains readable')
  assert.equal(
    completion.settlement.state,
    'settled',
    'the predecessor keeps its historical completion evidence',
  )
  assert.equal(completion.continuedAt, '2026-09-12T13:57:18.549Z')
})
