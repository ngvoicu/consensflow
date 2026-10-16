import { element } from '../dom.js'
import { itemLabel, laneName } from './board.js'
import { HARNESS_NAMES } from './dialogs.js'

/**
 * The history viewer: everything ConsensFlow keeps of a participant's
 * conversations, from the History button on its window's head (the chief's, a
 * session's). It reads through the daemon (`history.get`), a page at a time,
 * and changes nothing.
 *
 * One column, oldest above. Each conversation has a heading: the harness and
 * the agent it ran on, from when until when, and what ended it as far as the
 * ledger knows (a Switch chief; the window going to another conversation, a
 * `/clear`, `/new` or `/resume`; a window that opened on a fresh one); the
 * one going on now says so. Its items are labelled as the task drawer labels
 * them, a compaction's summary as one. A delivery ConsensFlow made shows as its
 * line, as `cf history` says it, and opens on a click to what went in. Page 1
 * is the most recent; Older and Newer move a page, a search keeps the items
 * that hold its words in any case, and Show tool output adds the tools'
 * outputs. Everything shows as text, never as markup.
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
  /** What is shown: whose history (the project's id and the participant), and which page. */
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
      void this.#read(1)
    })
    this.#tools.addEventListener('change', () => void this.#read(1))
    // A newer page goes on from the top; any other is read from its newest, at the bottom.
    this.#newer.addEventListener('click', () => void this.#read(this.#shown.number - 1, true))
    this.#older.addEventListener('click', () => void this.#read(this.#shown.number + 1))
    dialog.querySelector('[name="history-refresh"]').addEventListener('click', () => {
      void this.#read(this.#shown?.number || 1)
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
    this.#shown = { project: participant.projectId, handle: participant.handle, name, number: 1 }
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
    await this.#read(1)
  }

  /**
   * Reads page `number` with the search and the tools as the human set them, and shows it
   * scrolled to its newest, or to its `top`.
   */
  async #read(number, top = false) {
    const shown = this.#shown
    if (shown === null) return
    const read = ++this.#reads
    const find = this.#find.value.trim()
    const asked = {
      project: shown.project,
      handle: shown.handle,
      page: number,
      tools: this.#tools.checked,
    }
    if (find) asked.find = find
    try {
      const { history } = await this.#page.daemon('history.get', asked)
      if (read !== this.#reads) return
      shown.number = history.page
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

  #drawPaging({ page, pages }) {
    this.#status.textContent =
      pages === 0 ? '' : `Page ${page} of ${pages}${page === 1 ? ', the most recent' : ''}`
    this.#newer.disabled = page <= 1
    this.#older.disabled = page >= pages
  }

  /** A page as it shows: each conversation's heading and items, or what there is none of. */
  #draw(history, find) {
    if (history.pages === 0) {
      const { name } = this.#shown
      const none = find
        ? `Nothing in ${name}'s history holds "${find}".`
        : `Nothing is kept of ${name}'s conversations yet.`
      return [element('p', 'history-note', none)]
    }
    return history.conversations.map((conversation) => drawConversation(conversation))
  }
}

/** A conversation: its heading, then its items on the page. */
function drawConversation(conversation) {
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
    const outputs = conversation.toolsLeftOut === 1 ? 'output' : 'outputs'
    about.push(`${conversation.toolsLeftOut} tool ${outputs} left out`)
  }
  section.append(heading, element('p', 'history-about', about.join(' · ')))
  if (conversation.earlier) {
    section.append(element('p', 'history-more', 'Its earlier items are on an older page.'))
  }
  const list = element('ol', 'transcript history-items')
  list.append(...conversation.items.map(drawItem))
  section.append(list)
  if (conversation.later) {
    section.append(element('p', 'history-more', 'It goes on on a newer page.'))
  }
  return section
}

/** One item, labelled as the drawer labels it; a delivery as its line, opened on a click. */
function drawItem(item) {
  const entry = element('li', 'transcript-item history-item')
  entry.dataset.role = item.role
  if (item.compaction) entry.dataset.compaction = 'true'
  const head = [itemLabel(item)]
  if (item.at) head.push(clock(item.at))
  if (!item.complete) head.push('still writing')
  entry.append(element('div', 'transcript-head', head.join(' · ')))
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
