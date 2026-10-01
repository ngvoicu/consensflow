import { randomBytes } from 'node:crypto'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { send as sendPi, shownSession } from '../channels/pi.js'
import { launchConfiguration } from '../channels.js'
import { preparePiExtension } from '../pi-install.js'
import { roleConfiguration } from '../role-skills.js'
import {
  admission,
  executableFor,
  recordState,
  SHOWS_ANOTHER,
  switchedTo,
  windowText,
} from './shared.js'

/**
 * Pi, for the new core. Pi takes the session name we give it (`--session-id`
 * creates it the first time and resumes it after) and the first message as
 * its last argument. ConsensFlow's extension runs inside Pi: a message is a
 * file in its inbox, which it hands to Pi only when Pi is idle and the human's
 * editor is empty, and acknowledges only once Pi shows it as a user message.
 * The same extension marks each settled turn, which Pi's own log cannot, and
 * says which conversation the window shows, so a /new or /resume in it is
 * followed.
 */
const HOLD = 'Pi has not said yet which conversation its window shows'

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
        launch: launchId,
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
          ? interactiveStart(identity, nativeSession, windowText(message))
          : interactiveResume(identity, resume, windowText(message))
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

    async ready({ launch }) {
      const shown = await shownSession(launch.channel)
      if (shown === null) return HOLD
      return shown === launch.nativeSession ? true : SHOWS_ANOTHER
    },

    async deliver({ launch, pane, host, text }) {
      const sent = await send(
        {
          launch: launch.channel,
          session: launch.nativeSession,
          bridge: host,
          pane: pane.id,
          generation: pane.generation,
        },
        windowText(text),
      )
      return admission(sent, 'Pi refused it', { queued: true })
    },

    async observe({ launch }) {
      const [record, shown] = await Promise.all([
        answers('pi', launch.nativeSession, env, {
          piSettlement: { directory: launch.channel.settled, launchId: launch.channel.launchId },
        }),
        shownSession(launch.channel),
      ])
      const observed = { ...recordState(record), waiting: null }
      if (shown === null) return { ...observed, waiting: { reason: HOLD } }
      return shown === launch.nativeSession ? observed : switchedTo(observed, shown)
    },

    transcript({ launch }) {
      return { harness: 'pi', session: launch.nativeSession }
    },
  }
}
