import { envelope, pointer } from '../../hosts/lib/deliveries.js'

/**
 * The P5 prompt_async endpoint admits a request but may stay silent forever.
 * Every request therefore gets this module-owned `DEFAULT_DEADLINE_MS` of
 * 3_000 ms when its target does not supply one; a POST deadline is mapped to
 * `uncertain` by `src/channels.js`, while a pre-POST epoch-claim failure is
 * known to have sent zero bytes. When the watcher stamps
 * `record.expiresAt`, that absolute instant is authoritative for this native
 * request and is never recomputed from the channel timeout. A JSON
 * `admitted:null` response remains uncertain rather than becoming false.
 */
export const DEFAULT_DEADLINE_MS = 3_000

function launchConfig(target) {
  const launch = target?.launch
  if (launch === null || typeof launch !== 'object') {
    throw new Error('opencode-server delivery needs the lead launch configuration')
  }
  return launch
}

function endpointSpec(target, launch) {
  return launch.endpoint ?? launch.channel?.endpoint ?? target.channel?.endpoint ?? target.endpoint
}

function endpointUrl(endpoint) {
  const value = typeof endpoint === 'string' ? endpoint : endpoint?.url
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error('opencode-server delivery needs the discovered server endpoint')
  }
  return new URL(value)
}

function authHeader(target, launch, endpoint) {
  const auth =
    launch.auth ??
    target.auth ??
    (typeof endpoint === 'object' ? endpoint.auth : undefined) ??
    (launch.channel?.password === undefined && target.channel?.password === undefined
      ? undefined
      : {
          username: 'opencode',
          password: launch.channel?.password ?? target.channel?.password,
        })
  if (auth === undefined) return undefined
  if (typeof auth?.username !== 'string' || typeof auth?.password !== 'string') {
    throw new Error('opencode-server delivery needs username and password together')
  }
  return `Basic ${Buffer.from(`${auth.username}:${auth.password}`).toString('base64')}`
}

function claimRequest(target) {
  const pane = typeof target?.pane === 'object' ? target.pane?.id : target?.pane
  if (
    typeof pane !== 'string' ||
    !Number.isSafeInteger(target?.generation) ||
    target.generation < 1 ||
    !Number.isSafeInteger(target?.epoch) ||
    target.epoch < 0
  ) {
    throw new Error('opencode-server delivery needs pane, generation and observed epoch')
  }
  return { pane, generation: target.generation, epoch: target.epoch }
}

function validateCaller(target) {
  claimRequest(target)
  if (typeof target?.claimEpoch !== 'function' && typeof target?.bridge?.request !== 'function') {
    throw new Error('opencode-server delivery needs pane.claim_epoch')
  }
  const deadlineMs = target?.deadlineMs
  if (deadlineMs !== undefined && (!Number.isSafeInteger(deadlineMs) || deadlineMs < 0)) {
    throw new Error('opencode-server delivery needs a non-negative deadline')
  }
}

function zeroByteClaimRefusal(claimed) {
  return {
    ok: false,
    admitted: false,
    error: 'failed-with-zero-bytes',
    bytesWritten: 0,
    cause: claimed?.cause ?? claimed?.error ?? 'claim-refused',
  }
}

async function claimEpoch(target) {
  const request = claimRequest(target)
  try {
    if (typeof target.claimEpoch === 'function') return await target.claimEpoch(request)
    if (typeof target.bridge?.request === 'function') {
      return await target.bridge.request('pane.claim_epoch', request, {
        deadlineMs: target.deadlineMs,
      })
    }
  } catch (cause) {
    return { ok: false, error: 'transport', cause: cause?.error ?? cause?.message ?? String(cause) }
  }
  throw new Error('opencode-server delivery needs pane.claim_epoch')
}

/**
 * POST one admitted user turn to the P5-discovered OpenCode TUI server after
 * pane.claim_epoch confirms the caller's observed epoch and clear draft. Any
 * failed claim is retryable: the HTTP request has not started yet.
 */
export async function deliver(channel, target, record) {
  if (channel !== 'opencode-server') throw new Error(`unsupported OpenCode channel: ${channel}`)
  const launch = launchConfig(target)
  const endpointSpecValue = endpointSpec(target, launch)
  const endpoint = endpointUrl(endpointSpecValue)
  const session = target.session ?? launch.session
  if (typeof session !== 'string' || session.length === 0) {
    throw new Error('opencode-server delivery needs the native session id')
  }
  const url = new URL(
    `session/${encodeURIComponent(session)}/prompt_async`,
    endpoint.href.endsWith('/') ? endpoint.href : `${endpoint.href}/`,
  )
  const headers = { 'content-type': 'application/json' }
  const authorization = authHeader(target, launch, endpointSpecValue)
  if (authorization !== undefined) headers.authorization = authorization
  validateCaller(target)
  const text = record.channel === 'cf-read' ? pointer(record) : envelope(record)
  const request = {
    method: 'POST',
    headers,
    body: JSON.stringify({ parts: [{ type: 'text', text }] }),
  }
  const targetDeadlineMs = target?.deadlineMs ?? DEFAULT_DEADLINE_MS

  let response
  let responseText
  try {
    const expiresAt = record?.expiresAt
    if (Number.isFinite(expiresAt) && expiresAt <= Date.now()) {
      return { ok: false, admitted: false, error: 'expired', bytesWritten: 0 }
    }
    const claimed = await claimEpoch(target)
    if (claimed?.ok !== true) {
      return zeroByteClaimRefusal(claimed)
    }
    if (Number.isFinite(expiresAt) && expiresAt <= Date.now()) {
      return { ok: false, admitted: false, error: 'expired', bytesWritten: 0 }
    }
    const deadlineMs = Number.isFinite(expiresAt)
      ? Math.min(DEFAULT_DEADLINE_MS, targetDeadlineMs, expiresAt - Date.now())
      : Math.min(DEFAULT_DEADLINE_MS, targetDeadlineMs)
    response = await fetch(url, {
      ...request,
      signal: AbortSignal.timeout(deadlineMs),
    })
    responseText = await response.text()
  } catch (cause) {
    if (cause?.name === 'TimeoutError' || cause?.name === 'AbortError') {
      return { ok: false, error: 'deadline', cause: 'deadline' }
    }
    return { ok: false, error: 'transport', cause: cause?.message ?? String(cause) }
  }

  if (!response.ok) {
    return {
      ok: false,
      admitted: false,
      status: response.status,
      error: responseText || `http-${response.status}`,
    }
  }
  let details
  if (responseText.length > 0) {
    try {
      details = JSON.parse(responseText)
    } catch {
      return { ok: false, admitted: false, status: response.status, error: 'invalid-admission' }
    }
  }
  if (details?.admitted === null) {
    return {
      ok: false,
      admitted: null,
      error: 'uncertain',
      cause: 'admission-unknown',
      status: response.status,
    }
  }
  return {
    ok: true,
    admitted: details?.admitted === undefined ? true : details.admitted === true,
    status: response.status,
  }
}
