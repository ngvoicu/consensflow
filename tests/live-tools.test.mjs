import assert from 'node:assert/strict'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { claudeExtras, pastedHarnesses, windowEnv } from './live/live-window.mjs'
import { claudeSettlement } from './live/receipt-rig.mjs'
import { cargoMissing } from './rust-channels.mjs'

/**
 * What the live tools share and can be held to without a real harness: the
 * Claude window's extra arguments and the settings they name, and what the
 * daemon's record reader says of a Claude transcript. The tools themselves
 * spend real tokens and are run by hand (`npm run live:…`).
 */
describe('the live tools’ shared parts', { skip: cargoMissing }, () => {
  const root = mkdtempSync(join(tmpdir(), 'cf-live-tools-'))
  after(() => rmSync(root, { recursive: true, force: true }))

  it('give a Claude window its settings, written in the folder it runs in, and shut out MCP servers and the browser', async () => {
    const workspace = join(root, 'workspace')
    const [flag, file, ...more] = await claudeExtras(workspace)
    assert.equal(flag, '--settings')
    assert.deepEqual(more, ['--strict-mcp-config', '--no-chrome'])
    // The folder is the window's ConsensFlow home, as `windowEnv` says.
    assert.equal(windowEnv(workspace).CONSENSFLOW_HOME, join(workspace, '.consensflow'))
    assert.ok(file.startsWith(join(workspace, '.consensflow', 'integrations', 'claude')), file)
    assert.ok(existsSync(file))
    const settings = JSON.parse(readFileSync(file, 'utf8'))
    assert.equal(settings.skipDangerousModePermissionPrompt, true)
    // The chief of the project is not in the loop: nothing is put to the board.
    assert.deepEqual(settings.hooks.PreToolUse, [])
    // A second window writes over the first's settings, and the folder does not grow.
    assert.deepEqual(await claudeExtras(workspace), [flag, file, ...more])
  })

  it('open only the windows the app pastes into', () => {
    assert.deepEqual(pastedHarnesses(['claude', 'devin']), ['claude', 'devin'])
    assert.throws(() => pastedHarnesses(['codex']), /the app pastes into claude and devin only/)
  })

  it('read a Claude transcript as the daemon does: its settlement, whether it is settled, its last item', async () => {
    const folder = join(root, 'claude')
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
          content: [{ type: 'text', text: 'Hi\n  there' }],
          stop_reason: 'end_turn',
        },
      },
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
    mkdirSync(join(folder, 'projects'), { recursive: true })
    writeFileSync(
      join(folder, 'projects', 's1.jsonl'),
      `${records.map((record) => JSON.stringify(record)).join('\n')}\n`,
    )
    const env = { CLAUDE_CONFIG_DIR: folder, HOME: root }
    assert.deepEqual(await claudeSettlement('s1', env), {
      settled: true,
      settlement: 'settled',
      items: 2,
      last: 'assistant: Hi there',
    })
    assert.equal(await claudeSettlement('missing', env), null, 'no transcript, no reading')
  })
})
