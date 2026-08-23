import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { installSkill } from './install.js'
import { configRoot } from './roster.js'

const CMUX_REPO = 'https://github.com/manaflow-ai/cmux'

/**
 * Where the checkout lives: inside the one root, not the system temp.
 *
 * It used to be an `mkdtemp` thrown away on every call, which meant a full
 * clone each time an update was checked — and a directory outside the root,
 * which is the one place ConsensFlow is supposed to keep its state. Kept here
 * it is a cache: `fetch` instead of `clone`, which is faster, quieter on a slow
 * link, and `off` takes it back with everything else.
 */
export function cmuxCacheDir(env) {
  return join(configRoot(env), 'cache', 'cmux')
}

/**
 * cmux publishes its harness skills in its own repository's `skills/` tree.
 * `options.targets` narrows which harnesses receive them — in a host mode only
 * the harness that consults does.
 * ConsensFlow fetches them with a shallow clone and installs every file of
 * every skill under the manifest, sourced `cmux@<commit>` — so `skills
 * status` can say exactly which cmux version each file came from, updates
 * rewrite only unedited files, and uninstall knows what is ours to remove.
 */
export function installCmuxSkills(env, options = {}) {
  const repo = options.repo ?? CMUX_REPO
  const checkout = options.checkout ?? cmuxCacheDir(env)

  try {
    refresh(repo, checkout, env)

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
  } catch (cause) {
    // A cache that cannot be read is worth less than the disk it sits on.
    rmSync(checkout, { recursive: true, force: true })
    throw cause
  }
}

/**
 * Brings the cache up to date, or makes one. Any failure to update an existing
 * checkout — interrupted clone, a branch that moved, a directory someone else
 * touched — is answered by throwing it away and cloning again, so a broken
 * cache costs one slow run rather than every run after it.
 */
function refresh(repo, checkout, env) {
  if (existsSync(join(checkout, '.git'))) {
    const fetched = spawnSync('git', ['-C', checkout, 'fetch', '--depth', '1', 'origin', 'HEAD'], {
      env,
      encoding: 'utf8',
    })
    if (!fetched.error && fetched.status === 0) {
      const reset = spawnSync('git', ['-C', checkout, 'reset', '--hard', 'FETCH_HEAD'], {
        env,
        encoding: 'utf8',
      })
      if (!reset.error && reset.status === 0) return
    }
    rmSync(checkout, { recursive: true, force: true })
  }

  const clone = spawnSync('git', ['clone', '--depth', '1', repo, checkout], {
    env,
    encoding: 'utf8',
  })
  if (clone.error || clone.status !== 0) {
    throw new Error(
      `git clone of ${repo} failed — is git installed and the network up? (${clone.error?.message ?? clone.stderr?.trim()})`,
    )
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
