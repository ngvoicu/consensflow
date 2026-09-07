import { envelope, pointer } from '../../hosts/lib/deliveries.js'

/**
 * The P5 prompt_async endpoint admits a request but may stay silent forever.
 * Every request therefore gets this module-owned `DEFAULT_DEADLINE_MS` of
 * 3_000 ms when its target does not supply one; a deadline result is mapped
 * to `uncertain` by `src/channels.js`.
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

/** POST one admitted user turn to the P5-discovered OpenCode TUI server. */
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

  let response
  let responseText
  try {
    const deadlineMs = target?.deadlineMs ?? DEFAULT_DEADLINE_MS
    const text = record.channel === 'cf-read' ? pointer(record) : envelope(record)
    const request = {
      method: 'POST',
      headers,
      body: JSON.stringify({ parts: [{ type: 'text', text }] }),
    }
    request.signal = AbortSignal.timeout(deadlineMs)
    response = await fetch(url, {
      ...request,
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
  return {
    ok: true,
    admitted: details?.admitted === undefined ? true : details.admitted === true,
    status: response.status,
  }
}
