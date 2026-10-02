/** The pane a native send goes to, as the caller named it. */
export function paneOf(target) {
  const pane = target?.pane
  const id = typeof pane === 'string' ? pane : pane?.id
  const generation = typeof pane === 'object' ? pane?.generation : target?.generation
  if (typeof id !== 'string' || !Number.isSafeInteger(generation) || generation < 1) {
    throw new Error('native delivery needs pane {id, generation}')
  }
  return { id, generation }
}

/**
 * Pastes a body into the pane, as the human would type it. A request the
 * bridge had sent before it ended may have been written: uncertain, never a
 * refusal.
 */
export async function writePaste(bridge, pane, body) {
  try {
    return await bridge.request('pane.write_paste', {
      id: pane.id,
      generation: pane.generation,
      body,
    })
  } catch (cause) {
    return { ok: false, admitted: null, error: cause?.error ?? 'transport', cause: cause?.message }
  }
}

/**
 * Admit a native send as Rust admits a paste: the pane is current, its input
 * works and no paste is going in. No I/O; what the human typed holds nothing.
 */
export async function claim(target) {
  const pane = paneOf(target)
  try {
    const request = { pane: pane.id, generation: pane.generation }
    if (typeof target.claim === 'function') return await target.claim(request)
    if (target.bridge === null || typeof target.bridge?.request !== 'function') {
      throw new Error('native delivery needs pane.claim')
    }
    return await target.bridge.request('pane.claim', request, {
      deadlineMs: target.deadlineMs,
    })
  } catch (cause) {
    return { ok: false, error: 'transport', cause: cause?.error ?? cause?.message ?? String(cause) }
  }
}
