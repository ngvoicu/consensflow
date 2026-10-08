/**
 * The versions of the apps the updater smoke runs, and the one the update is
 * given. The installed apps are the bridge (`v3.0.0-alpha.81`) and the flip
 * release, and the update is this checkout, the release that ships no Node,
 * whose version is the flip's until a release moves it: an app takes only a
 * newer update (`install_archive`), so the update's build is given the next
 * version through the build's own override (`build.mjs`), never in a file of
 * the product's.
 */

/** A version as semver reads it: `3.0.0-alpha.81` is the core `[3, 0, 0]` and the pre-release `['alpha', 81]`. */
export function parseVersion(text) {
  const found = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/.exec(text)
  if (found === null) throw new Error(`not a semantic version: ${text}`)
  return {
    core: found.slice(1, 4).map(Number),
    pre: (found[4] ?? '')
      .split('.')
      .filter(Boolean)
      .map((part) => (/^\d+$/.test(part) ? Number(part) : part)),
  }
}

/** Semver's order: the core first, a pre-release before its release, numbers before words. */
export function compareVersions(a, b) {
  const [left, right] = [parseVersion(a), parseVersion(b)]
  for (let at = 0; at < 3; at += 1) {
    if (left.core[at] !== right.core[at]) return left.core[at] < right.core[at] ? -1 : 1
  }
  if (left.pre.length === 0 || right.pre.length === 0) {
    return left.pre.length === right.pre.length ? 0 : left.pre.length === 0 ? 1 : -1
  }
  for (let at = 0; at < Math.max(left.pre.length, right.pre.length); at += 1) {
    const [x, y] = [left.pre[at], right.pre[at]]
    if (x === undefined || y === undefined) return x === undefined ? -1 : 1
    if (x === y) continue
    if (typeof x !== typeof y) return typeof x === 'number' ? -1 : 1
    return x < y ? -1 : 1
  }
  return 0
}

/** The version after `text`: the next number of a pre-release (`alpha.81` to `alpha.82`), else the next patch. */
export function nextVersion(text) {
  const { core, pre } = parseVersion(text)
  const last = pre.findLastIndex((part) => typeof part === 'number')
  if (pre.length > 0 && last !== -1) {
    const bumped = pre.map((part, at) => (at === last ? part + 1 : part))
    return `${core.join('.')}-${bumped.join('.')}`
  }
  return `${core[0]}.${core[1]}.${core[2] + 1}`
}

/** The newest of `versions`. */
export function newest(versions) {
  return versions.reduce((latest, each) => (compareVersions(each, latest) > 0 ? each : latest))
}

/**
 * The release the flip is: the newest of `tags` (`v3.0.0-alpha.82`, as
 * `git tag` names them) that is newer than the bridge's. The update goes to an
 * installed app of the release before it, and a release between the bridge and
 * this checkout's, the flip's or a later one that still ships Node, is such an
 * app. Nothing between them is an error that says how to name the flip.
 */
export function flipTag(tags, bridge) {
  const later = tags
    .filter((tag) => /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag))
    .filter((tag) => compareVersions(tag.slice(1), bridge.slice(1)) > 0)
  if (later.length === 0) {
    throw new Error(
      `no release newer than the bridge (${bridge}) is tagged here: the flip release is not out, or its tag is not fetched (git fetch --tags); name the flip with --flip-ref <tag or commit> or --flip <a checkout of it>`,
    )
  }
  return later.reduce((latest, tag) =>
    compareVersions(tag.slice(1), latest.slice(1)) > 0 ? tag : latest,
  )
}

/**
 * The version the update is built as: this checkout's own where a release has
 * moved it past every installed app's, else the one after the newest installed
 * app's.
 */
export function updateVersion(checkout, installed) {
  const latest = newest(installed)
  return compareVersions(checkout, latest) > 0 ? checkout : nextVersion(latest)
}
