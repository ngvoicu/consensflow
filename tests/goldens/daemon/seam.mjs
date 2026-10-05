/**
 * The stand-ins a test gives a surface to call (the page's dispatcher, the
 * API's roster lookup): each call is recorded in the exchange or operation
 * that made it, with its arguments and what it answered, and the ledger calls
 * the stand-in makes while it serves are kept with it.
 */
import * as session from './session.mjs'

/** The exchange or operation the running code is part of, or undefined. */
function owner(frame) {
  for (let at = frame; at !== undefined; at = at.parent) {
    if (at.kind !== 'seam') return at.target
  }
  return undefined
}

const thenable = (result) => result !== null && typeof result?.then === 'function'

/** One call of the stand-in `name`, which `run` makes. */
function serve(name, method, args, run) {
  const parent = session.frames.getStore()
  const host = session.recording() === null ? undefined : owner(parent)
  if (host === undefined) return run()
  const seen = { seam: name, method, args: args.map(session.value), calls: [] }
  host.seams.push(seen)
  const settle = {
    answered: (answer) => {
      seen.result = session.value(answer)
      return answer
    },
    refused: (cause) => {
      seen.refusal = session.refusal(cause)
      throw cause
    },
  }
  let result
  try {
    result = session.frames.run({ kind: 'seam', target: seen, parent }, run)
  } catch (cause) {
    return settle.refused(cause)
  }
  return thenable(result) ? result.then(settle.answered, settle.refused) : settle.answered(result)
}

/** `target`, an object of stand-in methods, recording each call to one. */
export function proxy(name, target) {
  return new Proxy(target, {
    get(self, property, receiver) {
      const method = Reflect.get(self, property, receiver)
      if (typeof method !== 'function' || typeof property !== 'string') return method
      return (...args) => serve(name, property, args, () => method.apply(self, args))
    },
  })
}

/** `fn`, a stand-in function, recording each call to it. */
export const fn =
  (name, stand) =>
  (...args) =>
    serve(name, 'call', args, () => stand(...args))
