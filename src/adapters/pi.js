import { randomBytes } from 'node:crypto'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/runners.js'
import { send as sendPi } from '../channels/pi.js'
import { launchConfiguration } from '../channels.js'
import { preparePiExtension } from '../pi-install.js'
import { roleConfiguration } from '../role-skills.js'
import { admission, executableFor, recordState } from './shared.js'

/**
 * Pi, for the new core. Pi takes the session name we give it (`--session-id`
 * creates it the first time and resumes it after) and the first message as
 * its last argument. ConsensFlow's extension runs inside Pi: a message is a
 * file in its inbox, which it hands to Pi only when Pi is idle and the human's
 * editor is empty, and acknowledges only once Pi shows it as a user message.
 * The same extension marks each settled turn, which Pi's own log cannot.
 */
export function piAdapter({ env, send = sendPi, answers = cachedAnswers() }) {
  return {
    harness: 'pi',

    async prepare({
      launchId,
      participant,
      role,
      directory,
      resume,
      message,
      agent,
      instructions,
    }) {
      const executable = executableFor('pi', env)
      const extension = preparePiExtension(env)
      if (extension.path === null) {
        throw new Error(`ConsensFlow's Pi extension could not be installed: ${extension.reason}`)
      }
      const configuration = await launchConfiguration('pi', {
        launchId,
        workspace: directory,
        env,
        extensionPath: extension.path,
      })
      const roleSetup = await roleConfiguration('pi', {
        role,
        env,
        cwd: directory,
        executable,
        content: instructions,
      })
      const nativeSession =
        resume ??
        `cf-${participant.projectId}-${participant.handle}-${randomBytes(4).toString('hex')}`
      const identity = { kind: 'pi', model: agent?.model, thinking: agent?.thinking }
      const runner =
        resume === null
          ? interactiveStart(identity, nativeSession, message)
          : interactiveResume(identity, resume, message)
      return {
        argv: [executable, ...configuration.args, ...roleSetup.args, ...runner.args],
        env: { ...configuration.env, ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession,
        launch: { nativeSession, channel: configuration.channel },
      }
    },

    async started() {
      return {}
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
          launch: launch.channel,
          session: launch.nativeSession,
          bridge: host,
          pane: pane.id,
          generation: pane.generation,
          epoch: snapshot.inputEpoch,
        },
        text,
      )
      return admission(sent, 'Pi refused it')
    },

    async observe({ launch }) {
      const record = await answers('pi', launch.nativeSession, env, {
        piSettlement: { directory: launch.channel.settled, launchId: launch.channel.launchId },
      })
      return { ...recordState(record), waiting: null }
    },

    transcript({ launch }) {
      return { harness: 'pi', session: launch.nativeSession }
    },
  }
}
