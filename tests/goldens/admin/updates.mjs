/**
 * An update as Node says it: the CLI is brought to its latest release the way
 * it was installed, looked at again, and said as it came out (updated,
 * unchanged, failed, or unsupported where the install is not recognized),
 * with the last of what the tool wrote to either stream. Codex as its own
 * installer puts it is the CLI, unless a scenario says another.
 */
import { exe, failed, onPath, release, said, timedOut } from './kit.mjs'

const OWN = exe('$ROOT/home/.codex/bin/codex')

/**
 * An update of Codex, once for each of `rounds`: the first is checked, each
 * is updated with the program's answer, and looked at again as `after` says.
 */
function updating(name, first, rounds) {
  return {
    name,
    files: [OWN],
    effects: {
      run: {
        'codex --version': [first, ...rounds.map((round) => round.after)],
        'codex update': rounds.map((round) => round.run),
      },
      latest: { codex: [release('1.0.1'), ...rounds.map(() => release('1.0.1'))] },
    },
    steps: [{ op: 'check', id: 'codex' }, ...rounds.map(() => ({ op: 'update', id: 'codex' }))],
  }
}

/** A round whose program writes `run` and whose look afterwards says `after`. */
const round = (run, after = said('codex 1.0.1\n')) => ({ run, after })

/** `count` lines of text, `line 1` on. */
const lines = (count) => Array.from({ length: count }, (_, index) => `line ${index + 1}`).join('\n')

export function updates() {
  return [
    updating('updated: the version changed and the tool said so', said('codex 1.0.0\n'), [
      round(said('Updated to 1.0.1\n')),
    ]),
    updating(
      'an update is unchanged only where the same version was read twice, as updated otherwise',
      said('codex 1.0.0\n'),
      [
        round(said('already\n'), said('codex 1.0.0\n')),
        round(said('now\n'), said('codex 1.0.1\n')),
        round(said(''), said('unusual\n')),
        round(said(''), said('unusual\n')),
        round(said(''), said('codex 1.0.0\n')),
        round(said(''), said('codex 1.0.0\n')),
        round(said(''), failed('Command failed: codex --version\n')),
        round(said(''), said('codex 1.0.1\n')),
      ],
    ),
    updating(
      'an update that does not answer fails with its message, or with the time it was given',
      said('codex 1.0.0\n'),
      [
        round(
          failed('Command failed: codex update\nno network\n', {
            stdout: 'trying\n',
            stderr: 'no network\n',
          }),
          said('codex 1.0.0\n'),
        ),
        round(timedOut('half\n', 'warning\n'), said('codex 1.0.0\n')),
        round(
          failed('stdout maxBuffer length exceeded', { stdout: 'xxxxxxxx' }),
          said('codex 1.0.0\n'),
        ),
        round(failed('spawn $ROOT/home/.codex/bin/codex ENOENT'), said('codex 1.0.0\n')),
        round(failed('Command failed: codex update\n'), said('codex 1.0.1\n')),
        round(failed(''), said('codex 1.0.1\n')),
      ],
    ),
    updating(
      'what an update wrote is its standard output and then its standard error, trimmed',
      said('codex 1.0.0\n'),
      [
        round(said('out\n', 'err\n')),
        round(said('', 'only err\n')),
        round(said('only out')),
        round(said('\n\n  padded  \n\n')),
        round(said('﻿  out  ﻿')),
        round(said('\u0085kept\u0085')),
        round(said('one\r\ntwo\r\n')),
        round(said('a\n\nb\n')),
        round(said('')),
      ],
    ),
    updating(
      'what an update wrote is cut to its last twenty lines and its last two thousand units',
      said('codex 1.0.0\n'),
      [
        round(said(`${lines(20)}\n`)),
        round(said(`${lines(21)}\n`)),
        round(said(`${lines(30)}\n`)),
        round(said(lines(25), `${lines(25)}\n`)),
        round(said(`${'x'.repeat(2000)}\n`)),
        round(said(`${'x'.repeat(2001)}\n`)),
        round(said(`${'x'.repeat(3000)}\n`)),
        round(said(`${'é'.repeat(2500)}\n`)),
        round(said(`${'\u{1F600}'.repeat(1000)}\n`)),
        round(said(`${'\u{1F600}'.repeat(999)}ab\n`)),
        round(said(`${'\u{1F600}'.repeat(1200)}\n`)),
        round(said(`${'\u{1F600}'.repeat(1500)}\n`)),
        round(said(`${'a'.repeat(1990)}\n${'b'.repeat(1990)}\n`)),
        round(said(`${lines(30).replaceAll('line', 'l'.repeat(300))}\n`)),
      ],
    ),
    {
      name: 'an update of a CLI that is not installed, or of a name that is no harness, is an error',
      files: [exe('$ROOT/home/.codex/bin/codex')],
      effects: {
        run: { 'codex --version': [said('codex 1.0.0\n')] },
        latest: { codex: [release('1.0.0')] },
      },
      steps: [
        { op: 'update', id: 'devin' },
        { op: 'update', id: 'opencode' },
        { op: 'update', id: 'claude' },
        { op: 'update', id: 'kimi' },
        { op: 'update', id: '' },
        { op: 'update', id: 'claude-code' },
        { op: 'update', id: 'Codex' },
        { op: 'check', id: 'codex' },
      ],
    },
    {
      name: 'a CLI found where nothing is recognized is not updated, and nothing is run',
      files: [exe('$ROOT/bin/pi'), exe('$ROOT/bin/claude')],
      env: { PATH: onPath('$ROOT/bin') },
      effects: {
        run: { 'pi --version': [said('0.1.0\n')], 'claude --version': [said('2.1.0\n')] },
        latest: { pi: [release('0.1.1')], claude: [release('2.1.1')] },
      },
      steps: [
        { op: 'update', id: 'pi' },
        { op: 'update', id: 'claude' },
        { op: 'update', id: 'pi' },
      ],
    },
    {
      name: 'an update whose first look is older than five minutes looks again before it runs',
      files: [OWN],
      effects: {
        run: {
          'codex --version': [said('codex 1.0.0\n'), said('codex 1.0.0\n'), said('codex 1.0.1\n')],
          'codex update': [said('Updated\n')],
        },
        latest: { codex: [release('1.0.1'), release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'advance', ms: 300_000 },
        { op: 'update', id: 'codex' },
      ],
    },
    {
      name: 'an update that cannot start its program says so, as Homebrew not there',
      files: [exe('$ROOT/brew/Caskroom/codex/0.1/bin/codex')],
      env: { PATH: onPath('$ROOT/brew/Caskroom/codex/0.1/bin') },
      effects: {
        run: {
          'codex --version': [said('codex 1.0.0\n'), said('codex 1.0.0\n')],
          'brew upgrade --cask codex': [failed('spawn $ROOT/brew/bin/brew ENOENT')],
        },
        latest: { codex: [release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'update', id: 'codex' },
      ],
    },
  ]
}
