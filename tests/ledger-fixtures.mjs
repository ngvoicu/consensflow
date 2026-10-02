import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { openLedger } from '../src/ledger/index.js'

/**
 * What the ledger's tests share (TEST-BDC-01). The ledger is one SQLite file
 * in the home that holds every project, participant, task and inbox message;
 * each test gets a throwaway directory and a clock that moves one second per
 * reading.
 */

export async function withDir(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-ledger-'))
  try {
    return await fn(dir)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
}

export function clock() {
  let at = Date.parse('2026-09-19T10:00:00.000Z')
  return () => {
    at += 1000
    return new Date(at)
  }
}

/** Session names in a fixed order, so a test can say `zeus-amber-pine` and mean the first one. */
export function names() {
  const list = [
    'amber-pine',
    'brisk-birch',
    'calm-brook',
    'coral-canyon',
    'crisp-cedar',
    'dusky-cliff',
  ]
  let at = 0
  return () => list[at++ % list.length]
}

export async function withLedger(fn) {
  return withDir(async (dir) => {
    const ledger = openLedger(path.join(dir, 'consensflow.db'), { now: clock(), names: names() })
    try {
      return await fn(ledger, dir)
    } finally {
      ledger.close()
    }
  })
}

/** A project with a chief and two workers, the shape most tests start from. */
export function staff(ledger) {
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
  })
  ledger.addMember(project.id, {
    agent: 'diana',
    harness: 'codex',
    role: 'worker',
    tier: 'standard',
  })
  const id = (handle) => ledger.project(project.id).participants.find((p) => p.handle === handle).id
  return { project, id }
}

/** The id of a session (or any participant) by handle. */
export const sessionId = (ledger, projectId, handle) =>
  ledger.project(projectId).participants.find((p) => p.handle === handle).id

/** Delivers a message the way the dispatcher will: begin, then confirm. */
export function deliver(ledger, message) {
  ledger.beginDelivery(message.id)
  return ledger.confirmDelivery(message.id, { evidence: `native-${message.id}` })
}
