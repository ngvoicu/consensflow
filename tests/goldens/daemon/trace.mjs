/**
 * The event file, recorded: each line appended and each `forget`, with the
 * files of the folder as they stood after it. A line the trace dated itself
 * (the entry carried no `at`) has the time named `«now»`.
 */
import { relative } from 'node:path'
import * as real from '../../../src/core/trace.js'
import { added, folder, named } from './lines.mjs'
import * as session from './session.mjs'
import { treeOf } from './world.mjs'

export * from '../../../src/core/trace.js'

const LIMIT = 5_000_000

export function eventTrace(home, options = {}) {
  const trace = real.eventTrace(home, options)
  if (session.recording() === null) return trace
  const id = session.next('trace')
  const root = treeOf(home)
  if (root !== null) session.addRoot(root)
  session.touch()
  session.push({
    kind: 'trace.open',
    trace: id,
    folder: root === null ? home : relative(root, home).split('\\').join('/') || '.',
    limit: options.limit ?? LIMIT,
  })
  /** What a step left: the folder's files, and the new line noted when the trace dated it. */
  const left = (before, step, datesItself) => {
    const now = folder(home)
    if (datesItself) for (const line of added(before, now)) session.dated().add(line)
    session.push({ ...step, files: named(now, session.dated()) })
  }
  const recorded = (entry) => {
    const before = folder(home)
    trace(entry)
    left(
      before,
      { kind: 'trace.append', trace: id, entry: session.value(entry) },
      entry?.at === undefined,
    )
  }
  recorded.forget = (project) => {
    const before = folder(home)
    trace.forget(project)
    left(before, { kind: 'trace.forget', trace: id, project }, false)
  }
  return recorded
}
