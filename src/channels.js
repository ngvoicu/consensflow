import { execFile } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { createServer } from 'node:net'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { deliver as deliverClaude, send as sendClaude } from './channels/claude-peer.js'
import { deliver as deliverCodex, send as sendCodex } from './channels/codex.js'
import {
  DEFAULT_DEADLINE_MS,
  deliver as deliverOpenCode,
  send as sendOpenCode,
} from './channels/opencode.js'
import { deliver as deliverPi, send as sendPi } from './channels/pi.js'
import { deliver as deliverPty } from './channels/pty.js'

/**
 * Delivery channel launch contract.
 *
 * `launchConfiguration('opencode', { launchId, workspace })` returns
 * `{ args: ['--port', port, '--hostname', '127.0.0.1'], env:
 * { OPENCODE_SERVER_PASSWORD }, channel: { kind: 'opencode-server',
 * endpoint, password, ackTimeoutMs } }`. The endpoint and password are copied
 * into the reservation so the adapter can authenticate its POST without
 * rediscovery; `ackTimeoutMs` is the adapter's absolute acknowledgement
 * budget.
 *
 * `launchConfiguration('pi', { launchId, workspace })` returns
 * `{ args: ['--extension', absoluteExtensionPath], env:
 * { CF_DELIVERY_INBOX, CF_DELIVERY_ACK, CF_DELIVERY_QUARANTINE,
 * CF_DELIVERY_SETTLED, CF_DELIVERY_EXPIRED, CF_DELIVERY_LAUNCH_ID,
 * CF_DELIVERY_ACK_TIMEOUT_MS, CF_DELIVERY_EXTENSION_ACK_TIMEOUT_MS },
 * channel: { kind: 'pi-extension', launchId, inbox, ack, quarantine, settled,
 * expired, ackTimeoutMs, extensionAckTimeoutMs } }`. The inbox, ack,
 * settlement, expiry and quarantine directories are launch-scoped children of
 * the supplied workspace. Each inbox record carries the adapter's one
 * absolute expiry; the extension reads it and does not mint another deadline.
 * OpenCode's adapter owns its 3_000 ms HTTP deadline; Pi's launch owns its
 * 30_000 ms acknowledgement timeout. These are independent values.
 *
 * Adapter result error codes are `unknown-channel`, `channel-disabled`,
 * `transport`, `deadline`, `ack-timeout`, `admission-unknown`, `invalid-record`,
 * `expired`, `missing-envelope`, `failed-with-zero-bytes`, `peer-refused`,
 * and `stale-input-epoch`. Transport,
 * deadline, ack-timeout and admission-unknown are returned to callers as
 * `uncertain`. A confirmed zero-byte stale-input-epoch race can re-pend the
 * same delivery; other failed sends need an explicit resend with a new id.
 *
 * The optional entries below are enabled only by the live probes recorded in
 * findings-01.md: P5 for OpenCode and P6 for Pi, both on 2026-09-07.
 */
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

const CHANNELS = new Map([
  ['claude-peer', deliverClaude],
  ['pty-inline', deliverPty],
  ['cf-read', deliverPty],
  ['opencode-server', deliverOpenCode],
  ['codex-queue', deliverCodex],
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
    return {
      args: [],
      env: {},
      channel: {
        kind: 'codex-queue',
        preservesDraft: 1,
        launchId,
        executable: input.executable,
        cwd: workspace,
        ackTimeoutMs: 3000,
      },
    }
  }
  if (kind === 'opencode') {
    const port = await freeLoopbackPort()
    const password = randomBytes(24).toString('base64url')
    const endpoint = `http://127.0.0.1:${port}`
    return {
      args: ['--port', String(port), '--hostname', '127.0.0.1'],
      env: { OPENCODE_SERVER_PASSWORD: password, OPENCODE_SERVER_USERNAME: 'opencode' },
      channel: {
        kind: 'opencode-server',
        preservesDraft: 1,
        launchId,
        endpoint,
        password,
        ackTimeoutMs: DEFAULT_DEADLINE_MS,
      },
    }
  }
  if (kind === 'pi') {
    const root = join(workspace, '.consensflow', 'deliveries', launchId)
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
