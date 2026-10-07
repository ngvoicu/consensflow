/**
 * The versions of the two apps the updater smoke runs, and the one the update
 * is given. The installed app is the bridge (`v3.0.0-alpha.81`) and the update
 * is this checkout, whose version is the bridge's until a release moves it:
 * an app takes only a newer update (`install_archive`), so the update's build
 * is given the next version through the build's own override (`build.mjs`),
 * never in a file of the product's.
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

/**
 * The version the update is built as: this checkout's own where a release has
 * moved it past the installed app's, else the one after the installed app's.
 */
export function updateVersion(checkout, installed) {
  return compareVersions(checkout, installed) > 0 ? checkout : nextVersion(installed)
}
