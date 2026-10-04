/**
 * The harness records' goldens, as Node reads them: what `crates/cf-harness`
 * is held to. Every file is deterministic, so the unit suite holds the
 * committed copies equal to what this computes (tests/records-goldens.test.mjs),
 * and `npm run goldens:records` writes them again after a change to a reader,
 * to quota, or to a fixture.
 *
 * - `tests/goldens/records/sequences.json.gz`: the scenarios of the chunked
 *   suite, each record written a piece at a time with a look after each;
 * - `suite.json.gz`: the cases of the completion and harness-version suites;
 * - `sweep.json.gz`: each JSONL fixture with one part of a record changed;
 * - `tables.json`: quota's functions and `localeCompare`, as tables.
 *
 * A scenario file holds one scenario a line (runner.mjs says what one is),
 * gzipped. Readings are compared after unzipping, never as bytes: zlib's
 * header names the platform that wrote it. The generator sets `TZ` first,
 * for the resets that name no zone.
 */
import { readdirSync } from 'node:fs'
import { gunzipSync, gzipSync } from 'node:zlib'
import { claudeScenarios } from './claude.mjs'
import { devinCases, devinSequences } from './devin.mjs'
import { FIXTURES, read } from './fixtures.mjs'
import { guardScenarios } from './guards.mjs'
import { jsonlSequences } from './jsonl.mjs'
import { opencodeCases, opencodeSequences } from './opencode.mjs'
import { play } from './runner.mjs'
import { suiteScenarios } from './suite.mjs'
import { sweepScenarios } from './sweep.mjs'
import { DEFAULT_ZONE, tables } from './tables.mjs'
import { versionScenarios } from './versions.mjs'

export { DEFAULT_ZONE }

/** Scenarios played, one a line. */
async function played(scenarios) {
  const lines = []
  for (const scenario of scenarios) lines.push(JSON.stringify(await play(scenario)))
  return `[\n${lines.join(',\n')}\n]\n`
}

/**
 * Every golden's text, by its path under crates/cf-harness. Fails when a
 * fixture under tests/engine/fixtures/completion is read by no scenario.
 */
export async function recordsGoldens() {
  if (process.env.TZ !== DEFAULT_ZONE) throw new Error(`set TZ=${DEFAULT_ZONE} first`)
  const files = {
    'tests/goldens/records/sequences.json': await played([
      ...jsonlSequences(),
      ...opencodeSequences(),
      ...devinSequences(),
    ]),
    'tests/goldens/records/suite.json': await played([
      ...suiteScenarios(),
      ...claudeScenarios(),
      ...opencodeCases(),
      ...devinCases(),
      ...guardScenarios(),
      ...versionScenarios(),
    ]),
    'tests/goldens/records/sweep.json': await played(sweepScenarios()),
    'tests/goldens/records/tables.json': `${JSON.stringify(tables(), null, 2)}\n`,
  }
  const unread = readdirSync(FIXTURES, { recursive: true })
    .map((name) => name.replaceAll('\\', '/'))
    .filter((name) => /\.jsonl?$/.test(name) && !read.has(name))
  if (unread.length > 0) throw new Error(`fixtures no scenario reads: ${unread.join(', ')}`)
  return files
}

/** Whether a golden is kept gzipped: every scenario file is. */
export const gzipped = (relative) => !relative.endsWith('tables.json')

/** A golden's committed text, from its file's bytes. */
export const committedText = (relative, bytes) =>
  gzipped(relative) ? gunzipSync(bytes).toString('utf8') : bytes.toString('utf8')

/** A golden's text as the bytes its file holds. */
export const fileBytes = (relative, text) =>
  gzipped(relative) ? gzipSync(text, { level: 9 }) : Buffer.from(text, 'utf8')

/** Where a golden's file is, by its path: a scenario file ends `.gz`. */
export const filePath = (relative) => (gzipped(relative) ? `${relative}.gz` : relative)
