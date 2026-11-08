import { QUOTED_ADDRESS, SENTENCE_MARKS, WEB_ADDRESS } from './link-pattern.js'
import { LinkComputer } from './vendor/xterm.js'

/**
 * The web addresses a program prints as plain text, as links of a pane (T-1315).
 *
 * xterm's web-links addon reads the rows with a pattern, joining the rows xterm itself wrapped
 * (`isWrapped`), and hands each match on as a link. This is its link reader (`LinkComputer`) with
 * three rules of the app's own on what it found, each so that a link opens the address the program
 * meant or is no link at all:
 *
 * - **An address that may be cut is no link.** A program that lays its text out in rows of its own
 *   and writes them with CR/LF (Pi does) breaks a long address where its width ends, and xterm,
 *   which saw a line end, does not mark the next row as a continuation: `…/Function_(mathema` and
 *   `tics))` are two rows, and nothing says they are one address. So a plain address is no link
 *   when the row it ends in may go on in the next one and xterm did not say so: the text it ends
 *   in runs up to the row's last column, or ends the row's text within `EDGE_MARGIN` columns of it
 *   (the margin a program keeps: Pi pads its rows a column each side). The next row marked wrapped
 *   settles it the other way: xterm joined it, and the pattern itself ended the address. So does a
 *   bracket or quote that closes the pair the address stands in, right after it. A row that stops
 *   short of the edge keeps its link, whatever row follows it: lines of addresses, one under the
 *   other, are links. (An OSC 8 hyperlink carries its whole address on every row it is drawn on
 *   and does not come through here.)
 * - **An apostrophe that opens an address is a quotation mark.** The pattern reads an apostrophe
 *   inside an address as part of it (`/wiki/Bayes'_theorem`); one right before the `http` is the
 *   quote the address stands in, and the next apostrophe closes it.
 * - **Marks a pair closes right after are the address's.** `(https://a.example/Yahoo!)` is an
 *   address with its `!`, the program having put the brackets round it; a sentence's mark comes
 *   after the closing one. An address its pair closed is whole: it was not cut by its row's end.
 */

/** How many columns short of the last one a program's own margin may leave its rows. */
const EDGE_MARGIN = 2

/** What closes the character that opens an address: the pairs a program puts round one. */
const CLOSES = new Map([
  ['(', ')'],
  ['[', ']'],
  ['{', '}'],
  ['<', '>'],
  ['"', '"'],
  ["'", "'"],
  ['`', '`'],
  ['“', '”'],
  ['‘', '’'],
])

/** Whether the cell `a` comes before `b` (1-based columns and rows). */
const comesBefore = (a, b) => a.y < b.y || (a.y === b.y && a.x < b.x)

/** The characters of the cell at column `x` (0-based) of `line`, '' when it holds none. */
const charsAt = (line, x, cell) => line.getCell(x, cell)?.getChars() ?? ''

/** Whether the cell at `x` of `line` shows nothing: a space or a cell never written. */
function blankAt(line, x, cell) {
  const at = line.getCell(x, cell)
  if (!at) return true
  // The second half of a wide character has no width: it belongs to the character before it.
  return at.getWidth() !== 0 && (at.getChars() === '' || at.getChars() === ' ')
}

/** The character right before the start of `range`, '' when there is none. */
function before(terminal, range, cell) {
  const buffer = terminal.buffer.active
  const { x, y } = range.start
  const line = buffer.getLine(y - 1)
  if (x >= 2) return charsAt(line, x - 2, cell)
  const above = line?.isWrapped ? buffer.getLine(y - 2) : undefined
  return above ? charsAt(above, terminal.cols - 1, cell) : ''
}

/**
 * Where the address of `range` ends: its last cell's column (1-based) and row (1-based). The
 * reader reports an address that ends in a row's last column as ending at column 0 of the row
 * below.
 */
function endOf(terminal, range) {
  const { x, y } = range.end
  return x === 0 ? { x: terminal.cols, y: y - 1 } : { x, y }
}

/**
 * Whether the plain address ending at `end` (`endOf`) may be cut by its row's end.
 * @param {import('@xterm/xterm').Terminal} terminal
 */
function mayBeCut(terminal, end, cell) {
  const buffer = terminal.buffer.active
  const line = buffer.getLine(end.y - 1)
  const next = buffer.getLine(end.y)
  if (!line || next?.isWrapped) return false
  const { cols } = terminal
  // The run of text the address ends in goes on up to a blank, or to the last column.
  let after = end.x
  while (after < cols && !blankAt(line, after, cell)) after += 1
  if (after >= cols) return true
  // Only blanks after it: the address ends the row's text. Anything else, and the program put
  // a space after the address, which is where it ended.
  for (let x = after; x < cols; x += 1) if (!blankAt(line, x, cell)) return false
  return cols - after <= EDGE_MARGIN
}

/**
 * Makes `link`, an address a pair of brackets or quotes opens (the character before it), keep the
 * marks right after it that the pair closes (`(https://a.example/Yahoo!)`), and says whether the
 * pair does close right after it: an address its pair closed is whole whatever its row's width.
 */
function closedBy(terminal, link, cell) {
  const opener = before(terminal, link.range, cell)
  const closer = CLOSES.get(opener)
  if (!closer) return { link, closed: false }
  const end = endOf(terminal, link.range)
  const line = terminal.buffer.active.getLine(end.y - 1)
  let marks = ''
  for (let x = end.x; x < terminal.cols; x += 1) {
    const chars = charsAt(line, x, cell)
    if (chars === closer) {
      if (marks === '') return { link, closed: true }
      const kept = { x: end.x + marks.length, y: end.y }
      return {
        link: { ...link, range: { ...link.range, end: kept }, text: link.text + marks },
        closed: true,
      }
    }
    if (chars.length !== 1 || !SENTENCE_MARKS.includes(chars)) break
    marks += chars
  }
  return { link, closed: false }
}

/**
 * The link provider of the pane's plain web addresses, for `terminal.registerLinkProvider`.
 * `activate(event, address)` opens one, `hover` and `leave` are called as the pointer comes on
 * and goes off it.
 */
export function webLinks(terminal, { activate, hover, leave }) {
  const cell = terminal.buffer.active.getNullCell()
  return {
    provideLinks(y, callback) {
      const found = LinkComputer.computeLink(y, WEB_ADDRESS, terminal, activate)
      // An address a quotation mark opens ends at the apostrophe that closes it, and the pattern,
      // which takes an apostrophe for the address's, may have swallowed the quotation's other
      // addresses with it (`['https://a.example/x','https://b.example/y']`): the pattern that
      // ends at an apostrophe finds them.
      const opened = (link) => link.text.includes("'") && before(terminal, link.range, cell) === "'"
      const quoted = found.some(opened)
        ? LinkComputer.computeLink(y, QUOTED_ADDRESS, terminal, activate)
        : []
      const links = found
        .flatMap((link) =>
          opened(link)
            ? quoted.filter(
                ({ range }) =>
                  !comesBefore(range.start, link.range.start) &&
                  comesBefore(range.start, endOf(terminal, link.range)),
              )
            : [link],
        )
        .map((link) => closedBy(terminal, link, cell))
        .filter(
          ({ link, closed }) => closed || !mayBeCut(terminal, endOf(terminal, link.range), cell),
        )
        .map(({ link }) => link)
      for (const link of links) {
        link.hover = (event, address) => hover?.(event, address, link.range)
        link.leave = leave
      }
      callback(links.length > 0 ? links : undefined)
    },
  }
}
