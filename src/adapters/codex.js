import { setTimeout as wait } from 'node:timers/promises'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { sessionState as brokerState, send as sendCodex } from '../channels/codex.js'
import { launchConfiguration, withNativeBridge } from '../channels.js'
import { roleConfiguration } from '../role-skills.js'
import {
  admission,
  executableFor,
  recordState,
  SHOWS_ANOTHER,
  switchedTo,
  unnamed,
  windowText,
} from './shared.js'

/**
 * Codex, for the daemon. Codex runs under ConsensFlow's supervisor, the
 * bundle's native `cf codex-session`: an app-server, a broker that knows the
 * thread the TUI shows and queues messages on it, and the TUI attached to
 * both. The first message is Codex's last argument; the broker names the
 * thread once Codex starts it, and again whenever the human starts or resumes
 * another one in the window (/new, /resume), so the window is followed to
 * it. A Codex too old for the native queue is refused: nothing could reach
 * its window. Every window starts with the MCP servers Codex has, the chief's
 * and a member's alike (the owner's choice, 2026-10-06): a launch never lists
 * them, and its command line switches none off.
 */
const QUESTION_TOOL = [
  '--enable',
  'default_mode_request_user_input',
  '-c',
  'suppress_unstable_features_warning=true',
]

/**
 * A window that opens on Codex's "Update now?" prompt, whenever a newer
 * release exists, never starts its thread, so a chief opened without a first
 * message waited on it for good. And a login shell re-reads the user's
 * profile, which can put another install's `cf` ahead of this daemon's (an
 * eval worker's `cf` was the live app's older one): without it, commands keep
 * the PATH the daemon gave the window. Both probed on Codex 0.156.1.
 */
const WINDOW = ['-c', 'check_for_update_on_startup=false', '-c', 'allow_login_shell=false']

/**
 * Why a message waits while the broker names no thread or cannot take one: a
 * refusal there would spend the message's attempts in seconds (an answer to a
 * Codex worker was lost that way), so it is held.
 */
const HOLD =
  'the Codex window cannot take a message yet: starting, switching conversations or reconnecting'

export function codexAdapter({
  env,
  send = sendCodex,
  sessionState = brokerState,
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
        launch: launchId,
        cwd: directory,
        executable,
        content: instructions,
      })
      // An image agent's window is Codex on its own default model, whose
      // image tool draws: it names no model or effort of its own.
      const identity =
        agent?.designer === true
          ? { kind: 'codex' }
          : { kind: 'codex', model: agent?.model, effort: agent?.effort }
      const runner =
        resume === null
          ? interactiveStart(identity, null, windowText(message))
          : interactiveResume(identity, resume, windowText(message))
      // Codex's question tool (request_user_input) is behind a feature still
      // marked under development; the broker answers a member's from the board.
      // The chief has none: it asks the human in plain words in its window.
      const questions = role === 'chief' ? [] : QUESTION_TOOL
      const invocation = withNativeBridge(
        {
          command: executable,
          args: [...roleSetup.args, ...questions, ...WINDOW, ...runner.args],
        },
        configuration,
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
      if (launch.nativeSession !== null) return {}
      const deadline = Date.now() + discoverForMs
      while (Date.now() < deadline) {
        const thread = (await sessionState(launch.channel))?.sessionId
        if (typeof thread === 'string') {
          launch.nativeSession = thread
          return { nativeSession: thread }
        }
        await wait(discoverEveryMs)
      }
      throw new Error('the Codex broker never named the thread it opened')
    },

    async ready({ launch }) {
      const shown = await sessionState(launch.channel)
      if (shown?.available !== true) return HOLD
      return shown.sessionId === launch.nativeSession ? true : SHOWS_ANOTHER
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
      return admission(sent, 'the Codex broker refused it', { queued: true })
    },

    /** Its harness's own record of a conversation, read with no window open. */
    record({ conversation }) {
      return answers('codex', conversation.nativeSession, env)
    },

    async observe({ launch }) {
      if (launch.nativeSession === null) {
        return { items: [], settled: false, waiting: null, failed: false, quota: null }
      }
      const [record, shown] = await Promise.all([
        answers('codex', launch.nativeSession, env),
        sessionState(launch.channel),
      ])
      const observed = { ...recordState(record), waiting: null }
      if (typeof shown?.sessionId !== 'string') return unnamed(observed, HOLD)
      return shown.sessionId === launch.nativeSession
        ? observed
        : switchedTo(observed, shown.sessionId)
    },
  }
}
