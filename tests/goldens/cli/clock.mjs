/**
 * Preloaded into the CLI under recording (`node --import`): its clock reads
 * one instant whenever it is read, so that the time an agent is stamped with
 * is the same text in every recording (`instant.mjs`).
 */
import { CLOCK } from './instant.mjs'

const FIXED = Date.parse(CLOCK)

class FixedDate extends Date {
  constructor(...args) {
    if (args.length === 0) super(FIXED)
    else super(...args)
  }

  static now() {
    return FIXED
  }
}

globalThis.Date = FixedDate
