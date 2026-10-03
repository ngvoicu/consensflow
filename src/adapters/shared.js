import { harnessPath } from '../harnesses.js'
import { harnessForKind } from '../roster.js'

/** The absolute path of a harness's CLI here, or a refusal that names it. */
export function executableFor(kind, env) {
  const harness = harnessForKind(kind) ?? kind
  const path = harnessPath(harness, env)
  if (path === null) throw new Error(`${harness} is not installed on this machine`)
  return path
}

/**
 * What a harness record says, in the dispatcher's terms. A conversation with
 * no messages and nothing in flight is idle: a window opened without a task
 * has nothing to finish, and must still be able to receive.
 */
export function recordState(record) {
  const items = Array.isArray(record?.items) ? record.items : []
  const state = record?.settlement?.state
  const empty = items.length === 0 && record?.inFlight !== true
  return {
    items,
    settled: state === 'settled' || (state !== 'in-flight' && empty),
    failed: record?.failed === true,
    quota: record?.quota ?? null,
  }
}

/**
 * A window whose own question dialog is open waits for an answer there (the
 * chief's, from the human; a member's, from the board through its door):
 * nothing is pasted into it, and the board says it waits.
 */
export const dialogWaiting = (record) =>
  record?.asking === true ? { reason: 'its own question dialog is open' } : null

/**
 * A window that shows another conversation than its launch's (the human ran
 * /new, /clear or /resume in it): this reading is the old conversation's last
 * look, nothing in it settles, and the dispatcher follows the window to the
 * one it names. Until it has, a message waits rather than going in.
 */
export const switchedTo = (observed, nativeSession) => ({
  ...observed,
  settled: false,
  waiting: null,
  switched: { nativeSession },
})
export const SHOWS_ANOTHER = 'the window shows another conversation'

/**
 * A window that has not said which conversation it shows (it is starting,
 * switching conversations or reconnecting): a message waits, for this
 * reason. Until the window has named its first conversation, or while it
 * still draws its screen, the dispatcher reads it as starting, not waiting.
 */
export const unnamed = (observed, reason) => ({ ...observed, waiting: { reason }, unnamed: true })

/**
 * Text as a window can take it, for a first message and every later one. The
 * pane host refuses a frame with half a character in it (the dispatcher cuts
 * a long body at 3,000 code units, and an emoji across the cut leaves its
 * first half) and a paste with a control character other than tab and
 * newline; both were retried as a passing failure until the message was
 * dropped, and a harness's own API refuses half a character too. So half a
 * character is dropped, a CR before a newline goes as the host would drop
 * it, and every other control character is shown: as its Unicode picture
 * (ESC as ␛, a lone CR as ␍), or as U+FFFD for the C1 ones, which have none.
 * A window opened without a first message has none (null).
 */
/**
 * The ASCII for what Windows' console drops from a paste. A paste into Devin
 * on Windows reaches it as key presses, and every non-ASCII punctuation mark
 * and symbol among them is lost on the way while letters arrive (Devin
 * 3000.11, 2026-10-03; Codex 0.160 the same, though its messages go another
 * way): a message's dashes, quotes, arrows and currency signs vanished, and
 * its header's separator with them.
 */
const CONSOLE_ASCII = new Map(
  Object.entries({
    '·': '|',
    '•': '*',
    '—': '--',
    '–': '-',
    '‐': '-',
    '‑': '-',
    '−': '-',
    '…': '...',
    '‘': "'",
    '’': "'",
    '‚': "'",
    '“': '"',
    '”': '"',
    '„': '"',
    '«': '<<',
    '»': '>>',
    '→': '->',
    '←': '<-',
    '↔': '<->',
    '⇒': '=>',
    '×': 'x',
    '÷': '/',
    '±': '+/-',
    '≤': '<=',
    '≥': '>=',
    '≠': '!=',
    '≈': '~',
    '°': 'deg',
    '€': 'EUR',
    '£': 'GBP',
    '¥': 'JPY',
    '¿': '?',
    '¡': '!',
    '¦': '|',
    '¬': '!',
    '§': 'S',
    '¶': 'P',
    '©': '(c)',
    '®': '(R)',
    '™': '(TM)',
    '✓': 'OK',
    '✔': 'OK',
    '✅': 'OK',
    '✗': 'X',
    '✘': 'X',
    '❌': 'X',
    '─': '-',
    '━': '-',
    '═': '-',
    '│': '|',
    '┃': '|',
    '║': '|',
    '\ufffd': '?',
  }),
)

/**
 * A window's text as Windows' console carries it to Devin: letters and ASCII
 * as they are, and every other character it would drop in the ASCII that
 * spells it: a mapped mark (— as --, € as EUR), a shown control character
 * in caret notation (␛ as ^[), a space as a space, other box drawing as +,
 * and what Unicode also writes plainly (² as 2, ½ as 1/2). A character with
 * no ASCII spelling (an emoji) is left as it is.
 */
export function consoleText(text) {
  if (text === null) return null
  return Array.from(text.normalize('NFC'), (character) => {
    const code = character.codePointAt(0)
    if (code < 0x80 || /\p{L}/u.test(character)) return character
    const mapped = CONSOLE_ASCII.get(character)
    if (mapped !== undefined) return mapped
    if (code >= 0x2400 && code < 0x2420) return `^${String.fromCharCode(code - 0x2400 + 64)}`
    if (code === 0x2421) return '^?'
    if (/\s/u.test(character)) return ' '
    if (code >= 0x2500 && code < 0x2580) return '+'
    const plain = character.normalize('NFKD').replace(/\p{M}/gu, '').replace('\u2044', '/')
    return /^[\x20-\x7e]+$/.test(plain) ? plain : character
  }).join('')
}

export function windowText(text) {
  if (text === null) return null
  return Array.from(text.replaceAll('\r\n', '\n'), (character) => {
    const code = character.codePointAt(0)
    if (code >= 0xd800 && code <= 0xdfff) return ''
    if (code === 0x09 || code === 0x0a) return character
    if (code < 0x20) return String.fromCodePoint(0x2400 + code)
    if (code === 0x7f) return '\u2421'
    if (code >= 0x80 && code < 0xa0) return '\ufffd'
    return character
  }).join('')
}

/**
 * How a channel's answer to a send reads as an adapter delivery outcome.
 * Only a refusal before the channel's handover point (`admitted: false`)
 * says nothing reached the harness. Any other failure may have reached it,
 * so it is uncertain and the harness's own record decides, rather than a
 * blind second send.
 */
export function admission(sent, refusal, { queued = false } = {}) {
  if (sent?.ok === true) return queued ? { admitted: true, queued: true } : { admitted: true }
  const reason = sent?.cause ?? sent?.error ?? refusal
  return { admitted: sent?.admitted === false ? false : null, reason }
}
