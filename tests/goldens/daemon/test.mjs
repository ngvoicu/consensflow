/**
 * `node:test` as the suites import it: each test runs as the test of the
 * trace it makes (`session.beginTest` and `endTest`), with its name and the
 * `describe`s around it. Nothing else changes: the body is called with what
 * it was given, and what it returns is returned.
 */
import { basename } from 'node:path'
import * as real from 'node:test'
import * as session from './session.mjs'

export * from 'node:test'

/** The `describe`s whose bodies are running now, which register the tests inside them. */
const suites = []

function run(test, body, self, args) {
  session.beginTest(test)
  let result
  try {
    result = body.apply(self, args)
  } catch (cause) {
    session.endTest()
    throw cause
  }
  if (result === null || typeof result?.then !== 'function') {
    session.endTest()
    return result
  }
  return result.then(
    (done) => {
      session.endTest()
      return done
    },
    (cause) => {
      session.endTest()
      throw cause
    },
  )
}

function registered(original) {
  const register = (declaration) =>
    function (...args) {
      const at = args.findIndex((arg) => typeof arg === 'function')
      const name = typeof args[0] === 'string' ? args[0] : (args[at]?.name ?? '')
      if (at >= 0) {
        const body = args[at]
        const test = { file: basename(process.argv[1] ?? 'unknown'), path: [...suites, name] }
        args[at] = Object.defineProperty(
          function (...given) {
            return run(test, body, this, given)
          },
          'length',
          { value: body.length },
        )
      }
      return declaration.apply(this, args)
    }
  const wrapped = register(original)
  for (const variant of ['skip', 'todo', 'only']) {
    if (typeof original[variant] === 'function') wrapped[variant] = register(original[variant])
  }
  return wrapped
}

export const it = registered(real.it)
export const test = registered(real.test)
export default registered(real.default)

/** `describe`, with the names of the blocks a test is registered in. */
export const describe = (...args) => {
  const at = args.findIndex((arg) => typeof arg === 'function')
  const name = typeof args[0] === 'string' ? args[0] : (args[at]?.name ?? '')
  const body = args[at]
  args[at] = function (...given) {
    suites.push(name)
    try {
      return body.apply(this, given)
    } finally {
      suites.pop()
    }
  }
  return real.describe(...args)
}
