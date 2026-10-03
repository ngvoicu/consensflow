/**
 * How a message reads in its recipient's pane. Its header doubles as the
 * proof that it arrived: a delivery counts once the window's own record
 * shows the header's start (`markerOf`), so the two are written together.
 */

/** A body longer than this goes as its opening and the command that reads the rest. */
const INLINE_LIMIT = 4000
const OPENING = 3000

/** How a message reads in the recipient's pane. The header doubles as the arrival marker. */
export function deliveryText(message) {
  const from = message.sender === null ? 'ConsensFlow' : `@${message.sender}`
  const task =
    message.taskNumber === null || message.taskNumber === undefined
      ? ''
      : ` · T-${message.taskNumber}`
  const body =
    message.body.length <= INLINE_LIMIT
      ? message.body
      : `${message.body.slice(0, OPENING)}\n… (${message.body.length} characters; read all of it with: cf inbox read m-${message.id})`
  // A question says how to answer it; a result says what to do with it, so
  // the reader decides on the board even when its harness frames the message
  // as a request.
  const footer =
    message.kind === 'question'
      ? message.questions
        ? `\n\nRun in your shell: cf answer m-${message.id} "…" (a label or your own words${message.questions.length > 1 ? '; one line per question' : ''})`
        : message.urgent && message.taskNumber != null
          ? `\n\nT-${message.taskNumber} is paused for this. Run in your shell: cf answer m-${message.id} "…"; the chief resumes the task.`
          : `\n\nRun in your shell: cf answer m-${message.id} "…"`
      : message.kind === 'result' && message.taskNumber != null
        ? `\n\nDecide with: cf task accept T-${message.taskNumber} · cf task reopen T-${message.taskNumber} "…"`
        : ''
  return `[ConsensFlow m-${message.id}${task} · ${message.kind} from ${from}]\n${body}${footer}`
}

/**
 * The start of a message's header, as a window's record shows it once the
 * message arrived. The space after the id keeps m-1 from matching m-12; what
 * follows it is not part of the marker, since a window may not keep the ·
 * (Devin on Windows takes it as |, see consoleText).
 */
export const markerOf = (messageId) => `[ConsensFlow m-${messageId} `
