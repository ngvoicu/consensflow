/**
 * `node:test` as a recorded test file imports it: each test it defines runs
 * in a trace of its own (`trace.mjs`), named by its suites, its sentence and
 * the line it is defined on. A suite's body runs as it is defined, so the
 * suites a test is in are the ones whose bodies run around its `it`.
 */
import * as real from 'node:test'
import { begin, end } from './trace.mjs'

export * from 'node:test'

/** The suites whose bodies run now, outermost first. */
const suites = []

/** The line of the recorded test file that called the function this is in. */
function caller() {
  const file = process.argv[1] ?? ''
  const frame = new Error().stack.split('\n').find((line) => line.includes(file)) ?? ''
  return Number(/:(\d+):\d+\)?$/.exec(frame)?.[1] ?? 0)
}

/** `define` (`it`, `test`), each test it defines run in its own trace. */
function traced(define) {
  return (name, ...rest) => {
    const test = { suites: [...suites], name, line: caller() }
    const fn = rest.pop()
    const run = async (...args) => {
      begin(test)
      try {
        return await fn(...args)
      } finally {
        end()
      }
    }
    return define(name, ...rest, run)
  }
}

/** `define` (`describe`, `suite`), the tests its body defines named in it. */
function grouping(define) {
  return (name, ...rest) => {
    const fn = rest.pop()
    return define(name, ...rest, (...args) => {
      suites.push(name)
      try {
        return fn(...args)
      } finally {
        suites.pop()
      }
    })
  }
}

export const it = traced(real.it)
export const test = traced(real.test)
export const describe = grouping(real.describe)
export const suite = grouping(real.suite)
export default test
