import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { daemonCommand } from '../helpers.mjs'
import { startIntegration } from './harness.mjs'

/**
 * A window that does not come up says why on its own screen, and the screen
 * goes with the window: Pi on a machine with no login prints "No API key found
 * for the selected model … Use /login" and waits, and every task given to it
 * failed with only "the window never showed its first message". The daemon now
 * keeps what the window last showed (the real pane host's screen, and the
 * program's exit code where it ended) and quotes it in the failure the
 * requester and the human hear, and in one line of its log. A fake harness in
 * a real PTY prints those words and stays (or ends with the code 3), and the
 * daemon is told to wait eight seconds, not three minutes, for its first
 * message (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`): long enough for a slow machine's
 * window to have printed. The native daemon only: Node's said nothing of a
 * window's screen.
 */

const FAKE_AGENT = fileURLToPath(new URL('./fake-agent.mjs', import.meta.url))
const NATIVE = daemonCommand().kind === 'native'

/** The two lines the window printed, as a quote holds them. */
const SCREEN =
  'No API key found for the selected model\\. \\/ Use \\/login to log into a provider\\.'
/** The worker's window is a session of it: `@worker-amber-pine`. */
const WORKER = '@worker-[a-z]+-[a-z]+'

/**
 * A project whose chief is a fake Claude that works, and whose worker is one
 * with no login (`fakeEnv` says how), given a task by the chief. Resolves once
 * the task failed.
 */
async function aTaskThatFailed(fakeEnv) {
  const app = await startIntegration({
    fakeEnv: {
      CF_TEST_HARNESS: FAKE_AGENT,
      CONSENSFLOW_LAUNCH_TIMEOUT_MS: '8000',
      ...fakeEnv,
    },
  })
  try {
    const opened = await app.requestNode('project.open', {
      directory: app.workspace,
      agent: 'chief',
    })
    assert.equal(opened.ok, true, JSON.stringify(opened))
    const project = opened.project.id
    const added = await app.requestNode('member.add', { project, agent: 'worker' })
    assert.equal(added.ok, true, JSON.stringify(added))
    const chief = await app.openFrame(`p${project}-chief`)
    await app.tell(project, `DISPATCH --tier ${added.member.tier} Reply with exactly: NEVER_SHOWN`)
    const task = async () => (await app.requestNode('task.get', { project, task: 1 })).task
    await app.waitFor(async () => (await task())?.state === 'failed', 60_000)
    const inbox = async (participant) =>
      (await app.requestNode('inbox.get', { project, participant })).messages
    const log = () => readFileSync(join(app.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
    const session = chief.argv[chief.argv.indexOf('--session-id') + 1]
    return { app, project, inbox, log, session }
  } catch (cause) {
    await app.close()
    throw cause
  }
}

describe('a window that does not come up says why', {
  skip: !NATIVE && "Node's daemon says nothing of a window's screen",
}, () => {
  it('quotes the screen of a window that stayed, in the failure, the chief’s window and the log', async () => {
    const { app, project, inbox, log, session } = await aTaskThatFailed({
      CF_TEST_NO_LOGIN: 'worker',
    })
    try {
      const told = (await inbox('chief')).find(
        (message) => message.kind === 'note' && message.body.startsWith('T-1 failed:'),
      )
      assert.ok(told, JSON.stringify(await inbox('chief')))
      assert.match(
        told.body,
        new RegExp(
          `^T-1 failed: the window never showed its first message; its screen ended with: "${SCREEN}"\\. Reopen it with: cf task reopen T-1 "…"$`,
        ),
      )
      // The human hears it too, with whom it did not reach.
      assert.ok(
        (await inbox('human')).some((message) =>
          new RegExp(
            `^m-\\d+, a task from @chief on T-1, did not reach ${WORKER}: the window never showed its first message; its screen ended with: "${SCREEN}"\\.$`,
          ).test(message.body),
        ),
        JSON.stringify(await inbox('human')),
      )
      // The chief's own window shows it: it was pasted in, and its record kept it.
      await app.waitFor(
        () => app.transcript(session).includes('No API key found for the selected model'),
        30_000,
      )
      // One line of the daemon's log.
      assert.match(
        log(),
        new RegExp(
          `^\\S+ warn the launch of p${project}-worker-[a-z]+-[a-z]+ failed: the window never showed its first message; its screen ended with: "${SCREEN}"$`,
          'm',
        ),
      )
    } finally {
      await app.close()
    }
  })

  it('quotes the screen and the exit code of a window that ended', async () => {
    const { app, project, inbox, log } = await aTaskThatFailed({
      CF_TEST_NO_LOGIN_EXITS: 'worker',
    })
    try {
      const told = (await inbox('chief')).find(
        (message) => message.kind === 'note' && message.body.startsWith('T-1 failed:'),
      )
      assert.ok(told, JSON.stringify(await inbox('chief')))
      assert.match(
        told.body,
        new RegExp(
          `^T-1 failed: ${WORKER}'s window closed \\(exit code 3\\); its screen ended with: "${SCREEN}"\\. Reopen it with: cf task reopen T-1 "…"$`,
        ),
      )
      assert.match(
        log(),
        new RegExp(
          `^\\S+ warn the launch of p${project}-worker-[a-z]+-[a-z]+ failed: ${WORKER}'s window closed \\(exit code 3\\); its screen ended with: "${SCREEN}"$`,
          'm',
        ),
      )
    } finally {
      await app.close()
    }
  })
})
