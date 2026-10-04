/**
 * Loaded with `node --import`: every import of the ledger, by a test or by
 * the daemon's modules a test drives, gets the recording one instead
 * (`recording.mjs`), which itself imports the real one.
 */
import { registerHooks } from 'node:module'

const LEDGER = new URL('../../../src/ledger/index.js', import.meta.url).href
const RECORDING = new URL('./recording.mjs', import.meta.url).href

registerHooks({
  resolve(specifier, context, nextResolve) {
    const resolved = nextResolve(specifier, context)
    if (resolved.url !== LEDGER || context.parentURL === RECORDING) return resolved
    return { ...resolved, url: RECORDING, shortCircuit: true }
  },
})
