/**
 * The scenarios of `tests/harness-admin.test.mjs`, as Node holds them, played
 * the way the recorder plays every scenario: what its stand-in CLIs say and
 * what its `latest` and `run` stubs answer are the scripts here.
 */
import { exe, failed, onPath, release, said } from './kit.mjs'

export function nodeTests() {
  return [
    {
      name: 'administration lists missing harnesses without probing or installing them',
      steps: [{ op: 'check' }],
    },
    {
      name: 'version and update checks cache results and refresh on request',
      files: [exe('$ROOT/bin/codex')],
      effects: {
        run: { 'codex --version': [said('codex-cli 99.1.0\n'), said('codex-cli 99.1.0\n')] },
        latest: { codex: [release('99.2.0'), release('99.2.0')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'check', id: 'codex' },
        { op: 'check', id: 'codex', refresh: true },
      ],
    },
    {
      name: 'offline and invalid version output are explicit failures, not latest or incompatible',
      files: [exe('$ROOT/bin/claude')],
      effects: {
        run: { 'claude --version': [said('unusual\n')] },
        latest: { claude: [{ error: 'offline' }] },
      },
      steps: [
        { op: 'check', id: 'claude' },
        { op: 'check', id: '../invalid' },
      ],
    },
    {
      name: 'updates a harness with its own tool, checks it again, and says what happened',
      files: [exe('$ROOT/home/.codex/bin/codex'), exe('$ROOT/bin/pi')],
      env: { PATH: onPath('$ROOT/bin') },
      effects: {
        run: {
          'codex --version': [said('1.0.0\n'), said('1.0.1\n'), said('1.0.1\n')],
          'codex update': [
            said('Updated to 1.0.1\n'),
            failed('Command failed: codex update', { stderr: 'no network\n' }),
          ],
          'pi --version': [said('0.1.0\n')],
        },
        latest: {
          codex: [release('1.0.1'), release('1.0.1'), release('1.0.1')],
          pi: [release('0.1.1')],
        },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'update', id: 'codex' },
        { op: 'update', id: 'codex' },
        { op: 'update', id: 'pi' },
        { op: 'update', id: 'devin' },
      ],
    },
    {
      name: 'Devin diagnostics report the minimum native version',
      files: [exe('$ROOT/bin/devin')],
      effects: {
        run: { 'devin --version': [said('3000.6.14\n')] },
        latest: { devin: [release('3000.10.21')] },
      },
      steps: [{ op: 'check', id: 'devin' }],
    },
  ]
}
