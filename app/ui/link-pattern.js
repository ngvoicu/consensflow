/**
 * The web addresses a pane makes links of, as plain text (T-1307, T-1315).
 *
 * The rows of a pane are read with one pattern (`app/ui/web-links.js` hands it to the link reader
 * of xterm's web-links addon, which joins the rows xterm wrapped itself). The addon's own pattern
 * leaves out parentheses, brackets, `^`, `|` and braces altogether, so
 * `https://en.wikipedia.org/wiki/Function_(mathematics)` was a link to `…/Function_` (the review of
 * T-1291). This one reads an address the way an address is written:
 *
 * - the scheme is `http` or `https` in any case, then `//`;
 * - every mark an address can hold is part of it, commas, carets, equals signs and the rest;
 * - a parenthesis, a square bracket or a brace is part of it only with its pair inside it
 *   (`Function_(mathematics)`, `?a[]=1`, a host in brackets, `/users/{id}`), to as many levels as
 *   `DEPTH` says. One that is not paired is the sentence's: `(see https://a.example/b)` and
 *   `[https://a.example/b]` end the address before the closing mark;
 * - an apostrophe is part of it (`/wiki/Bayes'_theorem`) but never its last character, so
 *   `'https://a.example/b'` ends at `b`. An address a quotation mark opens (the character just
 *   before it is `'`) ends at the next apostrophe, which closes the quotation: `QUOTED_ADDRESS` is the
 *   same pattern with an apostrophe ending the address, and the reader takes its answer for such
 *   an address (`web-links.js`);
 * - a double quote, a back quote, an angle bracket, a backslash and a space end it, as do the
 *   curly quotes and the ellipsis a program puts around one;
 * - what a sentence puts after one is outside it: `.` `,` `:` `;` `!` `?`, and `*`, `~`, `^` and
 *   `|` that a line of markdown closes with (`SENTENCE_MARKS`; the reader keeps them in an address
 *   that brackets or quotes close right after them, `(https://a.example/Yahoo!)`).
 *
 * Nothing in it backtracks beyond the last mark of the address: the characters that need no pair
 * and the opening marks of a pair are told apart by their first character.
 */

/** How many pairs of brackets one holds in another: `Foo_(bar_(baz))` is two. */
const DEPTH = 3

/** The brackets, which an address holds only in pairs. */
const BRACKETS = '()\\[\\]{}'

/** The quotes that always end an address, and the curly ones a program puts around one. */
const QUOTES = '"`\\u2018\\u2019\\u201c\\u201d'

/** The marks a sentence puts after an address, and a line of markdown closes it with. */
export const SENTENCE_MARKS = '.,:;!?*~^|'

const PAIRS = [
  ['\\(', '\\)'],
  ['\\[', '\\]'],
  ['\\{', '\\}'],
]

/**
 * The pattern xterm's link reader takes. It is given without the `g` flag, which the reader adds,
 * and with `i`: the scheme's letters are the program's. With `apostrophe` an apostrophe is a
 * character of the address (never its last); without, it ends the address.
 */
function pattern(apostrophe) {
  const inner = apostrophe ? QUOTES : `${QUOTES}'`
  // A character that stands for itself in an address: not a space, a quote, a bracket or the like.
  const plain = `[^\\s${inner}<>\\\\${BRACKETS}\\u2026]`
  // A character an address may end with: a plain one that is no apostrophe and no mark a sentence ends with.
  const ending = `[^\\s${QUOTES}'<>\\\\${BRACKETS}\\u2026${SENTENCE_MARKS}]`
  // A pair of brackets with plain characters and pairs, `depth` more levels deep, between them.
  const pair = (depth) => {
    const between = depth === 0 ? plain : `${plain}|${pair(depth - 1)}`
    return PAIRS.map(([open, close]) => `${open}(?:${between})*${close}`).join('|')
  }
  return new RegExp(
    `https?://(?:${plain}|${pair(DEPTH - 1)})*(?:${ending}|${pair(DEPTH - 1)})`,
    'i',
  )
}

/** An address as a program prints it: an apostrophe inside it is part of it. */
export const WEB_ADDRESS = pattern(true)

/** An address a quotation mark opens: the next apostrophe closes it. */
export const QUOTED_ADDRESS = pattern(false)
