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
import { admission, dialogWaiting, executableFor, recordState } from './shared.js'

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
 * the human is looking at another one) and nothing is sent.
 *
 * The window's live status, which the plugin reports with the conversation
 * it shows, has the last word where the store cannot: a refused request
 * never reaches the store (OpenCode waits to retry it until the limit
 * resets), and an answer a lost window never finished stays unfinished there
 * for good once the conversation is reopened, though OpenCode is idle.
 */
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
      const runner =
        resume === null
          ? interactiveStart({ kind: 'opencode', model: agent?.model }, nativeSession, null)
          : interactiveResume({ kind: 'opencode' }, resume, null)
      return {
        argv: [executable, ...configuration.args, ...runner.args],
        env: { ...configuration.env, ...roleSetup.env },
        dropEnv: runner.dropEnv,
        nativeSession,
        launch: {
          nativeSession,
          channel: configuration.channel,
          directory,
          firstMessage: message,
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
      return (await shown(launch))?.sessionId === launch.nativeSession
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
      return admission(sent, 'OpenCode refused it', { queued: true })
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
      return {
        ...state,
        quota: retry ?? state.quota,
        settled: showing && !retrying && (state.settled || idle),
        waiting: dialogWaiting(record),
      }
    },

    transcript({ launch }) {
      return { harness: 'opencode', session: launch.nativeSession }
    },
  }
}
