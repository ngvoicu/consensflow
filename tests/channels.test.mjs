import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { launchConfiguration } from '../src/channels.js'
import { fakeNodeExecutable } from './helpers.mjs'

/** A stand-in `codex` that says its version, has the native queue or not, and counts its runs. */
function fakeCodex(root, { version, queue }) {
  return fakeNodeExecutable(
    join(root, 'codex'),
    `#!${process.execPath}
import { appendFileSync } from 'node:fs'
appendFileSync(${JSON.stringify(join(root, 'runs'))}, process.argv.slice(2).join(' ') + '\\n')
if (process.argv[2] === '--version') console.log('codex-cli ${version}')
else if (process.argv[2] === 'queue' && ${queue}) console.log('Usage: codex queue --thread <id> --message <text>')
else process.exit(2)
`,
  )
}

it('Codex launch enables only an installed native queue capability (TEST-PANE-109)', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native-capability-'))
  try {
    const executable = fakeCodex(root, { version: '0.159.2', queue: true })
    const configured = await launchConfiguration('codex', {
      launchId: 'launch-codex',
      workspace: root,
      env: process.env,
      executable,
    })
    assert.equal(configured.channel.kind, 'codex-queue')
    assert.equal(configured.channel.executable, executable)
    assert.equal(configured.channel.cwd, root)
    assert.deepEqual(configured.args, [])
    await launchConfiguration('codex', {
      launchId: 'launch-again',
      workspace: root,
      env: process.env,
      executable,
    })
    assert.deepEqual(
      (await readFile(join(root, 'runs'), 'utf8')).trim().split('\n'),
      ['queue --help'],
      'an unchanged Codex is asked once, not at every launch',
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

it('refuses to open a Codex without the native queue, naming its version', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native-capability-'))
  try {
    const executable = fakeCodex(root, { version: '0.150.0', queue: false })
    await assert.rejects(
      launchConfiguration('codex', {
        launchId: 'old-codex',
        workspace: root,
        env: process.env,
        executable,
      }),
      /^Error: Codex 0\.150\.0 has no native queue, which ConsensFlow needs to reach its window: update Codex\.$/,
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})
