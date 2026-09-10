import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { answers } from '../../hosts/lib/completion.js'
import { leadReady } from '../../hosts/lib/readiness.js'

const fixture = new URL('./fixtures/completion/claude-code/v265-tool-loop.jsonl', import.meta.url)
const session = '17499106-8778-48e1-a306-87bd186c9f7e'

async function staged(t) {
  const root = await mkdtemp(join(tmpdir(), 'cf-claude-v265-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  await mkdir(join(root, 'projects'))
  const records = (await readFile(fixture, 'utf8')).trimEnd().split('\n').map(JSON.parse)
  return async (take = records.length, mutate = (r) => r) => {
    await writeFile(
      join(root, 'projects', `${session}.jsonl`),
      records.slice(0, take).map(mutate).map(JSON.stringify).join('\n') + '\n',
    )
    return answers('claude-code', session, { CLAUDE_CONFIG_DIR: root })
  }
}

test('Claude 2.1.265 native direct and tool turns settle at their root finalizers', async (t) => {
  const read = await staged(t)
  for (const [take, text] of [
    [3, 'CF35_CLAUDE_READY'],
    [8, 'CF35_CLAUDE_TOOL_DONE'],
  ]) {
    const result = await read(take)
    assert.equal(result.unknown, undefined, result.reason)
    assert.equal(result.version, undefined)
    assert.equal(result.items.at(-1).text, text)
    assert.equal(result.items.at(-1).settled, true)
    assert.equal(result.settlement.boundary, 'system.turn_duration')
    assert.equal(leadReady({ answers: result, draftLatched: false, epoch: 7 }).state, 'ready')
  }
})

test('Claude 2.1.265 incomplete native prefixes never authorize an automatic send', async (t) => {
  const read = await staged(t)
  for (const take of [1, 2, 4, 5, 6, 7]) {
    const result = await read(take)
    assert.equal(result.unknown, undefined, result.reason)
    assert.notEqual(
      leadReady({ answers: result, draftLatched: false, epoch: 7 }).state,
      'ready',
      `prefix ${take}`,
    )
  }
  assert.deepEqual((await read(5)).settlement.evidence.openTools, [
    'toolu_016rxL6P8CoTD6xUKBhBQt9X',
  ])
  assert.deepEqual((await read(6)).settlement.evidence.openTools, [])
})

test('Claude 2.1.265 sidechain and pending-work durations cannot settle the root', async (t) => {
  const read = await staged(t)
  for (const fields of [
    { isSidechain: true },
    { pendingBackgroundAgentCount: 1 },
    { pendingWorkflowCount: 1 },
  ]) {
    const result = await read(undefined, (r) =>
      r.subtype === 'turn_duration' ? { ...r, ...fields } : r,
    )
    assert.equal(result.unknown, undefined, result.reason)
    assert.notEqual(leadReady({ answers: result, draftLatched: false, epoch: 7 }).state, 'ready')
  }
})

test('Claude settlement ignores future version metadata', async (t) => {
  const read = await staged(t)
  const result = await read(undefined, (r) => ({ ...r, version: '2.1.999' }))
  assert.equal(result.unknown, undefined)
  assert.equal(result.settlement.state, 'settled')
})
