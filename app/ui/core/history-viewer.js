import { button, element } from '../dom.js'
import { itemLabel, laneName } from './board.js'
import { HARNESS_NAMES } from './dialogs.js'

/** The most characters a search holds: the daemon refuses more. */
const FIND_MOST = 1_000

/** How many times an item copied again as it is read whole is read again from its start. */
const READS_OF_A_CHANGING_ITEM = 3

/**
 * The history viewer: everything ConsensFlow keeps of a participant's
 * conversations, from the History button on its window's head (the chief's, a
 * session's). It reads through the daemon (`history.get`, `history.part`), a
 * page at a time, and changes nothing.
 *
 * One column, oldest above. Each conversation has a heading: the harness and
 * the agent it ran on, from when until when, and what ended it as far as the
 * ledger knows (a Switch chief; the window going to another conversation, a
 * `/clear`, `/new` or `/resume`; a window that opened on a fresh one); the
 * one going on now says so. Its items are labelled as the task drawer labels
 * them, a compaction's summary as one. A delivery ConsensFlow made to this
 * participant shows as its line, as `cf history` says it, and opens on a click
 * to what went in. The first page is the most recent; Older goes on from the
 * oldest item shown and Newer from the newest, so no page repeats or skips an
 * item, whatever comes or grows meanwhile. A search keeps the items that hold
 * its words in any case, and says where it stopped when it went through as
 * much as one read may; it holds 1,000 characters at most, as the daemon does,
 * and says so of a longer one. Show tool output adds the tools' outputs. A
 * long item shows a part (from its start, or where the search found it), and
 * is read whole on asking, a part at a time by its place, every part of one
 * copy of it. Everything shows as text, never as markup.
 */
export class HistoryViewer {
  #dialog
  #title
  #who
  #find
  #tools
  #newer
  #older
  #status
  #body
  #page
  /**
   * What is shown: whose history (the project's id and the participant), and the
   * places the pages before and after this one go on from.
   */
  #shown = null
  /** Each read is numbered: what comes back for a read the human has left since is not shown. */
  #reads = 0

  /** `page.daemon` asks the daemon, and throws its refusal. */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#page = page
    this.#title = dialog.querySelector('.history-title')
    this.#who = dialog.querySelector('.history-who')
    this.#find = dialog.querySelector('[name="history-find"]')
    this.#tools = dialog.querySelector('[name="history-tools"]')
    this.#newer = dialog.querySelector('[name="history-newer"]')
    this.#older = dialog.querySelector('[name="history-older"]')
    this.#status = dialog.querySelector('.history-page')
    this.#body = dialog.querySelector('.history-body')
    dialog.querySelector('.history-search').addEventListener('submit', (event) => {
      event.preventDefault()
      void this.#read({})
    })
    this.#tools.addEventListener('change', () => void this.#read({}))
    // A newer page goes on from the top; any other is read from its newest, at the bottom.
    this.#newer.addEventListener('click', () => void this.#read({ after: this.#shown.newer }, true))
    this.#older.addEventListener('click', () => void this.#read({ before: this.#shown.older }))
    dialog.querySelector('[name="history-refresh"]').addEventListener('click', () => {
      void this.#read({})
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
    const name = laneName(participant)
    this.#shown = {
      project: participant.projectId,
      handle: participant.handle,
      name,
      older: null,
      newer: null,
    }
    this.#title.textContent = `History of ${name}`
    this.#who.textContent = [
      HARNESS_NAMES[participant.harness] ?? participant.harness,
      participant.agent,
    ]
      .filter(Boolean)
      .join(' · ')
    this.#find.value = ''
    this.#tools.checked = false
    this.#body.replaceChildren(element('p', 'history-note', 'Reading the history…'))
    if (!this.#dialog.open) this.#dialog.showModal()
    await this.#read({})
  }

  /**
   * Reads the page `place` says (before a place, after one, or the most recent) with the
   * search and the tools as the human set them, and shows it scrolled to its newest, or
   * to its `top`.
   */
  async #read(place, top = false) {
    const shown = this.#shown
    if (shown === null) return
    const read = ++this.#reads
    const find = this.#find.value.trim()
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
    const asked = {
      project: shown.project,
      handle: shown.handle,
      tools: this.#tools.checked,
      ...place,
    }
    if (find) asked.find = find
    try {
      const { history } = await this.#page.daemon('history.get', asked)
      if (read !== this.#reads) return
      shown.older = history.older
      shown.newer = history.newer
      this.#drawPaging(history)
      this.#body.replaceChildren(...this.#draw(history, find))
      this.#body.scrollTop = top ? 0 : this.#body.scrollHeight
    } catch (cause) {
      if (read !== this.#reads) return
      this.#body.replaceChildren(
        element('p', 'history-refusal', cause instanceof Error ? cause.message : String(cause)),
      )
    }
  }

  #drawPaging({ older, newer, partial }) {
    const where = newer === null ? 'The most recent' : 'Earlier items'
    this.#status.textContent = partial
      ? `${where}; the search stopped here to stay quick: Older searches further back`
      : where
    this.#newer.disabled = newer === null
    this.#older.disabled = older === null
  }

  /** A page as it shows: each conversation's heading and items, or what there is none of. */
  #draw(history, find) {
    const { name } = this.#shown
    const shown = history.conversations.some((conversation) => conversation.items.length > 0)
    if (history.conversations.length === 0 || (find && !shown)) {
      let none = `Nothing is kept of ${name}'s conversations yet.`
      if (find) {
        none = history.partial
          ? `Nothing found yet that holds "${find}": Older searches further back.`
          : `Nothing in ${name}'s history holds "${find}".`
      }
      return [element('p', 'history-note', none)]
    }
    return history.conversations.map((conversation) =>
      drawConversation(conversation, (item) => this.#whole(conversation.id, item)),
    )
  }

  /**
   * The whole text of item `item` of conversation `conversation`, read a part at a time by
   * its place. Every part is of one copy of it: a part of another (the item was copied
   * again meanwhile) reads it again from its start, so no text of two copies is shown;
   * after three reads it changed in, the viewer says so.
   */
  async #whole(conversation, item) {
    const { project, handle } = this.#shown
    for (let tries = 0; tries < READS_OF_A_CHANGING_ITEM; tries += 1) {
      let text = ''
      let from = 0
      let revision = null
      while (from !== null) {
        const { part } = await this.#page.daemon('history.part', {
          project,
          handle,
          conversation,
          row: item.row,
          from,
        })
        revision ??= part.revision
        if (part.revision !== revision) break
        text += part.text
        from = part.next
      }
      if (from === null) return text
    }
    throw new Error('It changed each time it was read: show it again to read it as it is now.')
  }
}

/** A conversation: its heading, then its items on the page; `whole` reads a cut item whole. */
function drawConversation(conversation, whole) {
  const section = element('section', 'history-conversation')
  section.dataset.conversation = String(conversation.id)
  const runsOn = [HARNESS_NAMES[conversation.harness] ?? conversation.harness, conversation.agent]
    .filter(Boolean)
    .join(' · ')
  const heading = element('h3', 'history-heading', runsOn)
  section.setAttribute('aria-label', `${runsOn}, ${period(conversation)}`)
  const about = [period(conversation)]
  const ended = endedWords(conversation)
  if (ended) about.push(ended)
  if (conversation.toolsLeftOut > 0) {
    // Counted so far and no further: that many, or more.
    const more = conversation.moreToolsLeftOut === true
    const outputs = conversation.toolsLeftOut === 1 && !more ? 'output' : 'outputs'
    const count = more ? `${conversation.toolsLeftOut} or more` : conversation.toolsLeftOut
    about.push(`${count} tool ${outputs} left out`)
  }
  section.append(heading, element('p', 'history-about', about.join(' · ')))
  const list = element('ol', 'transcript history-items')
  list.append(...conversation.items.map((item) => drawItem(item, whole)))
  section.append(list)
  return section
}

/**
 * One item, labelled as the drawer labels it; a delivery as its line, opened on a
 * click; a part of a long one with the way to read it whole, which draws it again.
 */
function drawItem(item, whole) {
  const entry = element('li', 'transcript-item history-item')
  entry.dataset.role = item.role
  if (item.compaction) entry.dataset.compaction = 'true'
  const head = [itemLabel(item)]
  if (item.at) head.push(clock(item.at))
  if (!item.complete) head.push('still writing')
  entry.append(element('div', 'transcript-head', head.join(' · ')))
  if (item.cut) {
    const said = item.cut.from > 0 ? `… from character ${item.cut.from + 1}` : 'Its first part'
    const more = button(
      `Show the whole item (${measure(item.cut.bytes)})`,
      'quiet-button',
      async () => {
        more.disabled = true
        try {
          const text = await whole(item)
          // A delivery is said only on a part from the start: its place holds in the whole.
          entry.replaceWith(drawItem({ ...item, text, cut: undefined }, whole))
        } catch (cause) {
          more.disabled = false
          more.after(
            element(
              'span',
              'history-refusal',
              cause instanceof Error ? cause.message : String(cause),
            ),
          )
        }
      },
    )
    const cut = element('p', 'history-cut', said)
    cut.append(' ', more)
    entry.append(cut)
  }
  if (!item.delivery) {
    entry.append(element('pre', 'transcript-body', item.text))
    return entry
  }
  // What the human typed with it, then the delivery, folded to its line.
  const typed = item.text.slice(0, item.delivery.at).trim()
  if (typed) entry.append(element('pre', 'transcript-body', typed))
  const delivery = element('details', 'history-delivery')
  delivery.append(
    element('summary', null, item.delivery.line),
    element('pre', 'transcript-body', item.text.slice(item.delivery.at)),
  )
  entry.append(delivery)
  return entry
}

/** A size as the page says it. */
const measure = (bytes) =>
  bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`

/** From when until when, or since when for the one going on now. */
const period = ({ startedAt, endedAt }) =>
  endedAt ? `${clock(startedAt)} to ${clock(endedAt)}` : `since ${clock(startedAt)}, going on now`

/** What ended a conversation, as far as the ledger knows; none for the one going on now. */
function endedWords({ endedAt, endedBy }) {
  if (!endedAt) return null
  if (endedBy === null) return 'ended (the ledger does not say why)'
  if (endedBy.by === 'switch') {
    const to = [HARNESS_NAMES[endedBy.harness] ?? endedBy.harness, endedBy.agent]
      .filter(Boolean)
      .join(' · ')
    return `ended by Switch chief, to ${to}`
  }
  if (endedBy.by === 'window') {
    return endedBy.resumed
      ? 'ended when the window resumed an earlier conversation (/resume)'
      : 'ended when the window went to another conversation (/clear, /new or /resume)'
  }
  return 'ended when a window opened on a fresh conversation'
}

const two = (number) => String(number).padStart(2, '0')

/** A time as the page shows it, on this machine's clock: the date and the minute. */
function clock(iso) {
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return String(iso)
  return `${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())} ${two(at.getHours())}:${two(at.getMinutes())}`
}
