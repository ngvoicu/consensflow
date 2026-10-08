import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { assertRepaired } from './updater-smoke/launchers.mjs'

/**
 * The terminal command, once the app that replaced the installed one has
 * started, as the updater smoke reads it: each command of this home that serves
 * this home runs the update's own `cf` and still pins this home, the app's log says
 * so, and no command that serves another home, in this home's `bin` or in its
 * own, is changed by a byte or spoken of. The commands are scripts of `sh`, and a
 * stand-in `cf` says its version, so these run where `sh` does.
 */

const POSIX_ONLY = process.platform === 'win32' && 'the commands under test are scripts of sh'
const MARK = '# Installed by ConsensFlow. Runs the app’s own cf.'
const NAMES = ['cf', 'consensflow']

const launcher = (home, program) =>
  `#!/bin/sh\n${MARK}\nexport CONSENSFLOW_HOME="${home}"\nexec "${program}" "$@"\n`
const oldShape = (home) =>
  `#!/bin/sh\n${MARK}\nexport CONSENSFLOW_HOME="${home}"\nexec "/old/node" "/old/cf.mjs" "$@"\n`

/**
 * A machine as the repair leaves it, with a `cf` that says its version, and what
 * `change` does to it; `body` is run on it, and it is removed after. `repaired`
 * names the commands of this home that serve this home: the other name, if there
 * is one, is another home's command in this home's `bin`, which stays as it was.
 * With `flip` the installed release was the flip's, whose `cf setup` wrote the
 * commands naming its `cf`, which is the update's path: there was nothing to
 * repair, and the log says nothing. Else it was the bridge's, whose named Node.
 */
function onAMachine({ repaired = ['cf'], change = () => {}, flip = false }, body) {
  const root = mkdtempSync(join(tmpdir(), 'cf-launchers-evidence-'))
  try {
    const box = {
      root,
      home: join(root, 'home'),
      bin: join(root, 'bin'),
      state: join(root, 'state'),
      other: join(root, 'other'),
      probe: join(root, 'probe'),
    }
    const cf = join(root, 'ConsensFlow.app', 'cf')
    for (const dir of [box.state, box.other, box.probe, join(root, 'ConsensFlow.app')]) {
      mkdirSync(join(dir, 'bin'), { recursive: true })
    }
    writeFileSync(cf, '#!/bin/sh\necho 3.0.0-alpha.81\n')
    chmodSync(cf, 0o755)
    const write = (home, name, text, mode = 0o755) => {
      writeFileSync(join(home, 'bin', name), text)
      chmodSync(join(home, 'bin', name), mode)
    }
    const written = (home) => (flip ? launcher(home, cf) : oldShape(home))
    const planted = {
      own: Object.fromEntries(
        NAMES.map((name) => [name, written(repaired.includes(name) ? box.state : box.other)]),
      ),
      other: { cf: written(box.other), consensflow: written(box.other) },
      repaired,
    }
    for (const name of NAMES) {
      write(box.state, name, repaired.includes(name) ? launcher(box.state, cf) : planted.own[name])
      write(box.other, name, planted.other[name])
    }
    const log = flip
      ? ''
      : repaired
          .map(
            (name) =>
              `consensflow: the terminal command ${join(box.state, 'bin', name)} now runs ${cf}\n`,
          )
          .join('')
    change({ box, cf, write })
    return body({ box, planted, cf, log, release: flip ? 'flip' : 'bridge' })
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

const check = ({ box, planted, cf, log, release }, over = {}) =>
  assertRepaired({ box, planted, cf, appLogText: log, version: '3.0.0-alpha.81', release, ...over })

describe('the terminal command, once the app that replaced the installed one has started', {
  skip: POSIX_ONLY,
}, () => {
  it('runs the update’s cf with the home’s pin, and leaves every other command as it was', () => {
    onAMachine({}, (machine) => check(machine))
  })

  it('has both names repaired when both serve the home', () => {
    onAMachine({ repaired: NAMES }, (machine) => check(machine))
  })

  it('is not repaired while it still names Node’s, loses its pin or its mark, or is not a program', () => {
    const scenarios = [
      [
        'still the old shape',
        ({ box, write }) => write(box.state, 'cf', oldShape(box.state)),
        /does not run/,
      ],
      [
        'the pin lost',
        ({ box, cf, write }) => write(box.state, 'cf', `#!/bin/sh\n${MARK}\nexec "${cf}" "$@"\n`),
        /lost the pin/,
      ],
      [
        'the mark lost',
        ({ box, cf, write }) =>
          write(
            box.state,
            'cf',
            `#!/bin/sh\nexport CONSENSFLOW_HOME="${box.state}"\nexec "${cf}" "$@"\n`,
          ),
        /lost its mark/,
      ],
      [
        'it runs the cf and names Node’s as well',
        ({ box, cf, write }) =>
          write(
            box.state,
            'cf',
            `${launcher(box.state, cf)}# was: exec "/old/node" "/old/cf.mjs"\n`,
          ),
        /still names Node's/,
      ],
      [
        'it cannot be run',
        ({ box, cf, write }) => write(box.state, 'cf', launcher(box.state, cf), 0o644),
        /not executable/,
      ],
    ]
    for (const [what, change, words] of scenarios) {
      onAMachine({ change }, (machine) => assert.throws(() => check(machine), words, what))
    }
  })

  it('is not repaired when the second name was left as it was though it serves the home', () => {
    onAMachine(
      {
        repaired: NAMES,
        change: ({ box, write }) => write(box.state, 'consensflow', oldShape(box.state)),
      },
      (machine) => assert.throws(() => check(machine), /does not run/),
    )
  })

  it('is no repair when it runs another cf than the daemon’s', () => {
    onAMachine({}, (machine) =>
      assert.throws(
        () => check(machine, { version: '3.0.0-alpha.99' }),
        /another cf than the daemon/,
      ),
    )
  })

  it('is not own-home-only when a command of another home was rewritten, or the log speaks of one', () => {
    const scenarios = [
      [
        'the command pinned to another home was repaired',
        ({ box, cf, write }) => write(box.state, 'consensflow', launcher(box.other, cf)),
        () => ({}),
        /serves another home, changed/,
      ],
      [
        'another home’s own command was repaired',
        ({ box, cf, write }) => write(box.other, 'cf', launcher(box.other, cf)),
        () => ({}),
        /another home's commands changed/,
      ],
      [
        'the log does not say the command was repaired',
        () => {},
        () => ({ appLogText: '' }),
        /does not say/,
      ],
      [
        'the log says the command pinned to another home was repaired too',
        () => {},
        ({ box, cf, log }) => ({
          appLogText: `${log}consensflow: the terminal command ${join(box.state, 'bin', 'consensflow')} now runs ${cf}\n`,
        }),
        /speaks of a command that serves another home/,
      ],
      [
        'the log says another home’s command was repaired too',
        () => {},
        ({ box, cf, log }) => ({
          appLogText: `${log}consensflow: the terminal command ${join(box.other, 'bin', 'cf')} now runs ${cf}\n`,
        }),
        /speaks of a command that serves another home/,
      ],
    ]
    for (const [what, change, over, words] of scenarios) {
      onAMachine({ change }, (machine) =>
        assert.throws(() => check(machine, over(machine)), words, what),
      )
    }
  })
})

describe('the terminal command of the flip release, once the update has started', {
  skip: POSIX_ONLY,
}, () => {
  it('is current: it names the cf of the bundle, which the update’s is at, and is as it was', () => {
    onAMachine({ flip: true }, (machine) => check(machine))
    onAMachine({ flip: true, repaired: NAMES }, (machine) => check(machine))
  })

  it('is not current when the app rewrote it, or spoke of it', () => {
    const scenarios = [
      [
        'a byte of it changed',
        ({ box, cf, write }) => write(box.state, 'cf', `${launcher(box.state, cf)}# repaired\n`),
        () => ({}),
        /which was current, changed/,
      ],
      [
        'the log says it was repaired',
        () => {},
        ({ box, cf }) => ({
          appLogText: `consensflow: the terminal command ${join(box.state, 'bin', 'cf')} now runs ${cf}\n`,
        }),
        /speaks of a command that was current/,
      ],
      [
        'it names another cf than the update’s',
        ({ box, write }) => write(box.state, 'cf', launcher(box.state, '/elsewhere/cf')),
        () => ({}),
        /does not run/,
      ],
    ]
    for (const [what, change, over, words] of scenarios) {
      onAMachine({ flip: true, change }, (machine) =>
        assert.throws(() => check(machine, over(machine)), words, what),
      )
    }
  })
})
