/**
 * Plants in the publisher (app/scripts/publish.mjs): the versioned release made
 * a draft and published whole, a run cut short finished by running it again,
 * a feed's latest.json never deleted for its replacement, and only the push of
 * a tag publishing. The tests of the publisher, against GitHub as
 * tests/github-sim.mjs has it, and the workflow's steps run as written, must
 * catch each.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { PUBLISH, PUBLISH_JS, RERUN } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `publish: ${name}`,
  edits: [[PUBLISH_JS, from, to]],
  runs: [PUBLISH],
  meant,
})

export const PLANTS = [
  plant(
    'the old latest.json is deleted before the new one is uploaded',
    'async function swap({ gh, dir, repo, feed, log }) {\n',
    "async function swap({ gh, dir, repo, feed, log }) {\n  await gh.must(['release', 'delete-asset', feed, LATEST, '--yes'], 'deleting')\n",
    'leaves the old latest.json where it was when the upload',
  ),
  plant(
    'the old latest.json is deleted, not set aside, so it cannot be put back',
    'await rename({ gh, repo }, held.get(LATEST), PREVIOUS)',
    "await gh.must(['release', 'delete-asset', feed, LATEST, '--yes'], 'deleting')",
    'puts the old latest.json back when the second rename fails',
  ),
  plant(
    'the previous latest.json is not put back when the second rename fails',
    'await rename({ gh, repo }, { ...held.get(LATEST), name: PREVIOUS }, LATEST)',
    'void held',
    'puts the old latest.json back when the second rename fails',
  ),
  plant(
    'a rename of a file that is not there is tried all the same',
    'if (asset === undefined) fail(`there is no file to rename to ${to}`)',
    'void asset',
    'says what is missing when the live latest.json is gone',
  ),
  plant(
    'a feed that already serves this latest.json is swapped all the same',
    'if (!served.ok || !served.body.equals(readFileSync(join(dir, LATEST)))) {',
    'if (true) {',
    'leaves a feed that already serves this latest.json untouched',
  ),
  plant(
    'a previous latest.json left by a cut swap is not put back first',
    'if (!has(LATEST) && has(PREVIOUS)) {',
    'if (false) {',
    'puts the previous latest.json back first',
  ),
  plant(
    'the versioned release is made public, not as a draft',
    "...['release', 'create', tag, '--draft', '--verify-tag'],",
    "...['release', 'create', tag, '--verify-tag'],",
    'makes the release a draft, and publishes it only once every file is there',
  ),
  plant(
    'the release is not made a prerelease',
    "...(version.includes('-') ? ['--prerelease'] : []),",
    '...[],',
    'makes the release a draft, and publishes it only once every file is there',
  ),
  plant(
    'a draft is published without looking at its files',
    "if (asset === undefined || asset.size !== size || (asset.state ?? 'uploaded') !== 'uploaded') {",
    'if (false) {',
    'does not publish a draft whose file is short',
  ),
  plant(
    'a draft left by a run that died is not emptied',
    "await gh.must(['release', 'delete-asset', tag, asset.name, '--yes'], `emptying the draft ${tag}`)",
    'void asset',
    'empties a draft that holds some of the files',
  ),
  plant(
    'a published release that lacks a file is kept as it is',
    "if (lacking.length === 0) return 'kept'",
    "return 'kept'",
    'adds what a published release lacks',
  ),
  plant(
    "a published release's files are not held to the files built",
    'if (wrong.length > 0) {',
    'if (false) {',
    'refuses a published release whose file is not the one built',
  ),
  plant(
    'the rule is not asked before anything moves',
    'if (problems.length > 0) {\n    fail(`${version} may not move a feed',
    'if (false) {\n    fail(`${version} may not move a feed',
    'is refused while the old feed still serves the release before the bridge',
  ),
  plant(
    'a failure to look at a release is taken for its absence',
    'return fail(`could not look at the release ${tag}: ${answer.stderr.trim()}`)',
    'return null',
    'stops where gh cannot tell whether a release is there',
  ),
  plant(
    'the old feed is not marked as pinned',
    'if (pinned) {',
    'if (false) {',
    'marks the old feed as pinned to the bridge',
  ),
  plant(
    'a hand run on a tag publishes: only the ref type is asked',
    "if (env.GITHUB_EVENT_NAME !== 'push' || env.GITHUB_REF_TYPE !== 'tag') {",
    "if (env.GITHUB_REF_TYPE !== 'tag') {",
    'refuses a hand run, though it is on a tag',
  ),
  plant(
    'any run publishes',
    "if (env.GITHUB_EVENT_NAME !== 'push' || env.GITHUB_REF_TYPE !== 'tag') {",
    'if (false) {',
    'refuses a push of a branch, a run outside a workflow',
  ),
  plant(
    'a tag other than the one pushed is published',
    'if (env.GITHUB_REF_NAME !== values.tag) {',
    'if (false) {',
    'refuses a push of a branch, a run outside a workflow',
  ),
  // No feed is moved backward: a run again after a later release went out leaves its feed at that release.
  plant(
    'a feed is swapped whatever release it names: the feed is not asked',
    'if (later !== null) {',
    'if (false) {',
    RERUN,
  ),
  plant(
    'a feed that names a later release fails the run instead of being left alone',
    'did = `superseded by ${later}`',
    "fail('superseded')",
    RERUN,
  ),
  plant(
    'a feed that was left alone is said to be kept',
    'did = `superseded by ${later}`',
    "did = 'kept'",
    RERUN,
  ),
  plant(
    'a feed that is left alone is not said to be, in the log',
    'log(\n        `${feed} names ${later}, which comes after ${version}: it is left alone, and nothing is moved backward`,\n      )',
    'void log',
    RERUN,
  ),
  plant(
    'a feed that cannot be read is swapped all the same',
    'if (!served.ok && served.status !== 404) {',
    'if (false) {',
    'changes no feed it cannot read',
  ),
  plant(
    'a feed that is listed and not served is left as it is, not mended',
    'if (!served.ok && served.status !== 404) {',
    'if (!served.ok) {',
    'swaps a latest.json that is listed and not served at all',
  ),
  plant(
    'a feed that is listed and not served is taken for one that names a later release',
    'const later = served.ok ? laterRelease(served.body, version) : null',
    "const later = served.ok ? laterRelease(served.body, version) : 'unknown'",
    'swaps a latest.json that is listed and not served at all',
  ),
  plant(
    'a feed is read once, not again past a blip',
    'const served = await reads.file(`${base}/${feed}/${LATEST}`)',
    'const served = await reads.once(`${base}/${feed}/${LATEST}`)',
    'reads a feed again past a blip',
  ),
  // What the check that follows is left to hold the new feeds to.
  plant(
    'the check is left no record of what the feeds named',
    'writeFileSync(join(dir, FEEDS_BEFORE), `${JSON.stringify(before)}\\n`)',
    'void before',
    'leaves the check a record of what each new feed named',
  ),
  plant(
    'the record says what the run publishes, not what the feed named',
    'named = served.ok ? namedRelease(served.body) : null',
    'named = version',
    'leaves the check a record of what each new feed named',
  ),
  plant(
    'the record holds the old feed too',
    'if (!pinned) before[feed] = named',
    'before[feed] = named',
    'leaves the check a record of what each new feed named',
  ),
]
