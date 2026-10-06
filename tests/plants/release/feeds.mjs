/**
 * Plants in the rule of the feeds and its checks (app/scripts/feeds.mjs): which
 * release is the bridge, what a later release finds true before it moves a
 * feed, what the feeds serve once it has, and that no feed is moved backward
 * (the comparison of what a feed names with the release run, and what the check
 * accepts of a feed left alone). The tests of the rule, against GitHub as
 * tests/github-sim.mjs has it, must catch each.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { FEEDS, FEEDS_JS, PUBLISH, RERUN } from './kit.mjs'

const plant = (name, from, to, meant, runs = [FEEDS]) => ({
  name: `feeds: ${name}`,
  edits: [[FEEDS_JS, from, to]],
  runs,
  meant,
})

/** Where a feed's latest.json is asked whether it names a release after the run's own. */
const COMPARISON = 'named !== null && compareVersions(named, version) > 0 ? named : null'
/** The tests of the rule that say which release a feed names, and what the publisher does about it. */
const ASKED = 'says which release a feed names where that one is after a given release'

/**
 * The comparison planted wrong, met twice: by the rule's own test of it, and by
 * the publisher run end to end against GitHub as the tests have it.
 */
const compared = (name, to, rerun = RERUN) => [
  plant(`${name} (asked of the rule)`, COMPARISON, to, ASKED),
  plant(`${name} (a run again, end to end)`, COMPARISON, to, rerun, [PUBLISH]),
]

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
    '(own !== null && laterRelease(found.body, own) !== null)),\n        { attempts, wait },',
    '(own !== null && laterRelease(found.body, own) !== null)),\n        { attempts: 1, wait },',
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
  // No feed is moved backward: the comparison of what a feed names with the release run, planted wrong.
  ...compared(
    'no feed is ever taken for one that names a later release',
    'named !== null && compareVersions(named, version) > 1 ? named : null',
  ),
  ...compared(
    'every feed that names a release is taken for one that names a later release',
    'named',
  ),
  ...compared(
    'the comparison is reversed: an earlier release is taken for a later one',
    'named !== null && compareVersions(named, version) < 0 ? named : null',
  ),
  ...compared(
    'the very same release is taken for a later one',
    'named !== null && compareVersions(named, version) >= 0 ? named : null',
    'for equal is not later',
  ),
  plant(
    'a version that is not a semantic one is compared all the same',
    "return typeof version === 'string' && SEMVER.test(version) ? version : null",
    "return typeof version === 'string' ? version : null",
    'moves a feed whose latest.json names a version that is not a semantic one',
    [PUBLISH],
  ),
  plant(
    'what is not a latest.json is taken for one that names a later release',
    "return typeof version === 'string' && SEMVER.test(version) ? version : null\n  } catch {\n    return null",
    "return typeof version === 'string' && SEMVER.test(version) ? version : null\n  } catch {\n    return '99.0.0'",
    'moves a feed whose latest.json is not a latest.json',
    [PUBLISH],
  ),
  // What the check accepts of a feed that a run left alone.
  plant(
    'the check does not accept a feed left at a later release',
    'found.ok && (found.body.equals(latest) || laterRelease(found.body, version) !== null)',
    'found.ok && found.body.equals(latest)',
    'accepts a new feed that names a later release',
  ),
  plant(
    'the check accepts what is not a later release instead of what is',
    'found.ok && (found.body.equals(latest) || laterRelease(found.body, version) !== null)',
    'found.ok && (found.body.equals(latest) || laterRelease(found.body, version) === null)',
    'does not approve a feed that names an earlier release, this one with other bytes, or none',
  ),
  plant(
    'the check approves any feed it can read',
    'found.ok && (found.body.equals(latest) || laterRelease(found.body, version) !== null)',
    'found.ok',
    'does not approve a feed that names an earlier release, this one with other bytes, or none',
  ),
  // What the check holds a feed to that the publisher left a record of: that it did not go back.
  ...[
    [
      'the check does not look at what a feed named before',
      'if (now !== null && before.has(feed) && compareVersions(now, before.get(feed)) < 0) {',
      'if (false) {',
    ],
    [
      'a feed that is as it was is taken for one that went backward',
      'compareVersions(now, before.get(feed)) < 0',
      'compareVersions(now, before.get(feed)) <= 0',
    ],
    [
      'a feed that moved forward is taken for one that went backward',
      'compareVersions(now, before.get(feed)) < 0',
      'compareVersions(now, before.get(feed)) > 0',
    ],
  ].flatMap(([name, from, to]) => [
    plant(
      `${name} (asked of the check)`,
      from,
      to,
      'an earlier release than it did when the publisher read it',
    ),
    plant(`${name} (a run again, end to end)`, from, to, RERUN, [PUBLISH]),
  ]),
  plant(
    'a record is trusted whatever it holds',
    "typeof named === 'string' && SEMVER.test(named)",
    "typeof named === 'string'",
    'an earlier release than it did when the publisher read it',
  ),
  plant(
    'a feed that names a later release is waited for as if it were served stale',
    '(found.body.equals(expected) ||\n            (own !== null && laterRelease(found.body, own) !== null)),',
    'found.body.equals(expected),',
    'and does not wait for it',
  ),
]
