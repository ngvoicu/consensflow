#!/usr/bin/env node
/**
 * The update feeds, the bridge that carries the old apps to the new ones, and
 * what a release must find true before it moves a feed, and after.
 *
 * An installed app reads the `latest.json` of a rolling GitHub release, the
 * feed of its channel. The apps before the bridge (3.0.0-alpha.80 and earlier)
 * read `update-alpha` and `update-stable`; the bridge and every release after
 * it read `feed-alpha` and `feed-stable` (app/feeds.json names both
 * generations of feeds, and the bridge; app/src-tauri/src/updates.rs reads the
 * new feeds). The bridge is the first release that reads them, and the one the
 * old apps are offered: they install it because it keeps the layout they check
 * for (Node, cf.mjs, src, hosts, package.json), and from it on they read the
 * new feeds.
 *
 * Two facts, kept apart. Which release is the bridge is recorded in
 * app/feeds.json (`bridge.version`): it is a decision, made when the release is
 * cut. That the bridge reached the old apps is not implied by it, nor by a tag
 * that holds the manifest, nor by any git history: a release that failed before
 * or while it moved `update-alpha` leaves that tag behind and the old apps where
 * they were. It is read from where the old apps read it, every time it matters:
 * the old feed of each channel in use (`bridge.legacy`) serves the bridge's own
 * latest.json byte for byte, and the bridge's Mac archive, installer and
 * portable download as they were published (its SHA256SUMS). The rule:
 *
 *   - the bridge moves the new feed of each of its channels and the old feed of
 *     each channel in use, once. A bridge whose archive lacks what the old apps
 *     check for is refused before anything moves;
 *   - a later release moves the new feeds only, and only when the bridge has
 *     reached the old feeds, which it establishes before it moves anything: when
 *     the old feeds do not serve the bridge, with its files, it is refused and
 *     nothing moves. The old feeds stay pinned to the bridge, and its files stay:
 *     nothing here deletes a release;
 *   - a release before the bridge is refused: it would read feeds no installed
 *     app reads, and name a bridge that is not the first.
 *
 * Applied by .github/workflows/release.yml (and, for the moving, by
 * app/scripts/publish.mjs):
 *
 *   node app/scripts/feeds.mjs plan --version <v> --archive <file> [--dry-run]
 *     prints the feeds this release moves, one per line, the new ones first
 *   node app/scripts/feeds.mjs prerequisites --version <v> --base <url> [--dry-run]
 *     says whether the old feeds serve the bridge, which a later release needs
 *   node app/scripts/feeds.mjs check --dir <dir> --version <v> --base <url>
 *     says whether, once published, the files and the feeds serve what the rule says
 *
 * `--dry-run` is for a hand run of the workflow, which publishes nothing: what
 * a tag would be refused for is said, and is not a failure.
 */
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join, posix } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const TARGET = 'darwin-aarch64'
const CHANNELS = ['alpha', 'stable']
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/

const BUNDLE = 'ConsensFlow.app/Contents/'
/**
 * What the updater of the apps before the bridge asks of an archive
 * (`validate_bundle` in app/src-tauri/src/update_install.rs at
 * v3.0.0-alpha.79, which 3.0.0-alpha.80 shares, in every app already
 * installed): these files, and something in these folders. An archive without
 * them is one those apps download and refuse.
 */
const OLD_APPS_REQUIRE = [
  `${BUNDLE}MacOS/node`,
  `${BUNDLE}Resources/cli/package.json`,
  `${BUNDLE}Resources/cli/bin/cf.mjs`,
  `${BUNDLE}Resources/cli/bin/cf`,
  `${BUNDLE}Resources/cli/hosts/`,
  `${BUNDLE}Resources/cli/src/`,
]
/** The bridge's files the old apps still need: the Mac archive, and the Windows installer and portable they send people to. */
const RETAINED = ['archive', 'installer', 'portable']

function fail(message) {
  throw new Error(message)
}

/** The channels a release belongs to: every release is alpha's, a stable one is stable's too. */
export function channelsOf(version) {
  if (!SEMVER.test(version)) fail(`not a semantic version: ${version}`)
  return version.includes('-') ? ['alpha'] : ['alpha', 'stable']
}

/** Semantic-version precedence: -1, 0 or 1 as `a` comes before `b`, is it, or comes after it. */
export function compareVersions(a, b) {
  for (const version of [a, b]) channelsOf(version)
  const split = (version) => {
    const dash = version.indexOf('-')
    return dash === -1
      ? [version.split('.').map(Number), null]
      : [version.slice(0, dash).split('.').map(Number), version.slice(dash + 1).split('.')]
  }
  const [aCore, aPre] = split(a)
  const [bCore, bPre] = split(b)
  for (const [index, part] of aCore.entries()) {
    if (part !== bCore[index]) return part < bCore[index] ? -1 : 1
  }
  if (aPre === null || bPre === null) return aPre === bPre ? 0 : aPre === null ? 1 : -1
  for (const [index, id] of aPre.entries()) {
    const other = bPre[index]
    if (other === undefined) return 1
    const numbers = [/^\d+$/.test(id), /^\d+$/.test(other)]
    if (numbers[0] !== numbers[1]) return numbers[0] ? -1 : 1
    if (id === other) continue
    if (numbers[0]) return Number(id) < Number(other) ? -1 : 1
    return id < other ? -1 : 1
  }
  return aPre.length === bPre.length ? 0 : -1
}

/** `manifest` itself, once it says what the rule needs of it: the feeds of both generations, and a bridge. */
export function checkManifest(manifest) {
  for (const group of ['feeds', 'legacy']) {
    for (const channel of CHANNELS) {
      if (typeof manifest?.[group]?.[channel] !== 'string') {
        fail(`app/feeds.json: ${group}.${channel} names no feed`)
      }
    }
  }
  const named = [...Object.values(manifest.feeds), ...Object.values(manifest.legacy)]
  if (new Set(named).size !== named.length) fail('app/feeds.json: a feed is named twice')
  const { version, legacy } = manifest.bridge ?? {}
  if (typeof version !== 'string') fail('app/feeds.json: bridge.version names no release')
  const belongs = channelsOf(version)
  if (!Array.isArray(legacy) || legacy.length === 0 || legacy.some((c) => !belongs.includes(c))) {
    fail(
      `app/feeds.json: bridge.legacy names the channels in use that ${version} belongs to (${belongs.join(', ')}); it names ${JSON.stringify(legacy)}`,
    )
  }
  return manifest
}

export const MANIFEST = checkManifest(JSON.parse(readFileSync(join(APP, 'feeds.json'), 'utf8')))

/**
 * What `version` is to the bridge `manifest` names: 'bridge' for the bridge
 * itself, 'later' for a release after it. A release before it is refused.
 */
export function roleOf(version, manifest = MANIFEST) {
  const bridge = manifest.bridge.version
  const order = compareVersions(version, bridge)
  if (order < 0) {
    fail(
      `${version} comes before the bridge ${bridge} that app/feeds.json names, and it would read feeds that no installed app does: set bridge.version to the first release made from this tree`,
    )
  }
  return order === 0 ? 'bridge' : 'later'
}

/** The feeds a release moves: the new feed of each of its channels, and for the bridge the old ones in use too. */
export function feedsMoved(version, manifest = MANIFEST) {
  const feeds = channelsOf(version).map((channel) => manifest.feeds[channel])
  if (roleOf(version, manifest) === 'later') return feeds
  return [...feeds, ...manifest.bridge.legacy.map((channel) => manifest.legacy[channel])]
}

/**
 * The files a release publishes, where `dist/` holds each: the order they are
 * uploaded in, and the names the rest of this file asks for them by.
 */
export function releaseAssets(version) {
  const mac = `ConsensFlow_${version}_aarch64`
  return {
    dmg: `${mac}.dmg`,
    archive: `${mac}.app.tar.gz`,
    signature: `${mac}.app.tar.gz.sig`,
    metadata: 'latest.json',
    installer: `nsis/ConsensFlow_${version}_x64-setup.exe`,
    portable: `portable/ConsensFlow_${version}_x64-portable.exe`,
  }
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

/**
 * The feeds a release moves, or why it may not: it comes before the bridge, or
 * it is the bridge and its archive (`missing`, see `missingForOldApps`) lacks
 * what the old apps would refuse it for.
 */
export function planFeeds({ version, missing, manifest = MANIFEST }) {
  const feeds = feedsMoved(version, manifest)
  if (roleOf(version, manifest) === 'bridge' && missing.length > 0) {
    fail(
      `the bridge must keep the layout the apps before it check for: its archive lacks ${missing.join(', ')}`,
    )
  }
  return feeds
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms))

/** The SHA-256 of `bytes`, in hex. */
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')

/** A read that got no success: the status it got (null where nothing answered) and why, as a problem says it. */
const refused = (status) => ({ ok: false, status, why: `HTTP ${status}` })
const unreachable = (error) => ({
  ok: false,
  status: null,
  why: `unreachable: ${error?.cause?.code ?? error?.message ?? error}`,
})

/** The body of `url`: `{ ok: true, body }`, or `{ ok: false, status, why }` for a status that is not a success or no answer at all. */
async function fetchBody(url) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(30_000) })
    if (!response.ok) {
      await response.body?.cancel()
      return refused(response.status)
    }
    return { ok: true, body: Buffer.from(await response.arrayBuffer()) }
  } catch (error) {
    return unreachable(error)
  }
}

/** The SHA-256 of what `url` serves, read as it streams: `{ ok: true, sha256 }` or what `fetchBody` says of a failure. */
async function fetchSha256(url) {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(15 * 60_000) })
    if (!response.ok) {
      await response.body?.cancel()
      return refused(response.status)
    }
    const hash = createHash('sha256')
    for await (const chunk of response.body) hash.update(chunk)
    return { ok: true, sha256: hash.digest('hex') }
  } catch (error) {
    return unreachable(error)
  }
}

/** What `read` answered once `accept` took it, or what it answered last, after `attempts` asks `wait` ms apart. */
async function until(read, accept, { attempts, wait }) {
  let found
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    found = await read()
    if (accept(found)) return found
    if (attempt < attempts) await sleep(wait)
  }
  return found
}

/**
 * Where a release's files and feeds are read from, `attempts` times, `wait` ms
 * apart where a read may be out of date, and only a few where it is a file
 * published long ago: past a blip, never waited on for a change. A hash is read
 * once for the life of the reader.
 */
export function reader({ attempts = 12, wait = 5000 } = {}) {
  const blips = { attempts: Math.min(attempts, 3), wait }
  const hashes = new Map()
  return {
    /** A feed's latest.json, read again while it is not `expected`: a replaced asset can be served stale for a moment. */
    feed: (url, expected) =>
      until(
        () => fetchBody(url),
        (found) => found.ok && found.body.equals(expected),
        { attempts, wait },
      ),
    /** A published file's body. */
    file: (url) =>
      until(
        () => fetchBody(url),
        (found) => found.ok,
        blips,
      ),
    /** What a published file hashes to. */
    hash(url) {
      if (!hashes.has(url)) {
        hashes.set(
          url,
          until(
            () => fetchSha256(url),
            (found) => found.ok,
            blips,
          ),
        )
      }
      return hashes.get(url)
    },
    /** A feed that may not be there: asked once. */
    once: (url) => fetchBody(url),
  }
}

/** The hashes a SHA256SUMS lists, by file name. */
function listedIn(sums) {
  const listed = new Map()
  for (const line of sums.split(/\r?\n/)) {
    const match = /^([0-9a-f]{64}) [ *](.+)$/.exec(line)
    if (match !== null) listed.set(match[2], match[1])
  }
  return listed
}

/**
 * What the old channels in use must serve, and have, for the bridge to have
 * reached the apps that read them: each serves the bridge's own latest.json
 * (the one its release published) byte for byte, that names the bridge and its
 * archive, and the bridge's Mac archive, installer and portable download as its
 * SHA256SUMS says. A feed or file that cannot be read is not absent: it is a
 * problem, whichever way it cannot be read.
 */
async function bridgeProblems({ base, manifest, reads }) {
  const { version, legacy } = manifest.bridge
  const tag = `v${version}`
  const assets = releaseAssets(version)
  const published = await reads.file(`${base}/${tag}/latest.json`)
  if (!published.ok) {
    return [`the bridge's ${tag}/latest.json cannot be read (${published.why}), so no feed can be held to it`]
  }
  const problems = []
  let document
  try {
    document = JSON.parse(published.body)
  } catch {
    return [`the bridge's ${tag}/latest.json is not a latest.json`]
  }
  const archive = `${base}/${tag}/${posix.basename(assets.archive)}`
  if (document.version !== version) {
    problems.push(`the bridge's ${tag}/latest.json names ${document.version}, not ${version}`)
  }
  if (document.platforms?.[TARGET]?.url !== archive) {
    problems.push(
      `the bridge's ${tag}/latest.json names ${document.platforms?.[TARGET]?.url ?? 'no archive'} for the Mac, not ${archive}`,
    )
  }
  for (const channel of legacy) {
    const feed = manifest.legacy[channel]
    const found = await reads.feed(`${base}/${feed}/latest.json`, published.body)
    if (!found.ok) {
      problems.push(`${feed} cannot be read (${found.why}): the apps before the bridge read it`)
      continue
    }
    if (found.body.equals(published.body)) continue
    let named
    try {
      named = JSON.parse(found.body).version
    } catch {
      problems.push(`${feed} serves something that is not a latest.json`)
      continue
    }
    problems.push(
      named === version
        ? `${feed} names the bridge ${version}, but is not the bridge's own latest.json byte for byte`
        : `${feed} serves ${named}, not the bridge ${version}: the apps that read it do not reach the bridge`,
    )
  }
  const sums = await reads.file(`${base}/${tag}/SHA256SUMS`)
  if (!sums.ok) {
    problems.push(`the bridge's ${tag}/SHA256SUMS cannot be read (${sums.why})`)
    return problems
  }
  const listed = listedIn(sums.body.toString('utf8'))
  for (const key of RETAINED) {
    const name = posix.basename(assets[key])
    const wanted = listed.get(name)
    if (wanted === undefined) {
      problems.push(`the bridge's ${tag}/SHA256SUMS lists no ${name}`)
      continue
    }
    const got = await reads.hash(`${base}/${tag}/${name}`)
    if (!got.ok) {
      problems.push(`the bridge's ${name} cannot be downloaded (${got.why})`)
    } else if (got.sha256 !== wanted) {
      problems.push(`the bridge's ${name} does not download as the one its SHA256SUMS lists`)
    }
  }
  return problems
}

/** What the release's own latest.json (`latest`, bytes) must say: its version, and its archive at the download address it was published to. */
function metadataProblems({ latest, version, base }) {
  const wanted = `${base}/v${version}/${posix.basename(releaseAssets(version).archive)}`
  let document
  try {
    document = JSON.parse(latest)
  } catch {
    return ["this release's latest.json is not JSON"]
  }
  const problems = []
  if (document.version !== version) {
    problems.push(`this release's latest.json names ${document.version}, not ${version}`)
  }
  if (document.platforms?.[TARGET]?.url !== wanted) {
    problems.push(
      `this release's latest.json names ${document.platforms?.[TARGET]?.url ?? 'no archive'} for the Mac, not ${wanted}`,
    )
  }
  return problems
}

/**
 * Whether every file of the release built in `dir` downloads from where it was
 * published as the one built, which is a hash read off the download beside a
 * hash of the file: the archive the apps install, the Windows files, the
 * metadata, the sums. `reads` is a `reader`.
 */
export async function assetProblems({ dir, version, base, reads }) {
  const problems = []
  const tag = `v${version}`
  for (const path of [...Object.values(releaseAssets(version)), 'SHA256SUMS']) {
    const name = posix.basename(path)
    const got = await reads.hash(`${base}/${tag}/${name}`)
    if (!got.ok) {
      problems.push(`${name} cannot be downloaded from ${tag} (${got.why})`)
    } else if (got.sha256 !== sha256(readFileSync(join(dir, ...path.split('/'))))) {
      problems.push(`${name} does not download from ${tag} as the one built`)
    }
  }
  return problems
}

/**
 * What a release must find true before it moves any feed. For the bridge there
 * is nothing the feeds can say yet; for a later release, the old channels in
 * use serve the bridge with its files (`bridgeProblems`). Resolves to the
 * problems found, which refuse the release.
 */
export async function checkPrerequisites({
  version,
  base,
  manifest = MANIFEST,
  attempts = 12,
  wait = 5000,
}) {
  let role
  try {
    role = roleOf(version, manifest)
  } catch (cause) {
    return [cause.message]
  }
  if (role === 'bridge') return []
  return bridgeProblems({ base, manifest, reads: reader({ attempts, wait }) })
}

/**
 * What the feeds serve once the release built in `dir` is published, as the
 * rule says: every file of the release downloads as the one built and its
 * latest.json names its archive, each new feed of its channels serves that
 * latest.json, and the old channels in use serve the bridge with its files
 * (`bridgeProblems`), this release being that bridge or a later one. An old
 * feed of a channel not in use may be absent, and when it is there does not
 * serve this release, which the rule does not move it to. Resolves to the
 * problems found.
 */
export async function checkFeeds({
  dir,
  version,
  base,
  manifest = MANIFEST,
  attempts = 12,
  wait = 5000,
}) {
  try {
    roleOf(version, manifest)
  } catch (cause) {
    return [cause.message]
  }
  const reads = reader({ attempts, wait })
  const latest = readFileSync(join(dir, releaseAssets(version).metadata))
  const problems = [
    ...(await assetProblems({ dir, version, base, reads })),
    ...metadataProblems({ latest, version, base }),
  ]
  for (const channel of channelsOf(version)) {
    const feed = manifest.feeds[channel]
    const found = await reads.feed(`${base}/${feed}/latest.json`, latest)
    if (!found.ok || !found.body.equals(latest)) {
      problems.push(
        `${feed} does not serve this release's latest.json${found.ok ? '' : ` (${found.why})`}`,
      )
    }
  }
  problems.push(...(await bridgeProblems({ base, manifest, reads })))
  for (const channel of CHANNELS.filter((name) => !manifest.bridge.legacy.includes(name))) {
    const feed = manifest.legacy[channel]
    const found = await reads.once(`${base}/${feed}/latest.json`)
    if (!found.ok && found.status === 404) continue
    if (!found.ok) {
      problems.push(`${feed} cannot be read (${found.why})`)
      continue
    }
    try {
      if (JSON.parse(found.body).version === version) {
        problems.push(`${feed} serves this release, which the rule does not move it to`)
      }
    } catch {
      problems.push(`${feed} serves something that is not a latest.json`)
    }
  }
  return problems
}

/** What a command found, said as it is by the one that runs it: refusals, or for a hand run what a tag would meet. */
function say(problems, { dry, ok }) {
  if (problems.length === 0) {
    process.stdout.write(`feeds: ${ok}\n`)
    return 0
  }
  for (const problem of problems) {
    process.stderr.write(`feeds${dry ? ' (a tag would be refused)' : ''}: ${problem}\n`)
  }
  return dry ? 0 : 1
}

async function main(argv) {
  const [command, ...rest] = argv
  const { values } = parseArgs({
    args: rest,
    options: {
      version: { type: 'string' },
      archive: { type: 'string' },
      dir: { type: 'string' },
      base: { type: 'string' },
      attempts: { type: 'string' },
      wait: { type: 'string' },
      'dry-run': { type: 'boolean', default: false },
    },
  })
  const dry = values['dry-run']
  const patience = {
    ...(values.attempts === undefined ? {} : { attempts: Number(values.attempts) }),
    ...(values.wait === undefined ? {} : { wait: Number(values.wait) }),
  }
  if (command === 'plan') {
    if (!values.version || !values.archive) fail('plan needs --version and --archive')
    let feeds
    try {
      feeds = planFeeds({
        version: values.version,
        missing: missingForOldApps(membersOf(values.archive)),
      })
    } catch (cause) {
      if (!dry) throw cause
      return say([cause.message], { dry, ok: '' })
    }
    process.stderr.write(
      `feeds: ${values.version} is ${roleOf(values.version) === 'bridge' ? 'the bridge' : 'a later release'}: ${feeds.join(', ')}\n`,
    )
    process.stdout.write(`${feeds.join('\n')}\n`)
    return 0
  }
  if (command === 'prerequisites') {
    if (!values.version || !values.base) fail('prerequisites needs --version and --base')
    const problems = await checkPrerequisites({
      version: values.version,
      base: values.base,
      ...patience,
    })
    const bridge = problems.length === 0 && roleOf(values.version) === 'bridge'
    return say(problems, {
      dry,
      ok: `${values.version} may move its feeds (${bridge ? 'it is the bridge' : 'the old feeds serve the bridge, with its files'})`,
    })
  }
  if (command === 'check') {
    if (!values.dir || !values.version || !values.base) {
      fail('check needs --dir, --version and --base')
    }
    const problems = await checkFeeds({
      dir: values.dir,
      version: values.version,
      base: values.base,
      ...patience,
    })
    return say(problems, { dry: false, ok: 'serve this release as the rule says' })
  }
  return fail('usage: feeds.mjs plan|prerequisites|check (see the top of this file)')
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
