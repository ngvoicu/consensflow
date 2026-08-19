import { spawnSync } from 'node:child_process'
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative } from 'node:path'
import { installSkill } from './install.js'

const CMUX_REPO = 'https://github.com/manaflow-ai/cmux'

/**
 * cmux publishes its agent skills in its own repository's `skills/` tree.
 * ConsensFlow fetches them with a shallow clone and installs every file of
 * every skill under the manifest, sourced `cmux@<commit>` — so `skills
 * status` can say exactly which cmux version each file came from, updates
 * rewrite only unedited files, and uninstall knows what is ours to remove.
 */
export function installCmuxSkills(env, options = {}) {
  const repo = options.repo ?? CMUX_REPO
  const checkout = mkdtempSync(join(tmpdir(), 'cf-cmux-'))

  try {
    const clone = spawnSync('git', ['clone', '--depth', '1', repo, checkout], {
      env,
      encoding: 'utf8',
    })
    if (clone.error || clone.status !== 0) {
      throw new Error(
        `git clone of ${repo} failed — is git installed and the network up? (${clone.error?.message ?? clone.stderr?.trim()})`,
      )
    }

    const rev = spawnSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { env, encoding: 'utf8' })
    const commit = (rev.stdout ?? '').trim().slice(0, 12)
    if (commit.length === 0) {
      throw new Error('git rev-parse produced no commit for the cmux checkout')
    }

    const skillsRoot = join(checkout, 'skills')
    const report = []
    for (const file of walk(skillsRoot)) {
      const relPath = relative(skillsRoot, file)
      report.push(
        ...installSkill(
          { relPath, content: readFileSync(file), source: `cmux@${commit}` },
          env,
          options,
        ),
      )
    }
    return { commit, report }
  } finally {
    rmSync(checkout, { recursive: true, force: true })
  }
}

function walk(dir) {
  const files = []
  let names = []
  try {
    names = readdirSync(dir)
  } catch {
    return files
  }
  for (const name of names) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) files.push(...walk(path))
    else files.push(path)
  }
  return files
}
