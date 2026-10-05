/**
 * What differs from one run of a suite to the next, and so is written down
 * under a name a player maps back: the ledger's file and the test's temporary
 * folder, the API's address, each window's token. `validate` refuses a trace
 * that still holds one of them, so nothing that varies is left unmapped.
 */
import { tmpdir } from 'node:os'

/** The text of `text` as it sits inside a JSON string. */
const inside = (text) => JSON.stringify(text).slice(1, -1)

export const tokenName = (tokens, real) => (typeof real === 'string' ? tokens.get(real) : undefined)

/**
 * `text` (a trace as JSON) with every real path, address and token named:
 * `«ledger»` the ledger's file, `«root»` the folder the test made, `«api»` the
 * host and port the API listened on, `«token:T1»` a window's token.
 */
export function mask(text, { ledgers, roots, apis, tokens }) {
  const swaps = [
    ...ledgers.map(({ record }) => [inside(record.file), '«ledger»']),
    ...[...roots].map((root) => [inside(root), '«root»']),
    ...apis.map((url) => [new URL(url).host, '«api»']),
    ...[...tokens].map(([real, name]) => [real, `«token:${name}»`]),
  ]
  // The longest first: the ledger's file lies inside the test's folder.
  swaps.sort((a, b) => b[0].length - a[0].length)
  return swaps.reduce((masked, [from, to]) => masked.replaceAll(from, to), text)
}

/**
 * Throws when `text` still holds what `mask` was to name: the machine's
 * temporary folder, or 64 hex digits that look drawn at random (a token no
 * `issue` gave; sixty-four of the same digit are somebody's padding).
 */
export function validate(text) {
  const left = []
  if (text.includes(inside(tmpdir()))) left.push(`a temporary path (${tmpdir()})`)
  const drawn = text.match(/[0-9a-f]{64}/g)?.some((run) => new Set(run).size >= 8)
  if (drawn) left.push('64 hex digits, a token no `issue` gave')
  if (left.length > 0)
    throw new Error(`the trace holds what varies from run to run: ${left.join(', ')}`)
}

/** A roster file as written down: the times it stamped itself with are the clock's, not the file's. */
export const maskStamps = (text) =>
  text.replace(/("(?:createdAt|updatedAt)": )"[^"]*"/g, '$1"«now»"')
