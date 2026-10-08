/**
 * What Windows' console carries of a non-ASCII character to a window that
 * reads key presses (Devin, Codex), spelled in ASCII where it would drop it.
 * The page's own: it gives it what the human types into a Devin or Codex
 * window. The daemon spells a Devin window's messages the same way in Rust
 * (`console_text`, crates/cf-base/src/text/console.rs), and both are held to
 * one recorded table (tests/console-text.test.mjs here,
 * crates/cf-harness/tests/launch/tables.rs there). No imports: it runs as it
 * is in the page.
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
