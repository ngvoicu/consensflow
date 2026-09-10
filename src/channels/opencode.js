import { spawn } from 'node:child_process'
import { realpath } from 'node:fs/promises'
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
    error: claimed?.error === 'stale-input-epoch' ? claimed.error : 'failed-with-zero-bytes',
    bytesWritten: 0,
    cause: claimed?.cause ?? claimed?.error ?? 'claim-refused',
  }
}

async function claimEpoch(target) {
  const config = target.launch?.channel ?? target.launch
  const operation = config?.preservesDraft === 1 ? 'pane.claim_native_epoch' : 'pane.claim_epoch'
  const request = claimRequest(target)
  try {
    if (typeof target.claimEpoch === 'function') return await target.claimEpoch(request, operation)
    if (typeof target.bridge?.request === 'function') {
      return await target.bridge.request(operation, request, {
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
async function sendText(target, text, record) {
  if (typeof text !== 'string') throw new Error('opencode-server delivery needs text')
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

export async function send(target, text) {
  return await sendText(target, text)
}

/**
 * A launch controller submits its task once, after its TUI server is ready.
 * New/reopened workers occasionally need more than 15s for the native
 * server under load, so the shared readiness-plus-admission budget defaults
 * to 60s. Every wait stays bounded by `timeoutMs` (readiness polls) or the
 * same lifetime signal (the single task POST).
 */
export async function seedSession({
  channel,
  sessionId,
  cwd,
  text,
  model,
  signal,
  timeoutMs = 60000,
}) {
  const endpoint = new URL(channel?.endpoint)
  if (
    channel?.kind !== 'opencode-server' ||
    endpoint.protocol !== 'http:' ||
    endpoint.hostname !== '127.0.0.1' ||
    typeof channel.password !== 'string' ||
    !channel.password ||
    !/^ses_[A-Za-z0-9]+$/.test(sessionId ?? '') ||
    typeof text !== 'string' ||
    !text ||
    !Number.isFinite(timeoutMs) ||
    timeoutMs <= 0
  )
    throw new Error('invalid OpenCode task launch')
  const body = { parts: [{ type: 'text', text }] }
  if (model) {
    const slash = model.indexOf('/')
    if (slash < 1 || slash === model.length - 1)
      throw new Error('OpenCode task model needs provider/model')
    body.model = { providerID: model.slice(0, slash), modelID: model.slice(slash + 1) }
  }
  const directory = await realpath(cwd)
  const headers = {
    authorization: `Basic ${Buffer.from(`opencode:${channel.password}`).toString('base64')}`,
    'content-type': 'application/json',
  }
  const lifetime = new AbortController()
  const cancel = () => lifetime.abort(signal.reason)
  if (signal?.aborted) cancel()
  else signal?.addEventListener('abort', cancel, { once: true })
  const timer = setTimeout(
    () => lifetime.abort(new Error('OpenCode task startup timed out')),
    timeoutMs,
  )
  try {
    for (;;) {
      lifetime.signal.throwIfAborted()
      let response
      try {
        response = await fetch(new URL('/global/health', endpoint), {
          headers,
          signal: lifetime.signal,
        })
      } catch {
        lifetime.signal.throwIfAborted()
        await sleep(100)
        continue
      }
      await readBoundedText(response, 64 * 1024)
      if (response.status === 401 || response.status === 403)
        throw new Error('OpenCode task server unauthorized')
      if (response.ok) break
      await sleep(100)
    }
    lifetime.signal.throwIfAborted()
    const url = new URL(`/session/${encodeURIComponent(sessionId)}/prompt_async`, endpoint)
    url.searchParams.set('directory', directory)
    // Once this POST starts, neither a transport failure nor a timeout proves
    // non-admission. Leave a visible failure; never retry the task here.
    let response
    try {
      response = await fetch(url, {
        method: 'POST',
        headers,
        body: JSON.stringify(body),
        signal: lifetime.signal,
      })
    } catch {
      throw new Error('OpenCode task admission is uncertain; task was not retried')
    }
    if (response.status !== 204) {
      await response.body?.cancel()
      throw new Error(
        `OpenCode task admission is uncertain (HTTP ${response.status}); task was not retried`,
      )
    }
  } finally {
    clearTimeout(timer)
    signal?.removeEventListener('abort', cancel)
  }
}

const CREATE_JSON_MAX_BYTES = 1024 * 1024
const CREATE_TERM_WAIT_MS = 2000
const CREATE_STOP_SETTLE_MS = 2000

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Read a response body with the size bound enforced while streaming, so a
 * hostile or runaway server cannot make us allocate the whole body first.
 */
async function readBoundedText(response, maxBytes) {
  const stream = response.body
  if (!stream || typeof stream.getReader !== 'function') {
    const text = await response.text()
    if (text.length > maxBytes) throw new Error('opencode session returned an oversized response')
    return text
  }
  const reader = stream.getReader()
  const chunks = []
  let total = 0
  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      total += value.byteLength
      if (total > maxBytes) {
        try {
          await reader.cancel()
        } catch {}
        throw new Error('opencode session returned an oversized response')
      }
      chunks.push(value)
    }
  } finally {
    reader.releaseLock()
  }
  return Buffer.concat(chunks).toString('utf8')
}

/**
 * Create one empty native session on a throwaway `serve` child, then stop it.
 *
 * Used only for fresh/unbound OpenCode leads and workers. The
 * child serves the launch configuration's endpoint just long enough to answer
 * an authenticated `/global/health` and a single `POST /session` with `{}` —
 * no model task, no title — and is reaped BEFORE the id returns, so the
 * normal TUI reuses the port. The whole startup/health/create phase is
 * bounded by `timeoutMs`; only the health probes retry. Every failure stops
 * the child; child stderr is drained and never surfaces in errors.
 */
export async function createSession({
  executable,
  cwd,
  env = process.env,
  configuration,
  timeoutMs = 15000,
} = {}) {
  if (typeof executable !== 'string' || executable.length === 0) {
    throw new Error('opencode session needs an executable')
  }
  if (typeof cwd !== 'string' || cwd.length === 0) {
    throw new Error('opencode session needs a working directory')
  }
  if (configuration === null || typeof configuration !== 'object') {
    throw new Error('opencode session needs launch configuration')
  }
  if (
    !Array.isArray(configuration.args) ||
    !configuration.args.every((a) => typeof a === 'string')
  ) {
    throw new Error('opencode session needs launch configuration')
  }
  const endpointValue = configuration.channel?.endpoint ?? configuration.endpoint
  let endpoint
  try {
    endpoint = new URL(typeof endpointValue === 'string' ? endpointValue : endpointValue?.url)
    if (endpoint.protocol !== 'http:' && endpoint.protocol !== 'https:')
      throw new Error('bad endpoint')
  } catch {
    throw new Error('opencode session needs launch configuration')
  }
  const password = configuration.channel?.password ?? configuration.env?.OPENCODE_SERVER_PASSWORD
  if (typeof password !== 'string' || password.length === 0) {
    throw new Error('opencode session needs launch configuration')
  }
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error('opencode session needs a positive timeout')
  }
  // Canonical workspace: macOS /var vs /private/var and symlinked folders
  // must compare equal against the directory the server returns.
  let canonical
  try {
    canonical = await realpath(cwd)
  } catch {
    throw new Error('opencode session needs a working directory')
  }
  const baseEnv = env && typeof env === 'object' ? env : process.env
  const authorization = `Basic ${Buffer.from(`opencode:${password}`).toString('base64')}`
  const deadline = Date.now() + timeoutMs

  let child
  try {
    child = spawn(executable, ['serve', ...configuration.args], {
      cwd: canonical,
      env: { ...baseEnv, ...(configuration.env ?? {}), OPENCODE_SERVER_USERNAME: 'opencode' },
      stdio: ['ignore', 'ignore', 'pipe'],
    })
  } catch {
    throw new Error('opencode serve failed to start')
  }
  // Reap on 'close', registered immediately: a missing executable emits
  // error/close without exit, and a signal-killed process reports exitCode
  // null with signalCode set — waiting on a future 'exit' alone hangs both.
  let spawnError = null
  let closed = false
  const reaped = new Promise((resolve) => {
    child.once('close', () => {
      closed = true
      process.removeListener('exit', onProcessExit)
      resolve()
    })
  })
  child.once('error', (cause) => {
    spawnError = cause
  })
  // Drain stderr so the child never blocks; contents stay private.
  child.stderr?.on('data', () => {})
  // App drain is 5s against a 15s startup timeout: on parent exit SIGKILL
  // synchronously — SIGTERM could outlive the parent and the port with it.
  const onProcessExit = () => {
    try {
      child.kill('SIGKILL')
    } catch {}
  }
  process.once('exit', onProcessExit)

  const stop = async () => {
    if (!closed && child.exitCode === null && child.signalCode == null && !spawnError) {
      try {
        child.kill('SIGTERM')
      } catch {}
    }
    const fallback = setTimeout(() => {
      if (!closed) {
        try {
          child.kill('SIGKILL')
        } catch {}
      }
    }, CREATE_TERM_WAIT_MS)
    let bound
    try {
      await Promise.race([
        reaped,
        new Promise((_, reject) => {
          bound = setTimeout(
            () => reject(new Error('opencode session failed to stop the server')),
            CREATE_TERM_WAIT_MS + CREATE_STOP_SETTLE_MS,
          )
        }),
      ])
    } finally {
      clearTimeout(fallback)
      clearTimeout(bound)
      // Leave the parent-exit hook until the child is actually gone.
      if (closed) process.removeListener('exit', onProcessExit)
    }
    if (!closed) throw new Error('opencode session failed to stop the server')
  }

  try {
    const healthUrl = new URL(
      'global/health',
      endpoint.href.endsWith('/') ? endpoint.href : `${endpoint.href}/`,
    )
    let ready = false
    while (Date.now() < deadline) {
      if (spawnError) throw new Error('opencode serve failed to start')
      if (closed || child.exitCode !== null) throw new Error('opencode serve exited early')
      const attemptMs = Math.min(500, Math.max(1, deadline - Date.now()))
      let response
      try {
        response = await fetch(healthUrl, {
          headers: { authorization },
          signal: AbortSignal.timeout(attemptMs),
        })
      } catch {
        await sleep(Math.min(100, Math.max(1, deadline - Date.now())))
        continue
      }
      if (response.status === 401) {
        try {
          await response.text()
        } catch {}
        throw new Error('opencode session unauthorized')
      }
      let drained = ''
      try {
        drained = await readBoundedText(response, 64 * 1024)
      } catch {}
      void drained
      if (response.ok) {
        ready = true
        break
      }
      await sleep(Math.min(100, Math.max(1, deadline - Date.now())))
    }
    if (!ready) {
      if (spawnError) throw new Error('opencode serve failed to start')
      if (closed || child.exitCode !== null) throw new Error('opencode serve exited early')
      throw new Error('opencode session timed out')
    }

    const remaining = deadline - Date.now()
    if (remaining <= 0) throw new Error('opencode session timed out')
    const sessionUrl = new URL(
      `session?directory=${encodeURIComponent(canonical)}`,
      endpoint.href.endsWith('/') ? endpoint.href : `${endpoint.href}/`,
    )
    let response
    try {
      response = await fetch(sessionUrl, {
        method: 'POST',
        headers: { authorization, 'content-type': 'application/json' },
        body: '{}',
        signal: AbortSignal.timeout(remaining),
      })
    } catch (cause) {
      if (cause?.name === 'TimeoutError' || cause?.name === 'AbortError') {
        throw new Error('opencode session timed out')
      }
      throw new Error('opencode session transport failed')
    }
    if (response.status === 401) {
      try {
        await response.text()
      } catch {}
      throw new Error('opencode session unauthorized')
    }
    if (!response.ok) throw new Error(`opencode session rejected with status ${response.status}`)
    let text
    try {
      text = await readBoundedText(response, CREATE_JSON_MAX_BYTES)
    } catch (cause) {
      if (cause?.message?.includes('oversized')) throw cause
      throw new Error('opencode session returned an unreadable response')
    }
    let body
    try {
      body = JSON.parse(text)
    } catch {
      throw new Error('opencode session returned invalid JSON')
    }
    const id = body?.id
    if (typeof id !== 'string' || !/^ses_[A-Za-z0-9]+$/.test(id)) {
      throw new Error('opencode session returned an invalid id')
    }
    if (body?.directory !== canonical) {
      throw new Error('opencode session returned the wrong directory')
    }
    await stop()
    return id
  } catch (cause) {
    await stop().catch(() => {})
    throw cause
  }
}

export async function deliver(channel, target, record) {
  if (channel !== 'opencode-server') throw new Error(`unsupported OpenCode channel: ${channel}`)
  return await sendText(
    target,
    record.channel === 'cf-read' ? pointer(record) : envelope(record),
    record,
  )
}
