// A person's own extension, loaded from Pi's settings (after the command line's): at each of its
// hooks it notes the word ConsensFlow's extension had written by then and whether Pi said it was
// idle, then does what the case asks of it (`globalThis.__piLoop.person[event]`).
import { readFileSync } from 'node:fs'

const EVENTS = [
  'session_start',
  'input',
  'before_agent_start',
  'agent_start',
  'turn_start',
  'agent_end',
  'agent_before_settle',
  'agent_settled',
  'session_before_compact',
  'session_compact',
]

export default function person(pi) {
  const loop = globalThis.__piLoop
  for (const event of EVENTS) {
    pi.on(event, async (payload, ctx) => {
      let word = null
      try {
        word = JSON.parse(readFileSync(loop.wordFile, 'utf8'))
      } catch {}
      loop.seen.push({ event, idle: ctx.isIdle(), state: word?.state ?? null, ended: word?.ended ?? null })
      // A person's own entries first (the case's second form), then what the case asks.
      await loop.extras?.[event]?.(payload, ctx, pi)
      const act = loop.person?.[event]
      if (act) return act(payload, ctx, pi)
      return undefined
    })
  }
}
