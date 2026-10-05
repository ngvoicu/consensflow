/**
 * The ledger as a recorded test opens it: the real one, whose events (one
 * for each change it makes, whoever asked) go into the test's trace in their
 * place, and which at its close writes the database it left, its folder
 * written «dir». What the engine asks of it is written where the engine asks
 * (`recording-dispatcher.mjs`).
 */
import { dirname } from 'node:path'
import * as real from '../../../../src/ledger/index.js'
import { closed, opened, record } from './trace.mjs'

export * from '../../../../src/ledger/index.js'

export function openLedger(file, options = {}) {
  const told = options.trace ?? (() => {})
  const ledger = real.openLedger(file, {
    ...options,
    // Each event the ledger logs, in its place among what the engine does.
    trace: (event) => {
      record({ seam: 'event', event: JSON.parse(JSON.stringify(event)) })
      return told(event)
    },
  })
  opened(dirname(file))
  const close = ledger.close.bind(ledger)
  ledger.close = (...args) => {
    const answer = close(...args)
    closed(file)
    return answer
  }
  return ledger
}
