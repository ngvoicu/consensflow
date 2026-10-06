#!/usr/bin/env node
/**
 * Publishes a release and moves its feeds, so that a run that died or was cut
 * short is finished by running it again, and no run leaves an installed app
 * without the feed it reads.
 *
 *   node app/scripts/publish.mjs --dir <dist> --tag v<version> --base <url>
 *
 * `dist` holds what the workflow's jobs built (app/scripts/feeds.mjs
 * `releaseAssets`) and notes.txt; `base` is where GitHub serves a release's
 * files (https://github.com/<owner>/<repo>/releases/download). It runs only
 * for the push of a version tag: a hand run publishes nothing. In order:
 *
 *   1. the rule (feeds.mjs): a release before the bridge is refused, a bridge
 *      whose archive lacks what the old apps check for is refused, and a later
 *      release is refused until the old feeds serve the bridge with its files.
 *      Nothing has been moved when it refuses;
 *   2. the versioned release, whose files are never replaced. None yet: a draft
 *      is made, filled, and published only when every file is there at its
 *      size. A draft left by a run that died is emptied and filled again, being
 *      nobody's. A published one is kept: what it lacks is added, and then every
 *      file is downloaded and held to the file built here, a mismatch refusing
 *      the run before a feed moves;
 *   3. each feed the rule names, the new ones first. A feed's latest.json is
 *      never deleted for its replacement: the replacement is uploaded beside it,
 *      and the two names swapped by renames (so the feed lacks the file for one
 *      API call, not for an upload), the previous one kept until the swap is
 *      done and put back if it fails. A feed that already serves this latest.json
 *      is left alone. What a run that died left beside latest.json
 *      (latest.next.json, latest.previous.json) is settled first.
 *
 * What the feeds serve afterwards is `feeds.mjs check`'s to say.
 */
import { spawnSync } from 'node:child_process'
import { copyFileSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, posix } from 'node:path'
import { pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import {
  assetProblems,
  checkPrerequisites,
  MANIFEST,
  membersOf,
  missingForOldApps,
  planFeeds,
  reader,
  releaseAssets,
  sha256,
} from './feeds.mjs'

const LATEST = 'latest.json'
/** Beside latest.json while one is being swapped for another. */
const NEXT = 'latest.next.json'
const PREVIOUS = 'latest.previous.json'

function fail(message) {
  throw new Error(message)
}

const nameOf = (path) => posix.basename(path)
const onDisk = (dir, path) => join(dir, ...path.split('/'))

/** The text of the release's page: its notes, and where each file is for whom. */
function bodyOf(notes, version) {
  return [
    notes.replace(/\n+$/, ''),
    '',
    '**Downloads.** macOS on Apple silicon: the DMG, signed with a Developer ID',
    'and notarized by Apple. Windows x64: the installer (`-setup.exe`), or the',
    'portable exe (`-portable.exe`), one file you run from anywhere: its first',
    'start unpacks Node and the CLI into',
    '`%LOCALAPPDATA%\\dev.ngvoicu.consensflow\\portable-runtime`. Its data stays in your user',
    'folder either way. Neither Windows file is code-signed, so Windows shows its',
    'unknown-publisher warning. The Mac app updates itself through',
    `ConsensFlow → Check for Updates on the ${version.includes('-') ? 'Alpha' : 'Stable'} channel.`,
    '',
  ].join('\n')
}

/**
 * A run of `gh`, as `publishRelease` is given it: `gh(args, { cwd })` resolves
 * to `{ status, stdout, stderr }`.
 */
function runner(gh, dir, log) {
  const ask = async (args) => {
    log(`gh ${args.join(' ')}`)
    return gh(args, { cwd: dir })
  }
  /** The output of a call that must succeed. */
  const must = async (args, what) => {
    const answer = await ask(args)
    if (answer.status !== 0) {
      fail(`${what}: ${(answer.stderr || answer.stdout).trim() || `gh exited ${answer.status}`}`)
    }
    return answer.stdout
  }
  /** What a release holds, or null where there is none; any other failure is not an absence. */
  const state = async (tag) => {
    const answer = await ask(['release', 'view', tag, '--json', 'assets,isDraft'])
    if (answer.status === 0) {
      const { assets, isDraft } = JSON.parse(answer.stdout)
      return { draft: isDraft, assets }
    }
    if (/not found|HTTP 404/i.test(answer.stderr)) return null
    return fail(`could not look at the release ${tag}: ${answer.stderr.trim()}`)
  }
  return { ask, must, state }
}

/** Renames a release's asset, which GitHub does in one call. */
async function rename({ gh, repo }, asset, to) {
  if (asset === undefined) fail(`there is no file to rename to ${to}`)
  const id = /\/releases\/assets\/(\d+)$/.exec(asset.apiUrl ?? '')?.[1]
  if (id === undefined) fail(`gh gave no address for ${asset.name}, which cannot be renamed`)
  await gh.must(
    ['api', '--method', 'PATCH', `repos/${repo}/releases/assets/${id}`, '-f', `name=${to}`],
    `renaming ${asset.name} to ${to}`,
  )
}

/**
 * The versioned release, made as it should be or finished from what a run that
 * died left: 'created', 'remade' (a draft filled again), 'completed' (a
 * published one that lacked files) or 'kept'. Its files are checked
 * (feeds.mjs `assetProblems`) by the caller, which is where a published release
 * that differs is found.
 */
async function releaseVersioned({ gh, dir, tag, version, files, notesFile }) {
  let state = await gh.state(tag)
  const made = state === null
  if (made) {
    await gh.must(
      [
        ...['release', 'create', tag, '--draft', '--verify-tag'],
        ...['--title', `ConsensFlow ${version}`, '--notes-file', notesFile],
        ...(version.includes('-') ? ['--prerelease'] : []),
      ],
      `making the release ${tag}`,
    )
    state = { draft: true, assets: [] }
  }
  if (!state.draft) {
    const held = new Set(state.assets.map((asset) => asset.name))
    const lacking = files.filter((file) => !held.has(nameOf(file)))
    if (lacking.length === 0) return 'kept'
    await gh.must(['release', 'upload', tag, ...lacking], `adding to the release ${tag}`)
    return 'completed'
  }
  for (const asset of state.assets) {
    await gh.must(['release', 'delete-asset', tag, asset.name, '--yes'], `emptying the draft ${tag}`)
  }
  await gh.must(['release', 'upload', tag, ...files], `filling the release ${tag}`)
  const filled = new Map((await gh.state(tag)).assets.map((asset) => [asset.name, asset]))
  for (const file of files) {
    const asset = filled.get(nameOf(file))
    const size = statSync(onDisk(dir, file)).size
    if (asset === undefined || asset.size !== size || (asset.state ?? 'uploaded') !== 'uploaded') {
      fail(
        `the draft ${tag} holds ${nameOf(file)} ${asset === undefined ? 'not at all' : `as ${asset.size} bytes, ${asset.state}`}, not as the ${size} built: nothing is public, run it again`,
      )
    }
  }
  await gh.must(['release', 'edit', tag, '--draft=false'], `publishing the release ${tag}`)
  return made ? 'created' : 'remade'
}

/**
 * Puts what a swap that was cut short left beside latest.json right: the
 * previous latest.json back if latest.json is gone, then what is left of a swap
 * gone. Resolves to what the feed holds now.
 */
async function settle({ gh, repo, feed }, state) {
  const has = (name) => state.assets.some((asset) => asset.name === name)
  const asset = (name) => state.assets.find((one) => one.name === name)
  let moved = false
  if (!has(LATEST) && has(PREVIOUS)) {
    await rename({ gh, repo }, asset(PREVIOUS), LATEST)
    moved = true
  } else if (has(PREVIOUS)) {
    await gh.must(['release', 'delete-asset', feed, PREVIOUS, '--yes'], `clearing ${PREVIOUS}`)
    moved = true
  }
  if (has(NEXT)) {
    await gh.must(['release', 'delete-asset', feed, NEXT, '--yes'], `clearing ${NEXT}`)
    moved = true
  }
  return moved ? gh.state(feed) : state
}

/**
 * Replaces a feed's latest.json by `dir/latest.json` without a moment in which
 * there is none that an upload could stretch: the new one is uploaded beside
 * it, and the names swapped by two renames. A failure puts back what was.
 */
async function swap({ gh, dir, repo, feed, log }) {
  copyFileSync(join(dir, LATEST), join(dir, NEXT))
  try {
    await gh.must(['release', 'upload', feed, NEXT], `uploading the new ${LATEST} to ${feed}`)
  } catch (cause) {
    await gh.ask(['release', 'delete-asset', feed, NEXT, '--yes'])
    throw cause
  }
  const held = new Map((await gh.state(feed)).assets.map((asset) => [asset.name, asset]))
  try {
    await rename({ gh, repo }, held.get(LATEST), PREVIOUS)
  } catch (cause) {
    await gh.ask(['release', 'delete-asset', feed, NEXT, '--yes'])
    throw cause
  }
  try {
    await rename({ gh, repo }, held.get(NEXT), LATEST)
  } catch (cause) {
    try {
      await rename({ gh, repo }, { ...held.get(LATEST), name: PREVIOUS }, LATEST)
    } catch (again) {
      throw new Error(
        `${cause.message}; and ${feed} lacks its ${LATEST} until this is run again, for putting ${PREVIOUS} back failed: ${again.message}`,
      )
    }
    throw cause
  }
  const cleared = await gh.ask(['release', 'delete-asset', feed, PREVIOUS, '--yes'])
  if (cleared.status !== 0) log(`${PREVIOUS} of ${feed} was not cleared; the next run settles it`)
}

/**
 * One feed made to serve `dir/latest.json`: the release made if there is none,
 * what a cut-short swap left settled, and latest.json uploaded, left, or swapped.
 * 'created', 'uploaded', 'replaced' or 'kept'. A feed of the old generation is
 * marked as pinned.
 */
async function moveFeed({ gh, dir, repo, feed, version, base, reads, pinned, log }) {
  const name = feed.slice(feed.indexOf('-') + 1)
  const label = `${name[0].toUpperCase()}${name.slice(1)}`
  let state = await gh.state(feed)
  const made = state === null
  if (made) {
    await gh.must(
      [
        ...['release', 'create', feed, '--prerelease', '--title', `${label} update feed`],
        ...['--notes', 'The latest.json installed apps read on this channel. Do not delete.'],
      ],
      `making the feed ${feed}`,
    )
    state = { draft: false, assets: [] }
  }
  state = await settle({ gh, repo, feed }, state)
  let did = 'kept'
  if (!state.assets.some((asset) => asset.name === LATEST)) {
    await gh.must(['release', 'upload', feed, LATEST], `uploading ${LATEST} to ${feed}`)
    did = made ? 'created' : 'uploaded'
  } else {
    const served = await reads.once(`${base}/${feed}/${LATEST}`)
    if (!served.ok || !served.body.equals(readFileSync(join(dir, LATEST)))) {
      await swap({ gh, dir, repo, feed, log })
      did = 'replaced'
    }
  }
  if (pinned) {
    await gh.must(
      [
        ...['release', 'edit', feed, '--title', `${label} update feed, pinned`],
        ...[
          '--notes',
          `Pinned to ConsensFlow ${version}, the first release to read feed-${name}: the apps before it read this feed and install it, and from it on read feed-${name}. Never move this feed, and never delete this release or the one it names.`,
        ],
      ],
      `marking ${feed} as pinned`,
    )
  }
  return did
}

/**
 * Publishes the release `tag` built in `dir`, and moves its feeds. `gh` is
 * `gh(args, { cwd })`, resolving to `{ status, stdout, stderr }`; `members` lists
 * an archive's members (as `tar -t` does); `repo` is `<owner>/<repo>`. Resolves
 * to what was done; rejects, with nothing moved, when the rule refuses it.
 */
export async function publishRelease({
  dir,
  tag,
  base,
  repo,
  gh: run,
  manifest = MANIFEST,
  members = membersOf,
  patience = {},
  log = () => {},
}) {
  if (!/^v\d/.test(tag)) fail(`${tag} is not a version tag`)
  const version = tag.slice(1)
  const assets = releaseAssets(version)
  const paths = Object.values(assets)
  for (const path of [...paths, 'notes.txt']) {
    let size = 0
    try {
      size = statSync(onDisk(dir, path)).size
    } catch {
      // Said below.
    }
    if (size === 0) fail(`missing ${path}`)
  }
  const feeds = planFeeds({
    version,
    missing: missingForOldApps(members(onDisk(dir, assets.archive))),
    manifest,
  })
  const problems = await checkPrerequisites({ version, base, manifest, ...patience })
  if (problems.length > 0) {
    fail(`${version} may not move a feed, and nothing was moved:\n- ${problems.join('\n- ')}`)
  }

  writeFileSync(
    join(dir, 'SHA256SUMS'),
    paths.map((path) => `${sha256(readFileSync(onDisk(dir, path)))}  ${nameOf(path)}\n`).join(''),
  )
  writeFileSync(join(dir, 'body.md'), bodyOf(readFileSync(join(dir, 'notes.txt'), 'utf8'), version))

  const gh = runner(run, dir, log)
  const files = [...paths, 'SHA256SUMS']
  const reads = reader(patience)
  const versioned = await releaseVersioned({ gh, dir, tag, version, files, notesFile: 'body.md' })
  const wrong = await assetProblems({ dir, version, base, reads })
  if (wrong.length > 0) {
    fail(
      `the release ${tag} (${versioned}) does not hold the files built here, and no feed was moved: a published release's files are not replaced, so delete the release if it was a mistake, and run again:\n- ${wrong.join('\n- ')}`,
    )
  }
  const moved = {}
  for (const feed of feeds) {
    moved[feed] = await moveFeed({
      gh,
      dir,
      repo,
      feed,
      version,
      base,
      reads,
      pinned: Object.values(manifest.legacy).includes(feed),
      log,
    })
    log(`${feed}: ${moved[feed]}`)
  }
  return { version, release: versioned, feeds: moved }
}

async function main(argv, env) {
  const { values } = parseArgs({
    args: argv,
    options: {
      dir: { type: 'string' },
      tag: { type: 'string' },
      base: { type: 'string' },
      repo: { type: 'string' },
    },
  })
  const repo = values.repo ?? env.GH_REPO ?? env.GITHUB_REPOSITORY
  if (!values.dir || !values.tag || !values.base || !repo) {
    fail('publish needs --dir, --tag and --base, and the repository (GH_REPO)')
  }
  if (env.GITHUB_EVENT_NAME !== 'push' || env.GITHUB_REF_TYPE !== 'tag') {
    fail(
      `only the push of a version tag publishes; this is ${env.GITHUB_EVENT_NAME ?? 'not a workflow'} on ${env.GITHUB_REF_TYPE ?? 'nothing'}`,
    )
  }
  if (env.GITHUB_REF_NAME !== values.tag) {
    fail(`this run is for ${env.GITHUB_REF_NAME}, and it was asked to publish ${values.tag}`)
  }
  const gh = (args, { cwd }) => {
    const ran = spawnSync('gh', args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
    return { status: ran.status ?? 1, stdout: ran.stdout ?? '', stderr: ran.stderr || ran.error?.message || '' }
  }
  const done = await publishRelease({
    dir: values.dir,
    tag: values.tag,
    base: values.base,
    repo,
    gh,
    log: (line) => process.stdout.write(`publish: ${line}\n`),
  })
  process.stdout.write(
    `publish: ${done.version}: the release was ${done.release}; ${Object.entries(done.feeds)
      .map(([feed, did]) => `${feed} ${did}`)
      .join(', ')}\n`,
  )
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2), process.env).then(
    () => {
      process.exitCode = 0
    },
    (cause) => {
      process.stderr.write(`publish: ${cause instanceof Error ? cause.message : String(cause)}\n`)
      process.exitCode = 1
    },
  )
}
