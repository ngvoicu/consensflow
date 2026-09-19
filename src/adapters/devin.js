import { setTimeout as wait } from 'node:timers/promises'
import { selectedSession } from '../../hosts/devin-receiver.mjs'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/runners.js'
import { send as sendDevin } from '../channels/devin.js'
import { prepareDevinIntegration, prepareDevinPrompt } from '../devin-install.js'
import { roleConfiguration } from '../role-skills.js'
import { admission, executableFor, recordState } from './shared.js'

/**
 * Devin, for the new core. Each launch runs on a config of our own (the
 * owner's, plus hooks that log every turn and carry the role instructions),
 * in full-permission mode, with the first message in a prompt file. Devin
 * names its session itself; its own wire log for this launch says which one
 * the window opened. A message is pasted, and only while no human is typing
 * and Devin still shows the conversation we know.
 */
export function devinAdapter({
  env,
  send = sendDevin,
  answers = cachedAnswers(),
  discoverEveryMs = 250,
  discoverForMs = 60_000,
}) {
  return {
    harness: 'devin',

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('devin', env)
      const configuration = await prepareDevinIntegration(env, {
        launchId,
        node: env.CONSENSFLOW_NODE ?? process.execPath,
        executable,
      })
      const roleSetup = await roleConfiguration('devin', {
        role,
        env,
        cwd: directory,
        executable,
        content: instructions,
      })
      const runner = await prepareDevinPrompt(
        resume === null
          ? interactiveStart({ kind: 'devin', model: agent?.model }, null, message)
          : interactiveResume({ kind: 'devin' }, resume, message),
        configuration,
      )
      return {
        argv: [executable, ...configuration.args, ...runner.args],
        env: { ...configuration.env, ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession: resume,
        launch: { nativeSession: resume, channel: configuration.channel },
      }
    },

    async started({ launch }) {
      if (launch.nativeSession !== null) return {}
      const deadline = Date.now() + discoverForMs
      while (Date.now() < deadline) {
        const session = await selectedSession(launch.channel.wire).catch(() => null)
        if (session !== null) {
          launch.nativeSession = session
          return { nativeSession: session }
        }
        await wait(discoverEveryMs)
      }
      throw new Error('Devin never said which session it opened (its wire log stayed empty)')
    },

    async ready({ pane, host }) {
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
      const sent = await send(
        {
          channel: launch.channel,
          session: launch.nativeSession,
          bridge: host,
          pane: pane.id,
          generation: pane.generation,
          epoch: snapshot.inputEpoch,
        },
        text,
      )
      return admission(sent, 'Devin refused the paste')
    },

    async observe({ launch }) {
      if (launch.nativeSession === null) {
        return { items: [], settled: false, waiting: null, failed: false }
      }
      const record = await answers('devin', launch.nativeSession, env)
      return { ...recordState(record), waiting: null }
    },

    transcript({ launch }) {
      return { harness: 'devin', session: launch.nativeSession, wire: launch.channel.wire }
    },
  }
}
