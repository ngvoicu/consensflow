import { button, element } from '../dom.js'
import { diagram, undrawn } from './diagrams.js'
import { render } from './markdown.js'
import {
  arrange,
  filesOf,
  firstFile,
  frontmatter,
  imageType,
  kindOf,
  linkTarget,
  measure,
  REGISTRY,
  readRegistry,
  specFile,
} from './specs-folder.js'

/**
 * The specs viewer: a project's specs folder in a dialog of its own, the
 * folder's tree on the left and the file chosen on the right. It reads through
 * the daemon (`specs.tree`, `specs.file`), which reads the folder the project
 * names, else `.specs`, and nothing outside it; it changes nothing.
 *
 * It opens on the registry, or on the first spec where there is none. The
 * registry shows as rows (a spec's title, its status as a chip, its priority,
 * its progress as a bar, its date), the first `MAX_ROWS` of them, and a row
 * opens its spec; a registry that is not Specmint's table shows as the
 * markdown it is. Markdown shows as the page draws an agent's, with its
 * headings, its tasks' boxes and its diagrams, the first `MAX_DIAGRAMS` of
 * them drawn, the rest shown as their source; a diagram still to draw when the
 * viewer shuts or another file is chosen is never drawn. A link to another file
 * of the folder opens it here, and any other link opens nothing. An HTML file
 * (a `SPEC.html`, a mockup) is not run in the app: it shows as a card, its
 * name, its size and its title read as text, and opens in the human's browser,
 * as the daemon's copy of it with the `../assets/` it names (`specs.open`). An
 * image shows as an image; any other text as it is, in monospace; anything
 * else, or anything too large, says so, with its size. Refresh reads the folder
 * again, and the file shown, even after a read that failed; a file chosen
 * again is read again.
 */

/** Specmint's statuses, each a chip of its own colour; any other is a plain one. */
const STATUSES = ['active', 'paused', 'completed', 'archived']
/** The most rows of the registry drawn: the rest are said, and are all in the tree. */
export const MAX_ROWS = 500
/** The most diagrams a document draws: the rest show as their source. */
export const MAX_DIAGRAMS = 50

export class SpecsViewer {
  #dialog
  #title
  #folder
  #cut
  #tree
  #view
  #page
  /** The project the viewer was opened on, by id: what Refresh reads, whatever came of the last read. */
  #project = null
  /** What is shown: the project (its id), the daemon's tree, the tree arranged, every file of it. */
  #shown = null
  /** The file shown, by its path. */
  #chosen = null
  /** Each read is numbered: what comes back for a read the human has left since is not shown. */
  #reads = 0
  /** The folders the human folded or unfolded, by path: they stay so when the tree is read again. */
  #folds = new Map()
  /** What the diagrams of the file shown are drawn for: aborted once it is no longer shown. */
  #drawing = new AbortController()

  /** `page.daemon` asks the daemon, and throws its refusal. */
  constructor(dialog, page) {
    this.#dialog = dialog
    this.#title = dialog.querySelector('.specs-title')
    this.#folder = dialog.querySelector('.specs-folder')
    this.#cut = dialog.querySelector('.specs-cut')
    this.#tree = dialog.querySelector('.specs-tree')
    this.#view = dialog.querySelector('.specs-file')
    this.#page = page
    dialog.querySelector('[name="specs-refresh"]').addEventListener('click', () => {
      void this.refresh()
    })
    // Shut, it holds nothing, and what is still on its way is not shown.
    dialog.addEventListener('close', () => {
      this.#reads += 1
      this.#stopDrawing()
      this.#project = null
      this.#shown = null
      this.#chosen = null
      this.#tree.replaceChildren()
      this.#view.replaceChildren()
    })
  }

  /** Opens on the specs folder of project `id` (named `name`), on its registry or first spec. */
  async open(id, name) {
    this.#project = id
    this.#shown = null
    this.#chosen = null
    this.#folds.clear()
    this.#title.textContent = `Specs of ${name}`
    this.#folder.textContent = ''
    this.#tree.replaceChildren()
    this.#view.replaceChildren(element('p', 'specs-note', 'Reading the specs folder…'))
    if (!this.#dialog.open) this.#dialog.showModal()
    await this.#read(id, null)
  }

  /**
   * Reads the folder again, and the file shown, which is shown again if it is
   * still there; after a read that failed, the folder is read as at the opening.
   */
  async refresh() {
    if (this.#project === null) return
    await this.#read(this.#project, this.#chosen)
  }

  /** The diagrams of the file shown, still to draw, are not drawn. */
  #stopDrawing() {
    this.#drawing.abort()
    this.#drawing = new AbortController()
  }

  /** Reads the tree of project `project`, and shows `path`, or the file the folder opens on. */
  async #read(project, path) {
    const read = ++this.#reads
    this.#stopDrawing()
    try {
      const { specs } = await this.#page.daemon('specs.tree', { project })
      // The registry names the specs and orders them: it is read with the tree. One that
      // cannot be read leaves them by name, and says why when it is chosen.
      const registry = specs.entries.some((entry) => entry.path === REGISTRY && !entry.entries)
        ? await this.#file(project, REGISTRY).catch(() => null)
        : null
      if (read !== this.#reads) return
      const rows = (registry?.text && readRegistry(registry.text)?.rows) || []
      const arranged = arrange(specs.entries, rows)
      this.#shown = { project, specs, arranged, files: new Set(filesOf(arranged)) }
      this.#folder.textContent = specs.folder
      this.#cut.textContent = specs.cut ?? ''
      this.#cut.hidden = !specs.cut
      const target = path !== null && this.#shown.files.has(path) ? path : firstFile(arranged)
      this.#drawTree()
      if (target === null) {
        this.#chosen = null
        this.#view.replaceChildren(element('p', 'specs-note', emptyWords(specs)))
        return
      }
      await this.#choose(target, target === REGISTRY ? registry : null)
    } catch (cause) {
      if (read !== this.#reads) return
      this.#view.replaceChildren(refusal(cause))
    }
  }

  /** The daemon's answer for the file at `path` of project `project`. */
  async #file(project, path) {
    return (await this.#page.daemon('specs.file', { project, path })).file
  }

  /**
   * Shows the file at `path`: as `answered` where it was just read, else read now. What comes
   * back for a file the human has left since is not drawn at all.
   */
  async #choose(path, answered = null) {
    const shown = this.#shown
    const read = ++this.#reads
    this.#stopDrawing()
    this.#chosen = path
    this.#markChosen()
    try {
      const file = answered ?? (await this.#file(shown.project, path))
      if (read !== this.#reads) return
      const drawn = this.#draw(file)
      const caption = element('p', 'specs-caption')
      caption.append(element('span', 'specs-path', file.path), ` · ${measure(file.size)}`)
      this.#view.replaceChildren(caption, drawn)
      this.#view.scrollTop = 0
    } catch (cause) {
      if (read !== this.#reads) return
      this.#view.replaceChildren(refusal(cause))
    }
  }

  /** A file as it is shown, by what the daemon answered and what its name says it is. */
  #draw(file) {
    if (typeof file.withheld === 'string') {
      return element('p', 'specs-withheld', file.withheld)
    }
    if (typeof file.image === 'string') {
      const image = element('img', 'specs-image')
      image.alt = file.path
      image.src = `data:${imageType(file.path)};base64,${file.image}`
      return image
    }
    const kind = kindOf(file.path)
    if (kind === 'registry')
      return this.#registry(file.text) ?? this.#markdown(file.text, file.path)
    if (kind === 'markdown') return this.#markdown(file.text, file.path)
    if (kind === 'html') return this.#htmlCard(file)
    const plain = element('pre', 'specs-plain')
    plain.append(element('code', null, file.text))
    return plain
  }

  /**
   * The registry as rows, each opening its spec, in the registry's order; none
   * where it is not Specmint's table.
   */
  #registry(text) {
    const registry = readRegistry(text)
    if (registry === null) return null
    const { arranged } = this.#shown
    const view = element('section', 'spec-registry')
    view.append(element('h1', 'registry-heading', registry.heading ?? 'Spec Registry'))
    const head = element('div', 'registry-row registry-head')
    head.setAttribute('aria-hidden', 'true')
    for (const name of ['Spec', 'Status', 'Priority', 'Progress', 'Updated']) {
      head.append(element('span', null, name))
    }
    const list = element('ol', 'registry-rows')
    list.setAttribute('aria-label', 'Specs')
    for (const row of registry.rows.slice(0, MAX_ROWS)) {
      const opens = specFile(arranged, row.id)
      const item = element('li')
      const line = button('', 'registry-row', () => {
        if (opens !== null) void this.#choose(opens)
      })
      line.dataset.spec = row.id
      line.disabled = opens === null
      if (opens === null) line.title = `${row.id} has no folder here`
      const named = element('span', 'registry-title')
      named.append(element('strong', null, row.title || row.id), element('small', null, row.id))
      const status = row.status.toLowerCase()
      const chip = element('span', 'status-chip', row.status)
      chip.dataset.status = STATUSES.includes(status) ? status : 'other'
      line.append(
        named,
        chip,
        element('span', 'registry-priority', row.priority),
        progress(row),
        element('span', 'registry-updated', row.updated),
      )
      item.append(line)
      list.append(item)
    }
    view.append(head, list)
    if (registry.rows.length > MAX_ROWS) {
      view.append(
        element(
          'p',
          'registry-more',
          `The registry lists ${registry.rows.length} specs: the first ${MAX_ROWS} are shown here. Every spec's folder is in the tree.`,
        ),
      )
    }
    return view
  }

  /** Markdown, as the page draws it, its frontmatter as a table of its fields. */
  #markdown(text, path) {
    const view = element('article', 'spec-markdown')
    const front = frontmatter(text)
    if (front !== null) {
      const fields = element('dl', 'spec-fields')
      for (const [name, value] of front.fields) {
        const field = element('div')
        field.append(element('dt', null, name), element('dd', null, value))
        fields.append(field)
      }
      view.append(fields)
    }
    const { specs, files } = this.#shown
    const { signal } = this.#drawing
    let diagrams = 0
    view.append(
      ...render(front?.body ?? text, {
        levels: true,
        link: (href) => {
          const target = linkTarget(path, href, specs.folder, files)
          return target === null ? null : () => void this.#choose(target)
        },
        fence: (lang, code) => {
          if (lang !== 'mermaid') return null
          diagrams += 1
          return diagrams <= MAX_DIAGRAMS ? diagram(code, signal) : undrawn(code, MAX_DIAGRAMS)
        },
      }),
    )
    return view
  }

  /**
   * An HTML file as a card: its title, read as text (what it holds is parsed
   * and never run), its name and size, and the button that opens it in the
   * human's browser, which says what came of it.
   */
  #htmlCard(file) {
    const name = file.path.slice(file.path.lastIndexOf('/') + 1)
    const card = element('section', 'spec-html')
    card.setAttribute('aria-label', name)
    card.append(
      element('h1', 'spec-html-title', titleOf(file.text) || name),
      element('p', 'spec-html-meta', `${name} · HTML · ${measure(file.size)}`),
      element(
        'p',
        'spec-html-help',
        'It opens in your browser, outside ConsensFlow: a copy of it, with the assets it names.',
      ),
    )
    const said = element('p', 'spec-html-said')
    said.setAttribute('role', 'status')
    const project = this.#shown.project
    card.append(
      button('Open in your browser', 'primary-button', async () => {
        said.textContent = ''
        delete said.dataset.tone
        try {
          await this.#page.daemon('specs.open', { project, path: file.path })
          said.textContent = `Opened ${file.path} in your browser.`
        } catch (cause) {
          said.textContent = cause instanceof Error ? cause.message : String(cause)
          said.dataset.tone = 'error'
        }
      }),
      said,
    )
    return card
  }

  /** The tree, drawn again, each folder as the human left it or as it opens. */
  #drawTree() {
    const list = element('ul', 'tree-level')
    list.append(...this.#shown.arranged.map((entry) => this.#treeEntry(entry)))
    this.#tree.replaceChildren(list)
    this.#markChosen()
  }

  #treeEntry(entry) {
    const item = element('li')
    if (!Array.isArray(entry.entries)) {
      const file = button(entry.label, 'tree-file', () => void this.#choose(entry.path))
      file.dataset.path = entry.path
      file.title = entry.path
      item.append(file)
      return item
    }
    const folder = element('details', 'tree-folder')
    folder.dataset.path = entry.path
    folder.open = this.#folds.get(entry.path) ?? entry.open
    folder.addEventListener('toggle', () => this.#folds.set(entry.path, folder.open))
    const summary = element('summary', null, entry.label)
    summary.title = entry.path
    const inner = element('ul', 'tree-level')
    inner.append(...entry.entries.map((inside) => this.#treeEntry(inside)))
    folder.append(summary, inner)
    item.append(folder)
    return item
  }

  /** The chosen file is marked in the tree, every folder it is in unfolded. */
  #markChosen() {
    for (const file of this.#tree.querySelectorAll('.tree-file')) {
      const chosen = file.dataset.path === this.#chosen
      if (chosen) file.setAttribute('aria-current', 'true')
      else file.removeAttribute('aria-current')
      if (!chosen) continue
      for (let at = file.parentElement; at !== this.#tree; at = at.parentElement) {
        if (at.localName === 'details') at.open = true
      }
    }
  }
}

/** A spec's progress: a bar, and "done/total"; its words alone where they are no count. */
function progress(row) {
  const cell = element('span', 'registry-progress')
  if (row.total === null) {
    cell.append(element('span', 'progress-words', row.progress))
    return cell
  }
  const bar = element('span', 'progress-bar')
  bar.setAttribute('role', 'progressbar')
  bar.setAttribute('aria-valuemin', '0')
  bar.setAttribute('aria-valuemax', String(row.total))
  bar.setAttribute('aria-valuenow', String(Math.min(row.done, row.total)))
  const share = row.total === 0 ? 0 : Math.min(1, row.done / row.total)
  const fill = element('span', 'progress-fill')
  fill.style.width = `${Math.round(share * 100)}%`
  bar.append(fill)
  cell.append(bar, element('span', 'progress-words', `${row.done}/${row.total}`))
  return cell
}

/**
 * The title an HTML file gives itself, as text: the file is parsed as a
 * template's content, which is inert (it runs no script, loads nothing, and its
 * `<base>` and `<meta>` say nothing to the page), and only its title's words
 * are taken.
 */
function titleOf(html) {
  const inert = document.createElement('template')
  inert.innerHTML = html
  return inert.content.querySelector('title')?.textContent.replace(/\s+/g, ' ').trim() ?? ''
}

/** What the viewer says of a folder it has nothing to show of. */
const emptyWords = (specs) =>
  specs.found
    ? `The specs folder ${specs.folder} holds no files yet.`
    : `No specs folder yet: ${specs.folder} is not in the project's folder. The chief makes it when it writes its first spec.`

/** A refusal of the daemon's, in its words. */
const refusal = (cause) =>
  element('p', 'specs-refusal', cause instanceof Error ? cause.message : String(cause))
