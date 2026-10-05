/**
 * The files `trace.js` and `log.js` write, read back after each step: every
 * file of the folder as its lines, those the writer dated with its own clock
 * with that time named `«now»` (the time is the machine's; the line is the
 * format). The daemon keeps both files in one folder, so what either dated
 * itself is named in a snapshot taken for the other.
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

/** The files of `dir` and their text (none for a folder that is not there), in name order. */
export function folder(dir) {
  try {
    return Object.fromEntries(
      readdirSync(dir)
        .sort()
        .map((name) => [name, readFileSync(join(dir, name), 'utf8')]),
    )
  } catch {
    return {}
  }
}

/** The lines of `files`, each with how many there are. */
function counted(files) {
  const counts = new Map()
  for (const text of Object.values(files)) {
    for (const line of text.split('\n')) counts.set(line, (counts.get(line) ?? 0) + 1)
  }
  return counts
}

/** The lines in `after` that `before` did not have, as many times as they are more. */
export function added(before, after) {
  const had = counted(before)
  const fresh = []
  for (const [line, count] of counted(after)) {
    for (let more = count - (had.get(line) ?? 0); more > 0; more -= 1) fresh.push(line)
  }
  return fresh
}

/** A line the trace dated: `{"at":"…"` is the first of its keys. A line the log dated: the time leads. */
const undated = (line) =>
  line.startsWith('{"at":"')
    ? line.replace(/^\{"at":"[^"]*"/, '{"at":"«now»"')
    : line.replace(/^\S+ /, '«now» ')

/** `files` with each line of `dated` (a writer's own dating) naming its time. */
export function named(files, dated) {
  return Object.fromEntries(
    Object.entries(files).map(([name, text]) => [
      name,
      text
        .split('\n')
        .map((line) => (dated.has(line) ? undated(line) : line))
        .join('\n'),
    ]),
  )
}
