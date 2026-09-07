import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { answers } from '../../hosts/lib/completion.js'
import { harnessTurns } from '../../hosts/lib/harness-transcript.js'

const FIXTURE = fileURLToPath(new URL('./fixtures/completion/pi/tool-loop.jsonl', import.meta.url))
const SESSION_ID = 'hazy-ridge'
const SESSION_FILE = `2026-08-24T18-00-00-000Z_${SESSION_ID}.jsonl`
const DECOY = 'DEFAULT_PI_PATH_DECOY'
const EXPECTED_USER = '[redacted 5865 chars]'
const EXPECTED_ASSISTANT =
  "I'll start by reading the design record and README, then run the verification commands."

async function stageFixture(sessionRoot) {
  const directory = path.join(sessionRoot, 'project')
  await fs.mkdir(directory, { recursive: true })
  await fs.copyFile(FIXTURE, path.join(directory, SESSION_FILE))
}

async function stageDecoy(sessionRoot) {
  const directory = path.join(sessionRoot, 'project')
  await fs.mkdir(directory, { recursive: true })
  const records = [
    { type: 'session', version: 3, id: SESSION_ID, timestamp: '2026-08-24T18:00:00.703Z' },
    {
      type: 'message',
      id: 'default-decoy-user',
      parentId: null,
      timestamp: '2026-08-24T18:00:00.935Z',
      message: {
        role: 'user',
        content: [{ type: 'text', text: DECOY }],
        timestamp: 1787594400933,
      },
    },
  ]
  await fs.writeFile(
    path.join(directory, SESSION_FILE),
    `${records.map((record) => JSON.stringify(record)).join('\n')}\n`,
  )
}

async function assertPiFixtureIsRead(env, label) {
  const [turns, completion] = await Promise.all([
    harnessTurns('pi', SESSION_ID, env),
    answers('pi', SESSION_ID, env),
  ])
  assert.equal(turns.length, 3, `${label}: harnessTurns should read the real Pi fixture`)
  assert.deepEqual(turns[0], { role: 'user', text: EXPECTED_USER }, `${label}: fixture user turn`)
  assert.deepEqual(
    turns[1],
    { role: 'assistant', text: EXPECTED_ASSISTANT },
    `${label}: fixture assistant turn`,
  )
  assert.equal(turns[2].role, 'assistant', `${label}: fixture terminal assistant turn`)
  assert.equal(
    turns.some((turn) => turn.text === DECOY),
    false,
    `${label}: default-path decoy was read by harnessTurns`,
  )

  assert.equal(
    completion.unknown,
    undefined,
    `${label}: completion.answers should parse the real Pi fixture`,
  )
  assert.equal(completion.version, '3', `${label}: Pi fixture version`)
  assert.equal(completion.items.length, 5, `${label}: completion item count`)
  assert.ok(
    completion.items.some((item) => item.id === '4cea719d' && item.role === 'assistant'),
    `${label}: structural assistant item`,
  )
  assert.ok(
    completion.items.some((item) => item.role === 'tool'),
    `${label}: structural tool items`,
  )
  assert.equal(
    completion.items.some((item) => item.text === DECOY),
    false,
    `${label}: default-path decoy was read by completion.answers`,
  )
}

test('TEST-PANE-67: tilde-expanded PI_CODING_AGENT_DIR drives harnessTurns and completion.answers', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-pi-paths-dir-'))
  try {
    await stageFixture(path.join(root, 'pi-custom', 'sessions'))
    await stageDecoy(path.join(root, '.pi', 'agent', 'sessions'))
    await assertPiFixtureIsRead(
      { HOME: root, PI_CODING_AGENT_DIR: '~/pi-custom' },
      'PI_CODING_AGENT_DIR tilde expansion',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('TEST-PANE-67: PI_CODING_AGENT_SESSION_DIR takes precedence over PI_CODING_AGENT_DIR', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-pi-paths-session-'))
  try {
    await stageFixture(path.join(root, 'pi-sessions'))
    await stageDecoy(path.join(root, 'pi-config', 'sessions'))
    await stageDecoy(path.join(root, '.pi', 'agent', 'sessions'))
    await assertPiFixtureIsRead(
      {
        HOME: root,
        PI_CODING_AGENT_DIR: '~/pi-config',
        PI_CODING_AGENT_SESSION_DIR: '~/pi-sessions',
      },
      'PI_CODING_AGENT_SESSION_DIR precedence',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('TEST-PANE-67: empty path variables fall back to the default Pi session directory', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-pi-paths-empty-default-'))
  try {
    await stageFixture(path.join(root, '.pi', 'agent', 'sessions'))
    await assertPiFixtureIsRead(
      { HOME: root, PI_CODING_AGENT_DIR: '', PI_CODING_AGENT_SESSION_DIR: '' },
      'empty-string default fallback',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})

test('TEST-PANE-67: empty PI_CODING_AGENT_SESSION_DIR falls back under the custom agent directory', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'cf-pi-paths-empty-session-'))
  try {
    await stageFixture(path.join(root, 'pi-custom', 'sessions'))
    await stageDecoy(path.join(root, '.pi', 'agent', 'sessions'))
    await assertPiFixtureIsRead(
      { HOME: root, PI_CODING_AGENT_DIR: '~/pi-custom', PI_CODING_AGENT_SESSION_DIR: '' },
      'empty-string session-dir fallback',
    )
  } finally {
    await fs.rm(root, { recursive: true, force: true })
  }
})
