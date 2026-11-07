/**
 * The web addresses a pane makes links of, as plain text (T-1307).
 *
 * xterm's web-links addon reads a row (or the rows a line wrapped over) with one pattern and
 * hands the match to the page. Its own pattern leaves out parentheses, brackets, `^`, `|` and
 * braces altogether, so `https://en.wikipedia.org/wiki/Function_(mathematics)` was a link to
 * `…/Function_` (the review of T-1291). This one reads an address the way an address is written:
 *
 * - the scheme is `http` or `https` in any case, then `//`;
 * - every mark an address can hold is part of it, commas, carets, equals signs and the rest;
 * - a parenthesis, a square bracket or a brace is part of it only with its pair inside it
 *   (`Function_(mathematics)`, `?a[]=1`, a host in brackets, `/users/{id}`), to as many levels as
 *   `DEPTH` says. One that is not paired is the sentence's: `(see https://a.example/b)` and
 *   `[https://a.example/b]` end the address before the closing mark;
 * - a quote, a back quote, an angle bracket, a backslash and a space end it, as do the curly
 *   quotes and the ellipsis a program puts around one;
 * - what a sentence puts after one is outside it: `.` `,` `:` `;` `!` `?`, and `*`, `~`, `^` and
 *   `|` that a line of markdown closes with.
 *
 * Nothing in it backtracks beyond the last mark of the address: the characters that need no pair
 * and the opening marks of a pair are told apart by their first character.
 */

/** How many pairs of brackets one holds in another: `Foo_(bar_(baz))` is two. */
const DEPTH = 3

/** The brackets, which an address holds only in pairs. */
const BRACKETS = '()\\[\\]{}'

/** The quotes, which end an address, and the curly ones a program puts around one. */
const QUOTES = '"\'`\\u2018\\u2019\\u201c\\u201d'

/** The marks a sentence puts after an address, and a line of markdown closes it with. */
const AFTER = '.,:;!?*~^|'

/** A character that stands for itself in an address: not a space, a quote, a bracket or the like. */
const PLAIN = `[^\\s${QUOTES}<>\\\\${BRACKETS}\\u2026]`

/** A character an address may end with: a plain one that is no mark a sentence ends with. */
const ENDING = `[^\\s${QUOTES}<>\\\\${BRACKETS}\\u2026${AFTER}]`

const PAIRS = [
  ['\\(', '\\)'],
  ['\\[', '\\]'],
  ['\\{', '\\}'],
]

/** A pair of brackets with plain characters and pairs, `depth` more levels deep, between them. */
function pair(depth) {
  const inside = depth === 0 ? PLAIN : `${PLAIN}|${pair(depth - 1)}`
  return PAIRS.map(([open, close]) => `${open}(?:${inside})*${close}`).join('|')
}

/**
 * The pattern xterm's web-links addon takes (`urlRegex`). It is given without the `g` flag, which
 * the addon adds, and with `i`: the scheme's letters are the program's.
 */
export const WEB_ADDRESS = new RegExp(
  `https?://(?:${PLAIN}|${pair(DEPTH - 1)})*(?:${ENDING}|${pair(DEPTH - 1)})`,
  'i',
)
