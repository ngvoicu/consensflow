import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { assertStarted, choose, NATIVE_CF, START_WORDS, startLine } from './choice.mjs'
import { daemonCommand } from './helpers.mjs'

/** A daemon's log, as each daemon writes its first line (crates/cf-daemon/src/start.rs; Node's, which the releases before the deletion ran). */
const NODE_LINE = '2026-10-06T10:00:00.000Z info start pid 4242 node v26.8.1 home /tmp/consensflow'
const RUST_LINE =
  '2026-10-06T10:00:00.000Z info start pid 4242 rust 3.0.0-alpha.79 home /tmp/consensflow'

describe('which daemon a test starts, in words', () => {
  it('starts the native cf of this checkout for `native`, and for nothing at all', () => {
    for (const named of ['native', undefined, '']) {
      const started = daemonCommand({ named })
      assert.deepEqual(
        [started.command, started.args],
        [NATIVE_CF, ['ui', '--json', '--no-open']],
        String(named),
      )
    }
  })

  it('starts the command a JSON array gives', () => {
    const started = daemonCommand({ named: '["/build/cf","ui","--json"]' })
    assert.deepEqual([started.command, started.args], ['/build/cf', ['ui', '--json']])
  })

  it('refuses a selector that is none of the words, and says Node’s is not one', () => {
    for (const named of [
      'nope',
      'node',
      'Native',
      '[]',
      '[1]',
      '["a",2]',
      '{"a":1}',
      'null',
      '[',
      'native ',
    ]) {
      assert.throws(
        () => daemonCommand({ named }),
        /CONSENSFLOW_TEST_DAEMON is native, or a JSON array of strings/,
        named,
      )
    }
  })

  it('names the variable it was asked for in what it refuses', () => {
    assert.equal(choose('A_SELECTOR', 'native'), null)
    assert.deepEqual(choose('A_SELECTOR', '["/x","y"]'), ['/x', 'y'])
    assert.throws(() => choose('A_SELECTOR', 'node'), /^Error: A_SELECTOR is /)
  })
})

describe('which daemon started, from its log', () => {
  it('reads the daemon from the runtime its start line names, whichever wrote it', () => {
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

  it('holds the daemon that started to the native one', () => {
    assert.equal(assertStarted(`${RUST_LINE}\n`, 4242).kind, 'native')
    assert.throws(
      () => assertStarted(`${NODE_LINE}\n`, 4242),
      /the native daemon was asked for, but the start line in its log says node v26\.8\.1/,
    )
    assert.throws(
      () => assertStarted(`${RUST_LINE}\n`, 1),
      /the native daemon was asked for, and its log holds no start line of pid 1/,
    )
    assert.throws(() => assertStarted('', 4242), /holds no start line of pid 4242/)
  })
})
