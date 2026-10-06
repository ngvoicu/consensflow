import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, sep } from 'node:path'
import { before, describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { check } from './goldens/daemon/check.mjs'
import { posixOnly, refreshed } from './goldens/daemon/document.mjs'
import { filled, referred } from './goldens/daemon/files.mjs'
import { mask, validate } from './goldens/daemon/mask.mjs'
import { Wire } from './goldens/daemon/wire.mjs'

/**
 * The daemon recorder (`tests/goldens/daemon/`): a small program that reaches
 * every surface runs with the recorder in, and the traces it leaves are held
 * to what the program did: an exchange to what the client sent and the API
 * answered, an operation to what the page asked and the stand-ins were asked,
 * a run to what `cf` was given and printed. Recording it twice leaves the
 * same bytes.
 */
const HOOKS = new URL('./goldens/daemon/hooks.mjs', import.meta.url).href
const PROGRAM = fileURLToPath(new URL('./goldens/daemon/fixtures/surfaces.mjs', import.meta.url))

/** The program run with the recorder in: the text of each trace it left, by name. */
function record() {
  const out = mkdtempSync(join(tmpdir(), 'cf-recorder-test-'))
  try {
    const env = {
      ...process.env,
      CF_DAEMON_TRACES: out,
      // The program's `cf` is Node, and the program is the one module that runs it.
      CF_DAEMON_CF: process.execPath,
      CF_DAEMON_SPAWNERS: JSON.stringify([pathToFileURL(PROGRAM).href]),
    }
    delete env.CF_LEDGER_TRACES
    // This run is a test runner of its own, not a child of the one that runs this file.
    delete env.NODE_TEST_CONTEXT
    const ran = spawnSync(
      process.execPath,
      ['--import', HOOKS, '--test', '--test-concurrency=1', PROGRAM],
      {
        env,
        encoding: 'utf8',
      },
    )
    assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
    return Object.fromEntries(
      readdirSync(out)
        .sort()
        .map((name) => [name.replace(/\.json$/, ''), readFileSync(join(out, name), 'utf8')]),
    )
  } finally {
    rmSync(out, { recursive: true, force: true })
  }
}

const steps = (trace, kind) => trace.steps.filter((step) => step.kind === kind)

describe('the daemon recorder', () => {
  let texts
  let traces
  before(() => {
    texts = record()
    traces = Object.fromEntries(
      Object.entries(texts).map(([name, text]) => [name, JSON.parse(text)]),
    )
  })

  it('leaves one trace for each test that reached a surface, named for its suite and numbered', () => {
    assert.deepEqual(Object.keys(traces), [
      'surfaces-001',
      'surfaces-002',
      'surfaces-003',
      'surfaces-004',
      'surfaces-005',
      'surfaces-006',
    ])
    assert.deepEqual(
      Object.values(traces).map((trace) => trace.surface),
      ['api', 'api', 'cf', 'page', 'screens', 'trace'],
    )
    const [first] = Object.values(traces)
    assert.deepEqual(first.test.path, ['a program that reaches every surface', 'exchanges'])
    assert.equal(first.test.file, 'surfaces.mjs')
  })

  it('holds an exchange as the client sent it and the API answered it, a body the API never read included', () => {
    const [created, refused, staff, nobody, revoked] = steps(traces['surfaces-001'], 'exchange')
    assert.deepEqual(created.request, {
      method: 'POST',
      target: '/api/tasks',
      authorization: 'Bearer «token:T1»',
      contentType: 'application/json',
      body: '{"tier":"standard","body":"Parser"}',
    })
    assert.equal(created.response.status, 201)
    assert.equal(created.response.contentType, 'application/json')
    assert.equal(JSON.parse(created.response.body).task.title, 'Parser')
    assert.equal(created.client, 'test')
    // The API refuses a member before it reads a body, and the trace has the body that was sent.
    assert.equal(refused.request.body, 'not json at all')
    assert.deepEqual([refused.response.status, refused.kicks], [403, 0])
    assert.equal(JSON.parse(refused.response.body).error, 'not-a-coordinator')
    assert.equal(nobody.request.authorization, 'Bearer nope')
    assert.equal(revoked.request.authorization, 'Bearer «token:T2»')
    assert.equal(revoked.response.status, 401)
    assert.equal(staff.request.body, null)
    assert.equal(staff.request.method, 'GET')
  })

  it('counts the kicks of an exchange, and folds what its own ledger calls read and logged into it', () => {
    const [created] = steps(traces['surfaces-001'], 'exchange')
    assert.equal(created.kicks, 1)
    assert.equal(created.clock.length, 2)
    assert.deepEqual(created.names, [])
    assert.deepEqual(
      created.events.map((event) => [event.kind, event.project]),
      [['task.opened', 1]],
    )
  })

  it('records the calls the API makes on the test’s stand-ins, with their arguments and answers', () => {
    const staff = steps(traces['surfaces-001'], 'exchange').find(
      (s) => s.request.target === '/api/staff',
    )
    assert.deepEqual(staff.seams, [
      {
        seam: 'roster',
        method: 'call',
        args: ['zeus'],
        calls: [],
        result: { model: 'm', effort: 'high' },
      },
    ])
  })

  it('names each token by the window it was issued for, and revoked tokens by that name', () => {
    const { steps: all } = traces['surfaces-001']
    assert.deepEqual(
      steps(traces['surfaces-001'], 'issue').map((step) => [
        step.token,
        step.project,
        step.participant.handle,
      ]),
      [
        ['T1', 1, 'chief'],
        ['T2', 1, 'zeus'],
      ],
    )
    assert.deepEqual(steps(traces['surfaces-001'], 'revoke'), [{ kind: 'revoke', token: 'T2' }])
    assert.ok(all.findIndex((s) => s.kind === 'revoke') > all.findIndex((s) => s.kind === 'issue'))
    for (const text of Object.values(texts)) assert.doesNotMatch(text, /[0-9a-f]{64}/)
  })

  it('keeps the calls a test makes on the ledger that change something, and leaves the API’s own and the reads out', () => {
    const { steps: all, ledger } = traces['surfaces-001']
    assert.deepEqual(
      steps(traces['surfaces-001'], 'ledger').map((step) => step.method),
      [
        'createProject',
        'addMember',
        'note',
        'createProject',
        'setProjectState',
        'deleteProject',
        'close',
      ],
    )
    const note = steps(traces['surfaces-001'], 'ledger').find((step) => step.method === 'note')
    assert.equal(note.clock.length, 2)
    assert.equal(note.events[0].kind, 'message.sent')
    assert.equal(all.at(-1).method, 'close')
    assert.equal(ledger.file, '«ledger»')
    assert.equal(ledger.final.tables.task.rows.length, 1, 'the database the API and the test left')
    assert.deepEqual(ledger.options, { now: false, names: false, trace: false })
  })

  it('gives the ledger a clock of its own that starts at one instant and reads a second later each time', () => {
    const [createProject] = steps(traces['surfaces-001'], 'ledger')
    assert.deepEqual(createProject.clock.slice(0, 2), [
      '2026-01-01T00:00:00.000Z',
      '2026-01-01T00:00:01.000Z',
    ])
  })

  it('marks an exchange that other steps overlapped, and puts its end where it came', () => {
    const { steps: all } = traces['surfaces-002']
    const poll = all.find((step) => step.kind === 'exchange')
    const close = all.find((step) => step.kind === 'api.close')
    assert.equal(poll.detached, true)
    assert.equal(close.detached, true)
    assert.equal(poll.request.target.split('?')[1], 'wait=25000')
    assert.equal(JSON.parse(poll.response.body).answer, null)
    const settled = all.filter((step) => step.kind === 'settle')
    assert.deepEqual(settled, [
      { kind: 'settle', exchange: poll.id },
      { kind: 'settle', close: close.id },
    ])
    assert.ok(
      all.indexOf(poll) < all.indexOf(close) && all.indexOf(close) < all.indexOf(settled[0]),
    )
  })

  it('holds a run of cf whole, with the environment it was given beyond the rig’s, and its exchanges as the run’s', () => {
    const { steps: all } = traces['surfaces-003']
    const [run] = steps(traces['surfaces-003'], 'run')
    assert.equal(run.argv[0], '-e')
    assert.equal(run.argv.at(-1), 'whoami')
    assert.deepEqual(run.env, {
      CONSENSFLOW_URL: 'http://«api»',
      CONSENSFLOW_TOKEN: '«token:T1»',
      EXTRA: 'yes',
    })
    assert.deepEqual(
      [run.stdin, run.stdout, run.stderr, run.code],
      ['typed in\n', 'said typed in\n 200', 'oops', 3],
    )
    const [made] = steps(traces['surfaces-003'], 'exchange')
    assert.deepEqual([made.client, made.run, made.request.target], ['cf', run.id, '/api/whoami'])
    assert.equal(
      run.detached,
      undefined,
      'nothing but its own exchange came between its start and end',
    )
    assert.ok(all.indexOf(run) < all.indexOf(made))
  })

  it('holds a page operation by its name, its body, the reply the bridge would carry, its kicks and its stand-ins’ calls', () => {
    const [world, open, board, close, list] = traces['surfaces-004'].steps
    assert.equal(world.kind, 'world')
    assert.deepEqual(Object.keys(world.files), ['consensflow/agents.json'])
    // The rest of a path after its folder's name is the platform's spelling.
    assert.deepEqual(world.env, { HOME: '«root»', CONSENSFLOW_HOME: `«root»${sep}consensflow` })
    assert.deepEqual(
      [open.name, open.body, open.kicks],
      ['project.open', { directory: '/work/app', agent: 'leto' }, 1],
    )
    assert.match(open.reply, /^\{"ok":true,"project":\{"id":1,"directory":"\/work\/app"/)
    const [dispatch] = open.seams
    assert.deepEqual([dispatch.seam, dispatch.method], ['dispatcher', 'openProject'])
    assert.deepEqual(
      dispatch.args[0].gate,
      { $undefined: true },
      'a key the test left undefined is written as one',
    )
    assert.deepEqual(
      dispatch.calls.map((call) => call.method),
      ['createProject'],
    )
    assert.equal(
      dispatch.calls[0].clock.length,
      4,
      'what the stand-in’s own ledger call read is kept with it',
    )
    assert.deepEqual(open.clock, [], 'the operation itself made no ledger call that read the clock')
    assert.equal(board.seams.length, 10, 'five projections for each of two lanes')
    assert.deepEqual(
      board.seams.slice(0, 5).map((s) => s.method),
      ['activity', 'pane', 'hidden', 'pendingSwitch', 'holding'],
    )
    assert.deepEqual([close.kicks, close.refusal.message], [0, 'the window would not close'])
    assert.equal(close.reply, '{"ok":false,"error":"the window would not close"}')
    assert.equal(list.kicks, 0)
  })

  it('treats a ledger the recorder did not open, and a file the test rewrote, as the test’s', () => {
    const { steps: all } = traces['surfaces-004']
    const staffed = all.find((step) => step.name === 'staff.last')
    assert.deepEqual(staffed.seams, [
      { seam: 'ledger', method: 'lastStaff', args: [], calls: [], result: [] },
    ])
    const rewritten = all.filter((step) => step.kind === 'world')[1]
    assert.deepEqual(rewritten.env, undefined)
    assert.deepEqual(JSON.parse(rewritten.files['consensflow/agents.json'].text).agents, [])
  })

  it('holds the screens with the UI token, the files before each exchange, and what a write left, its stamps named', () => {
    const trace = traces['surfaces-005']
    assert.deepEqual(trace.ui, { token: 'the-ui-token' })
    const [world, page, harnesses, added, edited, removed, whoami] = trace.steps
    assert.equal(
      world.files['bin/claude'].executable,
      process.platform === 'win32' ? undefined : true,
    )
    assert.equal(world.files['consensflow/agents.json'], undefined)
    for (const exchange of [page, harnesses]) {
      assert.equal(exchange.response.contentType, 'text/html; charset=utf-8')
      assert.match(exchange.response.body, /^<!DOCTYPE html>/)
      assert.equal(exchange.screens, true)
    }
    const roster = 'consensflow/agents.json'
    assert.equal(added.wrote[roster].before, null)
    assert.match(added.wrote[roster].after.text, /"createdAt": "«now»",\n {6}"updatedAt": "«now»"/)
    assert.equal(JSON.parse(edited.wrote[roster].before.text).agents[0].model, 'm')
    assert.equal(JSON.parse(edited.wrote[roster].after.text).agents[0].model, 'n')
    assert.deepEqual(
      [removed.response.status, JSON.parse(removed.response.body)],
      [400, { error: 'no agent named none' }],
    )
    assert.equal(removed.wrote, undefined)
    assert.equal(whoami.screens, undefined, 'a path the screens do not own is the API’s answer')
    assert.equal(whoami.response.status, 401)
  })

  it('holds the lines a trace and a log write, a line they dated themselves with its time named and the rest as they are', () => {
    const lines = traces['surfaces-006'].steps
    const [open, a, b, c, d, forgot] = lines
    assert.deepEqual([open.folder, open.limit], ['.', 150])
    assert.equal(a.files['events.jsonl'], '{"at":"«now»","kind":"a","project":1,"data":{}}\n')
    assert.match(b.files['events.jsonl'], /\n\{"at":"2026-10-05T10:00:00.000Z","kind":"b"/)
    // Past 150 bytes the file is set aside as .1, and kept once.
    assert.ok([c, d].some((step) => 'events.jsonl.1' in step.files))
    assert.ok(
      Object.keys(d.files).every((name) => /^(events\.jsonl(\.1)?|daemon\.log)$/.test(name)),
    )
    const survivors = Object.values(forgot.files).join('')
    assert.doesNotMatch(survivors, /"project":1/)
    assert.match(survivors, /"kind":"b"/)
    const written = lines.filter((step) => step.kind === 'log.write')
    assert.deepEqual(
      written.map((step) => [step.level, step.clock.length]),
      [
        ['info', 1],
        ['error', 1],
        ['warn', 1],
        ['warn', 1],
        ['info', 0],
      ],
    )
    assert.deepEqual(written[1].error, { text: 'Error: boom\n    at «frame»\n    at «frame»' })
    assert.match(written[0].files['daemon.log'], /^2026-10-05T12:00:00.000Z info start\n/)
    assert.match(written[4].files['daemon.log'], /\n«now» info no clock of its own\n$/)
    assert.ok('daemon.log.1' in written[3].files, 'rotated at 100 bytes')
  })

  it('leaves the same bytes when it records again', () => {
    assert.deepEqual(record(), texts)
  })
})

describe('what the recorder names and refuses', () => {
  const recording = (over = {}) => ({
    ledgers: [{ record: { file: '/tmp/x/consensflow.db' } }],
    roots: new Set(['/tmp/x']),
    apis: ['http://127.0.0.1:4242'],
    tokens: new Map([['a'.repeat(32) + 'b'.repeat(32), 'T1']]),
    ...over,
  })

  it('names the ledger’s file, the test’s folder, the API’s address and each token, the longest path first', () => {
    const text = JSON.stringify({
      db: '/tmp/x/consensflow.db',
      dir: '/tmp/x/consensflow',
      url: 'http://127.0.0.1:4242/api',
      authorization: `Bearer ${'a'.repeat(32)}${'b'.repeat(32)}`,
    })
    assert.deepEqual(JSON.parse(mask(text, recording())), {
      db: '«ledger»',
      dir: '«root»/consensflow',
      url: 'http://«api»/api',
      authorization: 'Bearer «token:T1»',
    })
  })

  it('refuses a trace that still holds a temporary path or a token no issue gave, and not padding that looks like one', () => {
    assert.throws(
      () => validate(JSON.stringify({ path: `${tmpdir()}/cf-x/agents.json` })),
      /temporary path/,
    )
    assert.throws(() => validate(`"${'0123456789abcdef'.repeat(4)}"`), /token no `issue` gave/)
    assert.doesNotThrow(() => validate(`"${'a'.repeat(200)}"`))
    assert.doesNotThrow(() => validate('"«token:T1» «root»/consensflow «api»"'))
  })

  it('reads the body of each request off the bytes a connection carried, however they came and whatever read them', () => {
    const wire = new Wire()
    const request = (body, head = '') =>
      `POST /a HTTP/1.1\r\nhost: x\r\n${head}content-length: ${Buffer.byteLength(body)}\r\n\r\n${body}`
    const bytes = Buffer.from(
      `${request('{"a":1}')}GET /b HTTP/1.1\r\nhost: x\r\n\r\n${request('é')}` +
        `POST /c HTTP/1.1\r\ntransfer-encoding: chunked\r\n\r\n3\r\nabc\r\n2;ext=1\r\nde\r\n0\r\n\r\n`,
    )
    // Fed a byte at a time: where a packet ends changes nothing.
    for (const byte of bytes) wire.push(Buffer.from([byte]))
    const read = wire.bodies.map((body) => Buffer.concat(body.chunks).toString('utf8'))
    assert.deepEqual(read, ['{"a":1}', '', 'é', 'abcde'])
    assert.ok(wire.bodies.every((body) => body.whole))
  })

  it('marks a body that stopped short with the length the client declared', () => {
    const wire = new Wire()
    wire.push(Buffer.from('POST /a HTTP/1.1\r\ncontent-length: 10\r\n\r\n12345'))
    assert.equal(wire.bodies.length, 0)
    wire.end()
    assert.deepEqual(
      wire.bodies.map((body) => [Buffer.concat(body.chunks).toString(), body.declared, body.whole]),
      [['12345', 10, false]],
    )
  })

  it('names a page a trace served, and refuses a page that is not the one Node makes', () => {
    const pages = { agents: 'A $TOKEN $VERSION', harnesses: 'H $TOKEN' }
    const trace = (target, body) => ({
      ui: { token: 'tok' },
      steps: [
        {
          kind: 'exchange',
          request: { target },
          response: { status: 200, contentType: 'text/html; charset=utf-8', body },
        },
      ],
    })
    const [{ response }] = referred(trace('/?token=tok', filled(pages.agents, 'tok')), pages).steps
    assert.deepEqual(response, {
      status: 200,
      contentType: 'text/html; charset=utf-8',
      page: 'agents',
    })
    assert.throws(() => referred(trace('/harnesses', 'H other'), pages), /not the one Node makes/)
    assert.throws(() => referred(trace('/elsewhere', 'x'), pages), /does not know/)
  })
})

describe('what a trace must be', () => {
  const trace = (steps, over = {}) => ({
    format: 1,
    surface: 'api',
    test: { file: 'x.mjs', path: ['a test'] },
    ledger: null,
    steps,
    ...over,
  })
  const exchange = (id, over = {}) => ({
    kind: 'exchange',
    id,
    client: 'test',
    request: {
      method: 'GET',
      target: '/api/whoami',
      authorization: null,
      contentType: null,
      body: null,
    },
    response: { status: 200, contentType: 'application/json', body: '{}' },
    kicks: 0,
    clock: [],
    names: [],
    events: [],
    seams: [],
    ...over,
  })
  const issue = (token) => ({
    kind: 'issue',
    token,
    project: 1,
    participant: { id: 2, handle: 'chief' },
  })
  const operation = (over = {}) => ({
    kind: 'operation',
    id: 1,
    name: 'board.get',
    body: {},
    kicks: 0,
    clock: [],
    names: [],
    events: [],
    seams: [],
    reply: '{"ok":true}',
    ...over,
  })

  it('is accepted whole, a token used after it was issued, an interval that was overlapped settled', () => {
    check(
      'whole',
      trace([
        issue('T1'),
        exchange(1, {
          request: {
            method: 'GET',
            target: '/',
            authorization: 'Bearer «token:T1»',
            contentType: null,
            body: null,
          },
          detached: true,
        }),
        exchange(2),
        { kind: 'settle', exchange: 1 },
      ]),
    )
  })

  it('refuses what a player could not rely on, and says which trace and which step', () => {
    const refused = [
      [trace([exchange(1)], { format: 2 }), /format 2/],
      [trace([exchange(1)], { surface: 'elsewhere' }), /surface elsewhere/],
      [trace([]), /no steps/],
      [trace([exchange(1)], { test: { file: 'x.mjs', path: ['a test'], line: 3 } }), /a test line/],
      [trace([{ kind: 'mystery' }]), /step 0 \(mystery\) is of a kind nobody knows/],
      [
        trace([
          exchange(1, {
            request: {
              method: 'GET',
              target: '/',
              authorization: 'Bearer «token:T3»',
              contentType: null,
              body: null,
            },
          }),
        ]),
        /uses T3, which no step issued before it/,
      ],
      [trace([issue('T1'), issue('T1')]), /T1 is issued twice/],
      [trace([{ kind: 'revoke', token: 'T9' }]), /revokes T9, which was never issued/],
      [trace([exchange(1, { detached: true })]), /exchange 1 never settled/],
      [trace([{ kind: 'settle', exchange: 4 }]), /ends exchange 4, which is not waiting/],
      [trace([exchange(1, { response: null })]), /has no answer and is not marked aborted/],
      [
        trace([
          exchange(1, {
            response: { status: 200, contentType: 'text/html', page: 'agents', body: 'x' },
          }),
        ]),
        /holds a page twice/,
      ],
      [
        trace([
          operation({
            clock: ['2026-01-01T00:00:00.000Z'],
            seams: [
              {
                seam: 'dispatcher',
                method: 'm',
                args: [],
                calls: [{ clock: ['2026-01-01T00:00:01.000Z'], names: [], events: [] }],
              },
            ],
          }),
        ]),
        /reads the clock itself and through a stand-in/,
      ],
      [trace([{ kind: 'world', files: {} }]), /first world of a trace is whole/],
      [
        trace([exchange(1, { response: { status: 200, contentType: null, body: tmpdir() } })]),
        /temporary path/,
      ],
      [
        trace([exchange(1)], { ledger: { file: '«ledger»', options: {}, final: null } }),
        /no database at the close/,
      ],
      [
        trace([exchange(1)], { ledger: { file: '«ledger»', options: {}, final: { tables: {} } } }),
        /does not end with the close that compares it/,
      ],
      [
        trace([exchange(1), { kind: 'ledger', method: 'close' }, exchange(2)], {
          ledger: { file: '«ledger»', options: {}, final: { tables: {} } },
        }),
        /does not end with the close that compares it/,
      ],
    ]
    for (const [bad, why] of refused) assert.throws(() => check('bad', bad), why)
  })

  it('refuses a name of what varies where no player puts it, and says where it stood', () => {
    const named = (over) => trace([exchange(1, over)])
    const refused = [
      [
        named({ response: { status: 200, contentType: null, body: '«ledger»' } }),
        /«ledger» is in steps\.#\.response\.body, where no player puts it/,
      ],
      [
        named({
          request: {
            method: 'GET',
            target: '/',
            authorization: null,
            contentType: null,
            body: '«now»',
          },
        }),
        /«now» is in steps\.#\.request\.body/,
      ],
      [
        named({
          request: {
            method: 'GET',
            target: '/«api»',
            authorization: null,
            contentType: null,
            body: null,
          },
        }),
        /«api» is in steps\.#\.request\.target/,
      ],
      [
        trace([{ kind: 'world', env: { '«root»': 'x' }, files: {} }]),
        /«root» is in steps\.#\.env \(a key\)/,
      ],
      [
        trace([{ kind: 'world', env: { HOME: '«frame»' }, files: {} }]),
        /«frame» is in steps\.#\.env\.\*, where no player puts it/,
      ],
      [
        trace([exchange(1)], {
          ledger: { file: '«root»', options: {}, final: null, unclosed: true },
        }),
        /«root» is in ledger\.file/,
      ],
    ]
    for (const [bad, why] of refused) assert.throws(() => check('bad', bad), why)
    // Where a player puts each: a file's text is where the clock's stamps are, a run's
    // environment is where the API's address and a window's token are.
    check(
      'places',
      trace([
        issue('T1'),
        {
          kind: 'world',
          env: { HOME: '«root»/home', URL: 'http://«api»', T: '«token:T1»' },
          files: {
            'agents.json': { text: '"createdAt": "«now»"' },
          },
        },
        { kind: 'world', files: { 'bin/x': null } },
      ]),
    )
  })
})

describe('the scenarios', () => {
  it('hold on Node as it is, without the recorder: they are tests of Node before they are recordings of it', () => {
    const env = { ...process.env }
    delete env.NODE_TEST_CONTEXT
    const dir = fileURLToPath(new URL('./goldens/daemon/scenarios/', import.meta.url))
    const files = readdirSync(dir)
      .filter((name) => name.endsWith('.test.mjs'))
      .map((name) => join(dir, name))
    assert.ok(files.length >= 3)
    const ran = spawnSync(process.execPath, ['--test', ...files], { env, encoding: 'utf8' })
    assert.equal(ran.status, 0, ran.stdout)
  })
})

describe('the runner', () => {
  const RECORD = fileURLToPath(new URL('./goldens/daemon/record.mjs', import.meta.url))

  it('records a suite into the folder it is given, records it again the same, and says where a file differs', () => {
    const out = mkdtempSync(join(tmpdir(), 'cf-recorder-out-'))
    try {
      const env = { ...process.env }
      delete env.NODE_TEST_CONTEXT
      delete env.CF_LEDGER_TRACES
      const run = (...args) =>
        spawnSync(process.execPath, [RECORD, '--only', 'core-trace', '--to', out, ...args], {
          env,
          encoding: 'utf8',
        })
      const made = run()
      assert.equal(made.status, 0, made.stderr)
      assert.deepEqual(readdirSync(out).sort(), [
        'core-trace-001.json.gz',
        'core-trace-002.json.gz',
        'daemon.json',
        'files.json',
        'operations.json',
        'pages',
      ])
      const same = run('--check')
      assert.equal(same.status, 0, same.stdout)
      assert.match(same.stdout, /7 files recorded, 0 differ/)
      writeFileSync(join(out, 'operations.json'), '{}')
      const differs = run('--check')
      assert.equal(differs.status, 1)
      assert.match(differs.stdout, /operations\.json: differs/)
    } finally {
      rmSync(out, { recursive: true, force: true })
    }
  })
})

describe('the document the recorder rewrites', () => {
  const world = (names) => ({
    kind: 'world',
    env: {},
    files: Object.fromEntries(names.map((name) => [name, { text: '' }])),
  })
  const files = {
    'a-001.json.gz': JSON.stringify({
      steps: [world(['bin/claude', 'bin/pi', 'bin/pi.cmd']), { kind: 'exchange', id: 1 }],
    }),
    'b-001.json.gz': JSON.stringify({
      steps: [world(['bin/pi', 'bin/pi.cmd', 'consensflow/agents.json'])],
    }),
    'c-001.json.gz': JSON.stringify({ steps: [{ kind: 'exchange', id: 1 }] }),
    'notes.json': '{"a":1}\n',
  }
  const fence = '```'
  const block = (marker, kind, text) => [marker, `${fence}${kind}`, text, fence].join('\n')
  const document = [
    'prose before',
    block('<!-- example: a-001 steps.1 -->', 'json', '{"stale":true}'),
    'prose between',
    block('<!-- file: notes.json -->', 'json', '{}'),
    block('<!-- list: posix -->', 'text', 'c-001'),
    'prose after',
  ].join('\n')

  it('puts each example as the step of the trace it names, each file as the file, and each list as what it lists, and leaves the prose', () => {
    const written = refreshed(document, files)
    assert.equal(
      written,
      [
        'prose before',
        block('<!-- example: a-001 steps.1 -->', 'json', '{\n  "kind": "exchange",\n  "id": 1\n}'),
        'prose between',
        block('<!-- file: notes.json -->', 'json', '{"a":1}'),
        block('<!-- list: posix -->', 'text', 'a-001'),
        'prose after',
      ].join('\n'),
    )
    assert.equal(refreshed(written, files), written, 'a document that is current is left as it is')
  })

  it('lists a trace by a script named for a harness that no .cmd stands beside, and no other', () => {
    assert.deepEqual(posixOnly(files), ['a-001'])
  })

  it('fills a block that was left empty', () => {
    const empty = ['<!-- list: posix -->', '```text', '```'].join('\n')
    assert.equal(refreshed(empty, files), block('<!-- list: posix -->', 'text', 'a-001'))
  })

  it('refuses a document that names a trace, a step or a file there is none of', () => {
    const shown = (marker) => block(marker, 'json', '{}')
    assert.throws(
      () => refreshed(shown('<!-- example: z-001 steps.0 -->'), files),
      /z-001, which is no trace/,
    )
    assert.throws(
      () => refreshed(shown('<!-- example: a-001 steps.9 -->'), files),
      /steps\.9, which is not there/,
    )
    assert.throws(
      () => refreshed(shown('<!-- file: nowhere.json -->'), files),
      /nowhere\.json, which is no file/,
    )
  })
})
