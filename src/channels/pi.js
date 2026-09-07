import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const ACK_POLL_MS = 10

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

function deliveryId(record) {
  if (!/^d-\d+$/.test(record?.id ?? ''))
    throw new Error('Pi delivery needs a filename-safe delivery id')
  return record.id
}

async function ackFor(path, id, timeoutMs) {
  const started = Date.now()
  while (Date.now() - started <= timeoutMs) {
    try {
      const ack = JSON.parse(await readFile(path, 'utf8'))
      if (ack?.id === id) return ack
    } catch (cause) {
      if (cause?.code !== 'ENOENT' && !(cause instanceof SyntaxError)) throw cause
    }
    await new Promise((resolve) => setTimeout(resolve, ACK_POLL_MS))
  }
  return null
}

/** Write a record for consensflow-delivery and wait for its ack file. */
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
  const id = deliveryId(record)
  if (typeof record?.answer !== 'string') return { ok: false, error: 'missing-envelope' }
  const inboxFile = join(inbox, `${id}.json`)
  const ackFile = join(ackDirectory, `${id}.json`)
  try {
    await mkdir(inbox, { recursive: true })
    await mkdir(dirname(ackFile), { recursive: true })
    const temporary = `${inboxFile}.tmp`
    const text = record.channel === 'cf-read' ? pointer(record) : envelope(record)
    await writeFile(temporary, `${JSON.stringify({ ...record, text })}\n`, 'utf8')
    await rename(temporary, inboxFile)
    const ack = await ackFor(ackFile, id, ackTimeoutMs)
    if (ack === null) return { ok: false, admitted: false, error: 'ack-timeout' }
    return { ok: true, admitted: ack.admitted === true, ack }
  } catch (cause) {
    return { ok: false, error: 'transport', cause: cause?.message ?? String(cause) }
  }
}
