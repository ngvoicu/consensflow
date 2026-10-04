import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { launchConfiguration, withNativeBridge } from '../src/channels.js'
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

it('launches the bundled supervisor with the Codex invocation it supervises', () => {
  const A = '01a09094-938f-7fd1-a2d3-315cf92b4559'
  const TOKEN = 'private-launch-token-1234567890'
  const invocation = {
    command: '/native/codex',
    args: ['resume', A],
    env: { EXISTING: 'preserved' },
    dropEnv: ['OPENAI_API_KEY'],
  }
  const configured = {
    channel: {
      kind: 'codex-queue',
      executable: '/native/codex',
      sessionBridge: { endpoint: 'http://127.0.0.1:1234', token: TOKEN },
    },
  }
  const wrapped = withNativeBridge(invocation, configured)
  // The bundle's native cf, spelled as this platform spells a program it starts.
  const cf = `../bin/${process.platform === 'win32' ? 'cf.exe' : 'cf'}`
  assert.equal(wrapped.command, fileURLToPath(new URL(cf, import.meta.url)))
  assert.deepEqual(wrapped.args, ['codex-session', '/native/codex', 'resume', A])
  assert.deepEqual(wrapped.env, invocation.env)
  assert.deepEqual(wrapped.dropEnv, invocation.dropEnv)
})
