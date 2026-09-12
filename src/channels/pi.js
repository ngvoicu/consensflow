import { randomBytes } from 'node:crypto'
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { claimEpoch } from './pty.js'

const ACK_POLL_MS = 10
const ACK_GRACE_MS = ACK_POLL_MS * 3

/** Ask the running extension; an old marker or the headless empty fallback is no proof. */
export async function probeEditor(config, session) {
  const unavailable = { ready: false, reason: 'native editor unavailable' }
  if (typeof session !== 'string') return unavailable
  const response = await probe(config, { session }, 'editor')
  if (response?.session !== session) return unavailable
  if (response.ready === true) return { ready: true }
  return { ready: false, reason: response.reason ?? unavailable.reason }
}

/** A fresh response from this launch, including after Pi /new or /resume. */
export async function currentSession(config) {
  const response = await probe(config, {}, 'session')
  return typeof response?.sessionId === 'string' && response.sessionId.length > 0
    ? response.sessionId
    : null
}

async function probe(config, fields, type) {
  if (
    config?.kind !== 'pi-extension' ||
    config.editorGuard !== 1 ||
    typeof config.inbox !== 'string' ||
    typeof config.ack !== 'string' ||
    typeof config.launchId !== 'string' ||
    !/^[A-Za-z0-9._-]+$/.test(config.launchId)
  )
    return null
  const id = `${type}-${randomBytes(16).toString('hex')}`
  const request = { id, launchId: config.launchId, ...fields, expiresAt: Date.now() + 1000 }
  const inboxFile = join(config.inbox, `${id}.json`)
  const ackFile = join(config.ack, `${id}.json`)
  const temporary = `${inboxFile}.tmp`
  try {
    await mkdir(config.inbox, { recursive: true })
    await writeFile(temporary, `${JSON.stringify(request)}\n`, 'utf8')
    await rename(temporary, inboxFile)
    const response = await ackFor(ackFile, id, request.expiresAt)
    if (
      response?.launchId !== request.launchId ||
      response?.expiresAt !== request.expiresAt ||
      Date.now() >= request.expiresAt
    )
      return null
    return response
  } catch {
    return null
  } finally {
    await Promise.all(
      [temporary, inboxFile, ackFile].map((file) => rm(file, { force: true }).catch(() => {})),
    )
  }
}

function launchConfig(target) {
  const launch = target?.launch
  if (launch === null || typeof launch !== 'object') {
    throw new Error('pi-extension delivery needs the lead launch configuration')
  }
  return launch
}

function channelConfig(target, launch) {
  return launch.channel?.kind === 'pi-extension' ? launch.channel : (target.channel ?? launch)
}

async function ackFor(path, id, expiresAt) {
  while (Date.now() <= expiresAt + ACK_GRACE_MS) {
    try {
      const ack = JSON.parse(await readFile(path, 'utf8'))
      if (ack?.id === id) return ack
    } catch (cause) {
      if (cause?.code !== 'ENOENT' && !(cause instanceof SyntaxError)) throw cause
    }
    const remaining = expiresAt + ACK_GRACE_MS - Date.now()
    if (remaining > 0) {
      await new Promise((resolve) => setTimeout(resolve, Math.min(ACK_POLL_MS, remaining)))
    }
  }
  return null
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

function messageConfig(target) {
  const launch = launchConfig(target)
  const config = channelConfig(target, launch)
  if (config?.kind !== 'pi-extension') {
    throw new Error('unsupported Pi channel: pi-extension')
  }
  const inbox = config.inbox
  const ackDirectory = config.ack
  if (typeof inbox !== 'string' || typeof ackDirectory !== 'string') {
    throw new Error('pi-extension delivery needs inbox and ack directories')
  }
  const ackTimeoutMs = config.ackTimeoutMs
  if (!Number.isFinite(ackTimeoutMs) || ackTimeoutMs < 0) {
    throw new Error('pi-extension delivery needs the channel ack timeout')
  }
  const launchId = config.launchId
  if (typeof launchId !== 'string' || !/^[A-Za-z0-9._-]+$/.test(launchId)) {
    throw new Error('pi-extension delivery needs the launch id')
  }
  return { config, inbox, ackDirectory, ackTimeoutMs, launchId }
}

async function publishRaw(inbox, ackDirectory, record) {
  const inboxFile = join(inbox, `${record.id}.json`)
  const ackFile = join(ackDirectory, `${record.id}.json`)
  const temporary = `${inboxFile}.tmp`
  try {
    await mkdir(inbox, { recursive: true })
    await mkdir(dirname(ackFile), { recursive: true })
    await writeFile(temporary, `${JSON.stringify(record)}\n`, 'utf8')
    return { inboxFile, ackFile, temporary }
  } catch (cause) {
    await rm(temporary, { force: true }).catch(() => {})
    return { error: { ok: false, error: 'transport', cause: cause?.message ?? String(cause) } }
  }
}

function readAckResult(ack) {
  if (ack === null) {
    return { ok: false, admitted: null, error: 'uncertain', cause: 'admission-unknown' }
  }
  if (ack.admitted === null) {
    return { ok: false, admitted: null, error: 'uncertain', cause: 'admission-unknown', ack }
  }
  if (ack.admitted === false) {
    return {
      ok: false,
      admitted: false,
      error: 'failed-with-zero-bytes',
      ...(ack.bytesWritten === 0 ? { bytesWritten: 0 } : {}),
      ack,
    }
  }
  if (ack.admitted !== true) {
    return { ok: false, admitted: null, error: 'uncertain', cause: 'invalid-admission', ack }
  }
  return { ok: true, admitted: true, ack }
}

/**
 * Send one raw worker followup as exact text, without a result envelope.
 * The inbox record is strictly bounded `{id, type, launchId, session, text,
 * expiresAt}` with a unique `m-<hex>` id, the launch's immutable launchId,
 * the target's native session and one absolute expiry. Native admission is
 * gated by pane.claim_epoch immediately before the inbox rename; a failed
 * claim is known zero bytes, a missing ack after publish is uncertain, and
 * the message is never retried automatically.
 */
export async function send(target, text) {
  const { config, inbox, ackDirectory, ackTimeoutMs, launchId } = messageConfig(target)
  if (typeof text !== 'string' || text.length === 0) {
    return { ok: false, admitted: false, error: 'invalid-record' }
  }
  const session = target?.session
  if (typeof session !== 'string' || session.length === 0) {
    return { ok: false, admitted: false, error: 'invalid-record' }
  }
  const id = `m-${randomBytes(16).toString('hex')}`
  const expiresAt = Date.now() + ackTimeoutMs
  if (!Number.isFinite(expiresAt)) {
    return { ok: false, admitted: false, error: 'missing-expiry', bytesWritten: 0 }
  }
  if (expiresAt <= Date.now()) {
    return { ok: false, admitted: false, error: 'expired', bytesWritten: 0 }
  }
  const record = { id, type: 'message', launchId, session, text, expiresAt }
  const prepared = await publishRaw(inbox, ackDirectory, record)
  if (prepared.error) return prepared.error
  const { inboxFile, ackFile, temporary } = prepared
  try {
    const claimed = await claimEpoch(
      target,
      config.editorGuard === 1 ? 'pane.claim_native_epoch' : 'pane.claim_epoch',
    )
    if (claimed?.ok !== true) {
      await rm(temporary, { force: true })
      return zeroByteClaimRefusal(claimed)
    }
    await rename(temporary, inboxFile)
    const ack = await ackFor(ackFile, id, expiresAt)
    if (ack === null) {
      await rm(inboxFile, { force: true })
    }
    return readAckResult(ack)
  } catch (cause) {
    await rm(temporary, { force: true }).catch(() => {})
    return { ok: false, error: 'transport', cause: cause?.message ?? String(cause) }
  }
}
