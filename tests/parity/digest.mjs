/**
 * What `npm run parity:records` compares of a reading (`records.mjs` writes
 * it for Node's, `crates/cf-harness/tests/parity.rs` makes it for Rust's):
 * never the text itself.
 */
import { createHash } from 'node:crypto'

/**
 * `value` with each text in it, a name of an object's member too, made whole
 * (`toWellFormed`: a lone surrogate is U+FFFD, as the Rust readers hold it),
 * and whether each already was.
 */
export function wellFormed(value) {
  let whole = true
  const walk = (value) => {
    if (typeof value === 'string') {
      const made = value.toWellFormed()
      whole &&= made === value
      return made
    }
    if (Array.isArray(value)) return value.map(walk)
    if (value !== null && typeof value === 'object') {
      return Object.fromEntries(
        Object.entries(value).map(([name, field]) => [walk(name), walk(field)]),
      )
    }
    return value
  }
  return [walk(value), whole]
}

/**
 * A reading's digest: each item's id, role, completeness, time (none, or
 * one), its text's UTF-16 length and SHA-256, and whether it is commentary;
 * then the reading's flags, quota and settlement. A reason that begins as
 * none of `ours` (ConsensFlow's own sentences) is the platform's, compared as
 * its class. Its texts are made whole first, and `wellFormed` says whether
 * they were: a lone surrogate, which the Rust readers read as U+FFFD, is a
 * difference kept on purpose, which the Rust half counts so.
 */
export function digest(raw, ours) {
  const [reading, whole] = wellFormed(raw)
  if (reading.unknown) {
    const own = ours.some((prefix) => reading.reason.startsWith(prefix))
    return {
      unknown: true,
      reason: own ? reading.reason : 'unreadable: «platform»',
      wellFormed: whole,
    }
  }
  return {
    items: reading.items.map((item) => [
      item.id,
      item.role,
      item.complete,
      // An item's time, or none where it holds none: undefined is not null.
      item.at === undefined ? [] : [item.at],
      item.text.length,
      createHash('sha256').update(item.text).digest('hex'),
      item.commentary === true,
    ]),
    inFlight: reading.inFlight,
    asking: reading.asking,
    failed: reading.failed,
    quota: reading.quota,
    settlement: reading.settlement.state,
    wellFormed: whole,
  }
}
