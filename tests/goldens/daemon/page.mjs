/**
 * The page operations, recorded: each operation reports its name, its body,
 * the reply the daemon would put on the bridge (or the words it refused in),
 * whether it kicked, what its own ledger calls drew and logged, and every call
 * it made on the stand-ins the test gave it (the dispatcher). A ledger the
 * test did not open through the recorder is a stand-in too.
 */
import { pageOperations as real } from '../../../src/core/page.js'
import * as seam from './seam.mjs'
import * as session from './session.mjs'

export * from '../../../src/core/page.js'

/** What the daemon puts on the bridge for an operation's answer, or for the words it refused in. */
const reply = (answer) => JSON.stringify({ ok: true, ...answer })
const refused = (cause) =>
  JSON.stringify({ ok: false, error: cause instanceof Error ? cause.message : String(cause) })

function operation(name, handle) {
  return async (body) => {
    if (session.recording() === null) return handle(body)
    const world = session.worldBefore()
    const step = {
      kind: 'operation',
      id: session.next('operation'),
      name,
      body: JSON.parse(JSON.stringify(body ?? null)),
      kicks: 0,
      clock: [],
      names: [],
      events: [],
      seams: [],
    }
    const interval = session.begin(step)
    try {
      const answer = await session.frames.run({ kind: 'operation', target: step }, () =>
        handle(body),
      )
      step.reply = reply(answer)
      return answer
    } catch (cause) {
      step.reply = refused(cause)
      step.refusal = session.refusal(cause)
      throw cause
    } finally {
      session.worldAfter(world, step)
      session.end(interval, { operation: step.id })
    }
  }
}

export function pageOperations({ ledger, dispatcher, env, kick }) {
  session.environment(env)
  const operations = real({
    ledger: session.isRecorded(ledger) ? ledger : seam.proxy('ledger', ledger),
    dispatcher: seam.proxy('dispatcher', dispatcher),
    env,
    kick: () => {
      session.kicked()
      return kick()
    },
  })
  return Object.fromEntries(
    Object.entries(operations).map(([name, handle]) => [name, operation(name, handle)]),
  )
}
