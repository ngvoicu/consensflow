import { claim } from './pty.js'

const DEFAULT_DEADLINE_MS = 3_000
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
}

function launchConfig(target) {
  const launch = target?.launch
  if (launch === null || typeof launch !== 'object' || Array.isArray(launch)) {
    throw new Error('codex-queue delivery needs the chief launch configuration')
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
  // Every Codex window runs under the supervisor: its broker is the only way in.
  if (!config.sessionBridge) throw new Error('codex-queue delivery needs the session broker')
  return config
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

function uncertain(cause) {
  return { ok: false, admitted: null, error: 'uncertain', cause }
}

/**
 * The broker's word on the window: the thread its TUI shows (null while it
 * starts, switches threads or has no TUI attached) and whether it would take
 * a delivery now (a thread shown, no switch, its app-server connected).
 * Undefined when the broker does not answer for this launch.
 */
export async function sessionState(config) {
  if (!config?.sessionBridge || !config.launchId) return undefined
  try {
    const response = await fetch(new URL('/session', config.sessionBridge.endpoint), {
      headers: { authorization: `Bearer ${config.sessionBridge.token}` },
      signal: AbortSignal.timeout(1000),
    })
    const current = await response.json()
    if (!response.ok || current.launchId !== config.launchId) return undefined
    if (current.sessionId !== null && !UUID.test(current.sessionId ?? '')) return undefined
    return { sessionId: current.sessionId, available: current.available === true }
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
  const config = launchConfig(target)
  const session = nativeSession(target)
  if (typeof text !== 'string') throw new Error('codex-queue delivery needs text')

  const deadline = deadlineAt(target, config)
  if (deadline <= Date.now()) return zeroByteRefusal(undefined, 'expired')

  const claimed = await claim(target)
  if (claimed?.ok !== true) return zeroByteRefusal(claimed)
  if (deadline <= Date.now()) return zeroByteRefusal(undefined, 'expired')

  return await sendCurrent(config, session, text, deadline)
}

export async function send(target, text) {
  return await sendText(target, text)
}
