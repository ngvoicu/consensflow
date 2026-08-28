import { existsSync, mkdirSync, readdirSync, rmdirSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'
import { detectHarnesses, knownHarnesses } from './harnesses.js'
import { fileState, loadManifest, saveManifest, sha256 } from './manifest.js'

/**
 * Installs one skill file into every detected harness's skills directory,
 * under the manifest's ownership rules:
 *
 * - a target we own and left unchanged is rewritten freely (that's an update);
 * - a target we own that the user edited is rewritten too, and reported as
 *   `replaced`: the skill is generated from the roster, so an edited copy is
 *   an agent answering for a roster that has moved;
 * - a target we never wrote is refused without force, always — that file is
 *   somebody else's, and `skillGaps` is what tells you it is there.
 *
 * `options.targets` narrows the harnesses written to; callers use it to leave a
 * host's own ConsensFlow integration alone (see skillTargets).
 */
export function installSkill({ relPath, content, source }, env, options = {}) {
  const manifest = loadManifest(env)
  const report = []

  for (const harness of options.targets ?? detectHarnesses(env)) {
    const path = join(harness.skillsDir, relPath)
    const owned = manifest.files[path]
    const existed = existsSync(path)

    // A file we never installed is still refused: that is somebody else's, and
    // `skillGaps` is the only thing that tells you it is sitting at our path.
    // A file we DID install is ours to rewrite even when it has been edited —
    // the skill is generated from the roster, not a document the user keeps,
    // and an edited copy makes every agent that reads it answer for a roster
    // that no longer exists (the owner's call, 2026-08-28). It says `replaced`
    // rather than `updated`, because losing an edit in silence is the only
    // part of this that would be wrong.
    if (existed && owned === undefined && options.force !== true) {
      report.push({ harness: harness.id, path, action: 'refused-unowned' })
      continue
    }
    const edited = existed && owned !== undefined && fileState(path, owned) === 'drifted'

    // Already exactly this, byte for byte? Then say so rather than claiming an
    // update. Reporting "312 updated" when nothing moved teaches the reader to
    // ignore the number, which is the one thing it exists to be read for — and
    // it meant every click rewrote 312 identical files.
    const next = sha256(content)
    if (
      existed &&
      owned !== undefined &&
      owned.sha256 === next &&
      fileState(path, owned) === 'ok'
    ) {
      // The bytes are already right, so nothing is written — but the record
      // still moves: a cmux file identical across two commits must be recorded
      // under the NEW one, or `skills status` reports a version we no longer
      // carry.
      manifest.files[path] = { sha256: next, source }
      report.push({ harness: harness.id, path, action: 'unchanged' })
      continue
    }

    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, content)
    manifest.files[path] = { sha256: next, source }
    report.push({
      harness: harness.id,
      path,
      action: owned === undefined ? 'installed' : edited ? 'replaced' : 'updated',
    })
  }

  saveManifest(manifest, env)
  return report
}

/**
 * What is installed, counted the way a reader asks about it.
 *
 * The manifest counts FILES, because that is what ownership is tracked at — a
 * skill is a directory, and cmux-browser alone is eleven files. Reporting the
 * file count alone invited the obvious question ("312 skills?"), so this
 * reports both, and splits ours from the ones we install on cmux's behalf.
 */
export function skillsSummary(env) {
  const rows = skillsStatus(env)
  const dirs = knownHarnesses(env)
  const skills = new Set()
  let ours = 0
  let cmux = 0
  let cmuxCommit = null

  for (const row of rows) {
    if (row.source === 'consensflow') ours += 1
    else {
      cmux += 1
      cmuxCommit = row.source
    }
    const dir = dirs.find((harness) => row.path.startsWith(`${harness.skillsDir}/`))
    if (dir === undefined) continue
    const name = row.path.slice(dir.skillsDir.length + 1).split('/')[0]
    skills.add(`${dir.id}/${name}`)
  }

  const harnesses = new Set([...skills].map((entry) => entry.split('/')[0]))
  return {
    files: rows.length,
    ours,
    cmux,
    cmuxCommit: cmuxCommit === null ? null : cmuxCommit.slice('cmux@'.length),
    skills: skills.size,
    harnesses: harnesses.size,
    perHarness: harnesses.size === 0 ? 0 : Math.round(skills.size / harnesses.size),
    drifted: rows.filter((row) => row.state === 'drifted').length,
    missing: rows.filter((row) => row.state === 'missing').length,
  }
}

/** One row per manifest-owned file: where it is and whether it is intact. */
export function skillsStatus(env) {
  const manifest = loadManifest(env)
  return Object.entries(manifest.files).map(([path, recorded]) => ({
    path,
    source: recorded.source,
    state: fileState(path, recorded),
  }))
}

/**
 * Removes every manifest-owned file, or the subset `options.filter` accepts.
 * A drifted file is refused without force (the user's edits are theirs);
 * anything not in the manifest is never touched. Emptied skill directories
 * are cleaned up.
 */
export function uninstallSkills(env, options = {}) {
  const manifest = loadManifest(env)
  const report = []

  for (const [path, recorded] of Object.entries(manifest.files)) {
    // A filter narrows the sweep (retiring one skill from one host, say);
    // without it every owned file goes.
    if (options.filter !== undefined && !options.filter(path, recorded)) continue
    const state = fileState(path, recorded)
    if (state === 'drifted' && options.force !== true) {
      report.push({ path, action: 'refused-drifted' })
      continue
    }
    rmSync(path, { force: true })
    // Climb away every directory the removal emptied, stopping at the harness's
    // skills root — a skill is a directory tree, and leaving hollow shells
    // behind reads as "still installed" in every harness's skill picker.
    let parent = dirname(path)
    while (basename(parent) !== 'skills') {
      try {
        if (readdirSync(parent).length > 0) break
        rmdirSync(parent)
      } catch {
        break
      }
      parent = dirname(parent)
    }
    delete manifest.files[path]
    report.push({ path, action: state === 'missing' ? 'already-gone' : 'removed' })
  }

  saveManifest(manifest, env)
  return report
}
