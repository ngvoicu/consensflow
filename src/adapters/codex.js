import { setTimeout as wait } from 'node:timers/promises'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { currentSession as brokerSession, send as sendCodex } from '../channels/codex.js'
import { launchConfiguration, withNativeBridge } from '../channels.js'
import { roleConfiguration } from '../role-skills.js'
import { admission, executableFor, recordState } from './shared.js'

/**
 * Codex, for the new core. Where Codex has its native queue, it runs under
 * ConsensFlow's supervisor (`hosts/codex-session.mjs`): an app-server, a broker
 * that knows the thread the TUI shows and queues messages on it, and the TUI
 * attached to both. The first message is Codex's last argument; the broker
 * names the thread once Codex starts it. A Codex without the queue runs bare
 * and gets its messages pasted, only while no human is typing.
 */
const QUESTION_TOOL = [
  '--enable',
  'default_mode_request_user_input',
  '-c',
  'suppress_unstable_features_warning=true',
]

export function codexAdapter({
  env,
  send = sendCodex,
  currentSession = brokerSession,
  answers = cachedAnswers(),
  discoverEveryMs = 250,
  discoverForMs = 60_000,
}) {
  return {
    harness: 'codex',

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('codex', env)
      const configuration = await launchConfiguration('codex', {
        launchId,
        workspace: directory,
        env,
        executable,
      })
      const roleSetup = await roleConfiguration('codex', {
        role,
        env,
        cwd: directory,
        executable,
        content: instructions,
      })
      const runner =
        resume === null
          ? interactiveStart(
              { kind: 'codex', model: agent?.model, effort: agent?.effort },
              null,
              message,
            )
          : interactiveResume({ kind: 'codex' }, resume, message)
      // Codex's question tool (request_user_input) is behind a feature still
      // marked under development; the broker answers it from the board.
      const invocation = withNativeBridge(
        { command: executable, args: [...roleSetup.args, ...QUESTION_TOOL, ...runner.args] },
        configuration,
        env.CONSENSFLOW_NODE ?? process.execPath,
      )
      return {
        argv: [invocation.command, ...invocation.args],
        env: { ...configuration.env, ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession: resume,
        launch: { nativeSession: resume, channel: configuration.channel },
      }
    },

    async started({ launch }) {
      if (launch.nativeSession !== null || launch.channel === null) return {}
      const deadline = Date.now() + discoverForMs
      while (Date.now() < deadline) {
        const thread = await currentSession(launch.channel).catch(() => undefined)
        if (typeof thread === 'string') {
          launch.nativeSession = thread
          return { nativeSession: thread }
        }
        await wait(discoverEveryMs)
      }
      throw new Error('the Codex broker never named the thread it opened')
    },

    async ready({ launch, pane, host }) {
      if (launch.channel !== null) return true
      const snapshot = await host.request('pane.snapshot', pane)
      return snapshot?.ok === true && snapshot.draftLatched !== true && !snapshot.pasteInFlight
    },

    async deliver({ launch, pane, host, text }) {
      const snapshot = await host.request('pane.snapshot', pane)
      if (snapshot?.ok !== true) {
        return {
          admitted: false,
          reason: `the window cannot be read: ${snapshot?.error ?? 'no answer'}`,
        }
      }
      if (launch.channel === null) {
        const written = await host.request('pane.write_paste', {
          ...pane,
          epoch: snapshot.inputEpoch,
          body: text,
        })
        return admission(written, 'the window refused the paste')
      }
      const sent = await send(
        {
          launch: launch.channel,
          session: launch.nativeSession,
          bridge: host,
          pane: pane.id,
          generation: pane.generation,
          epoch: snapshot.inputEpoch,
        },
        text,
      )
      return admission(sent, 'the Codex broker refused it', { queued: true })
    },

    async observe({ launch }) {
      if (launch.nativeSession === null) {
        return { items: [], settled: false, waiting: null, failed: false, quota: null }
      }
      const record = await answers('codex', launch.nativeSession, env)
      return { ...recordState(record), waiting: null }
    },

    transcript({ launch }) {
      return { harness: 'codex', session: launch.nativeSession }
    },
  }
}
