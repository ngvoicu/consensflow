import { execFile } from 'node:child_process'
import { setTimeout as wait } from 'node:timers/promises'
import { promisify } from 'node:util'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import {
  sessionAvailable as brokerAvailable,
  currentSession as brokerSession,
  send as sendCodex,
} from '../channels/codex.js'
import { launchConfiguration, withNativeBridge } from '../channels.js'
import { runnable } from '../harnesses.js'
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

/**
 * A window that opens on Codex's "Update now?" prompt, whenever a newer
 * release exists, never starts its thread, so a chief opened without a first
 * message waited on it for good. And a login shell re-reads the user's
 * profile, which can put another install's `cf` ahead of this daemon's (an
 * eval worker's `cf` was the live app's older one): without it, commands keep
 * the PATH the daemon gave the window. Both probed on Codex 0.156.1.
 */
const WINDOW = ['-c', 'check_for_update_on_startup=false', '-c', 'allow_login_shell=false']

/** The MCP servers Codex would start, as `codex mcp list --json` names them. */
async function codexMcpServers(executable, env) {
  const run = runnable(executable, ['mcp', 'list', '--json'], env)
  try {
    const { stdout } = await promisify(execFile)(run.file, run.args, {
      ...run.options,
      env,
      timeout: 15_000,
      maxBuffer: 1024 * 1024,
    })
    const servers = JSON.parse(stdout)
    return Array.isArray(servers) ? servers : []
  } catch (cause) {
    throw new Error(`could not list Codex's MCP servers to switch them off: ${cause.message}`)
  }
}

/**
 * A member runs in full-permission mode and reads what others wrote, so every
 * MCP server Codex would start is switched off: this Mac's Codex drives the
 * browser and the screen through them (the ChatGPT app's, since 2026-09-26).
 * Each gets a harmless, disabled definition; a bare `enabled=false` is refused
 * for servers defined outside config.toml, and a name that needs quotes would
 * define a new server instead, so such a name stops the launch.
 */
function mcpIsolation(servers) {
  return servers.flatMap(({ name }) => {
    if (!/^[A-Za-z0-9_-]+$/.test(name ?? '')) {
      throw new Error(`cannot switch off the Codex MCP server ${JSON.stringify(name)} for a member`)
    }
    return [
      '-c',
      `mcp_servers.${name}.command="/usr/bin/true"`,
      '-c',
      `mcp_servers.${name}.enabled=false`,
    ]
  })
}

export function codexAdapter({
  env,
  harness = 'codex',
  send = sendCodex,
  currentSession = brokerSession,
  sessionAvailable = brokerAvailable,
  mcpServers = codexMcpServers,
  answers = cachedAnswers(),
  discoverEveryMs = 250,
  discoverForMs = 60_000,
}) {
  return {
    harness,

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
      // The chief works with the human and keeps the human's connectors.
      const isolation = role === 'chief' ? [] : mcpIsolation(await mcpServers(executable, env))
      const runner =
        resume === null
          ? interactiveStart(
              { kind: harness, model: agent?.model, effort: agent?.effort },
              null,
              message,
            )
          : interactiveResume({ kind: harness }, resume, message)
      // Codex's question tool (request_user_input) is behind a feature still
      // marked under development; the broker answers it from the board.
      const invocation = withNativeBridge(
        {
          command: executable,
          args: [...roleSetup.args, ...QUESTION_TOOL, ...WINDOW, ...isolation, ...runner.args],
        },
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
      // Held, not failed: a refusal here would spend the message's attempts
      // in seconds (an answer to a Codex worker was lost that way).
      if (launch.channel !== null)
        return (await sessionAvailable(launch.channel))
          ? true
          : 'the Codex window cannot take a message yet: starting, resuming or reconnecting'
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
