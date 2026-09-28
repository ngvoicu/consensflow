import { selectedSession } from '../../hosts/devin-receiver.mjs'

/** Explicit user tasks only. Results enter through native hooks. */
export async function send(target, text) {
  const { channel, session, bridge, pane, generation, epoch } = target
  let current
  try {
    current = await selectedSession(channel.wire)
  } catch {
    return {
      ok: false,
      admitted: false,
      bytesWritten: 0,
      error: 'Devin conversation is unavailable',
    }
  }
  if (!session || current !== session)
    return {
      ok: false,
      admitted: false,
      bytesWritten: 0,
      error: 'Devin is displaying another conversation',
    }
  // Rust rejects typing or /new arriving since the caller's input snapshot.
  return bridge.request('pane.write_paste', { id: pane, generation, epoch, body: text })
}
