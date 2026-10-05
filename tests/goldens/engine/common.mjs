/**
 * What the engine's text goldens are written with.
 *
 * A long text is written as what it repeats, `{ repeat: [[unit, times], …] }`,
 * so a body of 40,000 characters is a line of the file and not a screen of it;
 * the player (`crates/cf-engine/tests/text/support.rs`) expands it the same
 * way. Node is always given the expanded text.
 */

/** A text of `unit` written `times`, then the next pair, and so on. */
export const repeated = (...parts) => ({ repeat: parts })

/** What a field holds: its text, or what it is a repeat of. */
export const expand = (value) =>
  value !== null && typeof value === 'object' && !Array.isArray(value) && 'repeat' in value
    ? value.repeat.map(([unit, times]) => unit.repeat(times)).join('')
    : value

/**
 * A text as every reader of Node's answer had it: half of a surrogate pair,
 * which `firstLine`'s cut can leave, is stored by the ledger as U+FFFD
 * (`node:sqlite` writes one so) and is U+FFFD in the JSON a `cf` reads. A row
 * says `halved` when its text had such a half.
 */
export function said(text) {
  return text.isWellFormed() ? { text } : { text: text.toWellFormed(), halved: true }
}

/** A number as a row holds it: JSON has no NaN or infinity. */
export const numeral = (number) => (Number.isFinite(number) ? number : { number: String(number) })

/** The number a row's `page` is. */
export const numberOf = (page) => (typeof page === 'object' ? Number(page.number) : page)
