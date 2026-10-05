import { randomBytes } from 'node:crypto'
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { claim } from './pty.js'

const ACK_POLL_MS = 10
/**
 * How long past a record's expiry the channel still looks for the
 * extension's verdict. The extension gives one at the expiry, from a timer
 * in Pi's process and then three file operations, so on a busy machine it
 * lands late: 30 ms missed it on CI. A verdict seen late is still Pi's own;
 * one missed leaves the message uncertain, for its record to decide.
 */
const ACK_GRACE_MS = 1000

function launchConfig(target) {
  const launch = target?.launch
  if (launch === null || typeof launch !== 'object') {
    throw new Error('pi-extension delivery needs the chief launch configuration')
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
    // The last millisecond is slept out, not read through: a loop that does
    // not wait reads the file again and again until the clock moves.
    const remaining = expiresAt + ACK_GRACE_MS - Date.now()
    if (remaining >= 0) {
      await new Promise((resolve) =>
        setTimeout(resolve, Math.max(1, Math.min(ACK_POLL_MS, remaining))),
      )
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

/** A failure before the inbox rename: the record never reached Pi's inbox. */
function zeroByteTransport(cause) {
  return {
    ok: false,
    admitted: false,
    error: 'transport',
    bytesWritten: 0,
    cause: cause?.message ?? String(cause),
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
  return { inbox, ackDirectory, ackTimeoutMs, launchId }
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
    return { error: zeroByteTransport(cause) }
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
 * The conversation ConsensFlow's extension says the window shows (it writes
 * it whenever it starts on one), or null before it has said.
 */
export async function shownSession(channel) {
  try {
    const shown = JSON.parse(
      await readFile(join(channel.settled, `${channel.launchId}.shown.json`), 'utf8'),
    )
    return shown?.launchId === channel.launchId && typeof shown.sessionId === 'string'
      ? shown.sessionId
      : null
  } catch (cause) {
    if (cause?.code === 'ENOENT' || cause instanceof SyntaxError) return null
    throw cause
  }
}

/**
 * Send one raw worker followup as exact text, without a result envelope.
 * The inbox record is strictly bounded `{id, type, launchId, session, text,
 * expiresAt}` with a unique `m-<hex>` id, the launch's immutable launchId,
 * the target's native session and one absolute expiry. Native admission is
 * gated by pane.claim immediately before the inbox rename, the handover
 * point: a failed claim, or any failure before the rename, is known zero
 * bytes. From the rename on, Pi may have taken the message, so a missing ack
 * or any error is uncertain and Pi's own record decides; the message is
 * never retried automatically.
 */
export async function send(target, text) {
  const { inbox, ackDirectory, ackTimeoutMs, launchId } = messageConfig(target)
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
    const claimed = await claim(target)
    if (claimed?.ok !== true) {
      await rm(temporary, { force: true })
      return zeroByteClaimRefusal(claimed)
    }
  } catch (cause) {
    await rm(temporary, { force: true }).catch(() => {})
    return zeroByteTransport(cause)
  }
  try {
    await rename(temporary, inboxFile)
    const ack = await ackFor(ackFile, id, expiresAt)
    if (ack === null) {
      await rm(inboxFile, { force: true })
    }
    return readAckResult(ack)
  } catch (cause) {
    // The record stays in the inbox: Pi may be taking it now, and the
    // extension refuses it once it expires.
    await rm(temporary, { force: true }).catch(() => {})
    return { ok: false, admitted: null, error: 'uncertain', cause: cause?.message ?? String(cause) }
  }
}
