/**
 * The ledger as a recorded test opens it: the real one, which at its close
 * writes the database it left into the test's trace (`trace.mjs`), its folder
 * written «dir». What the engine asks of it is written where the engine asks
 * (`recording-dispatcher.mjs`); what the test asks of it is the test's own.
 */
import { dirname } from 'node:path'
import * as real from '../../../../src/ledger/index.js'
import { closed, opened } from './trace.mjs'

export * from '../../../../src/ledger/index.js'

export function openLedger(file, options) {
  const ledger = real.openLedger(file, options)
  opened(dirname(file))
  const close = ledger.close.bind(ledger)
  ledger.close = (...args) => {
    const answer = close(...args)
    closed(file)
    return answer
  }
  return ledger
}
