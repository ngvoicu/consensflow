import { execFile } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { createServer } from 'node:net'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { send as sendClaude } from './channels/claude-peer.js'
import { send as sendCodex } from './channels/codex.js'
import { DEFAULT_DEADLINE_MS, send as sendOpenCode } from './channels/opencode.js'
import { send as sendPi } from './channels/pi.js'
import { configRoot } from './roster.js'

const PROBES = Object.freeze({
  'claude-code': Object.freeze({
    channel: 'claude-peer',
    probe: 'P17-ClaudePeer',
    date: '2026-09-09',
  }),
  codex: Object.freeze({ channel: 'codex-queue', probe: 'P14-Codex', date: '2026-09-08' }),
  opencode: Object.freeze({ channel: 'opencode-server', probe: 'P5', date: '2026-09-07' }),
  pi: Object.freeze({ channel: 'pi-extension', probe: 'P6', date: '2026-09-07' }),
})

export function enabledChannels(kind) {
  const channel = PROBES[kind]?.channel
  return channel ? [channel] : []
}

function requireLaunchInput(input) {
  if (input === null || typeof input !== 'object') {
    throw new Error('launch configuration needs {launchId, workspace}')
  }
  if (typeof input.launchId !== 'string' || !/^[A-Za-z0-9._-]+$/.test(input.launchId)) {
    throw new Error('launch configuration needs a filename-safe launch id')
  }
  if (typeof input.workspace !== 'string' || input.workspace.length === 0) {
    throw new Error('launch configuration needs a workspace')
  }
  return { launchId: input.launchId, workspace: resolve(input.workspace) }
}

async function freeLoopbackPort() {
  const server = createServer()
  await new Promise((resolvePromise, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolvePromise)
  })
  const address = server.address()
  const port = typeof address === 'object' && address !== null ? address.port : null
  await new Promise((resolvePromise, reject) => {
    server.close((cause) => (cause ? reject(cause) : resolvePromise()))
  })
  if (!Number.isInteger(port) || port <= 0) throw new Error('could not choose a loopback port')
  return port
}

const runHelp = promisify(execFile)

// Feature detection uses the resolved executable, never a second CLI from PATH.
async function hasNativeQueue(kind, executable) {
  if (typeof executable !== 'string' || !isAbsolute(executable)) return false
  if (kind === 'claude-code') return process.platform === 'darwin'
  try {
    const { stdout } = await runHelp(executable, ['queue', '--help'], {
      timeout: 2000,
      maxBuffer: 128 * 1024,
      encoding: 'utf8',
      env: { ...process.env, OPENAI_API_KEY: undefined, ANTHROPIC_API_KEY: undefined },
    })
    if (kind === 'codex') return /--thread\b/.test(stdout) && /--message\b/.test(stdout)
    return false
  } catch {
    return false
  }
}

export async function launchConfiguration(kind, input) {
  const { launchId, workspace } = requireLaunchInput(input)
  if (kind === 'claude-code') {
    // Claude registers its own peer inbox. No development channel, native
    // plugin, launch flag, settings or credential file is created by the app.
    if (!(await hasNativeQueue(kind, input.executable))) return { args: [], env: {}, channel: null }
    const env = input.env ?? process.env
    return {
      args: [],
      env: {},
      channel: {
        kind: 'claude-peer',
        preservesDraft: 1,
        launchId,
        configDir: resolve(env.CLAUDE_CONFIG_DIR ?? join(env.HOME ?? homedir(), '.claude')),
        ackTimeoutMs: 3000,
      },
    }
  }
  if (kind === 'codex') {
    if (!(await hasNativeQueue(kind, input.executable))) return { args: [], env: {}, channel: null }
    const port = await freeLoopbackPort()
    const token = randomBytes(24).toString('base64url')
    return {
      args: [],
      env: { CF_CODEX_SESSION_BRIDGE: JSON.stringify({ launchId, port, token }) },
      channel: {
        kind: 'codex-queue',
        sessionBridge: { endpoint: `http://127.0.0.1:${port}`, token },
        preservesDraft: 1,
        launchId,
        executable: input.executable,
        cwd: workspace,
        ackTimeoutMs: 3000,
      },
    }
  }
  if (kind === 'opencode') {
    let sessionBridge
    let integrationEnv = {}
    if (input.extensionPath) {
      if (input.env?.OPENCODE_TUI_CONFIG) {
        throw new Error(
          'OpenCode has a custom OPENCODE_TUI_CONFIG; its settings were preserved. Remove that launch override to enable ConsensFlow reply delivery.',
        )
      }
      const bridgePort = await freeLoopbackPort()
      const token = randomBytes(24).toString('base64url')
      sessionBridge = { endpoint: `http://127.0.0.1:${bridgePort}`, token }
      integrationEnv = {
        OPENCODE_TUI_CONFIG: join(dirname(input.extensionPath), 'tui.json'),
        CF_OPENCODE_SESSION_BRIDGE: JSON.stringify({ launchId, port: bridgePort, token }),
      }
    }
    const port = await freeLoopbackPort()
    const password = randomBytes(24).toString('base64url')
    const endpoint = `http://127.0.0.1:${port}`
    return {
      args: ['--port', String(port), '--hostname', '127.0.0.1'],
      env: {
        ...integrationEnv,
        OPENCODE_SERVER_PASSWORD: password,
        OPENCODE_SERVER_USERNAME: 'opencode',
      },
      channel: {
        kind: 'opencode-server',
        preservesDraft: 1,
        launchId,
        endpoint,
        password,
        ...(sessionBridge ? { sessionBridge } : {}),
        ackTimeoutMs: DEFAULT_DEADLINE_MS,
      },
    }
  }
  if (kind === 'pi') {
    if (!input.env || typeof input.env !== 'object')
      throw new Error('Pi launch needs an explicit environment')
    const root = join(configRoot(input.env), 'integrations', 'pi', launchId)
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    const quarantine = join(root, 'quarantine')
    const settled = join(root, 'settled')
    const expired = join(root, 'expired')
    const ackTimeoutMs = 30_000
    const extensionAckTimeoutMs = Math.floor(ackTimeoutMs * 0.8)
    const extensionPath =
      input.extensionPath ??
      join(
        dirname(fileURLToPath(import.meta.url)),
        '..',
        'hosts',
        'pi-extension',
        'consensflow-delivery.mjs',
      )
    return {
      args: ['--extension', extensionPath],
      env: {
        CF_DELIVERY_INBOX: inbox,
        CF_DELIVERY_ACK: ack,
        CF_DELIVERY_QUARANTINE: quarantine,
        CF_DELIVERY_SETTLED: settled,
        CF_DELIVERY_EXPIRED: expired,
        CF_DELIVERY_LAUNCH_ID: launchId,
        CF_DELIVERY_EDITOR_GUARD: '1',
        CF_DELIVERY_ACK_TIMEOUT_MS: String(ackTimeoutMs),
        CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS: String(extensionAckTimeoutMs),
      },
      channel: {
        kind: 'pi-extension',
        editorGuard: 1,
        launchId,
        inbox,
        ack,
        quarantine,
        settled,
        expired,
        ackTimeoutMs,
        extensionAckTimeoutMs,
      },
    }
  }
  throw new Error(`no launch configuration for harness: ${kind}`)
}

/** A worker follow-up uses the same launch-owned native ingress as a reply. */
export async function send(channel, target, text) {
  const sender = {
    'claude-peer': sendClaude,
    'codex-queue': sendCodex,
    'opencode-server': sendOpenCode,
    'pi-extension': sendPi,
  }[channel]
  if (!sender) return { ok: false, admitted: false, bytesWritten: 0, error: 'channel-disabled' }
  return await sender(target, text)
}

/** Keep native argument construction in the runners; only owned Codex panes need a supervisor. */
export function withNativeBridge(invocation, configuration, node) {
  if (configuration?.channel?.kind !== 'codex-queue' || !configuration.channel.sessionBridge)
    return invocation
  return {
    ...invocation,
    command: node,
    args: [
      fileURLToPath(new URL('../hosts/codex-session.mjs', import.meta.url)),
      configuration.channel.executable,
      ...invocation.args,
    ],
  }
}
