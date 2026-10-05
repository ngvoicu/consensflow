import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { gunzipSync } from 'node:zlib'
import { check } from './goldens/daemon/check.mjs'
import { posixOnly, refreshed } from './goldens/daemon/document.mjs'
import { dataFiles } from './goldens/daemon/files.mjs'
import { show } from './goldens/daemon/show.mjs'

/**
 * What Node answers on every surface of the daemon, as checked in for the Rust
 * players (`crates/cf-daemon/tests/goldens/`, made by `npm run goldens:daemon`):
 * the files that are pages and names are what Node makes now, every trace is
 * whole, the traces reach every route and operation, and the examples in
 * `tests/goldens/daemon/FORMAT.md` are steps of them.
 */
const GOLDENS =
  process.env.CF_DAEMON_GOLDENS ??
  fileURLToPath(new URL('../crates/cf-daemon/tests/goldens', import.meta.url))
const FORMAT = new URL('./goldens/daemon/FORMAT.md', import.meta.url)

const names = () =>
  readdirSync(GOLDENS)
    .filter((name) => name.endsWith('.json.gz'))
    .map((name) => name.replace(/\.json\.gz$/, ''))
    .sort()
const load = (name) => JSON.parse(gunzipSync(readFileSync(join(GOLDENS, `${name}.json.gz`))))

describe('the goldens of the daemon, as checked in', () => {
  it('holds the pages, the operations and the handle line to what Node makes now: npm run goldens:daemon after a change', async () => {
    const files = await dataFiles()
    for (const [name, text] of Object.entries(files)) {
      assert.equal(readFileSync(join(GOLDENS, ...name.split('/')), 'utf8'), text, name)
    }
  })

  it('has every trace whole: its steps of kinds the players know, its tokens issued, nothing left waiting or varying', () => {
    assert.ok(names().length > 100, 'the traces are there: npm run goldens:daemon')
    for (const name of names()) check(name, load(name))
  })

  it('has the traces the document says: each suite’s run of numbers, and no trace besides', () => {
    const document = readFileSync(FORMAT, 'utf8')
    const listed = new Set()
    for (const [, suite, first, last] of document.matchAll(/`([a-z-]+)-(\d{3})` to `(\d{3})`/g)) {
      for (let number = Number(first); number <= Number(last); number += 1) {
        listed.add(`${suite}-${String(number).padStart(3, '0')}`)
      }
    }
    for (const [, single] of document.matchAll(/\| `([a-z-]+-\d{3})` \|/g)) listed.add(single)
    assert.deepEqual(names(), [...listed].sort())
    // A trace the document points at (in "Where to look") is a trace there is.
    const known = new Set(names())
    for (const [, mentioned] of document.matchAll(/`((?:core|corners|cf)-[a-z-]+-\d{3})`/g)) {
      assert.ok(known.has(mentioned), `${mentioned} is in the document and not in the folder`)
    }
  })

  it('names the traces’ surfaces as the document says', () => {
    const surface = {
      'core-api': 'api',
      'core-daemon': 'api',
      'cf-board': 'cf',
      'corners-api': 'api',
      'core-page': 'page',
      'corners-page': 'page',
      'core-agents-server': 'screens',
      'corners-screens': 'screens',
      'core-trace': 'trace',
      'core-log': 'log',
    }
    for (const name of names()) {
      assert.equal(load(name).surface, surface[name.replace(/-\d{3}$/, '')], name)
    }
  })

  it('reaches every route of the agents’ API with an answer, and each way it refuses', () => {
    const reached = new Set()
    for (const name of names()) {
      for (const step of load(name).steps) {
        if (step.kind !== 'exchange' || step.screens || step.response === null) continue
        const route = step.request.target
          .split('?')[0]
          .replace(/^\/api\/tasks\/\d+\/(\w+)$/, '/api/tasks/:n/$1')
          .replace(/^\/api\/tasks\/\d+$/, '/api/tasks/:n')
          .replace(/^\/api\/(inbox|questions)\/\d+$/, '/api/$1/:id')
        reached.add(`${step.request.method} ${route} ${step.response.status}`)
        const error = step.response.body?.match(/^\{"error":"([^"]+)"/)?.[1]
        if (error !== undefined) reached.add(`${step.response.status} ${error}`)
      }
    }
    const wanted = [
      'GET /api/whoami 200',
      'GET /api/history 200',
      'GET /api/staff 200',
      'GET /api/tasks 200',
      'POST /api/tasks 201',
      'GET /api/tasks/:n 200',
      'GET /api/tasks/:n/transcript 200',
      ...['done', 'accept', 'cancel', 'pause', 'resume', 'reopen', 'tell'].map(
        (action) => `POST /api/tasks/:n/${action} 200`,
      ),
      'GET /api/inbox 200',
      'GET /api/inbox/:id 200',
      'POST /api/questions 201',
      'GET /api/questions/:id 200',
      'POST /api/notes 201',
      'POST /api/answers 201',
      ...['unauthorized', 'unknown-route', 'unknown-task', 'unknown-message', 'invalid-json'].map(
        (error) =>
          `${{ unauthorized: 401, 'unknown-route': 404, 'unknown-task': 404, 'unknown-message': 404, 'invalid-json': 400 }[error]} ${error}`,
      ),
      '403 not-a-coordinator',
      '403 not-the-chief',
      '403 ask-in-your-terminal',
      '403 not-your-question',
      '409 task-cancelled',
      '409 no-window',
      '413 too-large',
    ]
    assert.deepEqual(
      wanted.filter((key) => !reached.has(key)),
      [],
      'the traces do not reach these',
    )
  })

  it('reaches every route of the screens, and every page operation, and every line format', () => {
    const screens = new Set()
    const operations = new Set()
    const files = new Set()
    let refusedWith = null
    for (const name of names()) {
      for (const step of load(name).steps) {
        if (step.kind === 'exchange' && step.screens && step.response !== null) {
          const route = step.request.target
            .split('?')[0]
            .replace(/^\/api\/agents\/[^/]+$/, '/api/agents/:name')
          screens.add(`${step.request.method} ${route} ${step.response.status}`)
        }
        if (step.kind === 'operation') operations.add(step.name)
        if (step.kind === 'operation' && step.refusal !== undefined) refusedWith = step.refusal
        for (const file of Object.keys(step.files ?? {})) files.add(`${step.kind} ${file}`)
      }
    }
    assert.deepEqual(
      [
        'GET / 200',
        'GET /harnesses 200',
        'GET /api/agents 200',
        'POST /api/agents 201',
        'PATCH /api/agents/:name 200',
        'DELETE /api/agents/:name 204',
        'POST /api/preferences 200',
        'POST /api/harnesses/check 400',
        'POST /api/harnesses/update 400',
        'POST / 404',
      ].filter((key) => !screens.has(key)),
      [],
      'the traces do not reach these routes of the screens',
    )
    const listed = JSON.parse(readFileSync(join(GOLDENS, 'operations.json'), 'utf8')).operations
    assert.deepEqual(
      listed.filter((name) => !operations.has(name)),
      [],
      'every page operation is asked at least once',
    )
    assert.ok(refusedWith !== null)
    assert.deepEqual(
      [
        'trace.append events.jsonl',
        'trace.append events.jsonl.1',
        'trace.forget events.jsonl',
        'log.write daemon.log',
        'log.write daemon.log.1',
      ].filter((key) => !files.has(key)),
      [],
    )
  })

  it('has the document as the recorder writes it: its examples steps of the traces, the files it shows the files, its list of what is a POSIX recording', () => {
    const document = readFileSync(FORMAT, 'utf8')
    assert.ok(
      [...document.matchAll(/<!-- example: /g)].length >= 20,
      'the document has its worked examples',
    )
    const files = Object.fromEntries(
      names().map((name) => [
        `${name}.json.gz`,
        gunzipSync(readFileSync(join(GOLDENS, `${name}.json.gz`))).toString('utf8'),
      ]),
    )
    for (const file of [
      'operations.json',
      'daemon.json',
      'pages/agents.html',
      'pages/harnesses.html',
    ]) {
      files[file] = readFileSync(join(GOLDENS, ...file.split('/')), 'utf8')
    }
    assert.equal(
      refreshed(document, files),
      document,
      'npm run goldens:daemon rewrites the blocks of the document',
    )
    assert.ok(
      posixOnly(files).includes('core-agents-server-001'),
      'the screens’ traces are POSIX recordings',
    )
    assert.ok(!posixOnly(files).includes('core-api-001'), 'and an API trace is not')
  })

  it('shows a trace a line a step', () => {
    const lines = show(load('cf-board-023'))
    assert.match(lines[0], /^cf: cf hook claude: .* › puts the questions on the board/)
    assert.ok(
      lines.some((line) =>
        /exchange 2 by run 1: GET \/api\/questions\/\d+\?wait=20000 .* → 200 \(detached\)/.test(
          line,
        ),
      ),
    )
    assert.ok(lines.some((line) => /settle exchange 2 of run 1/.test(line)))
    assert.equal(lines.length, load('cf-board-023').steps.length + 1)
    assert.ok(existsSync(join(GOLDENS, 'pages', 'agents.html')))
  })
})
