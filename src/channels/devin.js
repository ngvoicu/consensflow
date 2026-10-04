import { selectedSession } from '../devin-wire.js'
import { writePaste } from './pty.js'

/** A message pasted into Devin's window, only while it shows the conversation it is for. */
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
  return writePaste(bridge, { id: pane, generation }, text)
}
