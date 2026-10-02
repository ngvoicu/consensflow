/**
 * The markdown agents write, as the page reads it. Inside a line: bold,
 * italic, `code` and links. How a line starts makes it a heading, a quote,
 * a list item or a table row, and fences hold code. What it finds comes out
 * as text, never as markup.
 */

/** A line that opens or closes code. */
const FENCE = /^\s*(`{3,}|~{3,})/
/** The marks before a line's text that make it a heading, a quote or a list item. */
const LINE_MARKS = /^\s*(?:>\s?)*\s*(?:#{1,6}\s+|[-*+]\s+|\d{1,9}[.)]\s+)?/
/** ASCII punctuation, which a backslash before it makes plain. */
const ESCAPABLE = /[!-/:-@[-`{-~]/
/**
 * `[words](address)`, and an image's `![words](address)`. The words hold
 * no bracket, so each `[` looks no further than the next one.
 */
const LINK = /!?\[([^[\]]*)\]\(([^()\s]*)(?:\s+"[^"]*")?\)/y

/**
 * What a folded step shows of a body: its first line that says something,
 * its marks gone, so "## **Done**: 14 tests" reads "Done: 14 tests". A
 * fence says nothing; the code after it says what it is.
 */
export function preview(text) {
  let code = false
  for (const line of text.split('\n')) {
    if (FENCE.test(line)) {
      code = !code
      continue
    }
    const said = (code ? line : plain(spans(bare(line)))).trim()
    if (said !== '') return said
  }
  return ''
}

/** A line without the marks that make it a heading, a quote, a list item or a table row. */
function bare(line) {
  const text = line.replace(LINE_MARKS, '')
  return text.trimStart().startsWith('|') ? cells(text).join(' · ') : text
}

/** A table row's cells, without the pipes around and between them; `\|` is a pipe in a cell. */
function cells(row) {
  const found = []
  for (const piece of row.trim().replace(/^\|/, '').replace(/\|$/, '').split('|')) {
    if (found.at(-1)?.endsWith('\\')) found[found.length - 1] += `|${piece}`
    else found.push(piece)
  }
  return found.map((cell) => cell.trim())
}

/** What spans say, their marks gone. */
const plain = (found) =>
  found.map((span) => span.text ?? span.code ?? plain(span.strong ?? span.em ?? span.link)).join('')

/**
 * A line's spans, in order: `{ text }`, `{ code }`, and `{ strong }`,
 * `{ em }` or `{ link, href }` around the spans inside them. A mark that
 * opens nothing, or that nothing closes, is text.
 */
function spans(line) {
  const found = []
  // Where looking for what closes a mark found nothing: nothing past there
  // closes it either, so a line full of open marks is read once.
  const unclosed = new Map()
  let text = ''
  for (let at = 0; at < line.length; ) {
    const [span, next] = spanAt(line, at, unclosed) ?? [{ text: line[at] }, at + 1]
    if ('text' in span) {
      text += span.text
    } else {
      if (text !== '') found.push({ text })
      found.push(span)
      text = ''
    }
    at = next
  }
  if (text !== '') found.push({ text })
  return found
}

/** The span that starts at `at`, and where it ends; null when none does. */
function spanAt(line, at, unclosed) {
  const mark = line[at]
  if (mark === '\\' && ESCAPABLE.test(line[at + 1] ?? '')) return [{ text: line[at + 1] }, at + 2]
  if (mark === '`') return code(line, at, unclosed)
  if (mark === '[' || (mark === '!' && line[at + 1] === '[')) return link(line, at)
  if (mark === '*' || mark === '_') return emphasis(line, at, unclosed)
  return null
}

/** `` `code` ``: what is between a run of backticks and the next run as long, as it is. */
function code(line, at, unclosed) {
  const marks = line.slice(at, at + runAt(line, at))
  const end = closer(line, at + marks.length, marks, unclosed)
  return end === -1
    ? [{ text: marks }, at + marks.length]
    : [{ code: line.slice(at + marks.length, end) }, end + marks.length]
}

/** `[words](address)`: the words, and where they lead. An image is its words too. */
function link(line, at) {
  LINK.lastIndex = at
  const found = LINK.exec(line)
  if (found === null) return null
  const [whole, words, href] = found
  return [{ link: words.trim() === '' ? [{ text: href }] : spans(words), href }, at + whole.length]
}

/**
 * `**bold**` and `*italic*`, or `__` and `_`: the first two marks of a run
 * open bold and one opens italic (a third opens italic inside bold), when
 * something follows them. An underscore inside a word is the word's.
 */
function emphasis(line, at, unclosed) {
  const run = runAt(line, at)
  const marks = line.slice(at, at + Math.min(run, 2))
  const opens =
    /\S/.test(line[at + run] ?? '') && !(marks[0] === '_' && /\w/.test(line[at - 1] ?? ''))
  const end = opens ? closer(line, at + marks.length, marks, unclosed) : -1
  if (end === -1) return [{ text: line.slice(at, at + run) }, at + run]
  const inner = spans(line.slice(at + marks.length, end))
  return [marks.length === 2 ? { strong: inner } : { em: inner }, end + marks.length]
}

/**
 * Where the marks that close what `marks` opened begin, looking from
 * `from`; -1 when none do. Code is passed over whole when looking for
 * emphasis, and a backslash's character with it.
 */
function closer(line, from, marks, unclosed) {
  if (from >= (unclosed.get(marks) ?? line.length)) return -1
  const [mark] = marks
  for (let at = from; at < line.length; ) {
    const run = runAt(line, at)
    if (line[at] === mark && closes(line, at, run, marks)) return at + run - marks.length
    if (mark !== '`' && line[at] === '\\') at += 2
    else if (mark !== '`' && line[at] === '`') at = code(line, at, unclosed)[1]
    else at += run
  }
  unclosed.set(marks, from)
  return -1
}

/**
 * Whether `run` marks at `at` close what `marks` opened: code ends at a run
 * exactly as long; emphasis at a run as long, or of three, right after
 * something, and an underscore not inside a word.
 */
function closes(line, at, run, marks) {
  if (marks[0] === '`') return run === marks.length
  return (
    /\S/.test(line[at - 1]) &&
    (run === marks.length || run >= 3) &&
    !(marks[0] === '_' && /\w/.test(line[at + run] ?? ''))
  )
}

/** How many of the character at `at` run from there. */
function runAt(line, at) {
  let end = at
  while (line[end] === line[at]) end += 1
  return end - at
}
