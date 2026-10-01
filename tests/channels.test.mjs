import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { launchConfiguration } from '../src/channels.js'
import { fakeExecutable } from './helpers.mjs'

it('Codex launch enables only an installed native queue capability (TEST-PANE-109)', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native-capability-'))
  try {
    const executable = fakeExecutable(join(root, 'codex'), { output: '--thread --message' })
    const configured = await launchConfiguration('codex', {
      launchId: 'launch-codex',
      workspace: root,
      executable,
    })
    assert.equal(configured.channel.kind, 'codex-queue')
    assert.equal(configured.channel.executable, executable)
    assert.equal(configured.channel.cwd, root)
    assert.deepEqual(configured.args, [])
    fakeExecutable(executable)
    assert.equal(
      (await launchConfiguration('codex', { launchId: 'old-codex', workspace: root, executable }))
        .channel,
      null,
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})
