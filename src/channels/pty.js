import { envelope, pointer } from '../../hosts/lib/deliveries.js'

const PTY_CHANNELS = new Set(['pty-inline', 'cf-read'])

function requireTarget(target) {
  const pane = target?.pane
  const id = typeof pane === 'string' ? pane : pane?.id
  const generation = typeof pane === 'object' ? pane?.generation : target?.generation
  if (typeof id !== 'string' || !Number.isSafeInteger(generation) || generation < 1) {
    throw new Error('PTY delivery needs pane {id, generation}')
  }
  if (!Number.isSafeInteger(target.epoch) || target.epoch < 0) {
    throw new Error('PTY delivery needs the caller-observed input epoch')
  }
  if (target.bridge === null || typeof target.bridge?.request !== 'function') {
    throw new Error('PTY delivery needs the JSON-lines bridge')
  }
  return { id, generation }
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
