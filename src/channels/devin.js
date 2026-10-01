import { selectedSession } from '../../hosts/devin-hooks.mjs'

/** Explicit user tasks only. Results enter through native hooks. */
export async function send(target, text) {
  const { channel, session, bridge, pane, generation } = target
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
  return bridge.request('pane.write_paste', { id: pane, generation, body: text })
}
