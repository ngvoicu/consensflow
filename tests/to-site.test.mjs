// install/to-site.mjs: copies the installers to the site, and --check says whether the site holds
// the repository's two installers, byte for byte, on the branch it will publish them from. A site
// that serves an installer older than the repository's is a release that installs without what
// the tests and the plants hold (the site branch once held round 3's install.sh when the
// repository's had the sweep of what a killed install leaves: T-840). --check reads the site's
// git objects, not its working tree: the site's checkout is on `main`, which holds no installer,
// while the installers wait on its `installers` branch (T-847). The scripts are run as they are,
// against a site of the test's own.

import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import { INSTALL_PS1, INSTALL_SH, ROOT, scriptKeyLine } from '../install/kit.mjs'

const TO_SITE = path.join(ROOT, 'install/to-site.mjs')
const KEY = scriptKeyLine(readFileSync(INSTALL_SH, 'utf8'))
const SCRIPTS = [INSTALL_SH, INSTALL_PS1]

let work

before(() => {
  work = mkdtempSync(path.join(os.tmpdir(), 'cf-to-site-'))
})

after(() => {
  if (work) rmSync(work, { recursive: true, force: true })
})

function git(folder, ...args) {
  const ran = spawnSync(
    'git',
    ['-C', folder, '-c', 'user.name=test', '-c', 'user.email=test@example.com', ...args],
    { encoding: 'utf8' },
  )
  assert.equal(ran.status, 0, `git ${args.join(' ')}: ${ran.stderr}`)
  return ran.stdout.trim()
}

/** Writes the installers `holds` into `folder`: a script's path (copied), or `{ name, text }`. */
function put(folder, holds) {
  for (const one of holds) {
    if (typeof one === 'string') copyFileSync(one, path.join(folder, path.basename(one)))
    else writeFileSync(path.join(folder, one.name), one.text)
  }
}

/** An earlier version of the script at `script`: the same but for one line. */
function stale(script) {
  return {
    name: path.basename(script),
    text: readFileSync(script, 'utf8').replace(/\n/, '\n# an earlier version\n'),
  }
}

/**
 * A site of the test's own, a git repository as the real one is, with `main` checked out: its page
 * shows `shows`, and `main` holds the installers `onMain`; where `onBranch` is given, a branch
 * `installers` holds those, and `main` stays the checkout. `uncommitted` are written into the
 * working tree after.
 */
function site({ shows = KEY, onMain = [], onBranch, uncommitted = [] } = {}) {
  const folder = mkdtempSync(path.join(work, 'site-'))
  git(folder, 'init', '-q', '-b', 'main')
  git(folder, 'config', 'core.autocrlf', 'false')
  writeFileSync(path.join(folder, 'index.html'), `<p>The update key is ${shows}</p>\n`)
  put(folder, onMain)
  git(folder, 'add', '-A')
  git(folder, 'commit', '-q', '-m', 'main')
  if (onBranch !== undefined) {
    git(folder, 'checkout', '-q', '-b', 'installers')
    put(folder, onBranch)
    git(folder, 'add', '-A')
    git(folder, 'commit', '-q', '-m', 'installers', '--allow-empty')
    git(folder, 'checkout', '-q', 'main')
  }
  put(folder, uncommitted)
  return folder
}

/** A folder that is a site's by its page alone, as the copy takes one. */
function plainSite({ shows = KEY } = {}) {
  const folder = mkdtempSync(path.join(work, 'plain-'))
  writeFileSync(path.join(folder, 'index.html'), `<p>The update key is ${shows}</p>\n`)
  return folder
}

function toSite(...args) {
  const ran = spawnSync(process.execPath, [TO_SITE, ...args], { encoding: 'utf8' })
  return { status: ran.status, out: `${ran.stdout}${ran.stderr}` }
}

const name = (script) => path.basename(script)
const dots = (words) => words.replaceAll('.', '\\.')

describe('npm run site:installers', () => {
  test("copies both installers into the site's folder, where the branch they go on is checked out", () => {
    const folder = site({ onBranch: [] })
    git(folder, 'checkout', '-q', 'installers')
    const ran = toSite(folder)
    assert.equal(ran.status, 0, ran.out)
    for (const script of SCRIPTS)
      assert.deepEqual(
        readFileSync(path.join(folder, name(script))),
        readFileSync(script),
        name(script),
      )
    // Copied, not yet committed: the branch tip is the old one, and --check says so.
    assert.equal(toSite('--check', folder).status, 1)
    git(folder, 'add', '-A')
    git(folder, 'commit', '-q', '-m', 'the installers')
    const checked = toSite('--check', folder)
    assert.equal(checked.status, 0, checked.out)
  })

  test('is refused while the site shows another key than the installers pin, or none', () => {
    for (const shows of ['RWSomeOtherKeyThatIsNotTheOneTheInstallersPin00000000', 'no key']) {
      const folder = plainSite({ shows })
      const ran = toSite(folder)
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /does not show the key the installers pin/)
      assert.ok(!existsSync(path.join(folder, 'install.sh')), 'nothing was copied')
    }
  })

  test("a site that holds no index.html is not a site's folder", () => {
    const empty = mkdtempSync(path.join(work, 'empty-'))
    assert.equal(toSite(empty).status, 2)
    assert.equal(toSite('--check', empty).status, 2)
  })
})

describe('npm run site:installers -- --check', () => {
  test("is satisfied by the installers branch's tip when it holds the two, the checkout holding none", () => {
    const folder = site({ onBranch: SCRIPTS })
    assert.ok(!existsSync(path.join(folder, 'install.sh')), 'the checkout is on main')
    const ran = toSite('--check', folder)
    assert.equal(ran.status, 0, ran.out)
    assert.match(ran.out, /the site holds this repository's installers/)
  })

  test('is satisfied by HEAD where there is no installers branch', () => {
    const ran = toSite('--check', site({ onMain: SCRIPTS }))
    assert.equal(ran.status, 0, ran.out)
  })

  test("refuses a branch tip whose installer is not the repository's, saying which and how to mend it", () => {
    for (const script of SCRIPTS) {
      const others = SCRIPTS.filter((one) => one !== script)
      // The current scripts lie in the working tree, uncommitted: the tip is what is read.
      const folder = site({ onBranch: [stale(script), ...others], uncommitted: SCRIPTS })
      const ran = toSite('--check', folder)
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, new RegExp(`${dots(name(script))} differs`))
      assert.match(ran.out, /npm run site:installers/)
      for (const other of others)
        assert.doesNotMatch(ran.out, new RegExp(`${dots(name(other))} differs`))
    }
  })

  test('refuses an installers branch that is stale where HEAD holds the current ones, and takes the branch where HEAD is stale', () => {
    const behind = site({ onMain: SCRIPTS, onBranch: SCRIPTS.map(stale) })
    assert.equal(toSite('--check', behind).status, 1)
    const ahead = site({ onMain: SCRIPTS.map(stale), onBranch: SCRIPTS })
    assert.equal(toSite('--check', ahead).status, 0)
  })

  test('refuses a tip that holds one of them, or none, whatever the checkout holds: the site is not ready', () => {
    for (const onBranch of [[INSTALL_SH], [INSTALL_PS1], []]) {
      const folder = site({ onBranch, uncommitted: SCRIPTS })
      const ran = toSite('--check', folder)
      assert.equal(ran.status, 1, ran.out)
      assert.match(ran.out, /is missing from/)
    }
  })

  test('fails, not skips, where the site is there and no ref holds the installers', () => {
    const ran = toSite('--check', site())
    assert.equal(ran.status, 1, ran.out)
    assert.match(ran.out, /install\.sh is missing from HEAD; install\.ps1 is missing from HEAD/)
  })

  test("refuses while the tip's page shows another key than the installers pin, as the copy does", () => {
    const ran = toSite('--check', site({ shows: 'no key', onBranch: SCRIPTS }))
    assert.equal(ran.status, 1, ran.out)
    assert.match(ran.out, /does not show the key the installers pin/)
  })

  test('a folder that is no git repository has no branch tips to read', () => {
    const ran = toSite('--check', plainSite())
    assert.equal(ran.status, 2, ran.out)
    assert.match(ran.out, /is not a git repository/)
  })
})

// The site the installers are served from lies beside the main checkout of this repository. Where
// it is there, the gate holds its branch tip to the repository's installers, and fails when it
// does not: the site's checkout is on `main`, which holds none, so only the tips can tell.
describe("the site beside this repository's folder", () => {
  const beside = path.join(ROOT, '..', 'sites', 'consensflow-site')
  test("holds the repository's installers on its tip", {
    skip: !existsSync(beside) && 'there is no site beside this repository here',
  }, () => {
    const ran = toSite('--check', beside)
    assert.equal(ran.status, 0, ran.out)
  })
})
