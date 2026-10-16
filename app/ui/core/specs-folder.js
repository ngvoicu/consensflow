/**
 * What the specs viewer makes of a project's specs folder, as the daemon reads
 * it (`specs.tree`: each folder's entries by name, a file with its size): the
 * order it lists them in, the registry's rows, where a link between files
 * leads, and the frontmatter a spec starts with. Nothing here reads a file or
 * draws: the viewer (`specs-viewer.js`) does, with these.
 *
 * Specmint's folder holds `registry.md` (a table of the specs), one folder per
 * spec named by its id (its `SPEC.md` or `SPEC.html`, `research-01.md` and on,
 * `interview-01.md` and on, and whatever else the chief kept there), and
 * `assets/`, the stylesheet and the script of the HTML variants.
 */

/** The registry, at the top of the folder. */
export const REGISTRY = 'registry.md'
/** The HTML variants' stylesheet and script, which every `SPEC.html` links as `../assets/…`. */
export const ASSETS = 'assets'
/** A spec's own file, by variant: Markdown first. */
const SPEC_FILES = ['SPEC.md', 'SPEC.html']
/** The registry's columns, in Specmint's order. */
const COLUMNS = ['id', 'title', 'status', 'priority', 'progress', 'updated']
/** The research and the interviews of a spec, by their number. */
const NUMBERED = [/^research-(\d+)\b/i, /^interview-(\d+)\b/i]
/** What a file is shown as, by its extension; anything else that is text is plain. */
const KINDS = {
  md: 'markdown',
  markdown: 'markdown',
  html: 'html',
  htm: 'html',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  svg: 'image',
}
/** The type an image is drawn with, by its extension. */
const IMAGE_TYPES = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
}

const byName = (a, b) => a.name.localeCompare(b.name, 'en', { numeric: true })
const isFolder = (entry) => Array.isArray(entry.entries)
const extensionOf = (path) => {
  const name = path.slice(path.lastIndexOf('/') + 1)
  return name.includes('.') ? name.slice(name.lastIndexOf('.') + 1).toLowerCase() : ''
}

/** What `path` is shown as: `registry`, `markdown`, `html`, `image`, or `text`. */
export function kindOf(path) {
  if (path === REGISTRY) return 'registry'
  return KINDS[extensionOf(path)] ?? 'text'
}

/** The type an image at `path` is drawn with. */
export const imageType = (path) => IMAGE_TYPES[extensionOf(path)] ?? 'application/octet-stream'

/**
 * The tree as the viewer lists it: the registry first; then each spec's folder,
 * named by its title in the registry (`rows`) where it has one and by its id
 * otherwise, those of the registry in its order and the rest by name; then any
 * other file at the top, by name; and `assets/` last, folded. Inside a spec's
 * folder, the spec first, then its research and its interviews by number, then
 * the rest by name, its folders folded. Each entry keeps the daemon's `name`
 * and `path`, and has the `label` it is listed by; a folder has `open`.
 */
export function arrange(all, rows = []) {
  // The daemon leaves dotfiles out; so does the viewer, whoever answered it.
  const entries = undotted(all)
  const place = new Map(rows.map((row, at) => [row.id, at]))
  const title = new Map(rows.map((row) => [row.id, row.title]))
  const rank = (entry) => place.get(entry.name) ?? rows.length
  const specs = entries
    .filter((entry) => isFolder(entry) && entry.name !== ASSETS)
    .sort((a, b) => rank(a) - rank(b) || byName(a, b))
  const files = entries.filter((entry) => !isFolder(entry))
  return [
    ...files.filter((entry) => entry.name === REGISTRY).map((entry) => listed(entry)),
    ...specs.map((spec) => ({
      ...listed(spec, title.get(spec.name) || spec.name),
      open: true,
      entries: insideSpec(spec.entries),
    })),
    ...files
      .filter((entry) => entry.name !== REGISTRY)
      .sort(byName)
      .map((entry) => listed(entry)),
    ...entries
      .filter((entry) => isFolder(entry) && entry.name === ASSETS)
      .map((assets) => ({ ...listed(assets), open: false, entries: byNames(assets.entries) })),
  ]
}

/** An entry as it is listed: by `label`, its name unless said. */
const listed = (entry, label = entry.name) => ({ name: entry.name, path: entry.path, label })

/** A spec's folder: its spec, its research and interviews by number, then the rest by name. */
function insideSpec(entries) {
  const order = (entry) => {
    if (!isFolder(entry) && SPEC_FILES.includes(entry.name)) {
      return [0, SPEC_FILES.indexOf(entry.name)]
    }
    for (const [at, form] of NUMBERED.entries()) {
      const found = isFolder(entry) ? null : form.exec(entry.name)
      if (found !== null) return [1 + at, Number(found[1])]
    }
    return [NUMBERED.length + 1, 0]
  }
  return undotted(entries)
    .sort((a, b) => {
      const [x, y] = [order(a), order(b)]
      return x[0] - y[0] || x[1] - y[1] || byName(a, b)
    })
    .map(folded)
}

/** A folder's entries by name, every folder in it folded. */
const byNames = (entries) => undotted(entries).sort(byName).map(folded)

const undotted = (entries) => entries.filter((entry) => !entry.name.startsWith('.'))

const folded = (entry) =>
  isFolder(entry)
    ? { ...listed(entry), open: false, entries: byNames(entry.entries) }
    : listed(entry)

/** Every file of an arranged tree, in the order it lists them. */
export function filesOf(arranged) {
  return arranged.flatMap((entry) => (isFolder(entry) ? filesOf(entry.entries) : [entry.path]))
}

/**
 * The file a spec opens on: its `SPEC.md` or `SPEC.html`, else its first file;
 * none where the folder has no file, or the tree no such spec.
 */
export function specFile(arranged, id) {
  const folder = arranged.find((entry) => isFolder(entry) && entry.name === id)
  return folder === undefined ? null : (filesOf([folder])[0] ?? null)
}

/** The file the viewer opens on: the registry, or the first spec where there is none. */
export function firstFile(arranged) {
  if (arranged.some((entry) => entry.path === REGISTRY)) return REGISTRY
  const spec = arranged.find((entry) => isFolder(entry) && entry.name !== ASSETS)
  return (spec && specFile(arranged, spec.name)) ?? filesOf(arranged)[0] ?? null
}

/**
 * A table row's cells, without the pipes around and between them; `\|` is a pipe in a cell.
 * The row is read once: each cell's pieces are kept and joined when it ends, so a cell of a
 * hundred thousand `\|` costs what its length does, not that again for each pipe.
 */
function cells(row) {
  const inner = row.trim().replace(/^\|/, '').replace(/\|$/, '')
  const found = []
  let pieces = []
  let from = 0
  for (let at = inner.indexOf('|'); at !== -1; at = inner.indexOf('|', at + 1)) {
    if (inner[at - 1] === '\\') {
      pieces.push(inner.slice(from, at - 1), '|')
    } else {
      pieces.push(inner.slice(from, at))
      found.push(pieces.join(''))
      pieces = []
    }
    from = at + 1
  }
  pieces.push(inner.slice(from))
  found.push(pieces.join(''))
  return found.map((cell) => cell.trim())
}

/** A cell of a table's rule: dashes, a colon at either end or none. */
const RULE_CELL = /^:?-+:?$/

/**
 * The registry as Specmint writes it: its heading, and each row of its table of
 * `ID | Title | Status | Priority | Progress | Updated`, the progress read as
 * done of total where it says so. None where the text holds no such table: the
 * viewer shows it as the markdown it is. Each line is read once, in time that
 * grows with its length alone: its cells are split and trimmed, and each cell
 * is held to a pattern that cannot read a character twice.
 */
export function readRegistry(text) {
  const lines = text.split(/\r?\n/)
  const head = lines.findIndex(
    (line, at) =>
      line.includes('|') &&
      cells(line)
        .map((cell) => cell.toLowerCase())
        .join('|') === COLUMNS.join('|') &&
      isRule(lines[at + 1] ?? '', COLUMNS.length),
  )
  if (head === -1) return null
  const heading = lines.slice(0, head).find((line) => /^\s*#{1,6}\s+/.test(line))
  const rows = []
  for (let at = head + 2; at < lines.length && lines[at].includes('|'); at += 1) {
    const said = cells(lines[at])
    const row = Object.fromEntries(COLUMNS.map((column, place) => [column, said[place] ?? '']))
    const progress = /^(\d+)\s*\/\s*(\d+)$/.exec(row.progress)
    row.done = progress === null ? null : Number(progress[1])
    row.total = progress === null ? null : Number(progress[2])
    rows.push(row)
  }
  return { heading: heading?.replace(/^\s*#{1,6}\s+/, '').trim() ?? null, rows }
}

/** Whether `line` is the rule under a table's head of `count` cells. */
function isRule(line, count) {
  if (!line.includes('-')) return false
  const rule = cells(line)
  return rule.length === count && rule.every((cell) => RULE_CELL.test(cell))
}

/**
 * Where a link in the file at `from` leads, in the folder: the path of a file
 * the folder holds (`files`), or none. A link is read relative to the file's
 * own folder, or, where it starts with the specs folder's name (`folder`, as
 * an agent writes a path from the project), from the folder's top; never out
 * of it. Anything with a scheme (`https:`, `javascript:`), an absolute path or
 * a fragment of the same file leads nowhere in the viewer.
 */
export function linkTarget(from, href, folder, files) {
  const address = href.trim()
  if (/^[a-z][a-z0-9+.-]*:/i.test(address) || /^[/\\]/.test(address)) return null
  let path = address.replace(/[?#].*$/, '')
  if (path === '') return null
  try {
    path = decodeURIComponent(path)
  } catch {
    return null
  }
  const here = from.includes('/') ? from.slice(0, from.lastIndexOf('/')).split('/') : []
  const top = `${folder.replace(/^\.\//, '').replace(/\/+$/, '')}/`
  const tries = [walk(here, path)]
  if (path.startsWith(top)) tries.push(walk([], path.slice(top.length)))
  return tries.find((target) => target !== null && files.has(target)) ?? null
}

/** `path` walked from the folder at `start` (its names): none where it climbs out of the top. */
function walk(start, path) {
  const names = [...start]
  for (const part of path.split('/')) {
    if (part === '' || part === '.') continue
    if (part === '..') {
      if (names.length === 0) return null
      names.pop()
    } else {
      names.push(part)
    }
  }
  return names.join('/')
}

/**
 * The frontmatter a spec starts with (`---`, `key: value` lines, `---`), as
 * its fields in order, and the text after it; none where it has none.
 */
export function frontmatter(text) {
  const lines = text.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') return null
  const end = lines.findIndex((line, at) => at > 0 && /^(---|\.\.\.)\s*$/.test(line))
  if (end === -1) return null
  const fields = lines
    .slice(1, end)
    .filter((line) => line.trim() !== '')
    .map((line) => {
      const colon = line.indexOf(':')
      return colon === -1
        ? [line.trim(), '']
        : [line.slice(0, colon).trim(), line.slice(colon + 1).trim()]
    })
  return { fields, body: lines.slice(end + 1).join('\n') }
}

/** A size as it is said: `812 bytes`, `12.5 KB`, `3.2 MB` (of 1024), as the daemon says one. */
export function measure(size) {
  if (size < 1024) return `${size} bytes`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}
