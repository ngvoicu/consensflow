import { open } from 'node:fs/promises'
import { setTimeout as wait } from 'node:timers/promises'
import { selectedSession, shownIn } from '../../hosts/devin-hooks.mjs'
import { cachedAnswers } from '../../hosts/lib/completion.js'
import { DEVIN_REFUSAL, exhaustedQuota } from '../../hosts/lib/quota.js'
import { interactiveResume, interactiveStart } from '../../hosts/lib/windows.js'
import { send as sendDevin } from '../channels/devin.js'
import { prepareDevinIntegration, prepareDevinPrompt } from '../devin-install.js'
import { roleConfiguration } from '../role-skills.js'
import {
  admission,
  dialogWaiting,
  executableFor,
  recordState,
  SHOWS_ANOTHER,
  switchedTo,
  windowText,
} from './shared.js'

/**
 * Devin, for the new core. Each launch runs on a config of our own (the
 * owner's, plus hooks that log every turn and carry the role instructions),
 * in full-permission mode, with the first message in a prompt file. Devin
 * names its session itself; its own wire log for this launch says which one
 * the window opened, and which one it shows after a /new or /resume, so the
 * window is followed to it. A message is pasted, behind whatever the input
 * box holds, and only while Devin still shows the conversation we know.
 */
const HOLD = 'Devin has not said yet which conversation its window shows'

export function devinAdapter({
  env,
  send = sendDevin,
  answers = cachedAnswers(),
  discoverEveryMs = 250,
  discoverForMs = 60_000,
}) {
  return {
    harness: 'devin',
    // Devin's own status line says it: "esc twice to interrupt".
    interrupt: { presses: 2 },

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
        cwd: directory,
        executable,
        content: instructions,
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
        windowText(text),
      )
      return admission(sent, 'Devin refused the paste')
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
      if (shown === undefined) return { ...observed, waiting: { reason: HOLD } }
      return shown === launch.nativeSession ? observed : switchedTo(observed, shown)
    },

    transcript({ launch }) {
      return { harness: 'devin', session: launch.nativeSession, wire: launch.channel.wire }
    },
  }
}

/**
 * What Devin's own wire log of this launch says: the conversation the window
 * shows, and its word on its quota, a refusal in its message text after the
 * latest prompt ("Reached overall message rate limit … reset in 35 minutes",
 * "Usage limit reached", "Quota exhausted"). The log only grows, so each look
 * reads what was appended since the last.
 */
async function readWire(channel) {
  const file = await open(channel.wire, 'r').catch(() => null)
  if (file === null) return { shown: channel.shown, quota: channel.quota ?? null }
  try {
    const { size } = await file.stat()
    let from = channel.wireOffset ?? 0
    if (size < from) from = 0
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
      }
    }
  } finally {
    await file.close()
  }
  return { shown: channel.shown, quota: channel.quota ?? null }
}
