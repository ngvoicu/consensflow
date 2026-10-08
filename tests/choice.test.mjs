import assert from 'node:assert/strict'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import {
  assertStarted,
  chooseHome,
  NATIVE_CF,
  NODE_CF,
  noteRan,
  START_WORDS,
  startLine,
  WAY_BACK,
} from './choice.mjs'
import { cliEnv, cliTarget } from './cli-target.mjs'
import { daemonCommand } from './helpers.mjs'
import { CLI_LEGS, DAEMON_LEGS, legEnv, ranProblem, runLeg } from './legs.mjs'

const CHOICE = pathToFileURL(fileURLToPath(new URL('./choice.mjs', import.meta.url))).href
const OTHER = { node: 'native', native: 'node' }

/** A daemon's log, as each daemon writes its first line (src/core/daemon.js, crates/cf-daemon/src/start.rs). */
const NODE_LINE = '2026-10-06T10:00:00.000Z info start pid 4242 node v26.8.1 home /tmp/consensflow'
const RUST_LINE =
  '2026-10-06T10:00:00.000Z info start pid 4242 rust 3.0.0-alpha.79 home /tmp/consensflow'

/** A folder of this test's own, made and removed round `body`. */
function inAFolder(body) {
  const folder = mkdtempSync(join(tmpdir(), 'cf-choice-'))
  try {
    return body(folder)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
}

describe('which daemon a test starts, in words', () => {
  it('starts Node’s for `node`, whatever the default is: `cf ui` through the door', () => {
    for (const fallback of ['node', 'native']) {
      const started = daemonCommand({ named: 'node', leg: '', fallback })
      assert.equal(started.kind, 'node')
      assert.equal(started.command, process.execPath)
      assert.deepEqual(started.args, [NODE_CF, 'ui', '--json', '--no-open'])
      // The product chooses by the home, not by an environment variable.
      assert.deepEqual(started.env, { CONSENSFLOW_NODE: process.execPath })
    }
  })

  it('starts the native cf of this checkout for `native`, whatever the default is', () => {
    for (const fallback of ['node', 'native']) {
      const started = daemonCommand({ named: 'native', leg: '', fallback })
      assert.equal(started.kind, 'native')
      assert.equal(started.command, NATIVE_CF)
      assert.deepEqual(started.args, ['ui', '--json', '--no-open'])
      assert.deepEqual(started.env, { CONSENSFLOW_NODE: process.execPath })
    }
  })

  it('starts the command a JSON array gives as the native daemon', () => {
    const started = daemonCommand({
      named: '["/build/cf","ui","--json"]',
      leg: '',
      fallback: 'node',
    })
    assert.deepEqual(
      [started.kind, started.command, started.args],
      ['native', '/build/cf', ['ui', '--json']],
    )
    assert.equal(started.env.CONSENSFLOW_DAEMON, undefined)
  })

  it('makes the choice in the home it is given: the file for Node’s, none for the native one’s', () => {
    inAFolder((home) => {
      assert.equal(existsSync(join(home, WAY_BACK)), false, 'a home with no choice made')
      daemonCommand({ named: 'node', leg: '', fallback: 'native', home })
      assert.equal(existsSync(join(home, WAY_BACK)), true)
      // A restart of the same home on the other daemon takes the file away.
      daemonCommand({ named: 'native', leg: '', fallback: 'node', home })
      assert.equal(existsSync(join(home, WAY_BACK)), false)
      // The default is the native one's, and its home has no file either.
      chooseHome('node', home)
      daemonCommand({ named: undefined, leg: '', home })
      assert.equal(existsSync(join(home, WAY_BACK)), false)
    })
  })

  it('makes a home that is not there yet, for Node’s', () => {
    inAFolder((folder) => {
      const home = join(folder, 'not', 'yet')
      chooseHome('node', home)
      assert.equal(existsSync(join(home, WAY_BACK)), true)
      chooseHome('native', join(folder, 'never', 'made'))
      assert.equal(existsSync(join(folder, 'never')), false, 'the native one makes nothing')
    })
  })

  it('refuses a choice it does not know and a home that is none', () => {
    assert.throws(() => chooseHome('both', '/h'), /no implementation is called both/)
    assert.throws(() => chooseHome('node', ''), /a home to choose in/)
    assert.throws(() => chooseHome('native', undefined), /a home to choose in/)
  })

  it('starts the default for nothing, and the default is the tests’ to name', () => {
    for (const named of [undefined, '']) {
      assert.equal(daemonCommand({ named, leg: '', fallback: 'node' }).kind, 'node')
      assert.equal(daemonCommand({ named, leg: '', fallback: 'native' }).kind, 'native')
    }
  })

  it('refuses a selector that is none of the words', () => {
    for (const named of [
      'nope',
      'Node',
      '[]',
      '[1]',
      '["a",2]',
      '{"a":1}',
      'null',
      '[',
      'native ',
    ]) {
      assert.throws(
        () => daemonCommand({ named, leg: '', fallback: 'node' }),
        /CONSENSFLOW_TEST_DAEMON is node, native, or a JSON array of strings/,
        named,
      )
    }
  })

  it('refuses a selection that is not its own to a run that names its leg', () => {
    const asked =
      (named, leg, fallback = 'node') =>
      () =>
        daemonCommand({ named, leg, fallback })
    assert.throws(
      asked('native', 'node'),
      /says this is the node leg, but CONSENSFLOW_TEST_DAEMON names native/,
    )
    assert.throws(
      asked('node', 'native'),
      /says this is the native leg, but CONSENSFLOW_TEST_DAEMON names node/,
    )
    assert.throws(
      asked('["/build/cf"]', 'node'),
      /node leg, but CONSENSFLOW_TEST_DAEMON names native/,
    )
    assert.throws(
      asked('', 'node', 'native'),
      /says this is the node leg, but CONSENSFLOW_TEST_DAEMON is not set, and the default is native/,
    )
    assert.throws(
      asked(undefined, 'native', 'node'),
      /native leg, but CONSENSFLOW_TEST_DAEMON is not set, and the default is node/,
    )
    assert.throws(asked('node', 'both'), /CONSENSFLOW_TEST_LEG is node or native: both/)
    // The selection that is the leg's own, and a run with no leg to name.
    assert.equal(asked('node', 'node')().kind, 'node')
    assert.equal(asked('native', 'native')().kind, 'native')
    assert.equal(asked('native', '')().kind, 'native')
    assert.equal(asked('native', undefined)().kind, 'native')
  })
})

describe('the legs of the dual runners', () => {
  it('say who they are in words of their own, not in the selection’s', () => {
    assert.deepEqual(
      DAEMON_LEGS.map(({ leg }) => leg),
      ['node', 'native'],
    )
    assert.deepEqual(
      CLI_LEGS.map(({ leg }) => leg),
      ['node', 'native'],
    )
  })

  it('name their daemon, or their cf, whatever the tests default to', () => {
    for (const fallback of ['node', 'native']) {
      for (const { selector, leg } of DAEMON_LEGS) {
        assert.equal(daemonCommand({ named: selector, leg, fallback }).kind, leg)
      }
      for (const { selector, leg } of CLI_LEGS) {
        assert.equal(cliTarget({ named: selector, leg, fallback }).kind, leg)
      }
    }
  })

  it('are refused a leg whose selector is gone once the default is the other one', () => {
    for (const { leg } of DAEMON_LEGS) {
      for (const named of [undefined, '']) {
        assert.throws(
          () => daemonCommand({ named, leg, fallback: OTHER[leg] }),
          new RegExp(`says this is the ${leg} leg, but CONSENSFLOW_TEST_DAEMON is not set`),
        )
      }
    }
    for (const { leg } of CLI_LEGS) {
      for (const named of [undefined, '']) {
        assert.throws(
          () => cliTarget({ named, leg, fallback: OTHER[leg] }),
          new RegExp(`says this is the ${leg} leg, but CONSENSFLOW_TEST_CLI is not set`),
        )
      }
    }
  })

  it('give their suites the selection and the label, whatever the caller has set', () => {
    const caller = {
      PATH: '/bin',
      CONSENSFLOW_TEST_DAEMON: 'native',
      CONSENSFLOW_TEST_CLI: '["/else/cf"]',
      CONSENSFLOW_TEST_LEG: 'native',
    }
    const [node, native] = DAEMON_LEGS
    assert.deepEqual(legEnv('CONSENSFLOW_TEST_DAEMON', node, caller), {
      PATH: '/bin',
      CONSENSFLOW_TEST_DAEMON: 'node',
      CONSENSFLOW_TEST_CLI: '["/else/cf"]',
      CONSENSFLOW_TEST_LEG: 'node',
    })
    const [cliNode, cliNative] = CLI_LEGS
    assert.equal(legEnv('CONSENSFLOW_TEST_CLI', cliNode, caller).CONSENSFLOW_TEST_CLI, 'node')
    assert.equal(legEnv('CONSENSFLOW_TEST_CLI', cliNative, {}).CONSENSFLOW_TEST_LEG, 'native')
    assert.equal(legEnv('CONSENSFLOW_TEST_DAEMON', native, {}).CONSENSFLOW_TEST_DAEMON, 'native')
    assert.equal(
      caller.CONSENSFLOW_TEST_DAEMON,
      'native',
      'the caller’s environment is left as it was',
    )
  })
})

describe('which daemon started, from its log', () => {
  it('reads the daemon from the runtime its start line names', () => {
    const node = startLine(`${NODE_LINE}\n`, 4242)
    assert.deepEqual(
      [node.kind, node.runtime, node.pid, node.line],
      ['node', 'node v26.8.1', 4242, NODE_LINE],
    )
    const rust = startLine(`${RUST_LINE}\n`, 4242)
    assert.deepEqual(
      [rust.kind, rust.runtime, rust.pid, rust.line],
      ['native', 'rust 3.0.0-alpha.79', 4242, RUST_LINE],
    )
    assert.equal(START_WORDS.node, 'node v')
    assert.equal(START_WORDS.native, 'rust ')
  })

  it('reads the start line of the process it is asked about, the last one a log holds for it', () => {
    const log = [
      NODE_LINE,
      '2026-10-06T10:00:01.000Z info stop: stdin ended; rss 90 MB',
      '2026-10-06T10:01:00.000Z info start pid 77 rust 3.0.0 home /tmp/consensflow',
      '2026-10-06T10:02:00.000Z info start pid 4242 rust 3.0.0 home /tmp/consensflow',
      '',
    ].join('\n')
    assert.equal(startLine(log, 77).kind, 'native')
    assert.equal(startLine(log, 4242).kind, 'native', 'a pid that started twice is the later start')
    assert.equal(startLine(log).kind, 'native', 'with no pid, the last start of the log')
    assert.equal(startLine(log, 5), null)
    assert.equal(startLine('', 4242), null)
  })

  it('takes a line for a start only when it is one', () => {
    for (const log of [
      '2026-10-06T10:00:00.000Z info alive: 3 passes, node v26.8.1 rust 1\n',
      '2026-10-06T10:00:00.000Z error start pid 4242 node v26.8.1 home /h\n',
      '2026-10-06T10:00:00.000Z info start pid 4242 go1.26 home /h\n',
      '  2026-10-06T10:00:00.000Z info start pid 4242 node v26.8.1 home /h\n',
    ]) {
      assert.equal(startLine(log, 4242), null, log)
    }
  })

  it('holds the daemon that started to the one asked for', () => {
    const asked = { kind: 'node' }
    assert.equal(assertStarted(asked, `${NODE_LINE}\n`, 4242).runtime, 'node v26.8.1')
    assert.throws(
      () => assertStarted(asked, `${RUST_LINE}\n`, 4242),
      /Node's daemon was asked for, but the start line in its log says rust 3\.0\.0-alpha\.79/,
    )
    assert.throws(
      () => assertStarted({ kind: 'native' }, `${NODE_LINE}\n`, 4242),
      /the native daemon was asked for, but the start line in its log says node v26\.8\.1/,
    )
    assert.equal(assertStarted({ kind: 'native' }, `${RUST_LINE}\n`, 4242).kind, 'native')
    assert.throws(
      () => assertStarted(asked, `${NODE_LINE}\n`, 1),
      /Node's daemon was asked for, and its log holds no start line of pid 1/,
    )
  })

  it('holds the home it ran on to the choice as well: the cf verbs of a home choose by its file', () => {
    inAFolder((home) => {
      chooseHome('node', home)
      assert.equal(assertStarted({ kind: 'node' }, `${NODE_LINE}\n`, 4242, home).kind, 'node')
      assert.throws(
        () => assertStarted({ kind: 'native' }, `${RUST_LINE}\n`, 4242, home),
        /the native daemon was asked for, but .* has a use-node file/,
      )
      chooseHome('native', home)
      assert.equal(assertStarted({ kind: 'native' }, `${RUST_LINE}\n`, 4242, home).kind, 'native')
      assert.throws(
        () => assertStarted({ kind: 'node' }, `${NODE_LINE}\n`, 4242, home),
        /Node's daemon was asked for, but .* has no use-node file/,
      )
    })
  })

  it('notes for the runner only a daemon whose home agrees with it', () => {
    const said = withRanFile(() => {
      inAFolder((home) => {
        chooseHome('native', home)
        assert.throws(() => assertStarted({ kind: 'node' }, `${NODE_LINE}\n`, 4242, home))
        assert.throws(() => assertStarted({ kind: 'native' }, `${NODE_LINE}\n`, 4242, home))
      })
    })
    assert.equal(said, null)
  })
})

/** Runs `body` with the file the runner names in the environment, and answers what was said in it. */
function withRanFile(body) {
  const folder = mkdtempSync(join(tmpdir(), 'cf-ran-'))
  const file = join(folder, 'ran')
  const before = process.env.CONSENSFLOW_TEST_RAN
  process.env.CONSENSFLOW_TEST_RAN = file
  try {
    body()
    return existsSync(file) ? readFileSync(file, 'utf8') : null
  } finally {
    if (before === undefined) delete process.env.CONSENSFLOW_TEST_RAN
    else process.env.CONSENSFLOW_TEST_RAN = before
    rmSync(folder, { recursive: true, force: true })
  }
}

describe('what a run says it ran, to the runner that labelled it', () => {
  it('is a line to the file the runner names, and nothing where none is named', () => {
    assert.equal(
      withRanFile(() => {
        noteRan('node')
        noteRan('native')
      }),
      'node\nnative\n',
    )
    const before = process.env.CONSENSFLOW_TEST_RAN
    delete process.env.CONSENSFLOW_TEST_RAN
    try {
      assert.doesNotThrow(() => noteRan('node'))
    } finally {
      if (before !== undefined) process.env.CONSENSFLOW_TEST_RAN = before
    }
  })

  it('has the daemon that started noted once it is the one asked for, and not before', () => {
    const said = withRanFile(() => {
      assertStarted({ kind: 'node' }, `${NODE_LINE}\n`, 4242)
      assertStarted({ kind: 'native' }, `${RUST_LINE}\n`, 4242)
      // A refusal fails the suite by itself, and what was refused is not what the leg ran;
      // nor does a log with no start line say what ran.
      assert.throws(() => assertStarted({ kind: 'node' }, `${RUST_LINE}\n`, 4242))
      assert.throws(() => assertStarted({ kind: 'native' }, `${NODE_LINE}\n`, 4242))
      assert.throws(() => assertStarted({ kind: 'native' }, '', 4242))
    })
    assert.equal(said, 'node\nnative\n')
  })

  it('fails the leg that ran what is not its own, or did not say', () => {
    const [node, native] = DAEMON_LEGS
    assert.equal(ranProblem(node, 'node\nnode\n'), null)
    assert.equal(ranProblem(native, 'native\n'), null)
    assert.equal(ranProblem(node, 'node\nnative\n'), 'the Node daemon: a suite ran the native one')
    assert.equal(ranProblem(native, 'node\n'), 'the native daemon: a suite ran the node one')
    for (const said of ['', '\n']) {
      assert.equal(
        ranProblem(node, said),
        'the Node daemon: its suites did not say which implementation they ran',
      )
    }
    assert.match(
      ranProblem(CLI_LEGS[0], 'native\n'),
      /^Node's cf\.mjs: a suite ran the native one$/,
    )
  })
})

describe('a leg of a dual runner, run', () => {
  /** A suite that says what the test names and passes, or fails. */
  function suite(folder, say, { fail = false } = {}) {
    const file = join(folder, 'suite.test.mjs')
    writeFileSync(
      file,
      `import { it } from 'node:test'
import { noteRan } from '${CHOICE}'
it('says what ran', () => {
  ${say === null ? '' : `noteRan(${JSON.stringify(say)})`}
  ${fail ? "throw new Error('the suite fails')" : ''}
})
`,
    )
    return file
  }
  /** The status the leg answers, and what the runner said of it besides its header. */
  const leg = (selected, ...suites) => {
    const folder = mkdtempSync(join(tmpdir(), 'cf-leg-test-'))
    const said = []
    try {
      const status = runLeg('CONSENSFLOW_TEST_DAEMON', selected, [suite(folder, ...suites)], {
        cwd: folder,
        stdio: 'ignore',
        say: (text) => said.push(text.trim()),
      })
      return { status, said: said.filter((text) => !text.startsWith('==')) }
    } finally {
      rmSync(folder, { recursive: true, force: true })
    }
  }

  it('passes when its suites pass and say they ran its own', () => {
    assert.deepEqual(leg(DAEMON_LEGS[0], 'node'), { status: 0, said: [] })
    assert.deepEqual(leg(DAEMON_LEGS[1], 'native'), { status: 0, said: [] })
  })

  it('fails when its suites pass and ran the other one, or said nothing', () => {
    assert.deepEqual(leg(DAEMON_LEGS[0], 'native'), {
      status: 1,
      said: ['the Node daemon: a suite ran the native one'],
    })
    assert.deepEqual(leg(DAEMON_LEGS[1], 'node'), {
      status: 1,
      said: ['the native daemon: a suite ran the node one'],
    })
    assert.deepEqual(leg(DAEMON_LEGS[0], null), {
      status: 1,
      said: ['the Node daemon: its suites did not say which implementation they ran'],
    })
  })

  it('fails when its suites fail, whatever they said', () => {
    assert.deepEqual(leg(DAEMON_LEGS[0], 'node', { fail: true }), { status: 1, said: [] })
  })
})

describe('which cf the suites of the CLI run, in the same words', () => {
  it('runs Node’s bin/cf.mjs for `node`, and the native cf of this checkout for `native`', () => {
    for (const fallback of ['node', 'native']) {
      const node = cliTarget({ named: 'node', leg: '', fallback })
      assert.deepEqual([node.kind, node.command, node.args], ['node', process.execPath, [NODE_CF]])
      const native = cliTarget({ named: 'native', leg: '', fallback })
      assert.deepEqual([native.kind, native.command, native.args], ['native', NATIVE_CF, []])
    }
  })

  it('runs the command a JSON array gives as the native cf, and the default for nothing', () => {
    const given = cliTarget({ named: '["/build/cf","--x"]', leg: '', fallback: 'node' })
    assert.deepEqual([given.kind, given.command, given.args], ['native', '/build/cf', ['--x']])
    assert.equal(cliTarget({ named: '', leg: '', fallback: 'node' }).kind, 'node')
    assert.equal(cliTarget({ named: undefined, leg: '', fallback: 'native' }).kind, 'native')
    assert.throws(
      () => cliTarget({ named: 'nope', leg: '', fallback: 'node' }),
      /CONSENSFLOW_TEST_CLI is node, native, or a JSON array of strings/,
    )
  })

  it('refuses a selection that is not its own to a run that names its leg', () => {
    assert.throws(
      () => cliTarget({ named: 'native', leg: 'node', fallback: 'node' }),
      /says this is the node leg, but CONSENSFLOW_TEST_CLI names native/,
    )
    assert.throws(
      () => cliTarget({ named: 'node', leg: 'native', fallback: 'node' }),
      /says this is the native leg, but CONSENSFLOW_TEST_CLI names node/,
    )
  })

  it('makes the choice in the home of a run, and names no runtime to either cf', () => {
    // A runtime named to the native cf would let it hand a verb on to Node and be
    // none the worse for it: the environment of a run is the test's own, and the
    // home in it the only thing that says which implementation answers.
    const native = cliTarget({ named: 'native', leg: '', fallback: 'node' })
    const node = cliTarget({ named: 'node', leg: '', fallback: 'node' })
    inAFolder((home) => {
      const env = { A: '1', CONSENSFLOW_HOME: home }
      assert.deepEqual(cliEnv(node, env), env, 'the environment is the test’s')
      assert.equal(existsSync(join(home, WAY_BACK)), true)
      assert.deepEqual(cliEnv(native, env), env)
      assert.equal(existsSync(join(home, WAY_BACK)), false)
    })
  })
})
