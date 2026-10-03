import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, utimes, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { describe, it } from 'node:test'
import { askingTurnEnd, bareMetrics, findSession } from '../evals/bare.mjs'
import { devinFolders } from '../src/harnesses.js'

const WORKSPACE = '/evals/workspace'

async function withHome(run) {
  const home = await mkdtemp(path.join(os.tmpdir(), 'cf-bare-'))
  try {
    await run(home)
  } finally {
    await rm(home, { recursive: true, force: true })
  }
}

describe("finding a bare harness's session in its own store", () => {
  it('Codex: the newest rollout whose first line names the workspace', async () => {
    await withHome(async (home) => {
      const day = path.join(home, '.codex', 'sessions', '2026', '09', '28')
      await mkdir(day, { recursive: true })
      const rollout = async (name, id, cwd, at) => {
        const file = path.join(day, `rollout-${name}.jsonl`)
        await writeFile(
          file,
          `${JSON.stringify({ type: 'session_meta', payload: { id, cwd } })}\n{"type":"x"}\n`,
        )
        await utimes(file, at / 1000, at / 1000)
      }
      const since = Date.parse('2026-09-28T20:00:00Z')
      await rollout('old', 'before-the-run', WORKSPACE, since - 60_000)
      await rollout('other', 'another-folder', '/elsewhere', since + 60_000)
      assert.equal(findSession('codex', { workspace: WORKSPACE, since, home }), null)
      await rollout('chief', 'the-chief', WORKSPACE, since + 30_000)
      assert.equal(findSession('codex', { workspace: WORKSPACE, since, home }), 'the-chief')
    })
  })

  it('OpenCode: the newest top-level session in the workspace, not a subagent’s', async () => {
    await withHome(async (home) => {
      const dir = path.join(home, '.local', 'share', 'opencode')
      await mkdir(dir, { recursive: true })
      const since = 1_790_000_000_000
      assert.equal(
        findSession('opencode', { workspace: WORKSPACE, since, home }),
        null,
        'no store yet',
      )
      const db = new DatabaseSync(path.join(dir, 'opencode.db'))
      db.exec(
        'CREATE TABLE session (id TEXT, directory TEXT, parent_id TEXT, time_created INTEGER)',
      )
      const add = db.prepare('INSERT INTO session VALUES (?, ?, ?, ?)')
      add.run('before', WORKSPACE, null, since - 1)
      add.run('ses_chief', WORKSPACE, null, since + 10)
      add.run('ses_sub', WORKSPACE, 'ses_chief', since + 20)
      add.run('ses_other', '/elsewhere', null, since + 30)
      db.close()
      assert.equal(findSession('opencode', { workspace: WORKSPACE, since, home }), 'ses_chief')
    })
  })

  it('Devin: the newest session in the workspace, its times in seconds', async () => {
    await withHome(async (home) => {
      // Where the app reads Devin's store on this platform.
      const dir = path.join(devinFolders({ HOME: home }).data, 'cli')
      await mkdir(dir, { recursive: true })
      const db = new DatabaseSync(path.join(dir, 'sessions.db'))
      db.exec('CREATE TABLE sessions (id TEXT, working_directory TEXT, created_at INTEGER)')
      const since = 1_790_000_000_000
      db.prepare('INSERT INTO sessions VALUES (?, ?, ?)').run('before', WORKSPACE, since / 1000 - 5)
      db.prepare('INSERT INTO sessions VALUES (?, ?, ?)').run(
        'the-chief',
        WORKSPACE,
        since / 1000 + 5,
      )
      db.close()
      assert.equal(findSession('devin', { workspace: WORKSPACE, since, home }), 'the-chief')
    })
  })

  it('refuses a harness that opens on an id it is given', () => {
    assert.throws(
      () => findSession('claude-code', { workspace: WORKSPACE, since: 0, home: '/h' }),
      /id of its own/,
    )
  })
})

describe("a bare chief's numbers", () => {
  const items = [
    { id: 'u1', role: 'user', text: 'Add the page', complete: true },
    { id: 'a1', role: 'assistant', text: 'Reading?', complete: false },
    { id: 't1', role: 'tool', text: 'ok', complete: true },
    { id: 'a2', role: 'assistant', text: 'Keep the old document? Publish today?', complete: true },
    { id: 'u2', role: 'user', text: 'Keep it. Not yet.', complete: true },
    { id: 'a3', role: 'assistant', text: 'Done: the page is in place.', complete: true },
  ]

  it('counts what it asked at the ends of its turns, with no board', () => {
    const metrics = bareMetrics(items, { filesChanged: ['site/legislatie.html'] })
    assert.deepEqual(
      [metrics.tasks, metrics.questionsOnBoard, metrics.chiefTurns, metrics.chiefLastWords],
      [[], 0, 3, 'Done: the page is in place.'],
    )
    assert.deepEqual([metrics.ownerQuestions.questions, metrics.ownerQuestions.turnsAsking], [2, 1])
    assert.deepEqual(metrics.ownerMessages, [12, 17], 'what the owner typed')
    assert.deepEqual(metrics.filesChanged, ['site/legislatie.html'])
  })

  it('names the turn end the owner should answer, only when it asks something', () => {
    assert.equal(askingTurnEnd(items.slice(0, 4))?.id, 'a2')
    assert.equal(askingTurnEnd(items), undefined, 'its last turn asked nothing')
    assert.equal(
      askingTurnEnd(items.slice(0, 2)),
      undefined,
      'a question mid-turn is not put to the owner',
    )
  })
})

describe('a bare record that never marks the end of a turn (Devin, OpenCode)', () => {
  const items = [
    { id: 'u1', role: 'user', text: 'Add the page', complete: true },
    { id: 'a1', role: 'assistant', text: 'Keep the old document? Publish today?', complete: false },
    { id: 'u2', role: 'user', text: 'Keep it. Not yet.', complete: true },
    {
      id: 'a2',
      role: 'assistant',
      text: 'Pages done. Waiting for your feedback.',
      complete: false,
    },
  ]

  it('takes the message the window rested on as the end of that turn', () => {
    assert.equal(
      askingTurnEnd(items.slice(0, 2)),
      undefined,
      'nothing marked, nothing seen at rest',
    )
    assert.equal(askingTurnEnd(items.slice(0, 2), new Set(['a1']))?.id, 'a1')
    const metrics = bareMetrics(items, { ends: new Set(['a1', 'a2']) })
    assert.deepEqual([metrics.ownerQuestions.questions, metrics.ownerQuestions.turnsAsking], [2, 1])
  })
})
