import { randomBytes } from 'node:crypto'
import { createServer } from 'node:net'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { DEFAULT_DEADLINE_MS } from './channels/opencode.js'
import { probeExecutable } from './harnesses.js'
import { configRoot } from './roster.js'

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

/**
 * A Codex window runs under ConsensFlow's supervisor, which needs the native
 * queue (and the app-server and remote TUI that came with it): a Codex
 * without it cannot be reached in its window, so it is refused by its
 * version. Detection asks the resolved executable, never a second CLI from
 * PATH.
 */
async function requireNativeQueue(executable, env) {
  if (typeof executable !== 'string' || !isAbsolute(executable))
    throw new Error('a Codex launch needs the absolute path of its CLI')
  const help = await probeExecutable(executable, ['queue', '--help'], env).catch((cause) => {
    throw new Error(
      `could not ask Codex whether it has its native queue: ${cause.killed ? 'it did not answer in time' : cause.message}`,
    )
  })
  if (/--thread\b/.test(help.stdout) && /--message\b/.test(help.stdout)) return
  const version = await probeExecutable(executable, ['--version'], env).then(
    ({ stdout }) => stdout.match(/\d+\.\d+\.\d+\S*/)?.[0] ?? null,
    () => null,
  )
  throw new Error(
    `${version === null ? 'This Codex' : `Codex ${version}`} has no native queue, which ConsensFlow needs to reach its window: update Codex.`,
  )
}

export async function launchConfiguration(kind, input) {
  const { launchId, workspace } = requireLaunchInput(input)
  if (kind === 'codex') {
    await requireNativeQueue(input.executable, input.env)
    const port = await freeLoopbackPort()
    const token = randomBytes(24).toString('base64url')
    return {
      args: [],
      env: { CF_CODEX_SESSION_BRIDGE: JSON.stringify({ launchId, port, token }) },
      channel: {
        kind: 'codex-queue',
        sessionBridge: { endpoint: `http://127.0.0.1:${port}`, token },
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
      },
      channel: {
        kind: 'pi-extension',
        launchId,
        inbox,
        ack,
        quarantine,
        settled,
        expired,
        ackTimeoutMs: 30_000,
      },
    }
  }
  throw new Error(`no launch configuration for harness: ${kind}`)
}

/** Native argument construction stays in the window builders; Codex opens under its supervisor. */
export function withNativeBridge(invocation, configuration, node) {
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
