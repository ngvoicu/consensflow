import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import test from 'node:test'
import { endOwned } from './support/owned-child.mjs'

test('ending a fixture process leaves a decoy using the same executable', () => {
  const owned = {
    executable: 'same-copy',
    alive: true,
    kill() {
      this.alive = false
    },
  }
  const decoy = { executable: 'same-copy', alive: true }
  const processes = [owned, decoy]
  endOwned(owned, (path) => {
    for (const process of processes) if (process.executable === path) process.alive = false
  })
  assert.equal(owned.alive, false)
  assert.equal(decoy.alive, true)
})

test('on Windows an owned child handle ends only its process', {
  skip: process.platform !== 'win32',
}, async () => {
  const owned = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 30000)'], { stdio: 'ignore' })
  const decoy = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 30000)'], { stdio: 'ignore' })
  const ownedClosed = once(owned, 'close')
  const decoyClosed = once(decoy, 'close')
  try {
    await Promise.all([once(owned, 'spawn'), once(decoy, 'spawn')])
    endOwned(owned)
    await ownedClosed
    assert.equal(decoy.exitCode, null)
    endOwned(owned)
    assert.equal(decoy.exitCode, null, 'an ended handle does not end the other process')
  } finally {
    endOwned(owned)
    endOwned(decoy)
    await Promise.all([ownedClosed, decoyClosed])
  }
})
