import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { envelope, pointer } from '../../hosts/lib/deliveries.js'
import { claimEpoch } from './pty.js'

const ACK_POLL_MS = 10
const ACK_GRACE_MS = ACK_POLL_MS * 3

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
    error: 'failed-with-zero-bytes',
    bytesWritten: 0,
    cause: claimed?.cause ?? claimed?.error ?? 'claim-refused',
  }
}

/**
 * Write a record for consensflow-delivery and wait for its ack file.
 * `record.expiresAt` is stamped by the watcher and is authoritative; this
 * adapter never recomputes it from the channel timeout. Native admission is
 * gated by pane.claim_epoch immediately before the inbox rename. Any failed
 * claim is retryable because no inbox record exists yet. A true ack is
 * admitted, false is a zero-byte failure, and null is uncertain.
 */
export async function deliver(channel, target, record) {
  if (channel !== 'pi-extension') throw new Error(`unsupported Pi channel: ${channel}`)
  const launch = launchConfig(target)
  const config = channelConfig(target, launch)
  const inbox = config.inbox
  const ackDirectory = config.ack
  if (typeof inbox !== 'string' || typeof ackDirectory !== 'string') {
    throw new Error('pi-extension delivery needs inbox and ack directories')
  }
  const ackTimeoutMs = config.ackTimeoutMs
  if (!Number.isFinite(ackTimeoutMs) || ackTimeoutMs < 0) {
    throw new Error('pi-extension delivery needs the channel ack timeout')
  }
  const id = record?.id
  if (typeof id !== 'string' || !/^d-\d+$/.test(id)) {
    return { ok: false, admitted: false, error: 'invalid-record' }
  }
  if (record.channel !== 'cf-read' && typeof record?.answer !== 'string') {
    return { ok: false, admitted: false, error: 'missing-envelope' }
  }
  let text
  try {
    text = record.channel === 'cf-read' ? pointer(record) : envelope(record)
  } catch (cause) {
    return {
      ok: false,
      admitted: false,
      error: 'invalid-record',
      cause: cause?.message ?? String(cause),
    }
  }
  const expiresAt = record.expiresAt
  if (!Number.isFinite(expiresAt)) {
    return { ok: false, admitted: false, error: 'missing-expiry', bytesWritten: 0 }
  }
  if (expiresAt <= Date.now()) {
    return { ok: false, admitted: false, error: 'expired', bytesWritten: 0 }
  }
  const inboxFile = join(inbox, `${id}.json`)
  const ackFile = join(ackDirectory, `${id}.json`)
  const temporary = `${inboxFile}.tmp`
  try {
    await mkdir(inbox, { recursive: true })
    await mkdir(dirname(ackFile), { recursive: true })
    await writeFile(temporary, `${JSON.stringify({ ...record, expiresAt, text })}\n`, 'utf8')
    const claimed = await claimEpoch(target)
    if (claimed?.ok !== true) {
      await rm(temporary, { force: true })
      return zeroByteClaimRefusal(claimed)
    }
    await rename(temporary, inboxFile)
    const ack = await ackFor(ackFile, id, expiresAt)
    if (ack === null) {
      // Withdraw a still-unread offer. The extension may already have read
      // it, so removing the file never proves zero-byte non-admission.
      await rm(inboxFile, { force: true })
      return { ok: false, admitted: null, error: 'uncertain', cause: 'admission-unknown' }
    }
    if (ack.admitted === null) {
      return { ok: false, admitted: null, error: 'uncertain', cause: 'admission-unknown', ack }
    }
    if (ack.admitted === false) {
      return { ok: false, admitted: false, error: 'failed-with-zero-bytes', ack }
    }
    if (ack.admitted !== true) {
      return { ok: false, admitted: null, error: 'uncertain', cause: 'invalid-admission', ack }
    }
    return { ok: true, admitted: true, ack }
  } catch (cause) {
    await rm(temporary, { force: true }).catch(() => {})
    return { ok: false, error: 'transport', cause: cause?.message ?? String(cause) }
  }
}
