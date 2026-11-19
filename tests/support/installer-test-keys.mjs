import { createHash } from 'node:crypto'
import path from 'node:path'

// The registry subtree an installer test suite owns.
export function installerTestKey(base, work) {
  const identity = createHash('sha256').update(path.resolve(work)).digest('hex')
  return `${base}\\${identity}`
}
