/**
 * Phase 8, TEST-PANE-69 (RED-only): frozen native Claude 2.1.263 records.
 *
 * The installed CLI reports 2.1.263 but hosts/lib/completion.js SUPPORTED
 * admits only 2.1.241/247/250, so every answers() call below currently
 * returns {unknown:true} and every admission assertion fails. The final
 * turn is genuinely complete in the source (grouped end_turn fragments
 * closed by system.turn_duration, no stop_hook_summary anywhere), which the
 * existing parser cannot settle either — reported as a separate BLOCKER.
 * No production edits in this pane; see fixtures/completion/README.md.
 */
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { answers } from '../../hosts/lib/completion.js'
import { leadReady } from '../../hosts/lib/readiness.js'

const FIXTURE = fileURLToPath(
  new URL('./fixtures/completion/claude-code/v263-tool-loop.jsonl', import.meta.url),
)
const SESSION = '5cbf8973-f472-448a-8763-59fb4268a9d7'

async function stage(take, mutate = (records) => records) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-claude-v263-'))
  const dir = path.join(root, 'projects')
  await fs.mkdir(dir, { recursive: true })
  const source = await fs.readFile(FIXTURE, 'utf8')
  const records = mutate(source.trimEnd().split('\n').map(JSON.parse)).map(JSON.stringify)
  await fs.writeFile(
    path.join(dir, `${SESSION}.jsonl`),
    `${records.slice(0, take ?? records.length).join('\n')}\n`,
  )
  return { CLAUDE_CONFIG_DIR: root }
}

function readiness(result) {
  return leadReady({ answers: result, draftLatched: false, epoch: 17 })
}

function admitted(result) {
  assert.equal(result.unknown, undefined, `2.1.263 must be admitted, got: ${result.reason}`)
  assert.equal(result.version, '2.1.263')
}

test('claude-v263: the installed 2.1.263 transcript is admitted, not rejected', async () => {
  const result = await answers('claude-code', SESSION, await stage())
  admitted(result)
  assert.ok(Array.isArray(result.items) && result.items.length > 0)
})

test('claude-v263: versioned ignored envelopes never poison admission', async () => {
  const result = await answers('claude-code', SESSION, await stage())
  admitted(result)
  assert.ok(
    result.items.every((item) => item.role !== undefined),
    'only assistant/user/tool items surface; mode/attachment/queue envelopes stay structural',
  )
})

test('claude-v263: the tool loop opens and closes by native tool_use_id', async () => {
  const open = await answers('claude-code', SESSION, await stage(8))
  admitted(open)
  assert.deepEqual(open.settlement.evidence.openTools, ['toolu_014xV8WiQG7e22f7RZQJYTSE'])
  assert.equal(open.inFlight, true)
  assert.notEqual(readiness(open).state, 'ready')

  const closed = await answers('claude-code', SESSION, await stage(9))
  admitted(closed)
  assert.deepEqual(closed.settlement.evidence.openTools, [])
  const early = closed.items.find((item) => item.id === 'msg_011CepVzTaLVYh2bCvw5RpQe')
  assert.equal(early.role, 'assistant')
  assert.ok(
    closed.items.some(
      (item) => item.id === 'toolu_014xV8WiQG7e22f7RZQJYTSE' && item.role === 'tool',
    ),
  )
})

test('claude-v263: grouped fragments share one native id and the advisor call closes', async () => {
  const result = await answers('claude-code', SESSION, await stage())
  admitted(result)
  const grouped = result.items.filter((item) => item.id === 'msg_011CepWUqv3VgZSDKwGzUWKH')
  assert.equal(grouped.length, 1, 'seven fragments, one native message id, one item')
  assert.ok(
    result.items.some(
      (item) => item.id === 'srvtoolu_016MAs2kPZC9C7R4dYkPDryP' && item.role === 'tool',
    ),
    'server_tool_use closed by its advisor_tool_result',
  )
  assert.ok(!result.settlement.evidence.openTools.includes('srvtoolu_016MAs2kPZC9C7R4dYkPDryP'))
})

test('claude-v263: both queue enqueue/remove pairs reconcile to empty', async () => {
  const result = await answers('claude-code', SESSION, await stage())
  admitted(result)
  assert.deepEqual(result.settlement.evidence.queuedTurns, [])
})

test('claude-v263: every incomplete prefix stays unready and never settles', async () => {
  for (const take of [8, 9, 19, 27]) {
    const prefix = await answers('claude-code', SESSION, await stage(take))
    admitted(prefix)
    assert.notEqual(prefix.settlement.state, 'settled', `fixture prefix ${take} settled early`)
    assert.notEqual(readiness(prefix).state, 'ready', `fixture prefix ${take} reads ready`)
  }
})

test('claude-v263: fixture evidence — end_turn groups exist, turn_duration closes, no hook', async () => {
  const source = await fs.readFile(FIXTURE, 'utf8')
  assert.equal(
    source.split('\n').filter((line) => line.includes('stop_hook_summary')).length,
    0,
    'v263 emits no stop_hook_summary; the parser must not require one',
  )
  assert.equal(
    source.split('\n').filter((line) => line.includes('"subtype": "turn_duration"')).length,
    1,
    'the actual final boundary record is present exactly once',
  )
  const finals = source
    .trimEnd()
    .split('\n')
    .map((line) => JSON.parse(line))
    .filter((record) => record?.message?.id === 'msg_011CepWsDm4DzKV4t8Fbdzno')
  assert.equal(finals.length, 2, 'thinking + text share the final native message id')
  assert.ok(finals.every((record) => record.message.stop_reason === 'end_turn'))
})

test('claude-v263: the now-complete actual final turn settles and reads ready', async () => {
  const result = await answers('claude-code', SESSION, await stage())
  admitted(result)
  const final = result.items.find((item) => item.id === 'msg_011CepWsDm4DzKV4t8Fbdzno')
  assert.equal(final.role, 'assistant')
  assert.equal(final.complete, true)
  assert.equal(final.settled, true)
  assert.equal(result.settlement.state, 'settled')
  assert.deepEqual(result.settlement.evidence.openTools, [])
  assert.deepEqual(result.settlement.evidence.queuedTurns, [])
  assert.equal(readiness(result).state, 'ready')
})

for (const field of ['pendingBackgroundAgentCount', 'pendingWorkflowCount']) {
  test(`claude-v263: turn_duration with ${field} cannot settle a result`, async () => {
    for (const count of [1, -1, '0', null]) {
      const env = await stage(undefined, (records) =>
        records.map((record) =>
          record.subtype === 'turn_duration' ? { ...record, [field]: count } : record,
        ),
      )
      const result = await answers('claude-code', SESSION, env)
      admitted(result)
      assert.notEqual(readiness(result).state, 'ready')
      assert.notEqual(result.settlement.state, 'settled')
    }
  })
}

test('claude-v263: a sidechain duration cannot close the root turn', async () => {
  const env = await stage(undefined, (records) =>
    records.map((record) =>
      record.subtype === 'turn_duration' ? { ...record, isSidechain: true } : record,
    ),
  )
  const result = await answers('claude-code', SESSION, env)
  assert.notEqual(readiness(result).state, 'ready')
})
