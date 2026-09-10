import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import test from 'node:test'
import { answers } from '../../hosts/lib/completion.js'
import { leadReady } from '../../hosts/lib/readiness.js'

test('Claude 2.1.266 settles the captured native tool turn only after its finalizer', async (t) => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native266-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const rows = (
    await readFile(
      new URL('./fixtures/completion/claude-code/v266-tool-loop.jsonl', import.meta.url),
      'utf8',
    )
  )
    .trim()
    .split('\n')
    .map(JSON.parse)
  const session = rows[0].sessionId
  await mkdir(join(root, 'projects'))
  for (const n of [rows.length - 1, rows.length]) {
    await writeFile(
      join(root, 'projects', `${session}.jsonl`),
      rows.slice(0, n).map(JSON.stringify).join('\n') + '\n',
    )
    const found = await answers('claude-code', session, { CLAUDE_CONFIG_DIR: root })
    assert.equal(found.unknown, undefined, found.reason)
    assert.equal(found.version, undefined)
    assert.equal(
      leadReady({ answers: found, draftLatched: false, epoch: 2 }).state === 'ready',
      n === rows.length,
    )
    if (n === rows.length) assert.equal(found.items.at(-1).text, 'CF_NATIVE_VERSION_PROBE_DONE')
  }
})

test('OpenCode 1.18.30 uses captured native event order and final completion metadata', async (t) => {
  const root = await mkdtemp(join(tmpdir(), 'cf-native130-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  await mkdir(join(root, 'opencode'))
  const fixture = JSON.parse(
    await readFile(
      new URL('./fixtures/completion/opencode/v130-tool-loop.json', import.meta.url),
      'utf8',
    ),
  )
  const db = new DatabaseSync(join(root, 'opencode/opencode.db'))
  for (const [table, rows] of Object.entries(fixture)) {
    const keys = Object.keys(rows[0])
    db.exec(
      `CREATE TABLE ${table} (${keys.map((key) => `"${key}" ${typeof rows[0][key] === 'number' ? 'INTEGER' : 'TEXT'}`).join(', ')})`,
    )
    const insert = db.prepare(
      `INSERT INTO ${table} (${keys.map((x) => `"${x}"`).join(', ')}) VALUES (${keys.map(() => '?').join(', ')})`,
    )
    for (const row of rows) insert.run(...keys.map((key) => row[key]))
  }
  db.close()
  const found = await answers('opencode', fixture.session[0].id, { XDG_DATA_HOME: root })
  assert.equal(found.unknown, undefined, found.reason)
  assert.equal(found.version, undefined)
  assert.equal(found.items.at(-1).text, 'CF_NATIVE_VERSION_PROBE_DONE')
  assert.equal(leadReady({ answers: found, draftLatched: false, epoch: 2 }).state, 'ready')
})
