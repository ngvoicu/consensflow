import { button, element } from '../dom.js'
import { itemLabel } from './board.js'
import { render } from './markdown.js'

/** The most characters a search holds: the daemon refuses more. */
const FIND_MOST = 1_000

/** How many times an item copied again as it is read whole is read again from its start. */
const READS_OF_A_CHANGING_ITEM = 3

/** How many of its last lines a call's output shows folded. */
const FOLDED_LINES = 5

/**
 * A line in which a command's output says how it exited, as each harness writes one:
 * Claude Code's `Exit code 1` (first), Devin's `Exit code: 1`, Pi's `Command exited with
 * code 1` (last), Codex's `Process exited with code 1`.
 */
const EXIT_LINE = /^\s*(?:exit code:?|(?:command|process) exited with code)\s*-?\d+\s*$/i

/** A header of a call that reads, edits or writes a file: its verb, its path, its lines. */
const FILE_CALL = /^(read|edit|write) (.+?)(:\d+(?:-\d+)?)?$/

/**
 * The history viewer: everything ConsensFlow keeps of a participant's
 * conversations, from the History button on its window's head (the chief's, a
 * session's). It reads through the daemon (`history.get`, `history.part`), a
 * page at a time, and changes nothing.
 *
 * One stream, oldest above, drawn as a harness draws its own (chief-harness
 * CHF-20, the approved mock): the items of every conversation, titled History,
 * with no heading, harness, model or name anywhere in it (CHF-11). Each item is
 * drawn by its kind (`prompt`, `text`, `thinking`, `tool`, `compaction`,
 * `notice`), or by its role where an older build copied it with none:
 * - the human's words and ConsensFlow's deliveries, in full, as blue blocks;
 * - the agent's words as markdown, and thinking dimmed, on the stream itself;
 * - a tool's call as a block: its header, its output folded to its last lines (an exit
 *   line kept in sight), its change as red and green rows, the time it took, and green,
 *   red or grey by how it stands;
 * - a compaction as a violet block, its tokens, its summary on a click.
 *
 * The first page is the most recent; a quiet first row loads the page before
 * the oldest item shown above it, so no item repeats or is skipped, whatever
 * comes or grows meanwhile, and Refresh reads the most recent again. A read
 * that goes through as much as one may before it finds anything says so in
 * that row, and goes on from where it stopped. A search keeps the items whose
 * words or call header hold it in any case; it holds 1,000 characters at most,
 * as the daemon does, and says so of a longer one. A long item shows a part
 * (from its start, or where the search found it), and a click row reads it
 * whole, a part at a time by its place, every part of one copy of it.
 * Everything shows as text, never as markup.
 */
export class HistoryViewer {
  #dialog
  #find
  #body
  #page
  /**
   * What is shown: whose history (the project's id and the participant's handle),
   * the search it shows, and the place the page before the items shown goes on from.
   */
  #shown = null
  /** Each read is numbered: what comes back for a read the human has left since is not shown. */
  #reads = 0

  /** `page.daemon` asks the daemon, and throws its refusal. */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#page = page
    this.#find = dialog.querySelector('[name="history-find"]')
    this.#body = dialog.querySelector('.history-body')
    dialog.querySelector('.history-search').addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#read()
    })
    dialog.querySelector('[name="history-refresh"]').addEventListener('click', () => {
      void this.#read()
    })
    // Shut, it holds nothing, and what is still on its way is not shown.
    dialog.addEventListener('close', () => {
      this.#reads += 1
      this.#shown = null
      this.#body.replaceChildren()
    })
  }

  /** Opens on the most recent page of `participant`'s history. */
  async open(participant) {
    this.#shown = {
      project: participant.projectId,
      handle: participant.handle,
      find: '',
      older: null,
    }
    this.#find.value = ''
    this.#body.replaceChildren(element('p', 'history-note', 'Reading the history…'))
    if (!this.#dialog.open) this.#dialog.showModal()
    await this.#read()
  }

  /**
   * Reads the most recent page with the search as the human set it, and shows it scrolled
   * to its newest; or, `before` a place, the page before it, drawn above the items shown
   * with what was in view kept where it was.
   */
  async #read(before = null) {
    const shown = this.#shown
    if (shown === null) return
    const read = ++this.#reads
    const find = before ? shown.find : this.#find.value.trim()
    const characters = [...find].length
    if (characters > FIND_MOST) {
      this.#body.replaceChildren(
        element(
          'p',
          'history-note',
          `A search holds at most 1,000 characters; this one has ${characters.toLocaleString('en')}.`,
        ),
      )
      return
    }
    // Every call is drawn with the rest: the tools' outputs are asked for.
    const asked = { project: shown.project, handle: shown.handle, tools: true }
    if (find) asked.find = find
    if (before) asked.before = before
    try {
      const { history } = await this.#page.daemon('history.get', asked)
      if (read !== this.#reads) return
      shown.find = find
      shown.older = history.older
      const items = history.items.map((item) => drawItem(item, (shown) => this.#whole(shown)))
      const list = this.#body.querySelector('ol.history-items')
      if (before && list) {
        const below = this.#body.scrollHeight - this.#body.scrollTop
        list.querySelector('.history-earlier-row')?.remove()
        this.#body.querySelector('.history-note')?.remove()
        list.prepend(...this.#earlier(history, find), ...items)
        this.#body.scrollTop = this.#body.scrollHeight - below
        return
      }
      this.#body.replaceChildren(...this.#draw(history, find, items))
      this.#body.scrollTop = this.#body.scrollHeight
    } catch (cause) {
      if (read !== this.#reads) return
      this.#body.replaceChildren(
        element('p', 'history-refusal', cause instanceof Error ? cause.message : String(cause)),
      )
    }
  }

  /** A page as it shows: the row for what comes before it, and its items in one list. */
  #draw(history, find, items) {
    const drawn = []
    if (history.items.length === 0) {
      let none = history.partial ? 'Nothing to show yet.' : 'Nothing is kept of this history yet.'
      if (find) {
        none = history.partial
          ? `Nothing found yet that holds "${find}".`
          : `Nothing in this history holds "${find}".`
      }
      drawn.push(element('p', 'history-note', none))
    }
    const list = element('ol', 'history-items')
    list.append(...this.#earlier(history, find), ...items)
    if (list.childElementCount > 0) drawn.push(list)
    return drawn
  }

  /**
   * The quiet first row that loads the page before the items shown, where there is one:
   * it says where a read or a search stopped to stay quick.
   */
  #earlier(history, find) {
    if (history.older === null) return []
    let said = '… earlier history, click to load'
    if (history.partial) {
      said = find
        ? '… the search stopped here to stay quick, click to search further back'
        : '… the read stopped here to stay quick, click to read further back'
    }
    const row = element('li', 'history-earlier-row')
    const load = button(said, 'history-row history-earlier', () => {
      load.disabled = true
      void this.#read(this.#shown.older)
    })
    row.append(load)
    return [row]
  }

  /**
   * The whole text of item `item`, read a part at a time by its place (its conversation and
   * row), and the copy of it it is (its revision). Every part is of one copy of it: a part of
   * another (the item was copied again meanwhile) reads it again from its start, so no text
   * of two copies is shown; after three reads it changed in, the viewer says so.
   */
  async #whole(item) {
    const { project, handle } = this.#shown
    for (let tries = 0; tries < READS_OF_A_CHANGING_ITEM; tries += 1) {
      let text = ''
      let from = 0
      let revision = null
      while (from !== null) {
        const { part } = await this.#page.daemon('history.part', {
          project,
          handle,
          conversation: item.conversation,
          row: item.row,
          from,
        })
        revision ??= part.revision
        if (part.revision !== revision) break
        text += part.text
        from = part.next
      }
      if (from === null) return { text, revision }
    }
    throw new Error('It changed each time it was read: show it again to read it as it is now.')
  }
}

/**
 * What an item is drawn as: what its reader said of it, else what its role says (a
 * compaction's summary a compaction), as `Item::kind` says it.
 */
function kindOf(item) {
  if (item.kind) return item.kind
  if (item.compaction) return 'compaction'
  return { user: 'prompt', assistant: 'text', tool: 'tool' }[item.role] ?? 'notice'
}

/**
 * One item, drawn by its kind; a part of a long one with the row that reads it whole,
 * which draws it again.
 */
function drawItem(item, whole) {
  const kind = kindOf(item)
  const entry = element('li', 'history-item')
  entry.dataset.id = item.id
  entry.dataset.kind = kind
  if (item.at) entry.title = clock(item.at)
  if (kind === 'tool') drawCall(entry, item)
  else if (kind === 'compaction') drawCompaction(entry, item)
  else if (kind === 'text') {
    const words = element('div', 'history-markdown')
    words.append(...render(item.text))
    entry.append(words)
  } else entry.append(...said(kind, item))
  if (!item.complete && kind !== 'tool') {
    entry.append(element('p', 'history-quiet', 'still writing…'))
  }
  if (item.cut) drawCut(entry, item, whole)
  return entry
}

/**
 * The words of a prompt, a thought or a notice, as text: a delivery after what the human
 * typed with it, both whole, in the one block (Gabriel, O3).
 */
function said(kind, item) {
  const className = kind === 'thinking' ? 'history-text history-thinking' : 'history-text'
  if (!item.delivery) return [element('pre', className, item.text)]
  const typed = item.text.slice(0, item.delivery.at).trim()
  const delivered = element('pre', className, item.text.slice(item.delivery.at))
  return typed ? [element('pre', className, typed), delivered] : [delivered]
}

/**
 * A call: its header, its change, its output folded to its last lines with the exit line
 * kept in sight where it failed, and the time it took; green, red or grey by how it
 * stands (`unknown` where an older build said nothing of it).
 */
function drawCall(entry, item) {
  const status = ['ok', 'failed', 'running'].includes(item.status) ? item.status : 'unknown'
  entry.dataset.status = status
  entry.append(callHeader(item.header ?? item.name ?? itemLabel(item)))
  if (item.diff) entry.append(drawDiff(item.diff))
  // An edit's change says what it did: its tool's words of it only where it failed.
  if (!item.diff || status === 'failed') entry.append(...drawOutput(item.text, status))
  if (status === 'running') entry.append(element('p', 'history-took', 'Still running'))
  else if (typeof item.tookMs === 'number') {
    entry.append(element('p', 'history-took', `Took ${took(item.tookMs)}`))
  }
}

/** A call's header: a file's verb, path and lines each their own colour, else the line. */
function callHeader(header) {
  const head = element('div', 'history-call')
  const file = FILE_CALL.exec(header)
  if (!file) {
    head.append(element('span', 'history-verb', header))
    return head
  }
  head.append(
    element('span', 'history-verb', file[1]),
    ' ',
    element('span', 'history-path', file[2]),
  )
  if (file[3]) head.append(element('span', 'history-range', file[3]))
  return head
}

/** An edit's change as rows: removed red, added green, a hunk's place and the rest quiet. */
function drawDiff(diff) {
  const rows = element('div', 'history-diff')
  for (const line of diff.split('\n')) {
    const row = element('div', 'history-diff-row', line)
    if (line.startsWith('@@') || line.startsWith('--- ') || line.startsWith('+++ ')) {
      row.dataset.sign = 'place'
    } else if (line.startsWith('-')) row.dataset.sign = 'removed'
    else if (line.startsWith('+')) row.dataset.sign = 'added'
    else row.dataset.sign = 'kept'
    rows.append(row)
  }
  return rows
}

/**
 * A call's output: its last lines, under a row that unfolds the rest, and a failed call's
 * exit line where they do not hold it. Blank lines at its end are no lines of it.
 */
function drawOutput(text, status) {
  const lines = text.replace(/\s+$/, '').split('\n')
  if (lines.length === 1 && lines[0] === '') return []
  const output = element('pre', 'history-output')
  if (lines.length <= FOLDED_LINES) {
    output.textContent = lines.join('\n')
    return [output]
  }
  const hidden = lines.length - FOLDED_LINES
  const shown = lines.slice(hidden)
  output.textContent = shown.join('\n')
  const exit =
    status === 'failed' && !shown.some((line) => EXIT_LINE.test(line))
      ? lines.slice(0, hidden).findLast((line) => EXIT_LINE.test(line))
      : undefined
  const pinned = exit === undefined ? null : element('pre', 'history-exit', exit.trim())
  const fold = button(
    `… (${hidden} earlier lines, click to expand)`,
    'history-row history-fold',
    () => {
      output.textContent = lines.join('\n')
      pinned?.remove()
      fold.remove()
    },
  )
  return pinned ? [fold, output, pinned] : [fold, output]
}

/** A compaction: its mark, its tokens, and its summary on a click where it wrote one. */
function drawCompaction(entry, item) {
  entry.append(element('div', 'history-call', '[compaction]'))
  const tokens =
    typeof item.tokens === 'number'
      ? `Compacted from ${item.tokens.toLocaleString('en')} tokens`
      : 'Compacted'
  if (!item.text) {
    entry.append(element('p', 'history-quiet', tokens))
    return
  }
  const summary = element('pre', 'history-text', item.text)
  summary.hidden = true
  const expand = button(`${tokens} (click to expand)`, 'history-row history-expand', () => {
    summary.hidden = !summary.hidden
    expand.setAttribute('aria-expanded', String(!summary.hidden))
  })
  expand.setAttribute('aria-expanded', 'false')
  entry.append(expand, summary)
}

/**
 * The row that reads a long item whole, after the part shown (before it, where a search
 * showed a part from further on): read, the item is drawn again whole, a delivery said
 * only of the copy the page showed.
 */
function drawCut(entry, item, whole) {
  const size = measure(item.cut.bytes)
  const said =
    item.cut.from > 0
      ? `… (${size} in all, shown from character ${(item.cut.from + 1).toLocaleString('en')}, click to load it whole)`
      : `… (${size} in all, click to load the rest)`
  const more = button(said, 'history-row history-more', async () => {
    more.disabled = true
    try {
      const { text, revision } = await whole(item)
      // A delivery is said only of the copy the page showed: another, copied since, is
      // drawn as it is.
      const delivery = revision === item.cut.revision ? item.delivery : undefined
      entry.replaceWith(drawItem({ ...item, text, delivery, cut: undefined }, whole))
    } catch (cause) {
      more.disabled = false
      entry.querySelector('.history-refusal')?.remove()
      more.after(
        element('p', 'history-refusal', cause instanceof Error ? cause.message : String(cause)),
      )
    }
  })
  if (item.cut.from > 0) entry.prepend(more)
  else entry.append(more)
}

/** How long a call took, as the mock says it: `0.1s`, `1.2s`, `1m 1.5s`. */
function took(ms) {
  const seconds = ms / 1000
  if (seconds < 60) return `${seconds.toFixed(1)}s`
  const minutes = Math.floor(seconds / 60)
  return `${minutes}m ${(seconds - minutes * 60).toFixed(1)}s`
}

/** A size as the page says it. */
const measure = (bytes) =>
  bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`

const two = (number) => String(number).padStart(2, '0')

/** A time as the page shows it, on this machine's clock: the date and the minute. */
function clock(iso) {
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return String(iso)
  return `${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())} ${two(at.getHours())}:${two(at.getMinutes())}`
}
