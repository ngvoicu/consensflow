/**
 * What a look says of a CLI: the version it prints, how it compares with the
 * release the feed says, and what Devin may be opened on. A text a CLI prints
 * is read where it says a version; a release is compared a number at a time.
 * Every case is one look of a CLI found on PATH, asked again each time.
 */
import { exe, failed, release, said, timedOut } from './kit.mjs'

/** What a CLI may print to `--version`: where a version is in it and where it is not. */
const VERSION_TEXTS = [
  'codex-cli 99.1.0\n',
  '99.1.0',
  'v1.2.3',
  'dev1.2.3',
  'claude 2.1.3 (Claude Code)\n',
  '(1.2.3)',
  '1.2.3)',
  '1.2.3-beta.1',
  '1.2.3-beta.1 build',
  '1.2.3-beta+meta',
  '1.2.3+meta',
  '1.2.3.4',
  '1.2.3x 4.5.6',
  '1.2.3- 4.5.6',
  'a1.2.3',
  'version=1.2.3',
  ' 1.2.3',
  ' 1.2.3',
  '\u00851.2.3',
  '﻿1.2.3',
  '1.2.3\u0085',
  '1.2.3 ',
  '１.２.３',
  '01.02.03',
  '1.2',
  '',
  '3000.6.14',
  'Devin 3000.10.21 (abc)\n',
  '1.2.3-',
  '1.2.3-é',
  '1.2.3-a_b',
  '1.2.3-a b',
  'v1.2.3-rc.1)',
  'vv1.2.3',
  '1.2.3 1.2.4',
  'x 1.2.3.4 1.2.5',
  'first line\nsecond 4.5.6\n',
  'codex-cli 0.159.2 (build 9c803229)\n',
  '\n\n 7.8.9 \n',
  'unusual',
]

/** What a feed may say against a version of 1.0.0. */
const RELEASES = [
  '1.0.1',
  '1.0.0',
  '0.9.9',
  '1.10.0',
  '2.0.0',
  '007.0.0',
  'v1.0.1',
  '1.0.1-beta',
  '1.0.1\n',
  '',
  '1.0',
  'not a version',
  'codex 1.0.1',
  '１.0.1',
  `${'9'.repeat(400)}.0.0`,
]

/** What Devin may print against a minimum of 3000.10.21. */
const DEVIN_VERSIONS = [
  '3000.6.14',
  '3000.9.99',
  '3000.10.20',
  '3000.10.21',
  '3000.10.22',
  '3000.11.0',
  '3001.0.0',
  '4000.0.0',
  '2999.99.99',
  'devin 3000.11.3 (9c803229faa4)\n',
  '3000.10.21-beta.1',
  'v3000.10.21',
  '3000.10',
  '',
  'unusual',
]

/**
 * A look at `command` (a CLI found on PATH) once for each of `looks`, the
 * first as it is asked and each after it asked again: what it prints, and the
 * release the feed says.
 */
function looks(name, command, looksOf) {
  return {
    name,
    files: [exe(`$ROOT/bin/${command}`)],
    effects: {
      run: { [`${command} --version`]: looksOf.map((each) => each.version) },
      latest: { [command]: looksOf.map((each) => each.release) },
    },
    steps: looksOf.map((_, index) => ({ op: 'check', id: command, refresh: index > 0 })),
  }
}

export function probes() {
  return [
    looks(
      'the version is read where a CLI says one, in what it prints',
      'codex',
      VERSION_TEXTS.map((text) => ({ version: said(text), release: release('99.2.0') })),
    ),
    looks(
      'a release is compared with the version a number at a time',
      'codex',
      RELEASES.map((remote) => ({ version: said('codex-cli 1.0.0\n'), release: release(remote) })),
    ),
    looks(
      'a version and a release that are no three whole numbers are compared with nothing',
      'codex',
      [
        ['codex-cli 1.0.0-beta\n', '1.0.0'],
        ['codex-cli 1.0.0-beta\n', '1.0.0-beta'],
        ['unusual\n', '1.0.0'],
        ['codex-cli 007.0.0\n', '7.0.0'],
        ['codex-cli 1.9.0\n', '1.10.0'],
        [`${'9'.repeat(400)}.0.0\n`, `${'9'.repeat(400)}.0.1`],
        ['codex-cli 1.0.0\n', 'v1.0.0'],
      ].map(([text, remote]) => ({ version: said(text), release: release(remote) })),
    ),
    looks(
      'Devin is ready only from its minimum version',
      'devin',
      DEVIN_VERSIONS.map((text) => ({ version: said(text), release: release('3000.10.21') })),
    ),
    looks('a feed that cannot be reached is an error with the words it gave', 'claude', [
      { version: said('claude 2.1.3\n'), release: { error: 'offline' } },
      { version: said('claude 2.1.3\n'), release: { error: '' } },
      { version: said('claude 2.1.3\n'), release: { error: 'Release version is unavailable' } },
      { version: said('claude 2.1.3\n'), release: release('') },
      { version: said('claude 2.1.3\n'), release: release('2.1.4') },
    ]),
    looks('a probe that does not answer says why, as it failed', 'codex', [
      {
        version: failed('Command failed: codex --version\nboom\n', { stderr: 'boom\n' }),
        release: release('1.0.0'),
      },
      { version: timedOut('partial'), release: release('1.0.0') },
      {
        version: failed('stdout maxBuffer length exceeded', { stdout: 'xxxxxxxx' }),
        release: release('1.0.0'),
      },
      { version: failed('spawn codex ENOENT'), release: release('1.0.0') },
      { version: said(''), release: release('1.0.0') },
      { version: said('codex-cli 1.0.0\n', 'a warning\n'), release: release('1.0.0') },
    ]),
    {
      name: 'every harness installed at once is looked at together and listed in the order of the list',
      files: [
        exe('$ROOT/bin/devin'),
        exe('$ROOT/bin/claude'),
        exe('$ROOT/bin/codex'),
        exe('$ROOT/bin/opencode'),
        exe('$ROOT/bin/pi'),
      ],
      effects: {
        run: {
          'devin --version': [said('3000.10.21\n')],
          'claude --version': [said('2.1.280 (Claude Code)\n')],
          'codex --version': [said('codex-cli 0.159.2\n')],
          'opencode --version': [said('1.4.0\n')],
          'pi --version': [said('0.3.1\n')],
        },
        latest: {
          devin: [release('3000.10.21')],
          claude: [release('2.1.281')],
          codex: [release('0.159.2')],
          opencode: [{ error: 'offline' }],
          pi: [release('0.3.1')],
        },
      },
      steps: [{ op: 'check' }, { op: 'check' }],
    },
    {
      name: 'only some harnesses installed: the others are rows that were never looked at',
      files: [exe('$ROOT/bin/pi'), exe('$ROOT/bin/claude')],
      effects: {
        run: { 'pi --version': [said('0.3.1\n')], 'claude --version': [said('2.1.280\n')] },
        latest: { pi: [release('0.3.2')], claude: [release('2.1.280')] },
      },
      steps: [{ op: 'check' }],
    },
  ]
}
