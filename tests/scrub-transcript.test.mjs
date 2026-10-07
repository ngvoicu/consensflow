import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const SCRIPT = fileURLToPath(new URL('./live/scrub-transcript.mjs', import.meta.url))
const FIXTURES = fileURLToPath(
  new URL('../crates/cf-harness/tests/fixtures/claude', import.meta.url),
)
const SESSION = '1a8340d5-4fae-474e-a6ff-a3af2182cbc1'

/** What a live run's transcript holds, with what is personal in it: a folder, a branch, an address, ids. */
const RECORDS = [
  { type: 'last-prompt', lastPrompt: 'Run it, jane', leafUuid: 'l1', sessionId: SESSION },
  {
    type: 'user',
    uuid: 'u1',
    parentUuid: null,
    message: { role: 'user', content: 'Run exactly this one shell command' },
    cwd: '/Users/jane/Projects/app',
    gitBranch: 'jane/private-work',
    session_id: SESSION,
    sessionId: SESSION,
  },
  {
    type: 'attachment',
    uuid: 'a1',
    attachment: {
      type: 'hook_success',
      hookName: 'Stop',
      hookEvent: 'Stop',
      exitCode: 0,
      durationMs: 14,
      command: 'curl https://example.com/secret-token',
      stdout: 'secret output',
    },
    sessionId: SESSION,
  },
  {
    type: 'attachment',
    uuid: 'a2',
    attachment: { type: 'session_context', context: { userEmail: 'jane@example.com' } },
    rendered: [{ content: 'jane@example.com' }],
    sessionId: SESSION,
  },
  {
    type: 'bridge-session',
    sessionId: SESSION,
    bridgeSessionId: 'cse_01Secret',
    ownerAccountUuid: 'account-1',
    ownerOrganizationUuid: 'organization-1',
  },
  {
    type: 'assistant',
    uuid: 'm1',
    message: {
      id: 'msg_1',
      role: 'assistant',
      content: [{ type: 'thinking', thinking: 'DONE', signature: 'c2lnbmF0dXJl' }],
    },
    requestId: 'req_01Secret',
    sessionId: SESSION,
  },
]

function scrubbed(records) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-scrub-'))
  try {
    const from = join(dir, 'transcript.jsonl')
    const to = join(dir, 'fixture.jsonl')
    writeFileSync(from, `${records.map((record) => JSON.stringify(record)).join('\n')}\n`)
    execFileSync(process.execPath, [SCRIPT, from, to], { stdio: 'ignore' })
    return readFileSync(to, 'utf8')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

describe('a Claude transcript made into a fixture', () => {
  it('keeps every record in its order and the words of the conversation, and loses what is personal', () => {
    const text = scrubbed(RECORDS)
    const kept = text
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
    assert.deepEqual(
      kept.map((record) => record.type),
      RECORDS.map((record) => record.type),
    )
    for (const personal of [
      SESSION,
      'jane',
      '/Users',
      'private-work',
      'secret',
      'example.com',
      'cse_',
      'account-1',
      'organization-1',
      'c2lnbmF0dXJl',
    ]) {
      assert.ok(!text.includes(personal), `${personal} is still there`)
    }
    assert.equal(kept[1].message.content, 'Run exactly this one shell command')
    assert.equal(kept[1].sessionId, '$SESSION')
    assert.equal(kept[1].cwd, '/work/app')
    assert.deepEqual(kept[2].attachment, {
      type: 'hook_success',
      hookName: 'Stop',
      hookEvent: 'Stop',
      exitCode: 0,
      durationMs: 14,
      command: '[scrubbed]',
    })
    assert.deepEqual(kept[3].attachment, { type: 'session_context' })
    assert.equal(kept[5].message.content[0].thinking, 'DONE')
  })

  it('is what the fixtures the Rust tests read are: none holds a folder, an address or an id of an account', () => {
    const names = readdirSync(FIXTURES).filter((name) => name.endsWith('.jsonl'))
    assert.ok(names.length >= 2, 'the fixtures are there')
    for (const name of names) {
      const text = readFileSync(join(FIXTURES, name), 'utf8')
      // A folder of a user, an address, and the ids of an account's bridge sessions.
      for (const found of [
        /\/Users\//,
        /\/home\//,
        /[\w.+-]+@[\w-]+\.\w+/,
        /cse_0\w{6,}/,
        /session_0\w{6,}/,
      ]) {
        assert.ok(!found.test(text), `${name} matches ${found}`)
      }
      for (const line of text.split('\n').filter(Boolean)) {
        const record = JSON.parse(line)
        if (record.sessionId !== undefined) assert.equal(record.sessionId, '$SESSION', name)
      }
    }
  })
})
