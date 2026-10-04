/**
 * What the records' scenarios share: the fixtures they read (every one of
 * them, so a fixture no scenario reads is said), a record cut into the
 * pieces its harness writes, where each JSONL harness keeps a transcript,
 * a writer's row, and a look by both readers.
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

export const FIXTURES = fileURLToPath(new URL('../../engine/fixtures/completion/', import.meta.url))

/** Every fixture a scenario read, so a fixture no scenario reads is said. */
export const read = new Set()

/** A fixture's text, by its path under the fixtures folder. */
export function fixture(name) {
  read.add(name)
  return readFileSync(`${FIXTURES}${name}`, 'utf8')
}

export const fixtureLines = (name) => fixture(name).trimEnd().split('\n')
export const fixtureJson = (name) => JSON.parse(fixture(name))

/** Each line in two halves and then its newline, so a look finds every kind of last line. */
export function pieces(lines) {
  return lines
    .flatMap((line) => {
      const half = Math.floor(line.length / 2)
      return [line.slice(0, half), line.slice(half), '\n']
    })
    .filter((piece) => piece.length > 0)
}

/** Where each JSONL harness keeps a session's transcript, under the root. */
export function transcript(kind, session) {
  if (kind === 'codex') {
    const dir = '$ROOT/sessions/2026/09/06'
    return {
      dir,
      file: `${dir}/rollout-2026-09-06T00-00-00-${session}.jsonl`,
      env: { CODEX_HOME: '$ROOT' },
    }
  }
  if (kind === 'claude-code') {
    const dir = '$ROOT/projects/-work-app'
    return { dir, file: `${dir}/${session}.jsonl`, env: { CLAUDE_CONFIG_DIR: '$ROOT' } }
  }
  const dir = '$ROOT/.pi/agent/sessions/--work-app--'
  return { dir, file: `${dir}/2026-09-06T00-00-00-000Z_${session}.jsonl`, env: { HOME: '$ROOT' } }
}

/** An insert or replace of `row` into `table`, as a writer's statement. */
export function upsert(db, table, row) {
  const columns = Object.keys(row)
  return {
    db,
    run: `insert or replace into ${table} (${columns.map((column) => `"${column}"`).join(', ')}) values (${columns.map(() => '?').join(', ')})`,
    params: columns.map((column) => row[column]),
  }
}

/** A look by the cached reader and a fresh one. */
export const both = (kind, session, options = {}) => ({ look: 'both', kind, session, options })
