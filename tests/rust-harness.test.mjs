import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { cargoMissing } from './rust-channels.mjs'
import {
  claudeSettings,
  consoleText,
  interactiveStart,
  interruptOf,
  readRecord,
  windowText,
} from './rust-harness.mjs'

/**
 * What the evals and the live tools ask of the harness code, through the test
 * binary that answers them (cf_harness::tooling holds the answers themselves):
 * that a question goes in and its answer comes back as the tools read it.
 */
describe('the questions the evals and the live tools put to the harness code', {
  skip: cargoMissing,
}, () => {
  const root = mkdtempSync(join(tmpdir(), 'cf-rust-harness-'))
  after(() => rmSync(root, { recursive: true, force: true }))

  /** A Claude config folder holding the session `s1`: the user's turn, and the answer that ended it. */
  function claudeFolder(name, { ended }) {
    const folder = join(root, name)
    const base = { sessionId: 's1', isSidechain: false }
    const records = [
      {
        ...base,
        type: 'user',
        uuid: 'u1',
        parentUuid: null,
        message: { role: 'user', content: 'Hello' },
      },
      {
        ...base,
        type: 'assistant',
        uuid: 'a1',
        parentUuid: 'u1',
        message: {
          id: 'm1',
          role: 'assistant',
          content: [{ type: 'text', text: 'Hi' }],
          stop_reason: ended ? 'end_turn' : null,
        },
      },
      ...(ended
        ? [
            {
              ...base,
              type: 'system',
              subtype: 'turn_duration',
              uuid: 'd1',
              parentUuid: 'a1',
              durationMs: 5,
              messageCount: 2,
            },
          ]
        : []),
    ]
    mkdirSync(join(folder, 'projects'), { recursive: true })
    writeFileSync(
      join(folder, 'projects', 's1.jsonl'),
      `${records.map((record) => JSON.stringify(record)).join('\n')}\n`,
    )
    return folder
  }

  it('reads a harness’s record of a conversation: its items, and whether the turn is over', async () => {
    const env = (folder) => ({ CLAUDE_CONFIG_DIR: folder, HOME: root, OPENAI_API_KEY: null })
    const over = await readRecord('claude-code', 's1', env(claudeFolder('over', { ended: true })))
    assert.deepEqual(
      over.items.map(({ id, role, text, complete }) => [id, role, text, complete]),
      [
        ['u1', 'user', 'Hello', true],
        ['m1', 'assistant', 'Hi', true],
      ],
    )
    assert.deepEqual([over.settled, over.settlement, over.failed], [true, 'settled', false])
    const working = await readRecord(
      'claude-code',
      's1',
      env(claudeFolder('working', { ended: false })),
    )
    assert.deepEqual([working.settled, working.settlement], [false, 'in-flight'])
  })

  it('says nothing of a conversation the harness kept no record of, or of one it was not given', async () => {
    const empty = join(root, 'empty')
    mkdirSync(empty)
    assert.equal(
      await readRecord('claude-code', 's1', { CLAUDE_CONFIG_DIR: empty, HOME: empty }),
      null,
    )
    assert.equal(
      await readRecord('claude-code', '', { HOME: empty }),
      null,
      'no session to look for',
    )
  })

  it('refuses a question it cannot answer, in the words the binary threw it in', async () => {
    await assert.rejects(readRecord('nothing', 's1', { HOME: root }), /no harness runs as nothing/)
  })

  it('opens a harness’s window as the app does: its command, arguments and the keys it must not inherit', async () => {
    assert.deepEqual(
      await interactiveStart(
        { kind: 'claude-code', model: 'claude-haiku-5-5', effort: 'low' },
        's-1',
        null,
      ),
      {
        command: 'claude',
        args: [
          '--session-id',
          's-1',
          '--model',
          'claude-haiku-5-5',
          '--effort',
          'low',
          '--permission-mode',
          'bypassPermissions',
        ],
        env: {},
        dropEnv: ['ANTHROPIC_API_KEY'],
      },
    )
    assert.equal((await interactiveStart({ kind: 'devin' }, null, 'Go on')).prompt, 'Go on')
    assert.equal(
      await interactiveStart({ kind: 'claude-code' }, null, null),
      null,
      'Claude needs an id',
    )
  })

  it('says the keys that interrupt a turn in a harness’s window, as its adapter tells the daemon', async () => {
    assert.deepEqual(await interruptOf('claude-code'), { presses: 1, closeAfterMs: null })
    assert.deepEqual(await interruptOf('devin'), { presses: 2, closeAfterMs: 1000 })
  })

  it('gives a message as a window takes it, and as Windows’ console carries it', async () => {
    assert.equal(await windowText('a\r\nb\u001bc'), 'a\nb␛c')
    assert.equal(await consoleText('a — b → €'), 'a -- b -> EUR')
  })

  it('writes the settings of a Claude window’s launch where the ConsensFlow folder says, and names it', async () => {
    const home = join(root, 'home')
    const launch = '00000000-0000-4000-8000-000000000002'
    const [flag, file] = await claudeSettings({ CONSENSFLOW_HOME: home }, launch, {
      boardQuestions: false,
    })
    assert.equal(flag, '--settings')
    assert.equal(file, join(home, 'integrations', 'claude', launch, 'settings.json'))
    assert.ok(existsSync(file))
    const settings = JSON.parse(readFileSync(file, 'utf8'))
    assert.deepEqual(settings.hooks.PreToolUse, [])
    assert.equal(settings.skipDangerousModePermissionPrompt, true)
  })
})
