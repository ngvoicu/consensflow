import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { stageBundle } from './bundle.mjs'
import { chooseHome } from './choice.mjs'
import { tempEnv, windowsEnv } from './helpers.mjs'

/**
 * `bin/cf.mjs`, the CLI's door in the flip release: a window's token and a home
 * that has not taken the way back send every command to the native `cf` beside
 * it, with the words as they came, so that one implementation writes each file
 * of the home; a home that has (the `use-node` file in it) runs Node's own CLI,
 * `src/cli.js`, in the door's process. Which of the two ran is told by the
 * modules the door loaded (`tests/fixtures/module-spy.mjs`), not by what it
 * printed, which the two implementations print alike (`goldens:cli`).
 *
 * `setup` and `doctor` go the way of every other verb: the native `cf` answers
 * them, and the door has nothing of its own for them. Their words name the home
 * they ran in, so they are held in one home and not compared across two
 * (`HOMED`).
 *
 * Where the door must not forward, it is run beside a stand-in for the native
 * `cf` that says it was reached (`reached`): a door that forwarded there anyway
 * would meet the real one, which hands a Node home's commands straight back to
 * the door, and so on for ever. A shell script is no program on Windows, which
 * has the real one there and is run by no plant.
 */

const WINDOWS = process.platform === 'win32'
const SPY = fileURLToPath(new URL('./fixtures/module-spy.mjs', import.meta.url))
/** The stand-in for the native cf: says it was reached, with the words it was given, and ends 7. */
const STAND_IN = '#!/bin/sh\nprintf "reached|"; printf "%s|" "$@"; exit 7\n'
/** Every verb the native cf answers, with the words a person gives it. */
const VERBS = [
  ['help'],
  ['--version'],
  ['catalog', '--harness', 'two words'],
  ['agent', 'list', '--json'],
  ['agent', 'add', 'mine', '--harness', 'claude', '--model', 'a model'],
  ['agent', 'frobnicate'],
  ['frobnicate'],
  ['ui', '--foo'],
]
/** The verbs whose words name the home they ran in (`home:`, where the command is made): no two homes give one answer. */
const HOMED = [['setup'], ['doctor']]

/** The door beside the real native cf, and the door beside a stand-in that says it was reached. */
let bundle
let guarded
let scratch
before(() => {
  bundle = stageBundle()
  scratch = mkdtempSync(join(tmpdir(), 'cf-door-'))
  const standIn = join(scratch, 'cf')
  writeFileSync(standIn, STAND_IN, { mode: 0o755 })
  guarded = WINDOWS ? stageBundle() : stageBundle({ cf: standIn })
})
after(() => {
  bundle.cleanup()
  guarded.cleanup()
  rmSync(scratch, { recursive: true, force: true })
})

/** A home of this test's own, with the file or without it. */
function home(wayBack) {
  const t = tempEnv()
  chooseHome(wayBack ? 'node' : 'native', t.env.CONSENSFLOW_HOME)
  return t
}

/**
 * What running `args` through the door of `from` in `t`'s home did: its answer
 * and which of Node's modules loaded.
 */
function door(args, t, env = {}, from = bundle) {
  const spied = join(t.root, `spied-${Math.random().toString(36).slice(2)}`)
  const ran = spawnSync(process.execPath, [from.cfMjs, ...args], {
    encoding: 'utf8',
    env: {
      ...t.env,
      ...windowsEnv(),
      ...env,
      NODE_OPTIONS: `--import=${pathToFileURL(SPY).href}`,
      CF_TEST_SPY: spied,
    },
    timeout: 60_000,
  })
  const loaded = existsSync(spied) ? readFileSync(spied, 'utf8').split('\n').filter(Boolean) : []
  return { code: ran.status, out: ran.stdout, err: ran.stderr, loaded: [...new Set(loaded)].sort() }
}

/** The same words to the real native cf itself, in `t`'s home: what forwarding must be equal to. */
function native(args, t, env = {}) {
  const ran = spawnSync(bundle.cf, args, {
    encoding: 'utf8',
    env: { ...t.env, ...windowsEnv(), ...env },
  })
  return { code: ran.status, out: ran.stdout, err: ran.stderr }
}

const answer = ({ code, out, err }) => ({ code, out, err })

describe('bin/cf.mjs, the door', () => {
  it('hands each verb to the native cf beside it whole, and its stdio and exit code back, when the home has no way back', {
    skip: WINDOWS && 'a shell script stands in for the native cf',
  }, () => {
    for (const args of [...VERBS, ...HOMED]) {
      const t = home(false)
      try {
        const forwarded = door(args, t, {}, guarded)
        assert.deepEqual(forwarded.loaded, ['use-node'], args.join(' '))
        assert.deepEqual(
          [forwarded.code, forwarded.out, forwarded.err],
          [7, `reached|${args.join('|')}|`, ''],
          args.join(' '),
        )
      } finally {
        t.cleanup()
      }
    }
  })

  it('forwards each verb to the native cf, whose answer is the answer, when the home has no way back', () => {
    for (const args of VERBS) {
      const t = home(false)
      const u = home(false)
      try {
        const forwarded = door(args, t)
        // Node's CLI did not load, and what answered is the native cf's: its words and its code.
        assert.deepEqual(forwarded.loaded, ['use-node'], args.join(' '))
        assert.deepEqual(answer(forwarded), native(args, u), args.join(' '))
      } finally {
        t.cleanup()
        u.cleanup()
      }
    }
  })

  it('runs each verb on Node’s own CLI, in its process, when the home has taken the way back', () => {
    for (const args of [...VERBS, ...HOMED]) {
      const t = home(true)
      const u = home(false)
      try {
        const ran = door(args, t, {}, guarded)
        assert.deepEqual(ran.loaded, ['cli', 'use-node'], args.join(' '))
        assert.ok(!ran.out.startsWith('reached|'), `${args.join(' ')}: ${ran.out}`)
        if (HOMED.includes(args)) {
          assert.equal(ran.code, 0, `${args.join(' ')}: ${ran.err}`)
        } else {
          // The two print alike, so the proof of who ran is the module and not the words.
          assert.deepEqual(answer(ran), native(args, u), args.join(' '))
        }
      } finally {
        t.cleanup()
        u.cleanup()
      }
    }
  })

  it('has the old switch in the environment change nothing, in either home', () => {
    // `CONSENSFLOW_DAEMON` was the switch before the flip; a terminal does not
    // inherit the app's environment, so a variable could never be the way back.
    for (const stray of ['node', 'native', '']) {
      const plain = home(false)
      const back = home(true)
      try {
        const forwarded = door(['catalog'], plain, { CONSENSFLOW_DAEMON: stray }, guarded)
        assert.deepEqual(forwarded.loaded, ['use-node'], `no file, ${JSON.stringify(stray)}`)
        const ran = door(['catalog'], back, { CONSENSFLOW_DAEMON: stray }, guarded)
        assert.deepEqual(ran.loaded, ['cli', 'use-node'], `the file, ${JSON.stringify(stray)}`)
      } finally {
        plain.cleanup()
        back.cleanup()
      }
    }
  })

  it('forwards a window’s token to the native cf, which is then the board, whatever the home says', () => {
    for (const wayBack of [false, true]) {
      const t = home(wayBack)
      try {
        const ran = door(['help'], t, { CONSENSFLOW_TOKEN: 'participant' })
        // Node's CLI did not load: the file, which is there or not, decided nothing.
        assert.deepEqual(ran.loaded, ['use-node'], `way back: ${wayBack}`)
        assert.equal(ran.code, 0, ran.err)
        assert.match(ran.out, /^cf inside a ConsensFlow window/)
      } finally {
        t.cleanup()
      }
    }
  })

  it('hands setup and doctor to the native cf as it hands every verb: it makes the command, and says it', () => {
    const t = home(false)
    try {
      const made = door(['setup'], t)
      assert.deepEqual(made.loaded, ['use-node'])
      assert.equal(made.code, 0, made.err)
      // The command names the native cf of this bundle: the native cf wrote it.
      const command = join(t.env.CONSENSFLOW_HOME, 'bin', WINDOWS ? 'cf.cmd' : 'cf')
      assert.ok(readFileSync(command, 'utf8').includes(bundle.cf), command)
      // `doctor` reads and writes nothing, so the same home gives the same answer to the cf itself.
      const said = door(['doctor'], t)
      assert.deepEqual(said.loaded, ['use-node'])
      assert.deepEqual(answer(said), native(['doctor'], t))
      assert.ok(said.out.split('\n').includes(`command:      ${bundle.cf}`), said.out)
    } finally {
      t.cleanup()
    }
  })

  it('says when the native cf beside it does not start', () => {
    const t = home(false)
    const bare = stageBundle()
    try {
      rmSync(bare.cf)
      const ran = spawnSync(process.execPath, [bare.cfMjs, 'help'], {
        encoding: 'utf8',
        env: t.env,
      })
      assert.equal(ran.status, 1)
      assert.equal(ran.stdout, '')
      assert.ok(ran.stderr.startsWith(`cf: ${bare.cf} did not start: `), ran.stderr)
    } finally {
      bare.cleanup()
      t.cleanup()
    }
  })
})
