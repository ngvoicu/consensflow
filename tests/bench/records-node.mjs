/**
 * Node's first look at a Claude transcript: the time it takes and the memory
 * it peaks at, for the Rust reader's (`npm run bench:records-memory`) to be
 * read beside, the same file looked at by both. The transcript is copied into
 * a folder of its own, as Claude Code's `projects` folder holds one (a clone
 * where the file system makes one, so nothing is read twice), and the
 * copy is removed after. The original is only read.
 *
 *   node tests/bench/records-node.mjs FILE
 *
 * The file's name, without `.jsonl`, is its session.
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { performance } from 'node:perf_hooks'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { refuseTailoredCollation } from '../goldens/records/tables.mjs'

const MEGABYTE = 1_048_576
const file = process.argv[2]
if (file === undefined) throw new Error('usage: node tests/bench/records-node.mjs FILE')
// Node's readers order ids as ICU's root collation does, as the Rust ones do.
refuseTailoredCollation()

const session = path.basename(file, '.jsonl')
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'consensflow-node-look-'))
try {
  const folder = path.join(root, 'projects', 'bench')
  fs.mkdirSync(folder, { recursive: true })
  fs.copyFileSync(file, path.join(folder, `${session}.jsonl`), fs.constants.COPYFILE_FICLONE)
  const env = { ...process.env, CLAUDE_CONFIG_DIR: root }
  // `maxRSS` is in kilobytes.
  const peak = () => (process.resourceUsage().maxRSS * 1024) / MEGABYTE
  const before = peak()
  const read = cachedAnswers()
  const started = performance.now()
  const reading = await read('claude-code', session, env)
  const took = performance.now() - started
  console.log(`items in the answer: ${reading.items?.length ?? reading.reason}`)
  console.log(`first look: ${took.toFixed(1)} ms`)
  console.log(`memory held before the look, at most: ${Math.round(before)} MB`)
  console.log(`memory held by the look, at most: ${Math.round(peak())} MB`)
} finally {
  fs.rmSync(root, { recursive: true, force: true })
}
