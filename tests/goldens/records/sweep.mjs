/**
 * A seeded sweep: each JSONL fixture with one part of one record dropped,
 * nulled or of another type, and one fresh look. Where Node's reader trips
 * on what it reads (a property of null, a value of the wrong type), its
 * reading is unknown, and the sweep holds the port to the same. The same
 * seed sweeps the same cases on every run.
 */
import { fixtureLines, transcript } from './fixtures.mjs'

/** A seeded sequence of numbers in [0, 1). */
function seeded(seed) {
  let state = seed >>> 0
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state / 2 ** 32
  }
}

/** Every place in `value` below its root, as a list of keys and indexes. */
function places(value, at = []) {
  if (value === null || typeof value !== 'object') return []
  const keys = Array.isArray(value) ? value.map((_, index) => index) : Object.keys(value)
  return keys.flatMap((key) => [[...at, key], ...places(value[key], [...at, key])])
}

/** The same place's value as another type: what a writer of another version might put there. */
function retyped(value) {
  if (typeof value === 'string') return value.length
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)
  if (value === null) return 'null'
  if (Array.isArray(value)) return {}
  return []
}

const SWEPT = [
  ['codex', 'codex/completed.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/errored-task-complete.jsonl', '01a074ec-7aff-74b0-8cf6-aa00d8e451cb'],
  ['codex', 'codex/interrupted.jsonl', '01a077f6-6663-7bc2-81cd-e287ccaabdbd'],
  ['codex', 'codex/forked.jsonl', '01a077fa-5968-7b62-8fdd-043410a3d4b9'],
  ['claude-code', 'claude-code/fragments.jsonl', '15fba934-d727-4777-8791-123675a63649'],
  ['claude-code', 'claude-code/queued-turn.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/queue-pop-all.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/interrupted.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/provider-429.jsonl', '33383216-87a0-4e6d-a273-07c4b229cdb1'],
  ['claude-code', 'claude-code/compaction.jsonl', '1b09fb15-feb1-4595-9f47-5eb9ff768191'],
  ['claude-code', 'claude-code/v265-tool-loop.jsonl', '17499106-8778-48e1-a306-87bd186c9f7e'],
  ['claude-code', 'claude-code/v268-late-ancestors.jsonl', '4e761651-511b-4065-8a65-6ff21582faad'],
  ['pi', 'pi/tool-loop.jsonl', 'hazy-ridge'],
  ['pi', 'pi/provider-429.jsonl', 'triton-jade-fern'],
  ['pi', 'pi/between-tool-steps.jsonl', 'hazy-ridge'],
]
const PER_FIXTURE = 12

export function sweepScenarios() {
  const random = seeded(20261004)
  const pick = (list) => list[Math.floor(random() * list.length)]
  return SWEPT.flatMap(([kind, name, session]) => {
    const lines = fixtureLines(name)
    return Array.from({ length: PER_FIXTURE }, (_, index) => {
      const line = Math.floor(random() * lines.length)
      const record = JSON.parse(lines[line])
      const place = pick(places(record))
      const how = pick(['dropped', 'nulled', 'retyped'])
      const parent = place.slice(0, -1).reduce((value, key) => value[key], record)
      const key = place.at(-1)
      if (how === 'dropped' && Array.isArray(parent)) parent.splice(key, 1)
      else if (how === 'dropped') delete parent[key]
      else if (how === 'nulled') parent[key] = null
      else parent[key] = retyped(parent[key])
      const { dir, file, env } = transcript(kind, session)
      return {
        name: `sweep ${index}: ${name}, record ${line}, ${place.join('.')} ${how}`,
        env,
        steps: [
          { mkdir: dir },
          { write: file, fixture: name, line, text: JSON.stringify(record) },
          { look: 'fresh', kind, session },
        ],
      }
    })
  })
}
