/**
 * What `npm run parity:launch` reads of a root before and after a plan
 * (`launch.mjs` writes it for Node's, `crates/cf-harness/tests/parity_launch.rs`
 * makes it for Rust's): every file, folder and link under it, by its path
 * there, and what a plan changed. A harness's own state is never listed,
 * only counted, for its text is not ConsensFlow's to hold equal.
 */
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const MODE = 0o777

/**
 * A file, folder or link as the comparison holds it: a file's mode (none on
 * Windows) and its text, or the SHA-256 of what is no UTF-8; a link's target,
 * never followed.
 */
function describe(file) {
  const found = fs.lstatSync(file)
  if (found.isSymbolicLink()) return { kind: 'link', target: fs.readlinkSync(file) }
  const mode = process.platform === 'win32' ? null : found.mode & MODE
  if (found.isDirectory()) return { kind: 'dir', mode }
  const bytes = fs.readFileSync(file)
  try {
    // The byte order mark is a character of the text, as a reader of it finds one.
    const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
    return { kind: 'file', mode, text }
  } catch {
    return { kind: 'file', mode, bytes: createHash('sha256').update(bytes).digest('hex') }
  }
}

/**
 * Everything under `root`, by its path there with `/` between the names, and
 * how many entries each of the `owned` folders holds: the folders are not
 * listed, nor is what is in them. An entry that goes while the owned folders
 * are counted (a harness clearing up) is not counted.
 */
export function snapshot(root, owned) {
  const entries = new Map()
  const counts = new Map(owned.map((folder) => [folder, 0]))
  const walk = (folder, relative, harnessOwns) => {
    let names
    try {
      names = fs.readdirSync(folder, { withFileTypes: true })
    } catch (error) {
      if (harnessOwns && error.code === 'ENOENT') return
      throw error
    }
    for (const entry of names) {
      const name = relative === '' ? entry.name : `${relative}/${entry.name}`
      const full = path.join(folder, entry.name)
      const within = owned.find((own) => name.startsWith(`${own}/`))
      if (within !== undefined) counts.set(within, counts.get(within) + 1)
      else if (!owned.includes(name)) entries.set(name, describe(full))
      if (entry.isDirectory()) walk(full, name, within !== undefined || owned.includes(name))
    }
  }
  walk(root, '', false)
  return { entries, counts }
}

/**
 * What a plan did to the tree: each path made or changed, with what it is
 * now, and each one removed, by path in the order JavaScript sorts text.
 */
export function changes(before, after) {
  const paths = [...new Set([...before.keys(), ...after.keys()])].sort()
  return paths.flatMap((name) => {
    const now = after.get(name)
    if (now === undefined) return [{ path: name, kind: 'removed' }]
    return JSON.stringify(before.get(name)) === JSON.stringify(now) ? [] : [{ path: name, ...now }]
  })
}

/** How many entries each owned folder gained, those that gained any. */
export function gained(before, after) {
  return Object.fromEntries(
    [...after].flatMap(([folder, count]) =>
      count === before.get(folder) ? [] : [[folder, count - before.get(folder)]],
    ),
  )
}

/**
 * The ways `root` reads in the text a plan makes besides as itself: as a file
 * URL (OpenCode's plugin), as JSON writes it (Windows' backslashes escaped),
 * as a URL's query holds it (`encodeURIComponent`; that with the parser's
 * `%27` for a quote; and a form's, with `+` for a space), and with slashes
 * where a window names a program of the bundle's.
 */
export function rootForms(root) {
  const component = encodeURIComponent(root)
  return {
    fileUrl: pathToFileURL(root).href,
    plain: [
      ...new Set([
        root,
        JSON.stringify(root).slice(1, -1),
        component,
        component.replaceAll("'", '%27'),
        new URLSearchParams({ d: root }).toString().slice(2),
        root.replaceAll('\\', '/'),
      ]),
    ],
  }
}
