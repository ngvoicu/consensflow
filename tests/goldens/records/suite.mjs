/**
 * The cases of `tests/engine/completion.test.mjs`, as scenarios: each test's
 * staging, as data, then the looks it took. What a test asserted of a
 * reading is in the reading the golden keeps whole.
 */

import { fixture, fixtureLines } from './fixtures.mjs'
import { START } from './runner.mjs'

/** Where `stageJsonl` stages each JSONL harness's transcript, under the root. */
function stagePlace(kind, session) {
  if (kind === 'codex') {
    return {
      dir: '$ROOT/sessions',
      file: `$ROOT/sessions/rollout-2026-09-06T00-00-00-${session}.jsonl`,
      env: { CODEX_HOME: '$ROOT' },
    }
  }
  if (kind === 'claude-code') {
    return {
      dir: '$ROOT/projects',
      file: `$ROOT/projects/${session}.jsonl`,
      env: { CLAUDE_CONFIG_DIR: '$ROOT' },
    }
  }
  const dir = '$ROOT/.pi/agent/sessions/project'
  return { dir, file: `${dir}/2026-09-06T00-00-00-000Z_${session}.jsonl`, env: { HOME: '$ROOT' } }
}

/**
 * `stageJsonl`'s staging, as steps: a fixture's lines (`take` of them,
 * `mutate`d as records, after `prepend`), ended by `finalAppend`, with Pi's
 * settlement evidence and an age when asked. Returns the steps, the env and
 * the file.
 */
export function staged(kind, session, name, options = {}) {
  let records = fixtureLines(name)
  if (options.take !== undefined) records = records.slice(0, options.take)
  if (options.mutate)
    records = options
      .mutate(records.map((line) => JSON.parse(line)))
      .map((record) => JSON.stringify(record))
  if (options.prepend)
    records = [...options.prepend.map((record) => JSON.stringify(record)), ...records]
  const { dir, file, env: base } = stagePlace(kind, session)
  let env = base
  const steps = [{ mkdir: dir }]
  if (kind === 'pi' && options.settlement !== undefined) {
    const settlement = options.settlement
    steps.push(
      { mkdir: '$ROOT/settled' },
      {
        write: `$ROOT/settled/${settlement.launchId}.json`,
        text: `${JSON.stringify({
          launchId: settlement.launchId,
          sessionId: settlement.sessionId ?? session,
          frontier: { id: settlement.frontierId },
          settledAt: START,
        })}\n`,
      },
    )
    env = {
      ...env,
      CF_DELIVERY_SETTLED: '$ROOT/settled',
      CF_DELIVERY_LAUNCH_ID: settlement.expectedLaunchId ?? settlement.launchId,
    }
  }
  steps.push({ write: file, text: `${records.join('\n')}${options.finalAppend ?? '\n'}` })
  if (options.ageMs !== undefined) steps.push({ mtime: file, ago: options.ageMs })
  return { steps, env, file }
}

/** A test's one staged fixture and one fresh look at it. */
export function once(name, kind, session, fixtureName, options = {}) {
  const { steps, env } = staged(kind, session, fixtureName, options)
  return { name, env, steps: [...steps, { look: 'fresh', kind, session }] }
}

const CODEX = '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'
const CLAUDE = '15fba934-d727-4777-8791-123675a63649'

const QUEUED = '1b09fb15-feb1-4595-9f47-5eb9ff768191'
const PI_QUIET_MS = 120_000

export function suiteScenarios() {
  return [...piAndHooks(), ...codexCases(), ...cachedCases(), ...piCases()]
}

function piAndHooks() {
  const body = '[consensflow receiver claim-one]\nComplete result\n[end of receiver claim-one]\n'
  const receiptRecords = [
    { type: 'session', id: 'native-receipt' },
    {
      type: 'custom_message',
      id: 'entry-one',
      customType: 'consensflow-worker-result',
      content: body,
      display: true,
    },
  ]
  const dir = '$ROOT/.pi/agent/sessions/project'
  return [
    {
      name: 'pi: native custom inbox messages retain full receipt body without becoming worker answers',
      env: { HOME: '$ROOT' },
      steps: [
        { mkdir: dir },
        {
          write: `${dir}/native-receipt.jsonl`,
          text: `${receiptRecords.map((record) => JSON.stringify(record)).join('\n')}\n`,
        },
        { look: 'fresh', kind: 'pi', session: 'native-receipt' },
      ],
    },
    once(
      'claude-code: synchronous hook context is exact native receipt evidence, not assistant text',
      'claude-code',
      CLAUDE,
      'claude-code/fragments.jsonl',
      {
        mutate(records) {
          records.push({
            type: 'attachment',
            uuid: 'hook-receipt',
            sessionId: CLAUDE,
            isSidechain: false,
            attachment: {
              type: 'hook_additional_context',
              hookEvent: 'UserPromptSubmit',
              hookName: 'UserPromptSubmit',
              content: [body],
            },
          })
          return records
        },
      },
    ),
  ]
}

function codexCases() {
  const mirrorInjected = (records) => {
    const mirror = records.find(
      (record) =>
        record.type === 'response_item' &&
        record.payload?.role === 'assistant' &&
        record.payload?.id === 'msg_04d9db96cf5fb8de016a9da268ba0c87d296ccd58a43338425',
    )
    mirror.payload.content[0].text += '\n<oai-mem-citation>injected mirror text</oai-mem-citation>'
    return records
  }
  const erroredThenCompleted = staged('codex', CODEX, 'codex/errored-task-complete.jsonl')
  return [
    once(
      'codex: native ids survive duplicate text and task_complete settles an exact final',
      'codex',
      CODEX,
      'codex/completed.jsonl',
    ),
    once(
      'codex: native AgentMessage remains canonical when its response mirror has injected text',
      'codex',
      CODEX,
      'codex/completed.jsonl',
      { mutate: mirrorInjected },
    ),
    once(
      'codex: errored task_complete, read before its sub-agent completed',
      'codex',
      CODEX,
      'codex/errored-task-complete.jsonl',
      { take: 11 },
    ),
    once(
      'codex: errored task_complete never promotes commentary and waits for sub-agent activity',
      'codex',
      CODEX,
      'codex/errored-task-complete.jsonl',
    ),
    {
      name: 'codex: an earlier provider failure does not label a later successful turn failed',
      env: erroredThenCompleted.env,
      steps: [
        ...erroredThenCompleted.steps,
        { append: erroredThenCompleted.file, text: fixture('codex/completed.jsonl') },
        { look: 'fresh', kind: 'codex', session: CODEX },
      ],
    },
    once(
      'codex: verified turn_aborted is cancellation',
      'codex',
      '01a077f6-6663-7bc2-81cd-e287ccaabdbd',
      'codex/interrupted.jsonl',
    ),
    once(
      'codex: a native fork reads as its own turns',
      'codex',
      '01a077fa-5968-7b62-8fdd-043410a3d4b9',
      'codex/forked.jsonl',
    ),
    once(
      'codex: a 60,000-character answer is never display-normalised',
      'codex',
      CODEX,
      'codex/big-answer.jsonl',
    ),
  ]
}

function cachedCases() {
  const grown = staged('claude-code', QUEUED, 'claude-code/queued-turn.jsonl', { take: 2 })
  const forgotten = staged('claude-code', QUEUED, 'claude-code/queued-turn.jsonl', { take: 2 })
  const cached = { look: 'cached', kind: 'claude-code', session: QUEUED }
  return [
    {
      name: 'cached: an unchanged transcript returns the previous result; a grown one is read again',
      env: grown.env,
      steps: [
        ...grown.steps,
        cached,
        cached,
        {
          append: grown.file,
          text: `${JSON.stringify({ type: 'system', subtype: 'stop_hook_summary', timestamp: '2026-09-19T00:00:00.000Z' })}\n`,
        },
        { ...cached, look: 'both' },
      ],
    },
    {
      name: 'cached: a conversation nobody reads any more is forgotten',
      env: forgotten.env,
      idle: 1000,
      steps: [
        ...forgotten.steps,
        cached,
        cached,
        { clock: 5000 },
        { look: 'cached', kind: 'claude-code', session: '7d1f4e63-0b4c-4f7a-9a8e-2b3c4d5e6f70' },
        cached,
      ],
    },
  ]
}

function piCases() {
  const session = 'hazy-ridge'
  const quiet = { ageMs: PI_QUIET_MS + 1_000 }
  const abort = (records) => {
    records.at(-1).message.stopReason = 'aborted'
    return records
  }
  const env = {
    HOME: '$ROOT',
    CF_DELIVERY_SETTLED: '$ROOT/settled',
    CF_DELIVERY_LAUNCH_ID: 'launch-1',
  }
  const first = { look: 'fresh', kind: 'pi', session: 'pi-first' }
  const marker = '$ROOT/settled/launch-1.working.json'
  return [
    {
      name: 'pi: before its first answer is saved, a turn the extension saw start is in flight',
      env,
      steps: [
        { mkdir: '$ROOT/settled' },
        first,
        {
          write: marker,
          text: JSON.stringify({ launchId: 'launch-1', sessionId: 'other', startedAt: 1 }),
        },
        first,
        {
          write: marker,
          text: JSON.stringify({ launchId: 'launch-1', sessionId: 'pi-first', startedAt: 1 }),
        },
        first,
      ],
    },
    once(
      'pi: a turn stays open between tool results',
      'pi',
      session,
      'pi/between-tool-steps.jsonl',
      {
        take: 5,
      },
    ),
    once(
      'pi: a step that says it stopped, in a quiet file, waits for its tools',
      'pi',
      session,
      'pi/between-tool-steps.jsonl',
      {
        mutate(records) {
          records.at(-1).message.stopReason = 'stop'
          return records
        },
        ...quiet,
      },
    ),
    once(
      'pi: toolCallId closes the loop and a 120-second quiet window derives settlement',
      'pi',
      session,
      'pi/tool-loop.jsonl',
      quiet,
    ),
    once(
      'pi: a turn stopped by Escape, settled by its evidence',
      'pi',
      session,
      'pi/tool-loop.jsonl',
      {
        mutate: abort,
        settlement: { launchId: 'launch-pi-1', frontierId: '3f9b029e' },
      },
    ),
    once(
      'pi: a turn stopped by Escape, settled by the quiet window',
      'pi',
      session,
      'pi/tool-loop.jsonl',
      {
        mutate: abort,
        ...quiet,
      },
    ),
    ...[
      [
        'matching settlement evidence settles at once',
        { launchId: 'launch-pi-1', frontierId: '3f9b029e' },
      ],
      [
        'evidence of another frontier waits',
        { launchId: 'launch-pi-1', frontierId: 'not-the-leaf' },
      ],
      [
        'evidence of another launch waits',
        { launchId: 'launch-pi-1', expectedLaunchId: 'another-launch', frontierId: '3f9b029e' },
      ],
    ].map(([label, settlement]) =>
      once(`pi: ${label}`, 'pi', session, 'pi/tool-loop.jsonl', { settlement }),
    ),
    ...[2, 3, 4, 5].map((take) =>
      once(
        `pi: a retry prefix of ${take} records stays unready`,
        'pi',
        'triton-jade-fern',
        'pi/provider-429.jsonl',
        { take },
      ),
    ),
    once('pi: a 429, quiet, is exhausted', 'pi', 'triton-jade-fern', 'pi/provider-429.jsonl', {
      take: 4,
      ...quiet,
    }),
    once(
      'pi: success after the retries, quiet, settles',
      'pi',
      'triton-jade-fern',
      'pi/provider-429.jsonl',
      quiet,
    ),
    ...[false, true].map((openTool) =>
      once(
        `pi: historical finals remain readable during the next turn${openTool ? ', with a tool open' : ''}`,
        'pi',
        session,
        'pi/tool-loop.jsonl',
        {
          mutate: (rows) => [
            ...rows.filter((row) => !openTool || row.message?.role !== 'toolResult'),
            {
              type: 'message',
              id: 'next-user',
              timestamp: new Date(START).toISOString(),
              message: { role: 'user', content: [{ type: 'text', text: 'Continue working' }] },
            },
          ],
        },
      ),
    ),
  ]
}
