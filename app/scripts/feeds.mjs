#!/usr/bin/env node
/**
 * The update feeds, and the rule that says which release moves which.
 *
 * An installed app reads the `latest.json` of a rolling GitHub release, the
 * feed of its channel. The apps before the flip release (3.0.0-alpha.79 and
 * earlier) read `update-alpha` and `update-stable`; the flip release and every
 * one after it read `feed-alpha` and `feed-stable` (app/feeds.json names both
 * generations; app/src-tauri/src/updates.rs reads the new one). The rule:
 *
 *   - every release moves the new feed of each channel it belongs to: alpha
 *     for every release, stable too for a stable one;
 *   - the flip release moves the old feeds of those channels as well, once.
 *     It is the first release that reads the new feeds, the first whose tree
 *     holds app/feeds.json: the release before it did not. (The release that
 *     changes the daemon's default is that one only if no release is made
 *     between; the rule does not wait for it.) It is the release the old apps
 *     are offered, and they install it because it keeps the layout they
 *     check for (Node, cf.mjs, src, hosts, package.json), so a flip release
 *     whose archive does not is refused here, before anything moves;
 *   - a later release moves the new feeds only. The old feeds stay pinned to
 *     the flip release, and its assets stay: nothing here deletes a release.
 *
 * Applied by .github/workflows/release.yml:
 *
 *   node app/scripts/feeds.mjs plan --version <v> --archive <file> --previous <tag>
 *     prints the feeds this release moves, one per line, the new ones first
 *   node app/scripts/feeds.mjs check --dir <dir> --version <v> --base <url>
 *     says whether the feeds serve what the rule says they do
 */
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const MANIFEST = JSON.parse(readFileSync(join(APP, 'feeds.json'), 'utf8'))
/** Where the manifest is in a release's tree, which is how the flip release is told. */
const MANIFEST_IN_TREE = 'app/feeds.json'
const TARGET = 'darwin-aarch64'

const BUNDLE = 'ConsensFlow.app/Contents/'
/**
 * What the updater of the apps before the flip release asks of an archive
 * (`validate_bundle` in app/src-tauri/src/update_install.rs at
 * v3.0.0-alpha.79, in every app already installed): these files, and
 * something in these folders. An archive without them is one those apps
 * download and refuse.
 */
const OLD_APPS_REQUIRE = [
  `${BUNDLE}MacOS/node`,
  `${BUNDLE}Resources/cli/package.json`,
  `${BUNDLE}Resources/cli/bin/cf.mjs`,
  `${BUNDLE}Resources/cli/bin/cf`,
  `${BUNDLE}Resources/cli/hosts/`,
  `${BUNDLE}Resources/cli/src/`,
]

function fail(message) {
  throw new Error(message)
}

/** The channels a release belongs to: every release is alpha's, a stable one is stable's too. */
export function channelsOf(version) {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) fail(`not a semantic version: ${version}`)
  return version.includes('-') ? ['alpha'] : ['alpha', 'stable']
}

/** What the old apps require of an archive that it lacks: the members of `names`, as `tar -t` lists them. */
export function missingForOldApps(names) {
  const listed = names.map((name) => name.replace(/^\.\//, ''))
  return OLD_APPS_REQUIRE.filter((required) =>
    required.endsWith('/')
      ? !listed.some((name) => name.startsWith(required))
      : !listed.includes(required),
  )
}

/** The members of the archive at `archive`. */
export function membersOf(archive) {
  const listing = execFileSync('tar', ['-tzf', archive], {
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  })
  return listing.split(/\r?\n/).filter((line) => line !== '')
}

function git(repo, args) {
  return spawnSync('git', ['-C', repo, ...args], { encoding: 'utf8' })
}

/**
 * Whether the release being made is the flip release: the one before it,
 * `previous`, did not hold the manifest. A `previous` that is not in the
 * repository is not guessed at: fetch the tags.
 */
export function isFlip({ repo, previous }) {
  if (previous === undefined || previous === '') {
    fail('no earlier release is reachable, so the flip release cannot be told: fetch the tags')
  }
  if (git(repo, ['rev-parse', '--verify', '--quiet', `${previous}^{commit}`]).status !== 0) {
    fail(`the earlier release ${previous} is not in this repository: fetch the tags`)
  }
  return git(repo, ['cat-file', '-e', `${previous}:${MANIFEST_IN_TREE}`]).status !== 0
}

/**
 * The feeds a release moves: the new feed of each of its channels, and for
 * the flip release the old ones too, which it refuses to move to an archive
 * whose `missing` (see `missingForOldApps`) the old apps would refuse.
 */
export function planFeeds({ version, flip, missing, manifest = MANIFEST }) {
  const channels = channelsOf(version)
  const feeds = channels.map((channel) => manifest.feeds[channel])
  if (!flip) return feeds
  if (missing.length > 0) {
    fail(
      `the flip release must keep the layout the apps before it check for: its archive lacks ${missing.join(', ')}`,
    )
  }
  return [...feeds, ...channels.map((channel) => manifest.legacy[channel])]
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms))

/** The body of `url` as bytes, or null where it is not there or could not be read. */
async function read(url) {
  try {
    const response = await fetch(url)
    return response.ok ? Buffer.from(await response.arrayBuffer()) : null
  } catch {
    return null
  }
}

/** What `url` serves once `accept` takes it: or what it served last, after `attempts` reads `wait` ms apart. */
async function readUntil(url, accept, { attempts, wait }) {
  let body = null
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    body = await read(url)
    if (accept(body)) return body
    if (attempt < attempts) await sleep(wait)
  }
  return body
}

/** The SHA-256 of what `url` serves, or null where it is not served. */
async function sha256Of(url) {
  try {
    const response = await fetch(url)
    if (!response.ok) return null
    const hash = createHash('sha256')
    for await (const chunk of response.body) hash.update(chunk)
    return hash.digest('hex')
  } catch {
    return null
  }
}

/** Whether `url`, a release asset, is served, asked for its first byte alone. */
async function served(url) {
  try {
    const response = await fetch(url, { headers: { range: 'bytes=0-0' } })
    await response.body?.cancel()
    return response.ok
  } catch {
    return false
  }
}

/**
 * What the feeds serve, as the rule says: every feed this release moved
 * (`<dir>/feeds.txt`) serves its `latest.json`, and the archive that names
 * downloads as the one built; no old feed that was not moved serves this
 * release, and the release it does serve is still there to download. A feed
 * just written may be served stale for a moment, so one that differs is read
 * again, `attempts` times, `wait` ms apart. Resolves to the problems found.
 */
export async function checkFeeds({
  dir,
  version,
  base,
  manifest = MANIFEST,
  attempts = 12,
  wait = 5000,
}) {
  const problems = []
  const moved = readFileSync(join(dir, 'feeds.txt'), 'utf8').split(/\r?\n/).filter(Boolean)
  const latest = readFileSync(join(dir, 'latest.json'))
  for (const feed of moved) {
    const found = await readUntil(`${base}/${feed}/latest.json`, (body) => body?.equals(latest), {
      attempts,
      wait,
    })
    if (!found?.equals(latest)) {
      problems.push(`${feed} does not serve this release's latest.json`)
    }
  }
  const archive = new URL(JSON.parse(latest).platforms[TARGET].url)
  const built = createHash('sha256')
    .update(readFileSync(join(dir, basename(archive.pathname))))
    .digest('hex')
  if ((await sha256Of(archive)) !== built) {
    problems.push(`the archive ${archive} does not download as the one built`)
  }
  for (const feed of Object.values(manifest.legacy).filter((feed) => !moved.includes(feed))) {
    const pinned = await read(`${base}/${feed}/latest.json`)
    if (pinned === null) continue
    let named
    try {
      named = JSON.parse(pinned)
    } catch {
      problems.push(`${feed} serves something that is not a latest.json`)
      continue
    }
    const url = named.platforms?.[TARGET]?.url
    if (named.version === version) {
      problems.push(`${feed} serves this release, which the rule does not move it to`)
    } else if (url === undefined || !(await served(url))) {
      problems.push(`${feed} is pinned to ${named.version}, whose archive is no longer served`)
    }
  }
  return problems
}

async function main(argv) {
  const [command, ...rest] = argv
  const { values } = parseArgs({
    args: rest,
    options: {
      version: { type: 'string' },
      archive: { type: 'string' },
      previous: { type: 'string' },
      repo: { type: 'string', default: '.' },
      dir: { type: 'string' },
      base: { type: 'string' },
      attempts: { type: 'string' },
      wait: { type: 'string' },
    },
  })
  if (command === 'plan') {
    if (!values.version || !values.archive) fail('plan needs --version, --archive and --previous')
    const flip = isFlip({ repo: resolve(values.repo), previous: values.previous })
    const feeds = planFeeds({
      version: values.version,
      flip,
      missing: missingForOldApps(membersOf(values.archive)),
    })
    process.stderr.write(
      `feeds: ${values.version} is ${flip ? 'the flip release' : 'a later release'}: ${feeds.join(', ')}\n`,
    )
    process.stdout.write(`${feeds.join('\n')}\n`)
    return 0
  }
  if (command === 'check') {
    if (!values.dir || !values.version || !values.base) {
      fail('check needs --dir, --version and --base')
    }
    const problems = await checkFeeds({
      dir: values.dir,
      version: values.version,
      base: values.base,
      ...(values.attempts === undefined ? {} : { attempts: Number(values.attempts) }),
      ...(values.wait === undefined ? {} : { wait: Number(values.wait) }),
    })
    for (const problem of problems) process.stderr.write(`feeds: ${problem}\n`)
    if (problems.length === 0) process.stdout.write('feeds: serve this release as the rule says\n')
    return problems.length === 0 ? 0 : 1
  }
  return fail('usage: feeds.mjs plan|check (see the top of this file)')
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).then(
    (code) => {
      process.exitCode = code
    },
    (cause) => {
      process.stderr.write(`feeds: ${cause instanceof Error ? cause.message : String(cause)}\n`)
      process.exitCode = 1
    },
  )
}
