import { randomBytes } from 'node:crypto'
import { createServer } from 'node:net'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { deliver as deliverOpenCode } from './channels/opencode.js'
import { deliver as deliverPi } from './channels/pi.js'
import { deliver as deliverPty } from './channels/pty.js'

/**
 * Delivery channel launch contract.
 *
 * `launchConfiguration('opencode', { launchId, workspace })` returns
 * `{ args: ['--port', port, '--hostname', '127.0.0.1'], env:
 * { OPENCODE_SERVER_PASSWORD }, channel: { kind: 'opencode-server',
 * endpoint, password } }`. The endpoint and password are copied into the
 * reservation so the adapter can authenticate its POST without rediscovery.
 *
 * `launchConfiguration('pi', { launchId, workspace })` returns
 * `{ args: ['--extension', absoluteExtensionPath], env:
 * { CF_DELIVERY_INBOX, CF_DELIVERY_ACK, CF_DELIVERY_QUARANTINE,
 * CF_DELIVERY_ACK_TIMEOUT_MS, CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS },
 * channel: { kind: 'pi-extension', inbox, ack, quarantine, ackTimeoutMs,
 * extensionAckTimeoutMs } }`. The inbox, ack, and quarantine directories are
 * launch-scoped children of the supplied workspace; the extension timeout is
 * strictly shorter than the adapter timeout.
 * OpenCode's adapter owns its 3_000 ms HTTP deadline; Pi's launch owns its
 * 30_000 ms acknowledgement timeout. These are independent values.
 *
 * Adapter result error codes are `unknown-channel`, `channel-disabled`,
 * `transport`, `deadline`, `ack-timeout`, and `missing-envelope`; transport,
 * deadline, and ack-timeout are returned to callers as `uncertain` because
 * admission is unknown.
 *
 * The optional entries below are enabled only by the live probes recorded in
 * findings-01.md: P5 for OpenCode and P6 for Pi, both on 2026-09-07.
 */
const PROBES = Object.freeze({
  opencode: Object.freeze({ channel: 'opencode-server', probe: 'P5', date: '2026-09-07' }),
  pi: Object.freeze({ channel: 'pi-extension', probe: 'P6', date: '2026-09-07' }),
})

const CHANNELS = new Map([
  ['pty-inline', deliverPty],
  ['cf-read', deliverPty],
  ['opencode-server', deliverOpenCode],
  ['pi-extension', deliverPi],
])
const ALWAYS_AVAILABLE = new Set(['pty-inline', 'cf-read'])

export function enabledChannels(kind) {
  const channels = [...ALWAYS_AVAILABLE]
  const probe = PROBES[kind]
  if (probe !== undefined) channels.push(probe.channel)
  return channels
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

export async function launchConfiguration(kind, input) {
  const { launchId, workspace } = requireLaunchInput(input)
  if (kind === 'opencode') {
    const port = await freeLoopbackPort()
    const password = randomBytes(24).toString('base64url')
    const endpoint = `http://127.0.0.1:${port}`
    return {
      args: ['--port', String(port), '--hostname', '127.0.0.1'],
      env: { OPENCODE_SERVER_PASSWORD: password },
      channel: { kind: 'opencode-server', endpoint, password },
    }
  }
  if (kind === 'pi') {
    const root = join(workspace, '.consensflow', 'deliveries', launchId)
    const inbox = join(root, 'inbox')
    const ack = join(root, 'ack')
    const quarantine = join(root, 'quarantine')
    const ackTimeoutMs = 30_000
    const extensionAckTimeoutMs = Math.floor(ackTimeoutMs * 0.8)
    const extensionPath = join(
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
        CF_DELIVERY_ACK_TIMEOUT_MS: String(ackTimeoutMs),
        CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS: String(extensionAckTimeoutMs),
      },
      channel: {
        kind: 'pi-extension',
        inbox,
        ack,
        quarantine,
        ackTimeoutMs,
        extensionAckTimeoutMs,
      },
    }
  }
  throw new Error(`no launch configuration for harness: ${kind}`)
}

/**
 * Deliver only through a channel the lead launch explicitly enabled. The
 * launch object remains on target and is passed untouched to the adapter.
 */
export async function deliver(channel, target, record) {
  if (!CHANNELS.has(channel)) return { ok: false, error: 'unknown-channel' }
  if (
    !ALWAYS_AVAILABLE.has(channel) &&
    (!Array.isArray(target?.enabledChannels) || !target.enabledChannels.includes(channel))
  ) {
    return { ok: false, error: 'channel-disabled' }
  }

  const result = await CHANNELS.get(channel)(channel, target, record)
  if (
    result?.error === 'transport' ||
    result?.error === 'ack-timeout' ||
    result?.error === 'deadline'
  ) {
    return { ok: false, error: 'uncertain', cause: result.cause ?? result.error }
  }
  return result
}
