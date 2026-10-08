import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { NATIVE_CF } from '../choice.mjs'
import { startIntegration } from './harness.mjs'
import { project, staff } from './staff.mjs'

/**
 * `cf <verb> --help` in a window, end to end against the real daemon
 * (`npm run test:integration`): every verb of the board answers `--help` and
 * `-h` with its usage and exit code 0, and asks the board nothing: what the
 * chief's report of 2026-10-08 found posted as a note saying "--help" (m-1542)
 * and refused as no task is the usage. A text that holds the word among others,
 * or follows a `--`, is still text.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))

/** Every command of the board that takes words, as the usage names it. */
const VERBS = [
  ['note'],
  ['ask'],
  ['tell'],
  ['answer'],
  ['inbox'],
  ['inbox', 'read'],
  ['staff'],
  ['whoami'],
  ['history'],
  ['task', 'add'],
  ['task', 'list'],
  ['task', 'get'],
  ['task', 'done'],
  ['task', 'accept'],
  ['task', 'cancel'],
  ['task', 'reopen'],
  ['task', 'pause'],
  ['task', 'resume'],
]

test('a window asks every verb of cf for help, and nothing is posted or read', async () => {
  const app = await startIntegration({ fakeEnv: { CF_TEST_HARNESS: FAKE_AGENT } })
  try {
    staff(app)
    const p = await project(app, { members: [['worker', 'worker']] })
    const frame = await app.openFrame(`p${p.id}-chief`)
    // `cf` as the chief runs it in its window: its own token, the native binary.
    const cf = (...words) =>
      spawnSync(NATIVE_CF, words, {
        env: {
          ...app.env,
          CONSENSFLOW_URL: frame.env.CONSENSFLOW_URL,
          CONSENSFLOW_TOKEN: frame.env.CONSENSFLOW_TOKEN,
        },
        encoding: 'utf8',
      })
    const everything = async () => ({
      human: await p.inbox('human'),
      chief: await p.inbox('chief'),
      tasks: (await p.board()).lanes.flatMap((lane) => lane.tasks.map((task) => task.number)),
    })
    const before = await everything()

    for (const verb of VERBS) {
      for (const word of ['--help', '-h']) {
        const ran = cf(...verb, word)
        const said = `cf ${verb.join(' ')} ${word}`
        assert.equal(ran.status, 0, `${said}: ${ran.stderr}`)
        assert.equal(ran.stderr, '', said)
        // The command's own line of the usage, in the list of them.
        assert.match(ran.stdout, new RegExp(`^  cf ${verb[0]} `), said)
      }
    }
    assert.deepEqual(await everything(), before, 'the board is as it was: nothing posted')

    // The same word as text, which a command takes as it takes any.
    const noted = cf('note', '--', '--help')
    assert.equal(noted.status, 0, noted.stderr)
    assert.match(noted.stdout, /^m-\d+ noted to @human; nothing waits on it\.\n$/)
    const seen = cf('note', 'see', '--help')
    assert.equal(seen.status, 0, seen.stderr)
    const bodies = (await p.inbox('human')).filter((m) => m.kind === 'note').map((m) => m.body)
    assert.deepEqual(bodies.sort(), ['--help', 'see --help'])
  } finally {
    await app.close()
  }
})
