import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const PTY_CHANNELS = new Set(['pty-inline', 'cf-read'])

function paneEpoch(target) {
  const pane = target?.pane
  const id = typeof pane === 'string' ? pane : pane?.id
  const generation = typeof pane === 'object' ? pane?.generation : target?.generation
  if (typeof id !== 'string' || !Number.isSafeInteger(generation) || generation < 1) {
    throw new Error('PTY delivery needs pane {id, generation}')
  }
  if (!Number.isSafeInteger(target.epoch) || target.epoch < 0) {
    throw new Error('PTY delivery needs the caller-observed input epoch')
  }
  return { id, generation }
}

function requireTarget(target) {
  const pane = paneEpoch(target)
  if (target.bridge === null || typeof target.bridge?.request !== 'function') {
    throw new Error('PTY delivery needs the JSON-lines bridge')
  }
  return pane
}

/** Guard a native send with Rust's current draft and input epoch, without I/O. */
export async function claimEpoch(target, operation = 'pane.claim_epoch') {
  const pane = paneEpoch(target)
  try {
    const request = { pane: pane.id, generation: pane.generation, epoch: target.epoch }
    if (typeof target.claimEpoch === 'function') return await target.claimEpoch(request, operation)
    if (target.bridge === null || typeof target.bridge?.request !== 'function') {
      throw new Error('native delivery needs pane.claim_epoch')
    }
    return await target.bridge.request(operation, request, {
      deadlineMs: target.deadlineMs,
    })
  } catch (cause) {
    return { ok: false, error: 'transport', cause: cause?.error ?? cause?.message ?? String(cause) }
  }
}

/** Paste one complete delivery or its cf-read pointer through Rust's arbiter. */
export async function deliver(channel, target, record) {
  if (!PTY_CHANNELS.has(channel)) throw new Error(`unsupported PTY channel: ${channel}`)
  const pane = requireTarget(target)
  const body = channel === 'pty-inline' ? envelope(record) : pointer(record)
  try {
    return await target.bridge.request(
      'pane.write_paste',
      {
        id: pane.id,
        generation: pane.generation,
        epoch: target.epoch,
        body,
      },
      { deadlineMs: target.deadlineMs },
    )
  } catch (cause) {
    return { ok: false, error: 'transport', cause: cause?.error ?? cause?.message ?? String(cause) }
  }
}
