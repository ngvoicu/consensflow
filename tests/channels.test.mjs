import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
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

const TURN_END = { hooks: [{ type: 'command', command: 'exit 0' }] }
// Claude's question tool, answered from the board through `cf hook claude`.
const QUESTION = {
  matcher: 'AskUserQuestion',
  hooks: [{ type: 'command', command: 'cf hook claude', timeout: 3600 }],
}
// Full-permission mode without the one-time acceptance dialog, and messages
// from ConsensFlow's other sessions delivered instead of held for approval
// (a bypass-mode session holds them by default and drops them after 5 min).
const YOLO = {
  permissions: { defaultMode: 'bypassPermissions' },
  skipDangerousModePermissionPrompt: true,
  crossSessionInbound: 'accept',
}

/** The settings file a Claude launch was given, read back from its flag. */
async function claudeSettings(configuration, home) {
  assert.equal(configuration.args.length, 2)
  assert.equal(configuration.args[0], '--settings')
  assert.ok(configuration.args[1].startsWith(join(home, 'integrations', 'claude')))
  return JSON.parse(await readFile(configuration.args[1], 'utf8'))
}

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
    const home = await mkdtemp(join(tmpdir(), 'cf-claude-home-'))
    const env = { HOME: home, CONSENSFLOW_HOME: home }
    try {
      for (const version of ['2.1.263', '2.1.265', '2.1.262', '2.2.0']) {
        const executable = await claudeExecutable(root)
        const configuration = await launchConfiguration('claude-code', {
          launchId: `retired-${version}`,
          workspace: root,
          executable,
          node: process.execPath,
          env,
        })
        // The only flag is the launch's settings file; no channel of its own.
        assert.deepEqual(await claudeSettings(configuration, home), {
          ...YOLO,
          hooks: { PreToolUse: [QUESTION], Stop: [TURN_END] },
        })
        assert.deepEqual(configuration.env, {}, version)
        if (process.platform === 'darwin') {
          assert.equal(configuration.channel?.kind, 'claude-peer', version)
          assert.equal(configuration.channel?.preservesDraft, 1)
        } else assert.equal(configuration.channel, null, version)
      }
      const missing = await launchConfiguration('claude-code', {
        launchId: 'retired-missing',
        workspace: root,
        env,
      })
      assert.deepEqual({ ...missing, args: [] }, { args: [], env: {}, channel: null })
      assert.deepEqual(await claudeSettings(missing, home), {
        ...YOLO,
        hooks: { PreToolUse: [QUESTION], Stop: [TURN_END] },
      })
      assert.deepEqual(await readdir(root), ['claude'], 'nothing is written into the project')
    } finally {
      await rm(root, { recursive: true, force: true })
      await rm(home, { recursive: true, force: true })
    }
  })

  it('ends every Claude turn in a Stop hook, merged with any coordinator hooks', async () => {
    // Claude omits its turn_duration record on some turns (every Calliope turn
    // on 2.1.274), and those answers never counted as finished. A Stop hook
    // makes Claude record stop_hook_summary at the end of every turn.
    const home = await mkdtemp(join(tmpdir(), 'cf-claude-turn-end-'))
    const env = { HOME: home, CONSENSFLOW_HOME: home }
    try {
      const worker = await launchConfiguration('claude-code', {
        launchId: 'turn-end',
        workspace: home,
        env,
      })
      assert.deepEqual(await claudeSettings(worker, home), {
        ...YOLO,
        hooks: { PreToolUse: [QUESTION], Stop: [TURN_END] },
      })
      const receiver = { type: 'command', command: 'receiver', asyncRewake: true }
      const lead = await claudeSettings(
        await launchConfiguration('claude-code', {
          launchId: 'turn-end-lead',
          workspace: home,
          env,
          hooks: { SessionStart: [{ hooks: [receiver] }], Stop: [{ hooks: [receiver] }] },
        }),
        home,
      )
      assert.deepEqual(lead.hooks.SessionStart, [{ hooks: [receiver] }])
      assert.deepEqual(lead.hooks.Stop, [{ hooks: [receiver] }, TURN_END])
      await assert.rejects(
        launchConfiguration('claude-code', { launchId: 'no-home', workspace: home }),
        /ConsensFlow environment/,
        'a launch without a home never falls back to the live one',
      )
    } finally {
      await rm(home, { recursive: true, force: true })
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
