import { spawn } from 'node:child_process'
import { isAbsolute } from 'node:path'
import { claimEpoch } from './pty.js'

export const DEFAULT_DEADLINE_MS = 3_000
export const MAX_CAPTURE_BYTES = 64 * 1024

const KILL_GRACE_MS = 100
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

function paneTarget(target) {
  const pane = target?.pane
  const id = typeof pane === 'object' ? pane?.id : pane
  const generation = typeof pane === 'object' ? pane?.generation : target?.generation
  if (
    typeof id !== 'string' ||
    id.length === 0 ||
    !Number.isSafeInteger(generation) ||
    generation < 1
  ) {
    throw new Error('codex-queue delivery needs pane {id, generation}')
  }
  if (!Number.isSafeInteger(target?.epoch) || target.epoch < 0) {
    throw new Error('codex-queue delivery needs the caller-observed input epoch')
  }
}

function launchConfig(target) {
  const launch = target?.launch
  if (launch === null || typeof launch !== 'object' || Array.isArray(launch)) {
    throw new Error('codex-queue delivery needs the lead launch configuration')
  }

  const config =
    launch.kind === 'codex-queue'
      ? launch
      : launch.channel?.kind === 'codex-queue'
        ? { ...launch, ...launch.channel }
        : null
  if (config === null) {
    throw new Error('codex-queue delivery needs a codex-queue launch configuration')
  }
  if (typeof config.executable !== 'string' || !isAbsolute(config.executable)) {
    throw new Error('codex-queue delivery needs an absolute launch executable')
  }
  if (typeof config.cwd !== 'string' || !isAbsolute(config.cwd)) {
    throw new Error('codex-queue delivery needs an absolute launch cwd')
  }
  return { config, launch }
}

function nativeSession(target) {
  if (typeof target?.session !== 'string' || !UUID.test(target.session)) {
    throw new Error('codex-queue delivery needs a canonical native session UUID')
  }
  return target.session
}

function timeoutBudget(target, config) {
  const configured = config.timeoutMs ?? config.ackTimeoutMs
  const budgets = [configured, target.deadlineMs].filter((value) => value !== undefined)
  if (budgets.some((value) => !Number.isSafeInteger(value) || value < 0)) {
    throw new Error('codex-queue delivery needs a non-negative deadline')
  }
  return Math.min(...budgets, DEFAULT_DEADLINE_MS)
}

function deadlineAt(target, config) {
  const now = Date.now()
  const configured = now + timeoutBudget(target, config)
  return configured
}

function zeroByteRefusal(cause, error = 'failed-with-zero-bytes') {
  return {
    ok: false,
    admitted: false,
    error,
    bytesWritten: 0,
    ...(cause === undefined
      ? {}
      : {
          cause:
            cause?.cause ?? cause?.error ?? (typeof cause === 'string' ? cause : 'claim-refused'),
        }),
  }
}

function appendCapture(current, chunk) {
  const remaining = MAX_CAPTURE_BYTES - Buffer.byteLength(current, 'utf8')
  if (remaining <= 0) return current
  let value = Buffer.from(chunk).subarray(0, remaining).toString('utf8')
  while (Buffer.byteLength(value, 'utf8') > remaining) value = value.slice(0, -1)
  return current + value
}

function childEnvironment(launch, config) {
  const env = { ...process.env, ...(launch.env ?? {}), ...(config.env ?? {}) }
  delete env.OPENAI_API_KEY
  return env
}

function uncertain(cause, details = {}) {
  return { ok: false, admitted: null, error: 'uncertain', cause, ...details }
}

function runQueue(config, launch, session, text, deadline) {
  const args = ['queue', '--thread', session, '--message', text]
  const timeoutMs = Math.max(0, deadline - Date.now())

  return new Promise((resolve) => {
    let child
    let stdout = ''
    let stderr = ''
    let timedOut = false
    let settled = false
    let timeout
    let killTimer

    const finish = (result) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      clearTimeout(killTimer)
      resolve(result)
    }

    const stop = () => {
      if (child?.exitCode === null && child.signalCode === null) {
        child.kill('SIGTERM')
        killTimer = setTimeout(() => {
          if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL')
        }, KILL_GRACE_MS)
        killTimer.unref?.()
      }
    }

    try {
      child = spawn(config.executable, args, {
        cwd: config.cwd,
        env: childEnvironment(launch, config),
        shell: false,
        stdio: ['ignore', 'pipe', 'pipe'],
      })
    } catch (cause) {
      finish(uncertain('transport', { cause: cause?.message ?? String(cause) }))
      return
    }

    if (timeoutMs <= 0) {
      timedOut = true
      stop()
    } else {
      timeout = setTimeout(() => {
        timedOut = true
        stop()
      }, timeoutMs)
      timeout.unref?.()
    }

    child.stdout.on('data', (chunk) => {
      stdout = appendCapture(stdout, chunk)
    })
    child.stderr.on('data', (chunk) => {
      stderr = appendCapture(stderr, chunk)
    })
    child.on('error', () => {
      finish(uncertain('transport', { stdout, stderr }))
    })
    child.on('close', (exitCode, signal) => {
      if (timedOut) {
        finish(uncertain('deadline', { exitCode, signal, stdout, stderr }))
        return
      }
      if (exitCode === 0 && signal === null) {
        // A successful helper exit is admission only. The watcher owns the
        // later native receipt, so this adapter never reports acceptance.
        finish({ ok: true, admitted: true })
        return
      }
      finish(uncertain('helper-exit', { exitCode, signal, stdout, stderr }))
    })
  })
}

/** Native TUI replies identify the main lead, independently of transcript recency. */
export async function currentSession(config) {
  return (await currentSessionState(config))?.sessionId
}

export async function currentSessionState(config) {
  if (!config?.sessionBridge || !config.launchId) return undefined
  try {
    const response = await fetch(new URL('/session', config.sessionBridge.endpoint), {
      headers: { authorization: `Bearer ${config.sessionBridge.token}` },
      signal: AbortSignal.timeout(1000),
    })
    const current = await response.json()
    if (!response.ok || current.launchId !== config.launchId) return undefined
    if (current.sessionId !== null && !UUID.test(current.sessionId ?? '')) return undefined
    return { sessionId: current.sessionId, empty: current.empty === true }
  } catch {
    return undefined
  }
}

async function sendCurrent(config, session, text, deadline) {
  if (Date.now() >= deadline) return zeroByteRefusal(undefined, 'expired')
  try {
    const response = await fetch(new URL('/deliver', config.sessionBridge.endpoint), {
      method: 'POST',
      headers: {
        authorization: `Bearer ${config.sessionBridge.token}`,
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        launchId: config.launchId,
        sessionId: session,
        text,
        expiresAt: deadline,
      }),
      signal: AbortSignal.timeout(Math.max(1, deadline - Date.now())),
    })
    const result = await response.json()
    if (response.ok && result.ok === true && result.admitted === true)
      return { ok: true, admitted: true }
    if (result.admitted === false && result.bytesWritten === 0 && typeof result.error === 'string')
      return zeroByteRefusal(undefined, result.error)
    return uncertain('native-queue-admission')
  } catch {
    return uncertain('native-queue-transport')
  }
}

async function sendText(target, text) {
  paneTarget(target)
  const { config, launch } = launchConfig(target)
  const session = nativeSession(target)
  if (typeof text !== 'string') throw new Error('codex-queue delivery needs text')

  const deadline = deadlineAt(target, config)
  if (deadline <= Date.now()) return zeroByteRefusal(undefined, 'expired')

  const claimed = await claimEpoch(target, 'pane.claim_native_epoch')
  if (claimed?.ok !== true)
    return zeroByteRefusal(
      claimed,
      claimed?.error === 'stale-input-epoch' ? claimed.error : undefined,
    )
  if (deadline <= Date.now()) return zeroByteRefusal(undefined, 'expired')

  if (config.sessionBridge) return await sendCurrent(config, session, text, deadline)
  return await runQueue(config, launch, session, text, deadline)
}

export async function send(target, text) {
  return await sendText(target, text)
}
