/**
 * Loaded with `node --import`: a recorded test file gets the recording
 * dispatcher and ledger, and `node:test` naming each test's trace; the
 * recording modules themselves get the real ones.
 */
import { registerHooks } from 'node:module'

const here = (file) => new URL(file, import.meta.url).href
const SOURCE = new URL('../../../../src/', import.meta.url).href
const REPLACED = new Map([
  [`${SOURCE}core/dispatcher.js`, here('./recording-dispatcher.mjs')],
  [`${SOURCE}ledger/index.js`, here('./recording-ledger.mjs')],
  ['node:test', here('./naming.mjs')],
])
const RECORDING = new Set(REPLACED.values())

registerHooks({
  resolve(specifier, context, nextResolve) {
    const resolved = nextResolve(specifier, context)
    const replacement = REPLACED.get(resolved.url)
    if (replacement === undefined || RECORDING.has(context.parentURL)) return resolved
    return { ...resolved, url: replacement, format: 'module', shortCircuit: true }
  },
})
