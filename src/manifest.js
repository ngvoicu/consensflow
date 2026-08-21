import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { configRoot } from './roster.js'

/**
 * The install manifest: every file ConsensFlow has written into an harness's
 * skills directory, with the hash it wrote. Ownership and drift both come
 * from here — a file we did not record is not ours to touch, and a recorded
 * file whose on-disk hash changed was edited by someone and is refused
 * without --force. Never a silent clobber in either direction.
 */

function manifestPath(env) {
  return join(configRoot(env), 'skills-manifest.json')
}

export function sha256(content) {
  return createHash('sha256').update(content).digest('hex')
}

export function loadManifest(env) {
  try {
    const parsed = JSON.parse(readFileSync(manifestPath(env), 'utf8'))
    return { files: parsed.files ?? {}, rosterSha: parsed.rosterSha }
  } catch {
    return { files: {} }
  }
}

export function saveManifest(manifest, env) {
  mkdirSync(configRoot(env), { recursive: true })
  writeFileSync(manifestPath(env), `${JSON.stringify(manifest, null, 2)}\n`)
}

/** 'ok' | 'drifted' | 'missing' for a manifest-owned path. */
export function fileState(path, recorded) {
  try {
    return sha256(readFileSync(path)) === recorded.sha256 ? 'ok' : 'drifted'
  } catch {
    return 'missing'
  }
}
