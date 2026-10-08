import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { stageBundle } from './bundle.mjs'
import { tempEnv, windowsEnv } from './helpers.mjs'

/**
 * `bin/cf.mjs`, the CLI's door, until it is deleted: every command, a window's
 * token or none, goes to the native `cf` beside it with the words as they came,
 * and its stdio and its exit code come back. There is nothing else behind it:
 * the `use-node` file the flip release sent a home to Node's CLI by is read by
 * nothing, and a home that still has one is forwarded like any other.
 *
 * `setup` and `doctor` go the way of every other verb. Their words name the
 * home they ran in, so they are held in one home and not compared across two
 * (`HOMED`).
 *
 * Where the answer must be the native `cf`'s own, it is run beside a stand-in
 * that says it was reached (`reached`). A shell script is no program on
 * Windows, which has the real one there.
 */

const WINDOWS = process.platform === 'win32'
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

/** What running `args` through the door of `from` in `t`'s home did. */
function door(args, t, env = {}, from = bundle) {
  const ran = spawnSync(process.execPath, [from.cfMjs, ...args], {
    encoding: 'utf8',
    env: { ...t.env, ...windowsEnv(), ...env },
    timeout: 60_000,
  })
  return { code: ran.status, out: ran.stdout, err: ran.stderr }
}

/** The same words to the real native cf itself, in `t`'s home: what forwarding must be equal to. */
function native(args, t, env = {}) {
  const ran = spawnSync(bundle.cf, args, {
    encoding: 'utf8',
    env: { ...t.env, ...windowsEnv(), ...env },
  })
  return { code: ran.status, out: ran.stdout, err: ran.stderr }
}

/** A home of this test's own, with the `use-node` file the flip release sent a home to Node by, or without it. */
function home(leftByTheFlipRelease) {
  const t = tempEnv()
  if (leftByTheFlipRelease) {
    mkdirSync(t.env.CONSENSFLOW_HOME, { recursive: true })
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'use-node'), '')
  }
  return t
}

describe('bin/cf.mjs, the door', () => {
  it('hands each verb to the native cf beside it whole, and its stdio and exit code back', {
    skip: WINDOWS && 'a shell script stands in for the native cf',
  }, () => {
    for (const args of [...VERBS, ...HOMED]) {
      const t = home(false)
      try {
        assert.deepEqual(
          door(args, t, {}, guarded),
          { code: 7, out: `reached|${args.join('|')}|`, err: '' },
          args.join(' '),
        )
      } finally {
        t.cleanup()
      }
    }
  })

  it('forwards each verb to the native cf, whose answer is the answer', () => {
    for (const args of VERBS) {
      const t = home(false)
      const u = home(false)
      try {
        assert.deepEqual(door(args, t), native(args, u), args.join(' '))
      } finally {
        t.cleanup()
        u.cleanup()
      }
    }
  })

  it('forwards each verb the same in a home that still has the use-node file: nothing reads it', {
    skip: WINDOWS && 'a shell script stands in for the native cf',
  }, () => {
    for (const stray of [{}, { CONSENSFLOW_DAEMON: 'node' }, { CONSENSFLOW_DAEMON: 'native' }]) {
      for (const args of [...VERBS, ...HOMED]) {
        const t = home(true)
        try {
          assert.deepEqual(
            door(args, t, stray, guarded),
            { code: 7, out: `reached|${args.join('|')}|`, err: '' },
            `${args.join(' ')} ${JSON.stringify(stray)}`,
          )
        } finally {
          t.cleanup()
        }
      }
    }
  })

  it('forwards a window’s token to the native cf, which is then the board', () => {
    const t = home(true)
    try {
      const ran = door(['help'], t, { CONSENSFLOW_TOKEN: 'participant' })
      assert.equal(ran.code, 0, ran.err)
      assert.match(ran.out, /^cf inside a ConsensFlow window/)
    } finally {
      t.cleanup()
    }
  })

  it('hands setup and doctor to the native cf as it hands every verb: it makes the command, and says it', () => {
    const t = home(false)
    try {
      const made = door(['setup'], t)
      assert.equal(made.code, 0, made.err)
      // The command names the native cf of this bundle: the native cf wrote it.
      const command = join(t.env.CONSENSFLOW_HOME, 'bin', WINDOWS ? 'cf.cmd' : 'cf')
      assert.ok(readFileSync(command, 'utf8').includes(bundle.cf), command)
      // `doctor` reads and writes nothing, so the same home gives the same answer to the cf itself.
      const said = door(['doctor'], t)
      assert.deepEqual(said, native(['doctor'], t))
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
