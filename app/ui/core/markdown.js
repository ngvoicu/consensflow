import { element } from '../dom.js'

/**
 * The markdown agents write, as the page reads it. Inside a line: bold,
 * italic, `code` and links. How a line starts makes it a heading, a quote,
 * a list item or a table row, and fences hold code. What it finds comes out
 * as text, never as markup: the elements are made here, and every word in
 * them is set as text.
 */

/** A line that opens or closes code. */
const FENCE = /^\s*(`{3,}|~{3,})/
/** A heading's marks: one to six #, before its words. */
const HEADING = /^\s*#{1,6}\s+/
/** A quote's mark. */
const QUOTE = /^\s*>\s?/
/** A list item: its indent, its mark (-, * or +, or a number), and its words. */
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/
/** What sits under each cell of a table's head. */
const RULE = /^:?-+:?$/
/** The marks before a line's text that make it a heading, a quote or a list item. */
const LINE_MARKS = /^\s*(?:>\s?)*\s*(?:#{1,6}\s+|[-*+]\s+|\d{1,9}[.)]\s+)?/
/** ASCII punctuation, which a backslash before it makes plain. */
const ESCAPABLE = /[!-/:-@[-`{-~]/
/**
 * `[words](address)`, and an image's `![words](address)`. The words hold
 * no bracket, so each `[` looks no further than the next one; the address
 * may hold a pair of parentheses, as a page named `Lexer_(computing)` does.
 */
const LINK = /!?\[([^[\]]*)\]\(((?:[^()\s]|\([^()\s]*\))*)(?:\s+"[^"]*")?\)/y

/**
 * A body as the page draws it: paragraphs, a line break where the agent
 * broke a line; headings as bold lines; lists, nested by their indent;
 * tables, quotes and fenced code. A link is its words, where it leads on
 * hover: nothing on the page follows it.
 */
export function render(text) {
  return blocks(text.split(/\r?\n/)).map(draw)
}

/**
 * What a folded step shows of a body: its first line that says something,
 * its marks gone, so "## **Done**: 14 tests" reads "Done: 14 tests". A
 * fence says nothing; the code after it says what it is.
 */
export function preview(text) {
  let code = false
  for (const line of text.split(/\r?\n/)) {
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

/**
 * The blocks `lines` make, in order, each read by the first of its kinds
 * that starts at its line: a paragraph is what none of the others is.
 */
function blocks(lines) {
  const found = []
  for (let at = 0; at < lines.length; ) {
    if (lines[at].trim() === '') {
      at += 1
      continue
    }
    const [block, next] =
      fencedAt(lines, at) ??
      headingAt(lines, at) ??
      quoteAt(lines, at) ??
      listAt(lines, at) ??
      tableAt(lines, at) ??
      paragraphAt(lines, at)
    found.push(block)
    at = next
  }
  return found
}

/**
 * Code between fences, as it is. Like each kind of block, it is read as
 * `[block, the line after it]`, or null when it does not start at `at`. A
 * fence nothing closes runs to the end.
 */
function fencedAt(lines, at) {
  const fence = FENCE.exec(lines[at])
  if (fence === null) return null
  const [, marks] = fence
  let end = at + 1
  while (end < lines.length && !closesFence(lines[end], marks)) end += 1
  return [{ code: lines.slice(at + 1, end).join('\n') }, end + 1]
}

/** Whether `line` closes a fence of `marks`: as many of its marks or more, and nothing else. */
function closesFence(line, marks) {
  const bare = line.trim()
  return bare.length >= marks.length && bare === marks[0].repeat(bare.length)
}

/** A heading, its words without its marks. */
function headingAt(lines, at) {
  return HEADING.test(lines[at])
    ? [{ heading: lines[at].replace(HEADING, '').trim() }, at + 1]
    : null
}

/** The lines of a quote, without its marks, read as blocks of their own. */
function quoteAt(lines, at) {
  const own = []
  for (; at < lines.length && QUOTE.test(lines[at]); at += 1) own.push(lines[at].replace(QUOTE, ''))
  return own.length === 0 ? null : [{ quote: blocks(own) }, at]
}

/**
 * A list. Each item is its line and every line after it indented past its
 * mark, read as blocks of their own, so a list nests by its indent. Items
 * of one kind at one indent are one list, blank lines between them or not.
 */
function listAt(lines, at) {
  const first = ITEM.exec(lines[at])
  if (first === null) return null
  const indent = first[1].length
  const ordered = /\d/.test(first[2])
  const items = []
  for (let item = first; ; ) {
    // Its lines lose as much indent as its mark took, so what nests under it starts the line.
    const width = item[1].length + item[2].length + 1
    const own = [item[3]]
    at += 1
    let next = filled(lines, at)
    while (next < lines.length && indentOf(lines[next]) > indent) {
      for (; at <= next; at += 1) own.push(lines[at].slice(Math.min(indentOf(lines[at]), width)))
      next = filled(lines, at)
    }
    items.push(blocks(own))
    item = ITEM.exec(lines[next] ?? '')
    if (item === null || item[1].length !== indent || /\d/.test(item[2]) !== ordered) break
    at = next
  }
  return [{ items, ordered, start: Number.parseInt(first[2], 10) }, at]
}

/** A table: a row of cells, a rule under each of them, then every row after with a pipe in it. */
function tableAt(lines, at) {
  if (!startsTable(lines, at)) return null
  const rows = [cells(lines[at])]
  for (at += 2; at < lines.length && lines[at].includes('|'); at += 1) rows.push(cells(lines[at]))
  return [{ rows }, at]
}

/** Whether a table starts at `at`: a row with a pipe in it, and a rule under each of its cells. */
const startsTable = (lines, at) => {
  if (!lines[at].includes('|') || at + 1 >= lines.length) return false
  const rule = cells(lines[at + 1])
  return rule.length === cells(lines[at]).length && rule.every((cell) => RULE.test(cell))
}

/** A paragraph: its lines, up to a blank one or one that starts another block. */
function paragraphAt(lines, at) {
  const own = [lines[at]]
  for (at += 1; at < lines.length && lines[at].trim() !== '' && !startsBlock(lines, at); at += 1) {
    own.push(lines[at])
  }
  return [{ paragraph: own }, at]
}

/** Whether the line at `at` starts a block other than a paragraph, which ends one. */
const startsBlock = (lines, at) =>
  [FENCE, HEADING, QUOTE, ITEM].some((form) => form.test(lines[at])) || startsTable(lines, at)

/** The first line from `at` on that is not blank; past the end when none is. */
function filled(lines, at) {
  while (at < lines.length && lines[at].trim() === '') at += 1
  return at
}

const indentOf = (line) => line.length - line.trimStart().length

/** A block as elements. */
function draw(block) {
  if ('code' in block) {
    const pre = element('pre')
    pre.append(element('code', null, block.code))
    return pre
  }
  if ('heading' in block) {
    const heading = element('p', 'md-heading')
    heading.append(...nodes(spans(block.heading)))
    return heading
  }
  if ('quote' in block) {
    const quote = element('blockquote')
    quote.append(...block.quote.map(draw))
    return quote
  }
  if ('items' in block) return drawList(block)
  if ('rows' in block) return drawTable(block.rows)
  const paragraph = element('p')
  paragraph.append(...lineNodes(block.paragraph))
  return paragraph
}

/** A list, read tight: an item's first paragraph is its line. */
function drawList({ items, ordered, start }) {
  const list = element(ordered ? 'ol' : 'ul')
  if (ordered && start !== 1) list.start = start
  for (const parts of items) {
    const [first, ...rest] = parts
    const item = element('li')
    item.append(
      ...(first?.paragraph ? [...lineNodes(first.paragraph), ...rest.map(draw)] : parts.map(draw)),
    )
    list.append(item)
  }
  return list
}

/** A table, its first row its head, in a frame that scrolls sideways when the table is wider. */
function drawTable([head, ...rows]) {
  const table = element('table')
  const top = element('thead')
  top.append(tableRow('th', head))
  table.append(top)
  if (rows.length > 0) {
    const body = element('tbody')
    body.append(...rows.map((row) => tableRow('td', row)))
    table.append(body)
  }
  const frame = element('div', 'md-table')
  frame.append(table)
  return frame
}

/** A table row, its cells `th` or `td`. */
function tableRow(tag, row) {
  const node = element('tr')
  for (const text of row) {
    const cell = element(tag)
    cell.append(...nodes(spans(text)))
    node.append(cell)
  }
  return node
}

/** A paragraph's lines as nodes, a line break between each. */
const lineNodes = (lines) =>
  lines.flatMap((line, at) => [...(at === 0 ? [] : [element('br')]), ...nodes(spans(line.trim()))])

/** Spans as nodes: text as text, and the rest as the element that marks it. */
function nodes(found) {
  return found.map((span) => {
    if ('text' in span) return span.text
    if ('code' in span) return element('code', null, span.code)
    const node = element('strong' in span ? 'strong' : 'em' in span ? 'em' : 'span')
    node.append(...nodes(span.strong ?? span.em ?? span.link))
    if ('link' in span) node.title = span.href
    return node
  })
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
