import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * The fixture folders of the specs viewer's tests (`fixtures/specs/<name>`),
 * as the daemon answers for them: `specs.tree` is `<name>.tree.json`, which
 * the daemon's own test reads each folder against
 * (`crates/cf-daemon/src/page/tests/viewer.rs`), and `specs.file` is each
 * file read as the daemon reads one: an image's bytes by its extension, text
 * where it is UTF-8 with no NUL, and otherwise its size and why it is not
 * shown. Synthetic, from Specmint's formats: no real project's specs.
 */

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'specs')
const IMAGE = /\.(png|jpe?g|gif|webp|svg)$/i
/** The largest file the daemon shows (`page::viewer::folder::MAX_FILE`). */
export const MAX_FILE = 512 * 1024

/** A size as the daemon says it (`folder::measure`). */
export function measure(size) {
  if (size < 1024) return `${size} bytes`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

/** What the daemon answers for a file of `bytes` at `path`. */
export function answerFor(path, bytes) {
  const size = bytes.length
  if (size > MAX_FILE) {
    return {
      path,
      size,
      withheld: `${path} is ${measure(size)}, more than the ${measure(MAX_FILE)} the viewer shows`,
    }
  }
  if (IMAGE.test(path)) return { path, size, image: Buffer.from(bytes).toString('base64') }
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes)
    if (!text.includes('\0')) return { path, size, text }
  } catch {}
  return { path, size, withheld: `${path} is ${measure(size)} that is neither text nor an image` }
}

/** Every file of a fixture folder by its path, as the daemon answers it. */
function filesOf(folder, at = '') {
  const files = {}
  for (const name of readdirSync(join(folder, at))) {
    if (name.startsWith('.')) continue
    const path = at === '' ? name : `${at}/${name}`
    if (statSync(join(folder, path)).isDirectory()) Object.assign(files, filesOf(folder, path))
    else files[path] = answerFor(path, readFileSync(join(folder, path)))
  }
  return files
}

/**
 * The fixture `name` as the stand-in daemon holds it, named `folder` in its
 * project: what `specs.tree` and `specs.file` answer.
 */
export function specsOf(name, folder = '.specs') {
  return {
    folder,
    found: true,
    entries: JSON.parse(readFileSync(join(FIXTURES, `${name}.tree.json`), 'utf8')),
    files: filesOf(join(FIXTURES, name)),
  }
}

/**
 * `specs` with a file added at `path` (in a folder the tree has, or at its top),
 * as the chief would write one: its entry in the tree and its answer.
 */
export function withFile(specs, path, bytes) {
  const names = path.split('/')
  let entries = specs.entries
  for (const [at, name] of names.slice(0, -1).entries()) {
    let folder = entries.find((entry) => entry.name === name && entry.entries)
    if (folder === undefined) {
      folder = { name, path: names.slice(0, at + 1).join('/'), entries: [] }
      entries.push(folder)
    }
    entries = folder.entries
  }
  entries.push({ name: names.at(-1), path, size: bytes.length })
  specs.files[path] = answerFor(path, bytes)
  specs.found = true
  return specs
}
