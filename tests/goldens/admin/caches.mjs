/**
 * What the admin keeps and shares: a row is kept five minutes from when its
 * look began and looked at again after, a CLI that moved or went is looked at
 * again at once, and a look that is running is shared by every call that asks
 * for it, a refresh too. The clock moves only where a step moves it, and an
 * answer is held where two calls must overlap.
 */
import { cli, exe, failed, onPath, release, said } from './kit.mjs'

const CODEX = `$ROOT/bin/codex`

/** A held answer, to be given by a step. */
const HELD = { held: true }

/** The step that gives a held probe of `command` its answer. */
const give = (command, answer) => ({
  op: 'release',
  kind: 'run',
  call: `${command} --version`,
  answer,
})

export function caches() {
  return [
    {
      name: 'a row is kept five minutes from when its look began, and looked at again after',
      files: [exe(CODEX)],
      effects: {
        run: {
          'codex --version': [said('1.0.0\n'), said('1.0.0\n'), said('1.0.1\n')],
        },
        latest: { codex: [release('1.0.1'), release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'advance', ms: 299_999 },
        { op: 'check', id: 'codex' },
        { op: 'advance', ms: 1 },
        { op: 'check', id: 'codex' },
        { op: 'advance', ms: 300_000 },
        { op: 'check', id: 'codex' },
      ],
    },
    {
      name: 'a look is dated when it began and kept from then, not from when it ended',
      files: [exe(CODEX)],
      effects: {
        run: { 'codex --version': [HELD, said('1.0.1\n')] },
        latest: { codex: [release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex', name: 'first' },
        { op: 'advance', ms: 100_000 },
        give('codex', said('1.0.0\n')),
        { op: 'advance', ms: 199_999 },
        { op: 'check', id: 'codex', name: 'kept' },
        { op: 'advance', ms: 1 },
        { op: 'check', id: 'codex', name: 'again' },
      ],
    },
    {
      name: 'a CLI that moved or went is looked at again at once, and one that is missing is kept too',
      files: [exe('$ROOT/a/codex'), exe('$ROOT/b/codex')],
      env: { PATH: onPath('$ROOT/a', '$ROOT/b') },
      effects: {
        run: { 'codex --version': [said('1.0.0\n'), said('2.0.0\n'), said('3.0.0\n')] },
        latest: { codex: [release('9.0.0'), release('9.0.0'), release('9.0.0')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'check', id: 'codex' },
        { op: 'remove', path: cli('$ROOT/a/codex') },
        { op: 'check', id: 'codex' },
        { op: 'remove', path: cli('$ROOT/b/codex') },
        { op: 'check', id: 'codex' },
        { op: 'check', id: 'codex' },
        { op: 'write', path: cli('$ROOT/a/codex'), executable: true },
        { op: 'check', id: 'codex' },
      ],
    },
    {
      name: 'a refresh of a harness that is not installed asks nothing',
      steps: [
        { op: 'check', id: 'codex', refresh: true },
        { op: 'check', id: 'codex', refresh: true },
      ],
    },
    {
      name: 'calls that ask while a look runs share it, a refresh too, and one after it ended looks again',
      files: [exe(CODEX)],
      effects: {
        run: { 'codex --version': [HELD, said('1.0.1\n')] },
        latest: { codex: [release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex', name: 'first' },
        { op: 'check', id: 'codex', name: 'second' },
        { op: 'check', id: 'codex', refresh: true, name: 'third' },
        give('codex', said('1.0.0\n')),
        { op: 'check', id: 'codex', refresh: true, name: 'fourth' },
      ],
    },
    {
      name: 'a look of every harness shares the look of one that is running',
      files: [exe(CODEX)],
      effects: {
        run: { 'codex --version': [HELD] },
        latest: { codex: [release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex', name: 'one' },
        { op: 'check', name: 'all' },
        give('codex', said('1.0.0\n')),
      ],
    },
    {
      name: 'the looks of two harnesses run side by side and each is shared on its own',
      files: [exe(CODEX), exe('$ROOT/bin/pi')],
      effects: {
        run: { 'codex --version': [HELD], 'pi --version': [HELD] },
        latest: { codex: [release('1.0.1')], pi: [release('0.2.0')] },
      },
      steps: [
        { op: 'check', id: 'codex', name: 'codex' },
        { op: 'check', id: 'pi', name: 'pi' },
        { op: 'check', id: 'codex', name: 'codex again' },
        give('pi', said('0.1.0\n')),
        give('codex', said('1.0.0\n')),
      ],
    },
    {
      name: 'a look that waits on the feed is shared by the calls that ask meanwhile',
      files: [exe(CODEX)],
      effects: {
        run: { 'codex --version': [said('1.0.0\n')] },
        latest: { codex: [HELD] },
      },
      steps: [
        { op: 'check', id: 'codex', name: 'first' },
        { op: 'check', id: 'codex', refresh: true, name: 'second' },
        { op: 'release', kind: 'latest', call: 'codex', answer: release('1.0.1') },
      ],
    },
    {
      name: 'a look that failed is kept as it failed',
      files: [exe(CODEX)],
      effects: {
        run: { 'codex --version': [failed('Command failed: codex --version\n')] },
        latest: { codex: [{ error: 'offline' }] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'check', id: 'codex' },
      ],
    },
    {
      name: 'a check made while an update runs looks again, and the update then looks once more',
      files: [exe('$ROOT/home/.codex/bin/codex')],
      effects: {
        run: {
          'codex --version': [said('1.0.0\n'), said('1.0.0\n'), said('1.0.1\n')],
          'codex update': [HELD],
        },
        latest: { codex: [release('1.0.1'), release('1.0.1'), release('1.0.1')] },
      },
      steps: [
        { op: 'check', id: 'codex' },
        { op: 'update', id: 'codex', name: 'update' },
        { op: 'check', id: 'codex', refresh: true, name: 'meanwhile' },
        { op: 'release', kind: 'run', call: 'codex update', answer: said('Updated\n') },
      ],
    },
  ]
}
