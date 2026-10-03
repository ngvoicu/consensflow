import { cachedAnswers } from '../../hosts/lib/completion.js'
import { opencodeRetryQuota } from '../../hosts/lib/quota.js'
import { childEnv, interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import {
  createSession as createOpenCodeSession,
  seedSession as seedOpenCodeSession,
  send as sendOpenCode,
  sessionState as shownState,
} from '../channels/opencode.js'
import { launchConfiguration } from '../channels.js'
import { prepareOpenCodeExtension } from '../opencode-install.js'
import { roleConfiguration } from '../role-skills.js'
import {
  admission,
  dialogWaiting,
  executableFor,
  recordState,
  SHOWS_ANOTHER,
  switchedTo,
  unnamed,
  windowText,
} from './shared.js'

/**
 * OpenCode, for the new core. A fresh conversation is created on a throwaway
 * `opencode serve` first, so its id is known before the window opens; the TUI
 * then runs its own server on a private port and password, with ConsensFlow's
 * plugin loaded. OpenCode ignores a prompt on a `--session` launch, so the
 * first message goes through that server once it answers, and every later one
 * through the plugin, which posts it to the session the TUI is showing.
 *
 * An empty conversation says nothing about the window: until the plugin
 * reports that the TUI shows this conversation, OpenCode is still loading (or
 * the human is on its home screen or session list) and nothing is sent. When
 * the human opens another conversation in it (/new, or one from the list),
 * the window is followed there.
 *
 * The window's live status, which the plugin reports with the conversation
 * it shows, has the last word where the store cannot: a refused request
 * never reaches the store (OpenCode waits to retry it until the limit
 * resets), and an answer a lost window never finished stays unfinished there
 * for good once the conversation is reopened, though OpenCode is idle.
 */
const STARTING = 'the OpenCode window is starting: its plugin does not answer yet'
const HOLD = 'the OpenCode window shows no conversation: its home screen or session list is open'

export function openCodeAdapter({
  env,
  createSession = createOpenCodeSession,
  seedSession = seedOpenCodeSession,
  send = sendOpenCode,
  sessionState = shownState,
  answers = cachedAnswers(),
}) {
  const shown = (launch) => sessionState(launch.channel).catch(() => undefined)

  return {
    harness: 'opencode',

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('opencode', env)
      const extension = prepareOpenCodeExtension(env)
      if (extension.path === null) {
        throw new Error(`ConsensFlow's OpenCode plugin could not be installed: ${extension.reason}`)
      }
      const roleSetup = await roleConfiguration('opencode', {
        role,
        env,
        launch: launchId,
        cwd: directory,
        executable,
        content: instructions,
      })
      const configuration = await launchConfiguration('opencode', {
        launchId,
        workspace: directory,
        env,
        executable,
        extensionPath: extension.path,
      })
      const nativeSession =
        resume ??
        (await createSession({
          executable,
          cwd: directory,
          env: childEnv({ ...env, ...roleSetup.env }),
          configuration,
        }))
      const identity = { kind: 'opencode', model: agent?.model }
      const runner =
        resume === null
          ? interactiveStart(identity, nativeSession, null)
          : interactiveResume(identity, resume, null)
      return {
        argv: [executable, ...configuration.args, ...runner.args],
        env: { ...configuration.env, ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession,
        launch: {
          nativeSession,
          channel: configuration.channel,
          directory,
          firstMessage: windowText(message),
          resumed: resume !== null,
          model: agent?.model,
          effort: agent?.effort,
        },
      }
    },

    async started({ launch }) {
      if (launch.firstMessage === null) return {}
      await seedSession({
        channel: launch.channel,
        sessionId: launch.nativeSession,
        cwd: launch.directory,
        text: launch.firstMessage,
        ...(launch.resumed ? { resume: true } : { model: launch.model, variant: launch.effort }),
      })
      return {}
    },

    async ready({ launch }) {
      const window = await shown(launch)
      if (window === undefined) return STARTING
      if (window.sessionId === null) return HOLD
      return window.sessionId === launch.nativeSession ? true : SHOWS_ANOTHER
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
      return admission(sent, 'OpenCode refused it', { queued: true })
    },

    /** Its harness's own record of a conversation, read with no window open. */
    record({ conversation }) {
      return answers('opencode', conversation.nativeSession, env)
    },

    async observe({ launch }) {
      const [record, window] = await Promise.all([
        answers('opencode', launch.nativeSession, env),
        shown(launch),
      ])
      const showing = window?.sessionId === launch.nativeSession
      const state = recordState(record)
      const retry = showing ? opencodeRetryQuota(window.status, Date.now()) : null
      const idle = showing && window.status?.type === 'idle'
      // A session waiting to retry a request is at work, whatever its record says.
      const retrying = showing && window.status?.type === 'retry'
      const observed = {
        ...state,
        quota: retry ?? state.quota,
        settled: showing && !retrying && (state.settled || idle),
        waiting: dialogWaiting(record),
      }
      // Until its plugin answers, the window names no conversation: a message
      // waits, as ready() holds it.
      if (window === undefined) return unnamed(observed, STARTING)
      if (window.sessionId === null) return unnamed(observed, HOLD)
      return showing ? observed : switchedTo(observed, window.sessionId)
    },

    transcript({ launch }) {
      return { harness: 'opencode', session: launch.nativeSession }
    },
  }
}
