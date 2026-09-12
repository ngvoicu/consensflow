import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { PassThrough } from 'node:stream'
import { describe, it } from 'node:test'
import { Bridge } from '../src/bridge.js'
import { send as sendOpenCode } from '../src/channels/opencode.js'
import { enabledChannels, launchConfiguration, send } from '../src/channels.js'

function bridgePair() {
  const nodeToRust = new PassThrough()
  const rustToNode = new PassThrough()
  const node = new Bridge({
    input: rustToNode,
    output: nodeToRust,
    defaultDeadlineMs: 1000,
    idPrefix: 'n-',
    peerIdPrefix: 'r-',
  })
  const rust = new Bridge({
    input: nodeToRust,
    output: rustToNode,
    defaultDeadlineMs: 1000,
    idPrefix: 'r-',
    peerIdPrefix: 'n-',
  })
  return { node, rust }
}

function listen(server) {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => resolve(server.address().port))
  })
}

function closeServer(server) {
  return new Promise((resolve, reject) => {
    server.close((cause) => (cause ? reject(cause) : resolve()))
  })
}

// A native server message targets the thread while its TUI keeps the composer.
it('OpenCode worker messages claim native authority and preserve raw text (TEST-PANE-109)', async () => {
  const { node, rust } = bridgePair()
  const claims = []
  rust.on('pane.claim_native_epoch', (body) => {
    claims.push(body)
    return { ok: true }
  })
  const received = []
  const server = createServer(async (req, res) => {
    let body = ''
    for await (const chunk of req) body += chunk
    received.push({ url: req.url, body: JSON.parse(body) })
    res.writeHead(204).end()
  })
  const port = await listen(server)
  try {
    const text = 'Keep my exact message\nincluding the second line.'
    const result = await sendOpenCode(
      {
        pane: 'worker',
        generation: 3,
        epoch: 7,
        bridge: node,
        session: 'ses_probe',
        launch: {
          kind: 'opencode-server',
          preservesDraft: 1,
          endpoint: `http://127.0.0.1:${port}`,
        },
      },
      text,
    )
    assert.equal(result.admitted, true)
    assert.deepEqual(claims, [{ pane: 'worker', generation: 3, epoch: 7 }])
    assert.deepEqual(received, [
      { url: '/session/ses_probe/prompt_async', body: { parts: [{ type: 'text', text }] } },
    ])
  } finally {
    node.close()
    rust.close()
    await closeServer(server)
  }
})

it('Codex launch enables only an installed native queue capability (TEST-PANE-109)', async () => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native-capability-'))
  const executable = join(root, 'codex')
  const { chmod } = await import('node:fs/promises')
  try {
    await writeFile(executable, '#!/bin/sh\nprintf "%s\\n" "--thread --message"\n')
    await chmod(executable, 0o755)
    const configured = await launchConfiguration('codex', {
      launchId: 'launch-codex',
      workspace: root,
      executable,
    })
    assert.equal(configured.channel.kind, 'codex-queue')
    assert.equal(configured.channel.preservesDraft, 1)
    assert.equal(configured.channel.executable, executable)
    assert.equal(configured.channel.cwd, root)
    assert.deepEqual(configured.args, [])
    await writeFile(executable, '#!/bin/sh\nexit 0\n')
    assert.equal(
      (await launchConfiguration('codex', { launchId: 'old-codex', workspace: root, executable }))
        .channel,
      null,
    )
  } finally {
    await rm(root, { recursive: true, force: true })
  }
})

describe('retired Claude development channel (TEST-PANE-121)', () => {
  async function claudeExecutable(root) {
    const executable = join(root, 'claude')
    const { chmod } = await import('node:fs/promises')
    await writeFile(
      executable,
      `#!/bin/sh\nprintf called > '${join(root, 'probe-called')}'\nexit 1\n`,
    )
    await chmod(executable, 0o755)
    return executable
  }

  it('opens Claude without development channels regardless of version', async () => {
    const root = await mkdtemp(join(tmpdir(), 'cf-claude-retired-'))
    try {
      for (const version of ['2.1.263', '2.1.265', '2.1.262', '2.2.0']) {
        const executable = await claudeExecutable(root)
        const configuration = await launchConfiguration('claude-code', {
          launchId: `retired-${version}`,
          workspace: root,
          executable,
          node: process.execPath,
        })
        assert.deepEqual(configuration.args, [], version)
        assert.deepEqual(configuration.env, {}, version)
        if (process.platform === 'darwin') {
          assert.equal(configuration.channel?.kind, 'claude-peer', version)
          assert.equal(configuration.channel?.preservesDraft, 1)
        } else assert.equal(configuration.channel, null, version)
      }
      assert.deepEqual(
        await launchConfiguration('claude-code', {
          launchId: 'retired-missing',
          workspace: root,
        }),
        { args: [], env: {}, channel: null },
      )
      assert.deepEqual(await readdir(root), ['claude'], 'no delivery directories are authored')
    } finally {
      await rm(root, { recursive: true, force: true })
    }
  })

  it('offers no claude-channel to the lead harness', () => {
    assert.deepEqual(enabledChannels('claude-code'), ['claude-peer'])
  })

  it('a stale claude-channel refuses sending without writing', async () => {
    const root = await mkdtemp(join(tmpdir(), 'cf-claude-stale-'))
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    await mkdir(inbox, { recursive: true })
    await mkdir(ack, { recursive: true })
    const claims = []
    const stale = {
      session: '11111111-2222-4333-8444-555555555555',
      pane: 'lead-pane',
      generation: 4,
      epoch: 19,
      enabledChannels: enabledChannels('claude-code'),
      launch: {
        kind: 'claude-channel',
        launchId: 'stale-claude',
        inbox,
        ack,
        ackTimeoutMs: 50,
      },
      claimEpoch: async (request) => {
        claims.push(request)
        return { ok: true }
      },
    }
    assert.deepEqual(await send('claude-channel', stale, 'worker follow-up'), {
      ok: false,
      admitted: false,
      bytesWritten: 0,
      error: 'channel-disabled',
    })
    assert.deepEqual(claims, [], 'no input epoch is claimed')
    assert.deepEqual(await readdir(inbox), [], 'no inbox record is offered')
    assert.deepEqual(await readdir(ack), [], 'no acknowledgement is minted')
    await rm(root, { recursive: true, force: true })
  })
})
