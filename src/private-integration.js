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
import { configRoot } from './roster.js'

/** Publish a verified immutable bundle; never replace code a live pane may have loaded. */
export function preparePrivateIntegration(env, kind, names, generated = () => []) {
  const files = names.map((name) => [name, readFileSync(new URL(`../${name}`, import.meta.url))])
  const hash = createHash('sha256')
  for (const [name, bytes] of files) hash.update(name).update('\0').update(bytes).update('\0')
  const root = join(configRoot(env), 'extensions', kind)
  const destination = join(root, hash.digest('hex'))
  files.push(...generated(destination))
  let temporary
  try {
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
        if (!['ENOTEMPTY', 'EEXIST'].includes(error.code)) throw error
      }
    }
    for (const [name, bytes] of files) {
      if (!readFileSync(join(destination, name)).equals(bytes))
        throw new Error(
          `Private ${kind} integration differs from this build; existing files were preserved`,
        )
    }
    return destination
  } finally {
    if (temporary) rmSync(temporary, { recursive: true, force: true })
  }
}
