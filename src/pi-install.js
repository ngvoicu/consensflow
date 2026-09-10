import { createHash } from 'node:crypto'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { harnessPath } from './harnesses.js'
import { configRoot } from './roster.js'

const FILES = [
  'hosts/pi-extension/consensflow-delivery.mjs',
  'hosts/lib/deliveries.js',
  'hosts/lib/policy.js',
  'package.json',
]

/** Content-addressed copies keep running Pi processes on their original code. */
export function preparePiExtension(env) {
  if (!harnessPath('pi', env)) return { state: 'not-installed', path: null }
  let temporary
  try {
    const files = FILES.map((file) => [file, readFileSync(new URL(`../${file}`, import.meta.url))])
    const hash = createHash('sha256')
    for (const [name, bytes] of files) hash.update(name).update('\0').update(bytes).update('\0')
    const root = join(configRoot(env), 'extensions', 'pi')
    const destination = join(root, hash.digest('hex'))
    if (!existsSync(destination)) {
      mkdirSync(root, { recursive: true, mode: 0o700 })
      temporary = mkdtempSync(join(root, '.install-'))
      for (const [name, bytes] of files) {
        const target = join(temporary, name)
        mkdirSync(dirname(target), { recursive: true, mode: 0o700 })
        writeFileSync(target, bytes, { mode: 0o600 })
      }
      try {
        renameSync(temporary, destination)
      } catch (error) {
        if (!existsSync(destination)) throw error
      }
    }
    for (const [name, bytes] of files) {
      if (!readFileSync(join(destination, name)).equals(bytes)) {
        throw new Error(
          'Private Pi extension differs from this build; existing files were preserved',
        )
      }
    }
    return { state: 'installed-unverified', path: join(destination, FILES[0]) }
  } catch (error) {
    return { state: 'error', path: null, reason: error.message }
  } finally {
    if (temporary) rmSync(temporary, { recursive: true, force: true })
  }
}
