/**
 * Plants in the rule of the feeds and its checks (app/scripts/feeds.mjs): which
 * release is the bridge, what a later release finds true before it moves a
 * feed, and what the feeds serve once it has. The tests of the rule, against
 * GitHub as tests/github-sim.mjs has it, must catch each.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { FEEDS, FEEDS_JS } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `feeds: ${name}`,
  edits: [[FEEDS_JS, from, to]],
  runs: [FEEDS],
  meant,
})

export const PLANTS = [
  plant(
    'the bridge is taken for a later release',
    "return order === 0 ? 'bridge' : 'later'",
    "return 'later'",
    'moves the bridge to the new feed and the old feed',
  ),
  plant(
    'a release before the bridge is let through',
    'if (order < 0) {',
    'if (order < -1) {',
    'refuses a release before the bridge',
  ),
  plant(
    'a later release moves the old feeds too',
    "if (roleOf(version, manifest) === 'later') return feeds",
    "if (roleOf(version, manifest) === 'never') return feeds",
    'moves a later release to the new feeds only',
  ),
  plant(
    'an old feed is held to the bridge by what it names, not byte for byte',
    'if (found.body.equals(published.body)) continue',
    'if (found.body.equals(published.body) || true) continue',
    'refuses where the old feed still serves the release before the bridge',
  ),
  plant(
    'an old feed that cannot be read is taken for absent',
    'problems.push(`${feed} cannot be read (${found.why}): the apps before the bridge read it`)',
    'void found',
    'refuses a feed that is not there: a required feed is not absent',
  ),
  plant(
    "the bridge's archive is asked to be served, not to be the one published",
    '} else if (got.sha256 !== wanted) {',
    '} else if (false) {',
    'refuses a bridge archive that serves junk',
  ),
  plant(
    "the bridge's Windows files are not asked for",
    "const RETAINED = ['archive', 'installer', 'portable']",
    "const RETAINED = ['archive']",
    "refuses the bridge's Windows installer or portable that is gone",
  ),
  plant(
    'the bridge is not looked for after the release is published',
    'problems.push(...(await bridgeProblems({ base, manifest, reads })))',
    'void bridgeProblems',
    'finds the bridge not carried to the old feed',
  ),
  plant(
    'a later release finds nothing to check before it moves',
    "if (role === 'bridge') return []",
    'return []',
    'refuses where the old feed still serves the release before the bridge',
  ),
  plant(
    'only the archive of the release is held to the file built',
    "for (const path of [...Object.values(releaseAssets(version)), 'SHA256SUMS']) {",
    'for (const path of [releaseAssets(version).archive]) {',
    'finds a file of the release that does not download as the one built',
  ),
  plant(
    'a feed served stale is read once',
    '(found) => found.ok && found.body.equals(expected),\n        { attempts, wait },',
    '(found) => found.ok && found.body.equals(expected),\n        { attempts: 1, wait },',
    'waits out an old feed served stale for a moment',
  ),
  plant(
    'the old feed of a channel not in use may not be absent',
    'if (!found.ok && found.status === 404) continue',
    'if (false) continue',
    'lets the old feed of a channel not in use be absent',
  ),
  plant(
    'versions are compared as text',
    'if (part !== bCore[index]) return part < bCore[index] ? -1 : 1',
    'if (String(part) !== String(bCore[index])) return String(part) < String(bCore[index]) ? -1 : 1',
    'orders versions as semantic versioning does',
  ),
  plant(
    "the bridge's latest.json is not asked to name the bridge",
    'if (document.version !== version) {\n    problems.push(`the bridge',
    'if (false) {\n    problems.push(`the bridge',
    'refuses a bridge whose latest.json names another release',
  ),
  plant(
    "a release's latest.json is not asked to name its own archive",
    'if (document.platforms?.[TARGET]?.url !== wanted) {',
    'if (false) {',
    'finds a latest.json that names no archive of this release',
  ),
  plant(
    'a missing SHA256SUMS is let pass',
    "problems.push(`the bridge's ${tag}/SHA256SUMS cannot be read (${sums.why})`)",
    'void sums',
    'refuses where SHA256SUMS is gone',
  ),
]
