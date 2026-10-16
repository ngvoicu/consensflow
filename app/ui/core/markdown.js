import { element } from '../dom.js'

/**
 * The markdown agents write, as the page reads it. Inside a line: bold,
 * italic, `code` and links. How a line starts makes it a heading, a quote,
 * a list item (a task, with its box ticked or not) or a table row; a line of
 * three dashes is a rule, and fences hold code. What it finds comes out as
 * text, never as markup: the elements are made here, and every word in them
 * is set as text.
 */

/** A line that opens or closes code, and the words after its marks (`mermaid`). */
const FENCE = /^\s*(`{3,}|~{3,})\s*([^`\s]*)/
/** A heading's marks: one to six #, before its words. */
const HEADING = /^\s*(#{1,6})\s+/
/** A rule: three or more of one of -, * or _, and nothing else but spaces. */
const RULE_LINE = /^\s*([-*_])(?:\s*\1){2,}\s*$/
/** The box a task's words start with, ticked or not. */
const TASK = /^\[([ xX])\](?:\s+|$)/
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
 * tables, quotes, rules and fenced code. A link is its words, where it leads
 * on hover: nothing on the page follows it.
 *
 * A reader of whole documents (the specs viewer) says more in `options`:
 * `levels` draws each heading as the heading it is (`h1` to `h6`); `link(href)`
 * is what a link does when it is followed, or null where it goes nowhere, and
 * a link with somewhere to go is a button; `fence(lang, code)` draws fenced
 * code of its own (a diagram), or says null for the page's.
 */
export function render(text, options = {}) {
  return blocks(text.split(/\r?\n/)).map((block) => draw(block, options))
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
    if (!code && RULE_LINE.test(line)) continue
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

/**
 * A table row's cells, without the pipes around and between them; `\|` is a pipe in a cell,
 * kept as it is written for the cell's words to read. The row is read once: a cell of a hundred
 * thousand `\|` costs what its length does, not that again for each pipe.
 */
function cells(row) {
  const inner = row.trim().replace(/^\|/, '').replace(/\|$/, '')
  const found = []
  let from = 0
  for (let at = inner.indexOf('|'); at !== -1; at = inner.indexOf('|', at + 1)) {
    if (inner[at - 1] === '\\') continue
    found.push(inner.slice(from, at))
    from = at + 1
  }
  found.push(inner.slice(from))
  return found.map((cell) => cell.trim())
}

/**
 * How deep quotes and lists nest: a line of `- - - …` or `> > > …` past it is
 * read as words, not as a list in a list in a list, which would draw nothing a
 * human reads and run the page out of stack.
 */
const MAX_NESTING = 32

/**
 * The blocks `lines` make, in order, each read by the first of its kinds
 * that starts at its line: a paragraph is what none of the others is. They
 * are `depth` quotes and lists deep.
 */
function blocks(lines, depth = 0) {
  const found = []
  const nests = depth < MAX_NESTING
  for (let at = 0; at < lines.length; ) {
    if (lines[at].trim() === '') {
      at += 1
      continue
    }
    const [block, next] =
      fencedAt(lines, at) ??
      headingAt(lines, at) ??
      (nests ? quoteAt(lines, at, depth) : null) ??
      ruleAt(lines, at) ??
      (nests ? listAt(lines, at, depth) : null) ??
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
  const [, marks, lang] = fence
  let end = at + 1
  while (end < lines.length && !closesFence(lines[end], marks)) end += 1
  return [{ code: lines.slice(at + 1, end).join('\n'), lang: lang.toLowerCase() }, end + 1]
}

/** Whether `line` closes a fence of `marks`: as many of its marks or more, and nothing else. */
function closesFence(line, marks) {
  const bare = line.trim()
  return bare.length >= marks.length && bare === marks[0].repeat(bare.length)
}

/** A heading, its words without its marks, and its level: how many marks it had. */
function headingAt(lines, at) {
  const marks = HEADING.exec(lines[at])
  return marks === null
    ? null
    : [{ heading: lines[at].replace(HEADING, '').trim(), level: marks[1].length }, at + 1]
}

/** A rule across the page. */
function ruleAt(lines, at) {
  return RULE_LINE.test(lines[at]) ? [{ rule: true }, at + 1] : null
}

/** The lines of a quote, without its marks, read as blocks of their own, one level deeper. */
function quoteAt(lines, at, depth) {
  const own = []
  for (; at < lines.length && QUOTE.test(lines[at]); at += 1) own.push(lines[at].replace(QUOTE, ''))
  return own.length === 0 ? null : [{ quote: blocks(own, depth + 1) }, at]
}

/**
 * A list. Each item is its line and every line after it indented past its
 * mark, read as blocks of their own, so a list nests by its indent. Items
 * of one kind at one indent are one list, blank lines between them or not.
 * An item whose words start with a box (`[ ]`, `[x]`) is a task, done or
 * not (`done`; null for an item that is no task). What is in an item is one
 * level deeper.
 */
function listAt(lines, at, depth) {
  const first = ITEM.exec(lines[at])
  if (first === null) return null
  const indent = first[1].length
  const ordered = /\d/.test(first[2])
  const items = []
  for (let item = first; ; ) {
    // Its lines lose as much indent as its mark took, so what nests under it starts the line.
    const width = item[1].length + item[2].length + 1
    const box = TASK.exec(item[3])
    const done = box === null ? null : box[1] !== ' '
    const own = [box === null ? item[3] : item[3].slice(box[0].length)]
    at += 1
    let next = filled(lines, at)
    while (next < lines.length && indentOf(lines[next]) > indent) {
      for (; at <= next; at += 1) own.push(lines[at].slice(Math.min(indentOf(lines[at]), width)))
      next = filled(lines, at)
    }
    items.push({ parts: blocks(own, depth + 1), done })
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
  [FENCE, HEADING, QUOTE, RULE_LINE, ITEM].some((form) => form.test(lines[at])) ||
  startsTable(lines, at)

/** The first line from `at` on that is not blank; past the end when none is. */
function filled(lines, at) {
  while (at < lines.length && lines[at].trim() === '') at += 1
  return at
}

const indentOf = (line) => line.length - line.trimStart().length

/** A block as elements, as `options` says (see `render`). */
function draw(block, options) {
  if ('code' in block) {
    const own = options.fence?.(block.lang, block.code)
    if (own) return own
    const pre = element('pre')
    pre.append(element('code', null, block.code))
    return pre
  }
  if ('heading' in block) {
    const heading = element(options.levels ? `h${block.level}` : 'p', 'md-heading')
    heading.append(...nodes(spans(block.heading), options))
    return heading
  }
  if ('quote' in block) {
    const quote = element('blockquote')
    quote.append(...block.quote.map((inner) => draw(inner, options)))
    return quote
  }
  if ('rule' in block) return element('hr')
  if ('items' in block) return drawList(block, options)
  if ('rows' in block) return drawTable(block.rows, options)
  const paragraph = element('p')
  paragraph.append(...lineNodes(block.paragraph, options))
  return paragraph
}

/**
 * A list, read tight: an item's first paragraph is its line. A task's box is
 * shown ticked or not, and cannot be changed here.
 */
function drawList({ items, ordered, start }, options) {
  const list = element(ordered ? 'ol' : 'ul')
  if (ordered && start !== 1) list.start = start
  for (const { parts, done } of items) {
    const [first, ...rest] = parts
    const drawn = (inner) => draw(inner, options)
    const item = element('li')
    if (done !== null) {
      item.className = 'md-task'
      const box = element('input')
      box.type = 'checkbox'
      box.checked = done
      box.disabled = true
      item.append(box)
    }
    item.append(
      ...(first?.paragraph
        ? [...lineNodes(first.paragraph, options), ...rest.map(drawn)]
        : parts.map(drawn)),
    )
    list.append(item)
  }
  return list
}

/** A table, its first row its head, in a frame that scrolls sideways when the table is wider. */
function drawTable([head, ...rows], options) {
  const table = element('table')
  const top = element('thead')
  top.append(tableRow('th', head, options))
  table.append(top)
  if (rows.length > 0) {
    const body = element('tbody')
    body.append(...rows.map((row) => tableRow('td', row, options)))
    table.append(body)
  }
  const frame = element('div', 'md-table')
  frame.append(table)
  return frame
}

/** A table row, its cells `th` or `td`. */
function tableRow(tag, row, options) {
  const node = element('tr')
  for (const text of row) {
    const cell = element(tag)
    cell.append(...nodes(spans(text), options))
    node.append(cell)
  }
  return node
}

/** A paragraph's lines as nodes, a line break between each. */
const lineNodes = (lines, options) =>
  lines.flatMap((line, at) => [
    ...(at === 0 ? [] : [element('br')]),
    ...nodes(spans(line.trim()), options),
  ])

/**
 * Spans as nodes: text as text, and the rest as the element that marks it. A
 * link `options.link` gives somewhere to go is a button that goes there.
 */
function nodes(found, options = {}) {
  return found.map((span) => {
    if ('text' in span) return span.text
    if ('code' in span) return element('code', null, span.code)
    const follow = 'link' in span ? options.link?.(span.href) : null
    if (follow) {
      const link = element('button', 'md-link')
      link.type = 'button'
      link.title = span.href
      link.append(...nodes(span.link, options))
      link.addEventListener('click', follow)
      return link
    }
    const node = element('strong' in span ? 'strong' : 'em' in span ? 'em' : 'span')
    node.append(...nodes(span.strong ?? span.em ?? span.link, options))
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
