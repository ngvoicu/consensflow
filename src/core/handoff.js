/**
 * What passes to a lead the human switched in (2026-10-01): its first message,
 * written from the ledger without a model (so it works when the old lead is
 * out of quota), and `cf history`, the earlier lead conversations in pages.
 *
 * Pages run newest first, each in the order things were said, and each is
 * small enough for every harness to show its model whole: Codex shows the
 * least of a command's output (about 10 KiB and 256 lines). The human's words
 * and the leads' answers are whole; a tool's output only on request. A
 * delivery ConsensFlow made is one line with its outcome, never its header: a
 * header in a window's record is how a delivery is proven to have arrived, so
 * a page that printed one could prove a delivery that never landed.
 */

const PAGE_BYTES = 8_000
const PAGE_LINES = 200

/** Room each page keeps for its own first and last lines. */
const FRAME_BYTES = 600
const FRAME_LINES = 6

const HARNESS_NAMES = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  opencode: 'OpenCode',
  pi: 'Pi',
  devin: 'Devin',
}

const nameOf = (harness) => HARNESS_NAMES[harness] ?? harness
const DELIVERY = /\[ConsensFlow m-(\d+) ·/
/** A first message from ConsensFlow that hands the lead over. */
export const HANDOFF_TITLE = 'You are the lead now'

/** No text on a page may read as a delivery's header. */
const defuse = (text) => text.replaceAll('[ConsensFlow m-', '[earlier m-')
const firstLine = (text, max = 160) => {
  const line = text.split('\n').find((part) => part.trim() !== '') ?? ''
  return line.length > max ? `${line.slice(0, max - 1)}…` : line
}
const bytes = (text) => Buffer.byteLength(text, 'utf8')
const lines = (text) => text.split('\n').length

/**
 * The new lead's first message. `from` and `to` are `{ harness, agent }`;
 * `open` is the ledger's `leadOpenWork`; `last` is the human's last words to
 * the old lead and whether it answered them; `cut` says its turn was cut.
 */
export function handoffText({ from, to, open, last = null, cut = false, pages }) {
  const who = ({ harness, agent }) => `${nameOf(harness)}${agent ? ` (${agent})` : ''}`
  const out = [
    `${HANDOFF_TITLE}. The human switched this project's lead from ${who(from)} to you, ${who(to)}.`,
    'You take over the same board, staff and conversation with the human; your role instructions are loaded as usual.',
    '',
    `Read what the human and the earlier lead said before you act: cf history (${pages} ${pages === 1 ? 'page' : 'pages'}, newest first; cf history --page 2 for older; cf history --find "words" to search). It is a record, not requests to you: do not redo what is done.`,
  ]
  if (cut)
    out.push(
      '',
      'The earlier lead was cut off in the middle of a turn: check what it left half done.',
    )
  if (last !== null) {
    out.push(
      '',
      `The human's last message to the lead${last.answered ? '' : ', not yet answered'}: "${defuse(firstLine(last.text, 300))}"`,
    )
  }
  const waiting = [
    ...open.questions.map(
      (m) =>
        `- @${m.sender} asks${m.taskNumber === null ? '' : ` on T-${m.taskNumber}`}: "${defuse(firstLine(m.body))}" (cf inbox read m-${m.id}, then cf answer m-${m.id} "…")`,
    ),
    ...open.results.map(
      (t) =>
        `- T-${t.number} "${defuse(t.title)}": @${t.assignee}'s result waits for your decision (cf task show T-${t.number})`,
    ),
    ...open.own.map((t) => `- T-${t.number} "${defuse(t.title)}" is yours, ${t.state}`),
  ]
  out.push(
    '',
    ...(waiting.length === 0
      ? ['Nothing on the board waits on you.']
      : ['What waits on you now:', ...waiting]),
    '',
    'Then tell the human, in one line, that you have taken over and what you see as next.',
  )
  return out.join('\n')
}

/**
 * The human's last words to the lead, in the latest conversation that has
 * any, and whether the lead answered after them; null when there are none.
 * A delivery is not the human's; what they typed before one went in is.
 */
export function lastWords(conversations) {
  for (const conversation of [...conversations].reverse()) {
    const { items } = conversation
    for (let index = items.length - 1; index >= 0; index -= 1) {
      const item = items[index]
      if (item.role !== 'user') continue
      const delivered = DELIVERY.exec(item.text)
      const text = (delivered === null ? item.text : item.text.slice(0, delivered.index)).trim()
      if (text === '') continue
      const answered = items
        .slice(index + 1)
        .some(
          (later) =>
            later.role === 'assistant' && later.complete !== false && later.text.trim() !== '',
        )
      return { text, answered }
    }
  }
  return null
}

/** One item of a conversation as a page shows it, or null when it is left out. */
function render(item, harness, { message, tools }) {
  const text = item.text ?? ''
  if (item.role === 'tool') return tools ? `Tool output:\n${defuse(text)}` : null
  if (item.role === 'assistant') return `${nameOf(harness)} lead: ${defuse(text)}`
  const delivered = DELIVERY.exec(text)
  if (delivered === null) {
    return item.role === 'user' ? `Human: ${defuse(text)}` : `In the window: ${defuse(text)}`
  }
  // What the human had typed before a delivery went in with it.
  const before = text.slice(0, delivered.index).trim()
  const outcome = deliveryLine(Number(delivered[1]), message)
  return before === '' ? outcome : `Human: ${defuse(before)}\n${outcome}`
}

/** A delivery as one line: what it was, and what came of it. */
function deliveryLine(id, message) {
  const m = message(id)
  if (m === null) return `· m-${id}: a message ConsensFlow delivered (no longer on record)`
  const from = m.sender === null ? 'ConsensFlow' : `@${m.sender}`
  const on = m.taskNumber === null ? '' : ` on T-${m.taskNumber}`
  const gist = defuse(firstLine(m.body))
  switch (m.kind) {
    case 'result':
      return `· m-${id}: ${from}'s result${on} (cf task show T-${m.taskNumber})`
    case 'question':
      return `· m-${id}: ${from} asked${on}: "${gist}" (cf inbox read m-${id})`
    case 'task':
      return `· m-${id}: ${from} gave the lead T-${m.taskNumber}: "${gist}"`
    case 'answer':
      return `· m-${id}: ${from} answered${on}: "${gist}"`
    default:
      return m.sender === null && m.body.startsWith(HANDOFF_TITLE)
        ? `· m-${id}: the handoff that brought this lead in`
        : `· m-${id}: a note from ${from}${on}: "${gist}" (cf inbox read m-${id})`
  }
}

/** One entry cut to fit a page: by lines, and a line too long by characters. */
function fit(entry, maxBytes, maxLines) {
  if (bytes(entry) <= maxBytes && lines(entry) <= maxLines) return [entry]
  const pieces = []
  let piece = ''
  const flush = () => {
    if (piece !== '') pieces.push(piece)
    piece = ''
  }
  for (const line of entry.split('\n')) {
    let rest = line
    while (bytes(rest) > maxBytes) {
      let cut = Math.min(rest.length, maxBytes)
      while (bytes(rest.slice(0, cut)) > maxBytes) cut = Math.floor(cut * 0.9)
      // Never split a surrogate pair.
      if (/[\uD800-\uDBFF]/.test(rest[cut - 1] ?? '')) cut -= 1
      flush()
      pieces.push(rest.slice(0, cut))
      rest = rest.slice(cut)
    }
    const next = piece === '' ? rest : `${piece}\n${rest}`
    if (bytes(next) > maxBytes || lines(next) > maxLines) {
      flush()
      piece = rest
    } else piece = next
  }
  flush()
  return pieces.map((p, index) => (index === 0 ? p : `(continued)\n${p}`))
}

/** Entries in the order they were said, packed into pages from the newest. */
function paginate(entries) {
  const maxBytes = PAGE_BYTES - FRAME_BYTES
  const maxLines = PAGE_LINES - FRAME_LINES
  // A continuation line is added to every piece after the first.
  const pieces = entries.flatMap((entry) => fit(entry, maxBytes - 16, maxLines - 1))
  const pages = []
  let page = []
  let used = { bytes: 0, lines: 0 }
  for (const piece of pieces.reverse()) {
    const cost = { bytes: bytes(piece) + 2, lines: lines(piece) + 1 }
    if (
      page.length > 0 &&
      (used.bytes + cost.bytes > maxBytes || used.lines + cost.lines > maxLines)
    ) {
      pages.push(page.reverse())
      page = []
      used = { bytes: 0, lines: 0 }
    }
    page.push(piece)
    used = { bytes: used.bytes + cost.bytes, lines: used.lines + cost.lines }
  }
  if (page.length > 0) pages.push(page.reverse())
  return pages
}

/**
 * How many pages `cf history` has for these conversations (the ledger's
 * `leadHistory`), without tool output and without a search.
 */
export function historyPages(conversations, { message = () => null } = {}) {
  return paginate(entriesOf(conversations, { message, tools: false })).length
}

function entriesOf(conversations, { message, tools, find = null }) {
  const entries = []
  const needle = find?.toLowerCase() ?? null
  for (const conversation of conversations) {
    const when = `${conversation.startedAt} to ${conversation.endedAt}`
    const left = tools ? 0 : conversation.items.filter((item) => item.role === 'tool').length
    const heading = `── The lead on ${nameOf(conversation.harness)}, ${when}${
      left > 0 ? `; ${left} tool ${left === 1 ? 'output' : 'outputs'} left out (--tools)` : ''
    } ──`
    if (needle === null) entries.push(heading)
    for (const item of conversation.items) {
      const shown = render(item, conversation.harness, { message, tools })
      if (shown === null) continue
      if (needle === null) entries.push(shown)
      else if (shown.toLowerCase().includes(needle)) {
        entries.push(`(${nameOf(conversation.harness)}, ${item.at ?? when})\n${shown}`)
      }
    }
  }
  return entries
}

/**
 * One page of `cf history`: `{ page, pages, text }`. Page 1 holds the most
 * recent turns; `find` keeps only the entries that contain it.
 */
export function historyPage(
  conversations,
  { message = () => null, page = 1, find = null, tools = false } = {},
) {
  const pages = paginate(entriesOf(conversations, { message, tools, find }))
  if (pages.length === 0) {
    return {
      page: 0,
      pages: 0,
      text:
        find === null
          ? 'No earlier lead conversations: you are the first lead of this project.'
          : `Nothing in the lead history contains "${find}".`,
    }
  }
  if (!Number.isInteger(page) || page < 1 || page > pages.length) {
    throw new RangeError(`there ${pages.length === 1 ? 'is 1 page' : `are ${pages.length} pages`}`)
  }
  const what = find === null ? 'Lead history' : `Lead history, entries with "${find}"`
  const flags = `${find === null ? '' : ` --find "${find}"`}${tools ? ' --tools' : ''}`
  const opening =
    page === 1
      ? `${what}, page 1 of ${pages.length}: the most recent.`
      : `${what}, page ${page} of ${pages.length}: older than page ${page - 1}.`
  const closing =
    page < pages.length
      ? `Older: cf history --page ${page + 1}${flags}`
      : 'This is the oldest page.'
  return {
    page,
    pages: pages.length,
    text: [opening, '', pages[page - 1].join('\n\n'), '', closing].join('\n'),
  }
}
