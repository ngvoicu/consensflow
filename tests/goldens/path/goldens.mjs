/**
 * The goldens of Node's `path.join` and `path.normalize`, as Node computes
 * them: what the Rust port in crates/cf-base (`cf_base::path`) is held to,
 * for the POSIX and the Windows flavour both, whatever system this runs on.
 * The file is deterministic, so the unit suite holds the committed copy equal
 * to what this computes (tests/path-goldens.test.mjs), and `npm run
 * goldens:path` writes it again after a change.
 *
 * `tests/goldens/path.json` holds:
 * - `joins`: the parts of each case, and what `path.posix.join` and
 *   `path.win32.join` answered. The cases are every segment alone, every
 *   ordered pair of segments, and a seeded sample of distinct triples;
 * - `normalizes`: each segment as a text of its own, and what
 *   `path.posix.normalize` and `path.win32.normalize` answered.
 *
 * The segments are the texts an environment variable may hold, the odd ones
 * included: dots and separators of either kind, drives, UNC and device roots,
 * Windows' reserved names, and text outside ASCII (a letter, two Japanese
 * characters, an emoji), where JavaScript's lengths, in UTF-16 units, are not
 * the lengths of the text's bytes.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { join, posix, win32 } from 'node:path'

const SEGMENTS = [
  // Nothing, dots and separators.
  '',
  '.',
  '..',
  '...',
  'a',
  'a/',
  '/',
  '//',
  '///',
  '\\',
  '\\\\',
  'a\\b',
  'a/b',
  // Drives, with and without a separator after them.
  'C:',
  'c:',
  'C:\\',
  'C:/',
  'C:..\\cf',
  'C:a',
  'a:b',
  ':',
  // UNC and device roots.
  '\\\\server\\share',
  '//server/share/x',
  '\\\\?\\C:\\x',
  '\\\\.\\pipe\\x',
  // Windows' reserved names: whole, with an extension, with a colon after
  // them, and with one more character after them (CONa, CONé), which Node
  // cuts off before it looks the name up when the path has no colon. Half of
  // an emoji is cut off the same way (CON😀) and is no name.
  'CON',
  'con.txt',
  'COM1:',
  'LPT9',
  'NUL:x',
  'CONa',
  'NULL',
  'CONé',
  'CON😀',
  'COM¹:x',
  '\\\\?\\COM1:',
  '\\\\.\\COM1:x',
  '\\\\?\\COM¹:x',
  // Names that are not dots, and paths that climb.
  '~',
  '~/x',
  '.cf',
  '..a',
  'a..',
  './a',
  '../a',
  'a/../..',
  '/a/./b/../c/',
  // What ConsensFlow joins: a user's home, its folder and the roster's file.
  '/home/me',
  'C:\\Users\\me',
  '.consensflow',
  'agents.json',
  // Text outside ASCII.
  'é',
  '日本',
  '日本/x',
  '😀',
  // Trailing separators.
  '../',
  '..\\',
  'C:\\x\\',
  '\\\\server\\share\\',
  '日本/',
]

const TRIPLES = 3000

/** A seeded sequence of numbers in [0, 1): the same sample every run. */
function seeded(seed) {
  let state = seed >>> 0
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state / 2 ** 32
  }
}

/** `count` distinct triples of segments, drawn by a seeded generator. */
function sampledTriples(count) {
  const random = seeded(20261004)
  const pick = () => SEGMENTS[Math.floor(random() * SEGMENTS.length)]
  const seen = new Set()
  const triples = []
  while (triples.length < count) {
    const triple = [pick(), pick(), pick()]
    const key = JSON.stringify(triple)
    if (seen.has(key)) continue
    seen.add(key)
    triples.push(triple)
  }
  return triples
}

/** `value` itself, after a check that JSON holds it: no half of a surrogate pair. */
function wellFormed(value) {
  if (!value.isWellFormed()) throw new Error(`a golden text with half an emoji: ${value}`)
  return value
}

/** A list as one item a line: diffs read item by item. */
const lines = (items) => items.map((item) => `    ${JSON.stringify(item)}`).join(',\n')
const section = (name, items) => `  "${name}": [\n${lines(items)}\n  ]`

function pathGolden() {
  const singles = SEGMENTS.map((segment) => [segment])
  const pairs = SEGMENTS.flatMap((first) => SEGMENTS.map((second) => [first, second]))
  const joins = [...singles, ...pairs, ...sampledTriples(TRIPLES)].map((parts) => ({
    parts: parts.map(wellFormed),
    posix: wellFormed(posix.join(...parts)),
    win32: wellFormed(win32.join(...parts)),
  }))
  const normalizes = SEGMENTS.map((path) => ({
    path: wellFormed(path),
    posix: wellFormed(posix.normalize(path)),
    win32: wellFormed(win32.normalize(path)),
  }))
  const text = `{\n${section('joins', joins)},\n${section('normalizes', normalizes)}\n}\n`
  return { text, counts: { joins: joins.length, normalizes: normalizes.length } }
}

/** Every golden, by its path under crates/cf-base; and how many cases each list holds. */
export function pathGoldens() {
  const { text, counts } = pathGolden()
  return { files: { 'tests/goldens/path.json': text }, counts }
}

/** Writes every golden into `crate`. */
export function writePathGoldens(crate) {
  const { files, counts } = pathGoldens()
  for (const [relative, text] of Object.entries(files)) {
    const path = join(crate, ...relative.split('/'))
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, text)
  }
  return { written: Object.keys(files).length, counts }
}
