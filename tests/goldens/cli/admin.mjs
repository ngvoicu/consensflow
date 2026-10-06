/**
 * Scenarios for `cf setup` and `cf doctor`: what they say and what they leave
 * in the folder, with the harnesses' CLIs standing in on a PATH of its own.
 * They are recorded for the day Rust answers them (the launcher and the stale
 * hooks it needs are another landing's), and `cf` still hands them to Node's
 * sources until then: the Rust player does not play them yet.
 */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { ALL_STUBS, dir, file, handwritten, launchers, stub, withOwn } from './fixtures.mjs'

const V1 = readFileSync(
  fileURLToPath(new URL('../../fixtures/v1-agents.json', import.meta.url)),
  'utf8',
)

const WINDOWS = process.platform === 'win32'
/** A command of someone else's in the place of a launcher, which `cf` leaves alone. */
const foreign = (name, text = '#!/bin/sh\necho mine\n') =>
  file(`consensflow/bin/${name}${WINDOWS ? '.cmd' : ''}`, text, !WINDOWS)

const setup = (name, files = [], more = {}) => ({
  name: `setup: ${name}`,
  args: ['setup'],
  files,
  ...more,
})

const doctor = (name, files = [], more = {}) => ({
  name: `doctor: ${name}`,
  args: ['doctor'],
  files,
  ...more,
})

/** A Claude Code settings file with hooks in it. */
const settings = (hooks, path = 'home/.claude/settings.json') =>
  file(path, `${JSON.stringify({ model: 'x', hooks }, null, 2)}\n`)

export function adminScenarios() {
  const some = [stub('claude'), stub('codex')]
  return [
    setup('a home with nothing in it and no harness'),
    setup('the home of the user, with no variable for it', [], {
      env: { CONSENSFLOW_HOME: null },
    }),
    setup('two harnesses', some),
    setup('every harness, which prepares the extensions of Pi and OpenCode', ALL_STUBS),
    setup('again, with the launchers it wrote', launchers('$NODE')),
    setup('launchers of another runtime are replaced', launchers('$ROOT/old/node')),
    setup('launchers that do not pin their home are replaced', launchers('$NODE', { pin: false })),
    setup('a command of someone else is left alone', [foreign('cf')]),
    setup('a legacy agents file is kept as it is', [file('consensflow/agents.json', V1)]),
    setup('the agents of the human are counted', [withOwn()]),
    setup('an agents file it cannot read, after what it said so far', [
      handwritten('{"agents": [,]}'),
    ]),
    setup('a folder where the launchers go is a file', [file('consensflow/bin', 'not a folder')]),
    setup('a word that is no option', [], { args: ['setup', 'x'] }),
    setup('an option', [], { args: ['setup', '--json'] }),
    setup('a word after --', [], { args: ['setup', '--', 'x'] }),
    setup('nothing after --', [], { args: ['setup', '--'] }),
    setup('an input it does not read', [], { stdin: 'x\n' }),
    doctor('a home with nothing in it and no harness'),
    doctor('the home of the user, with no variable for it', [], {
      env: { CONSENSFLOW_HOME: null },
    }),
    doctor('two harnesses', some),
    doctor('every harness', ALL_STUBS),
    doctor('the agents of the human are counted', [withOwn()]),
    doctor('launchers of this runtime', launchers('$NODE')),
    doctor(
      'launchers of this runtime that do not pin their home',
      launchers('$NODE', { pin: false }),
    ),
    doctor('launchers of a runtime that is gone', launchers('$ROOT/gone/node')),
    doctor('launchers of another runtime that is there', [
      ...launchers('$ROOT/other/node'),
      file('other/node', '#!/bin/sh\n', true),
    ]),
    doctor('only the first launcher is looked at', launchers('$NODE', { names: ['cf'] })),
    doctor('a command of someone else is no launcher of ours', [foreign('consensflow')]),
    doctor('a launcher that names no runtime', [
      foreign('consensflow', '#!/bin/sh\n# Installed by ConsensFlow\necho x\n'),
    ]),
    doctor('a hook an older version left in the settings of Claude Code', [
      settings({
        SessionStart: [{ hooks: [{ command: '/x/consensflow/hook.mjs' }] }],
        Stop: [{ hooks: [{ command: 'other' }] }],
        PreToolUse: [{ matcher: 'Ask', hooks: [{ command: 'consensflow-hook' }] }],
        Odd: 'not a list',
      }),
    ]),
    doctor('settings with no hook of ours', [
      settings({ Stop: [{ hooks: [{ command: 'other' }] }] }),
    ]),
    doctor('settings that are no JSON', [file('home/.claude/settings.json', '{"hooks": ,}')]),
    doctor(
      'settings in the folder the variable names',
      [settings({ Stop: [{ command: 'consensflow' }] }, 'claude/settings.json')],
      { env: { CLAUDE_CONFIG_DIR: '$ROOT/claude' } },
    ),
    doctor(
      'settings in the home of the user, with no variable for the folder',
      [settings({ Stop: [{ command: 'consensflow' }] })],
      { env: { CLAUDE_CONFIG_DIR: null } },
    ),
    doctor('an agents file it cannot read, after what it said so far', [handwritten('[]')]),
    doctor('words after it are no matter', [withOwn()], { args: ['doctor', '--anything', 'x'] }),
    doctor('an input it does not read', [], { stdin: 'x\n' }),
    doctor('every harness, the agents, launchers and a hook', [
      ...ALL_STUBS,
      withOwn(),
      ...launchers('$NODE'),
      settings({ Stop: [{ command: 'consensflow' }] }),
    ]),
    doctor('an empty folder is not made into more', [dir('consensflow')]),
  ]
}
