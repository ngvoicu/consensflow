import { open } from 'node:fs/promises'
import { setTimeout as wait } from 'node:timers/promises'
import { selectedSession, shownIn } from '../../hosts/devin-hooks.mjs'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { DEVIN_REFUSAL, exhaustedQuota } from '../../hosts/lib/quota.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { send as sendDevin } from '../channels/devin.js'
import { consoleText } from '../console-text.js'
import { prepareDevinIntegration, prepareDevinPrompt } from '../devin-install.js'
import { onWindows } from '../harnesses.js'
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
 * Devin, for the daemon. Each launch runs on a config of our own (the
 * owner's, plus a hook that gives a session its role instructions),
 * in full-permission mode, with the first message in a prompt file. Devin
 * names its session itself; its own wire log for this launch says which one
 * the window opened, and which one it shows after a /new or /resume, so the
 * window is followed to it. A message is pasted, behind whatever the input
 * box holds, and only while Devin still shows the conversation we know.
 */
const HOLD = 'Devin has not said yet which conversation its window shows'

/**
 * Devin's shell on Windows is Git Bash, which names folders /c/Users/…, and
 * its file tools write such a path to C:\c\Users\…: a Devin worker's file
 * landed there and its task's work was lost (2026-10-03).
 */
const WINDOWS_PATHS =
  'This machine runs Windows and your shell is Git Bash: give file tools Windows paths (C:\\Users\\…) or paths relative to the project folder, never /c/… paths, which they write under C:\\c\\.'

/** The role text a Devin window gets: on Windows, with how to name a file there. */
export const devinRoleText = (instructions, env) =>
  onWindows(env) ? `${instructions}\n\n${WINDOWS_PATHS}\n` : instructions

export function devinAdapter({
  env,
  send = sendDevin,
  answers = cachedAnswers(),
  discoverEveryMs = 250,
  discoverForMs = 60_000,
}) {
  return {
    harness: 'devin',
    // Devin's own status line says it: "esc twice to interrupt". The same two
    // at a Devin already idle open its rewind ("Revert"), and the next Enter
    // confirms it, cutting the conversation back: a third Escape a second
    // later closes it, and changes nothing after a stopped turn or at an idle
    // prompt (probed 2026-10-03).
    interrupt: { presses: 2, closeAfterMs: 1_000 },

    async prepare({ launchId, role, directory, resume, message, agent, instructions }) {
      const executable = executableFor('devin', env)
      const configuration = await prepareDevinIntegration(env, {
        launchId,
        node: env.CONSENSFLOW_NODE ?? process.execPath,
        executable,
        boardQuestions: role !== 'chief',
      })
      const roleSetup = await roleConfiguration('devin', {
        role,
        env,
        launch: launchId,
        cwd: directory,
        executable,
        content: devinRoleText(instructions, env),
      })
      const identity = { kind: 'devin', model: agent?.model, effort: agent?.effort }
      const runner = await prepareDevinPrompt(
        resume === null
          ? interactiveStart(identity, null, windowText(message))
          : interactiveResume(identity, resume, windowText(message)),
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

    async ready({ launch, pane, host }) {
      const { shown } = await readWire(launch.channel)
      if (shown === undefined) return HOLD
      if (shown !== launch.nativeSession) return SHOWS_ANOTHER
      const snapshot = await host.request('pane.snapshot', pane)
      return snapshot?.ok === true && !snapshot.pasteInFlight
    },

    async deliver({ launch, pane, host, text }) {
      const sent = await send(
        {
          channel: launch.channel,
          session: launch.nativeSession,
          bridge: host,
          pane: pane.id,
          generation: pane.generation,
        },
        // Windows' console drops a paste's non-ASCII marks on their way to Devin.
        onWindows(env) ? consoleText(windowText(text)) : windowText(text),
      )
      return admission(sent, 'Devin refused the paste')
    },

    /** Its harness's own record of a conversation, read with no window open. */
    record({ conversation }) {
      return answers('devin', conversation.nativeSession, env)
    },

    async observe({ launch }) {
      if (launch.nativeSession === null) {
        return { items: [], settled: false, waiting: null, failed: false, quota: null }
      }
      const [record, { shown, quota }] = await Promise.all([
        answers('devin', launch.nativeSession, env),
        readWire(launch.channel),
      ])
      const observed = { ...recordState(record), waiting: dialogWaiting(record), quota }
      if (shown === undefined) return unnamed(observed, HOLD)
      return shown === launch.nativeSession ? observed : switchedTo(observed, shown)
    },
  }
}

/**
 * What Devin's own wire log of this launch says: the conversation the window
 * shows, and its word on its quota, a refusal after the latest prompt: in its
 * message text ("Reached overall message rate limit … reset in 35 minutes",
 * "Usage limit reached", "Quota exhausted"), or as the prompt's own error.
 * The log only grows, so each look reads what was appended since the last.
 */
async function readWire(channel) {
  const file = await open(channel.wire, 'r').catch(() => null)
  if (file === null) return { shown: channel.shown, quota: channel.quota ?? null }
  try {
    const { size } = await file.stat()
    let from = channel.wireOffset ?? 0
    // A log that shrank was replaced: read from its start, carrying nothing of the old one.
    if (size < from) [from, channel.wireCarry] = [0, '']
    if (size > from) {
      const buffer = Buffer.alloc(size - from)
      await file.read(buffer, 0, size - from, from)
      channel.wireOffset = size
      const lines = `${channel.wireCarry ?? ''}${buffer.toString('utf8')}`.split('\n')
      channel.wireCarry = lines.pop()
      for (const line of lines) {
        let event
        try {
          event = JSON.parse(line)
        } catch {
          continue
        }
        channel.shown = shownIn(event) ?? channel.shown
        if (event.method === 'session/prompt') channel.quota = null
        const text =
          event.update?.sessionUpdate === 'agent_message_chunk' ? event.update.content?.text : null
        if (typeof text === 'string' && DEVIN_REFUSAL.test(text)) {
          channel.quota = exhaustedQuota(text, Date.now())
        }
        // Or a refusal of the prompt itself, as Devin 3000.11 writes one: a
        // JSON-RPC error, "Quota exhausted." (-32011, resource_exhausted).
        const refused = event.error
        if (
          refused?.code === -32011 ||
          refused?.data?.['cognition.ai/errorKind'] === 'resource_exhausted' ||
          (typeof refused?.message === 'string' && DEVIN_REFUSAL.test(refused.message))
        ) {
          channel.quota = exhaustedQuota(refused.message ?? '', Date.now())
        }
        // Or the turn's own end for it, as Devin wrote it on a Pro plan
        // (2026-10-03): cause quota_exhausted, its words in errorMessage
        // ("Your daily usage quota has been exhausted.").
        if (event.cause === 'quota_exhausted') {
          channel.quota = exhaustedQuota(event.errorMessage ?? '', Date.now())
        }
      }
    }
  } finally {
    await file.close()
  }
  return { shown: channel.shown, quota: channel.quota ?? null }
}
