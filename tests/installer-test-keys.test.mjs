import assert from 'node:assert/strict'
import test from 'node:test'
import { installerTestKey } from './support/installer-test-keys.mjs'

test('cleaning one installer suite leaves another suite registry entries', () => {
  const registry = new Map()
  const owned = installerTestKey('HKCU\\Software\\example', 'run-one')
  const other = installerTestKey('HKCU\\Software\\example', 'run-two')
  registry.set(owned, 'ours')
  registry.set(other, 'the other suite')
  registry.delete(owned)
  assert.equal(registry.get(other), 'the other suite')
})
